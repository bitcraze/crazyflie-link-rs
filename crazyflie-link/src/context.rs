use crate::error::{Error, Result};
use crate::Connection;
use crate::RadioThread;
use crazyradio::Channel;
use std::collections::BTreeMap;
use hex::FromHex;
use std::sync::{Arc, Mutex, Weak};
use url::Url;

pub struct LinkContext {
    radios: Mutex<BTreeMap<usize, Weak<RadioThread>>>,
}

impl Default for LinkContext {
    fn default() -> Self {
        LinkContext::new()
    }
}

impl LinkContext {
    pub fn new() -> Self {
        Self {
            radios: Mutex::new(BTreeMap::new()),
        }
    }

    fn get_radio(&self, radio_nth: usize) -> Result<Arc<RadioThread>> {
        let mut radios = self.radios.lock().unwrap();

        radios.entry(radio_nth).or_insert(Weak::new());

        let radio = match Weak::upgrade(&radios[&radio_nth]) {
            Some(radio) => radio,
            None => {
                let new_radio = crazyradio::Crazyradio::open_nth(radio_nth)?;
                let new_radio = Arc::new(RadioThread::new(new_radio));
                radios.insert(radio_nth, Arc::downgrade(&new_radio));

                new_radio
            }
        };
        Ok(radio)
    }

    fn parse_uri(&self, uri: &str) -> Result<(usize, Channel, [u8; 5])> {
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

        Ok((radio, channel, address))
    }

    pub fn scan(&self, address: [u8; 5]) -> Result<Vec<String>> {
        let channels = self.get_radio(0)?.scan(
            Channel::from_number(0)?,
            Channel::from_number(125)?,
            address,
            vec![0xff],
        )?;

        let mut found = Vec::new();

        for channel in channels {
            let channel: u8 = channel.into();
            let address = hex::encode(address.to_vec()).to_uppercase();
            found.push(format!("radio://0/{}/2M/{}", channel, address));
        }

        Ok(found)
    }

    pub fn open_link(&self, uri: &str) -> Result<Connection> {
        let (radio_nth, channel, address) = self.parse_uri(uri)?;

        let radio = self.get_radio(radio_nth)?;

        Connection::new(radio, channel, address)
    }
}
