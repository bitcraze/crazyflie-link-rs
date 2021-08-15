use crate::error::{Error, Result};
use crate::Connection;
use crate::connection::ConnectionFlags;
use crate::crazyradio::SharedCrazyradio;
use crate::crazyradio::Channel;
use std::collections::BTreeMap;
use hex::FromHex;
use std::sync::{Arc, Weak};
use url::Url;
use futures_util::lock::Mutex;

pub struct LinkContext {
    radios: Mutex<BTreeMap<usize, Weak<SharedCrazyradio>>>,
    executor: Arc<dyn async_executors::LocalSpawnHandle<()> + Send + Sync>,
}

impl LinkContext {
    pub fn new(executor: Arc<dyn async_executors::LocalSpawnHandle<()> + Send + Sync>) -> Self {
        Self {
            radios: Mutex::new(BTreeMap::new()),
            executor,
        }
    }

    async fn get_radio(&self, radio_nth: usize) -> Result<Arc<SharedCrazyradio>> {
        let mut radios = self.radios.lock().await;

        radios.entry(radio_nth).or_insert(Weak::new());

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

    pub async fn scan(&self, address: [u8; 5]) -> Result<Vec<String>> {
        let channels = self.get_radio(0).await?.scan_async(
            Channel::from_number(0)?,
            Channel::from_number(125)?,
            address,
            vec![0xff],
        ).await?;

        let mut found = Vec::new();

        for channel in channels {
            let channel: u8 = channel.into();
            let address = hex::encode(address.to_vec()).to_uppercase();
            found.push(format!("radio://0/{}/2M/{}", channel, address));
        }

        Ok(found)
    }

    pub async fn scan_selected(&self, uris: Vec<&str>) -> Result<Vec<String>> {
        let mut found = Vec::new();
        for uri in uris {
            let (radio_nth, channel, address, _) = self.parse_uri(uri)?;
            let radio = self.get_radio(radio_nth).await?;
            let (ack, _) = radio.send_packet_async(channel, address, vec![0xFF, 0xFF, 0xFF]).await?;
            if ack.received {
                found.push(format!(
                    "{}{}",
                    uri, "?safelink=0&ackfilter=0"
                ));
            }
        }
        Ok(found)
    }

    pub async fn open_link(&self, uri: &str) -> Result<Connection> {
        let (radio_nth, channel, address, flags) = self.parse_uri(uri)?;

        let radio = self.get_radio(radio_nth).await?;

        Connection::new(self.executor.clone(), radio, channel, address, flags).await
    }
}
