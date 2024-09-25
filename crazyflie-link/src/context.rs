//! # Link Context
//!
//! The Link context keeps track of the radio dongles opened and used by connections.
//! It also keeps track of the async executor

use crate::connection::ConnectionFlags;
use crate::crazyradio::Channel;
use crate::crazyradio::SharedCrazyradio;
use crate::error::{Error, Result};
use crate::Connection;
use futures_util::lock::Mutex;
use hex::FromHex;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use std::time::Duration;
use rusb::DeviceList;
use url::Url;

/// Context for the link connections
pub struct LinkContext {
    radios: Mutex<BTreeMap<usize, Weak<SharedCrazyradio>>>
}

impl LinkContext {
    /// Create a new link context
    pub fn new() -> Self {
        Self {
            radios: Mutex::new(BTreeMap::new())
        }
    }

    async fn get_radio(&self, radio_nth: usize) -> Result<Arc<SharedCrazyradio>> {
        let mut radios = self.radios.lock().await;

        radios.entry(radio_nth).or_insert_with(Weak::new);

        let radio = match Weak::upgrade(&radios[&radio_nth]) {
            Some(radio) => radio,
            None => {
                let new_radio = crate::crazyradio::Crazyradio::open_nth_async(radio_nth).await?;
                let new_radio = Arc::new(SharedCrazyradio::new(new_radio));
                radios.insert(radio_nth, Arc::downgrade(&new_radio));

                new_radio
            }
        };
        Ok(radio)
    }

    fn parse_uri(&self, uri: &str) -> Result<(usize, Channel, [u8; 5], ConnectionFlags)> {
        let uri = Url::parse(uri)?;

        if uri.scheme() != "radio" {
            return Err(Error::InvalidUri);
        }

        let radio: usize = uri.domain().ok_or(Error::InvalidUri)?.parse()?;

        let path: Vec<&str> = uri.path_segments().ok_or(Error::InvalidUri)?.collect();
        if path.len() != 3 {
            return Err(Error::InvalidUri);
        }
        let channel = Channel::from_number(path[0].parse()?)?;
        let _rate = path[1];
        let addr_str = path[2];

        if addr_str.len() > 10 {
            return Err(Error::InvalidUri);
        }

        let address = match <[u8; 5]>::from_hex(format!("{:0>10}", addr_str)) {
            Ok(address) => address,
            Err(_) => return Err(Error::InvalidUri),
        };

        let mut flags = ConnectionFlags::SAFELINK | ConnectionFlags::ACKFILTER;
        for (key, value) in uri.query_pairs() {
            match key.as_ref() {
                "safelink" => {
                    if value == "0" {
                        flags.remove(ConnectionFlags::SAFELINK);
                    }
                }
                "ackfilter" => {
                    if value == "0" {
                        flags.remove(ConnectionFlags::ACKFILTER);
                    }
                }
                _ => continue,
            };
        }

        Ok((radio, channel, address, flags))
    }

    /// Scan for Crazyflies at some given address
    ///
    /// This function will send a packet to every channels and look for an acknowledgement in return.
    ///
    /// The address argument will set the radio packets address to scan for.
    ///
    /// It returns a list of URIs that can be passed to the [LinkContext::open_link()] function.
    pub async fn scan(&self, address: [u8; 5]) -> Result<Vec<String>> {
        let channels = match self.get_radio(0).await {
            Ok(radio) => {
              radio.scan_async(
                  Channel::from_number(0)?,
                  Channel::from_number(125)?,
                  address,
                  vec![0xff],
              )
              .await?
            }
            Err(_) => Vec::new(),
        };

        let mut found = Vec::new();

        for channel in channels {
          let channel: u8 = channel.into();
          let address = hex::encode(address.to_vec()).to_uppercase();
          found.push(format!("radio://0/{}/2M/{}", channel, address));
        }

        // Spawn a blocking function onto the runtime
        let found = tokio::task::spawn_blocking(|| {
          for device in DeviceList::new()?.iter() {
            let device_desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };
  
            if device_desc.vendor_id() == 0x0483 && device_desc.product_id() == 0x5740 {
                let timeout = Duration::from_secs(1);
                let handle = match device.open() {
                  Ok(d) => d,
                  Err(_) => continue,
                };
  
              let language = match handle.read_languages(timeout).unwrap_or_default().first() {
                Some(l) => *l,
                None => continue,
              };
  
              let serial = handle.read_serial_number_string(language, &device_desc, timeout)?;
  
              found.push(format!("usb://{}",serial));
            }
          }
          Result::<_>::Ok(found)
        }).await.unwrap()?;

        Ok(found)
    }

    /// Scan for a given list of URIs
    ///
    /// Send a packet to each URI and detect if an acknowledgement is sent back.
    ///
    /// Returns the list of URIs that acknowledged
    pub async fn scan_selected(&self, uris: Vec<&str>) -> Result<Vec<String>> {
        let mut found = Vec::new();
        for uri in uris {
            let (radio_nth, channel, address, _) = self.parse_uri(uri)?;
            let radio = self.get_radio(radio_nth).await?;
            let (ack, _) = radio
                .send_packet_async(channel, address, vec![0xFF, 0xFF, 0xFF])
                .await?;
            if ack.received {
                found.push(format!("{}{}", uri, "?safelink=0&ackfilter=0"));
            }
        }
        Ok(found)
    }

    /// Open a link connection to a given URI
    ///
    /// If successful, the link [Connection] is returned.
    pub async fn open_link(&self, uri: &str) -> Result<Connection> {
        let (radio_nth, channel, address, flags) = self.parse_uri(uri)?;

        let radio = self.get_radio(radio_nth).await?;

        Connection::new(radio, channel, address, flags).await
    }
}
