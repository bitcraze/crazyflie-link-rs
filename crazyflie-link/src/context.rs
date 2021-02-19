use crate::RadioThread;
use crate::error::{Result, Error};
use crate::Connection;
use crazyradio::Channel;
use lazy_static::lazy_static;
use regex::Regex;

pub struct LinkContext {
    radios: Vec<RadioThread>,
}

impl LinkContext {
    pub fn new() -> Result<Self> {
        let radio = crazyradio::Crazyradio::open_first()?;
        let radio = RadioThread::new(radio);

        Ok(Self {
            radios: vec![radio]
        })
    }

    pub fn scan(&self) -> Result<Vec<String>> {
        let channels = self.radios.first().unwrap().scan(Channel::from_number(0)?,
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

        return Connection::new(self.radios[radio_nth].clone(), channel, [0xe7;5]);
    }
}