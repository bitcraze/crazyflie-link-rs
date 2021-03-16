use crate::error::{Error, Result};
use crate::Connection;
use crate::RadioThread;
use crazyradio::Channel;
use hex;
use std::sync::{Arc, Mutex, Weak};
use url::Url;

pub struct LinkContext {
    radios: Vec<Mutex<Weak<RadioThread>>>,
}

impl LinkContext {
    pub fn new() -> Self {
        Self {
            radios: vec![Mutex::new(Weak::new())],
        }
    }

    fn get_radio(&self, radio_nth: usize) -> Result<Arc<RadioThread>> {
        let mut radio = self.radios[radio_nth].lock().unwrap();
        let radio = match Weak::upgrade(&radio) {
            Some(radio) => radio,
            None => {
                let new_radio = crazyradio::Crazyradio::open_nth(radio_nth)?;
                let new_radio = Arc::new(RadioThread::new(new_radio));
                *radio = Arc::downgrade(&new_radio);
                new_radio
            }
        };
        Ok(radio)
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
        let uri = Url::parse(uri)?;

        if uri.scheme() != "radio" {
            return Err(Error::InvalidUri);
        }

        let radio_nth: usize = uri.domain().ok_or(Error::InvalidUri)?.parse()?;

        let path: Vec<&str> = uri.path_segments().ok_or(Error::InvalidUri)?.collect();
        if path.len() != 3 {
            return Err(Error::InvalidUri);
        }
        let channel = Channel::from_number(path[0].parse()?)?;
        let _datarate = path[1];
        let address_str = path[2];

        if address_str.len() != 10 {
            return Err(Error::InvalidUri);
        }
        let mut address = [0u8; 5];
        for i in 0..5 {
            address[i] = u8::from_str_radix(&address_str[2*i..(2*i+2)], 16)?;
        }

        if radio_nth >= self.radios.len() {
            return Err(Error::InvalidUri);
        }

        let radio = self.get_radio(radio_nth)?;

        return Connection::new(radio, channel, address);
    }
}
