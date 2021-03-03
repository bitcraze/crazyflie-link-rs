use crate::RadioThread;
use crate::error::{Result, Error};
use crate::Connection;
use crazyradio::Channel;
use lazy_static::lazy_static;
use regex::Regex;
use std::sync::{Arc, Weak, Mutex};

pub struct LinkContext {
    radios: Vec<Mutex<Weak<RadioThread>>>,
}

impl LinkContext {
    pub fn new() -> Self {
        Self {
            radios: vec![Mutex::new(Weak::new())]
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

    pub fn scan(&self) -> Result<Vec<String>> {
        let channels = self.get_radio(0)?.scan(Channel::from_number(0)?,
                                                                     Channel::from_number(125)?,
                                                                     [0xe7;5],
                                                                     vec![0xff])?;
        
        let mut found = Vec::new();

        for channel in channels {
            let channel: u8 = channel.into();
            found.push(format!("radio://0/{}/2M/E7E7E7E7E7", channel));
        }
        
        Ok(found)
    }

    pub fn open_link(&self, uri: &str) -> Result<Connection> {
        lazy_static! {
            static ref URI_REGEX: Regex = Regex::new(r"^radio://(\d+)/(\d+)/2M/([0-9a-hA-H]{10})$").unwrap();
        }

        let matches: regex::Captures = URI_REGEX.captures(uri).ok_or(Error::InvalidUri)?;

        let radio_nth: usize = matches[1].parse()?;
        let channel = Channel::from_number(matches[2].parse()?)?;
        
        if radio_nth >= self.radios.len() {
            return Err(Error::InvalidUri);
        }

        let radio = self.get_radio(radio_nth)?;

        return Connection::new(radio, channel, [0xe7;5]);
    }
}