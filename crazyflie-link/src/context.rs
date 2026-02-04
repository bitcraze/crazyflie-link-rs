//! # Link Context
//!
//! The Link context keeps track of the radio dongles opened and used by connections.
//! It also keeps track of the async executor

use crate::connection::Connection;
use crate::connection::ConnectionTrait;
use crate::crazyflie_usb_connection::CrazyflieUSBConnection;
use crate::crazyradio::{SharedCrazyradio, WeakSharedCrazyradio};
use crate::crazyradio_connection::CrazyradioConnection;
use crate::error::{Error, Result};
use futures_util::lock::Mutex;

use std::collections::BTreeMap;

/// Context for the link connections
pub struct LinkContext {
    radios: Mutex<BTreeMap<usize, WeakSharedCrazyradio>>,
}

impl LinkContext {
    /// Create a new link context
    pub fn new() -> Self {
        Self {
            radios: Mutex::new(BTreeMap::new()),
        }
    }

    /// Get SharedCrazyradio for the given radio index
    /// 
    /// If the radio is not already opened, a new instance of SharedCrazyradio for this radio is returned.
    /// Otherwise, the radio is opened and a SharedCrazyradio instance using it is returned.
    /// 
    /// Returns and error if the radio cannot be opened.
    pub async fn get_radio(&self, radio_nth: usize) -> Result<SharedCrazyradio> {
        let mut radios = self.radios.lock().await;

        radios.entry(radio_nth).or_insert_with(WeakSharedCrazyradio::default);

        let radio = match radios[&radio_nth].upgrade() {
            Some(radio) => radio,
            None => {
                let new_radio = crate::crazyradio::Crazyradio::open_nth_async(radio_nth).await?;
                let new_radio = SharedCrazyradio::new(new_radio);
                radios.insert(radio_nth, new_radio.downgrade());

                new_radio
            }
        };
        Ok(radio)
    }

    /// Scan for Crazyflies at some given address
    ///
    /// This function will send a packet to every channels and look for an acknowledgement in return.
    ///
    /// The address argument will set the radio packets address to scan for.
    ///
    /// It returns a list of URIs that can be passed to the [LinkContext::open_link()] function.
    pub async fn scan(&self, address: [u8; 5]) -> Result<Vec<String>> {
        let mut found = Vec::new();

        found.extend(CrazyradioConnection::scan(self, address).await?);
        found.extend(CrazyflieUSBConnection::scan().await?);

        Ok(found)
    }

    /// Scan for a given list of URIs
    ///
    /// Send a packet to each URI and detect if an acknowledgement is sent back.
    ///
    /// Returns the list of URIs that acknowledged
    pub async fn scan_selected(&self, uris: Vec<&str>) -> Result<Vec<String>> {
        let mut found = Vec::new();

        found.extend(CrazyradioConnection::scan_selected(self, uris.clone()).await?);
        found.extend(CrazyflieUSBConnection::scan_selected(uris).await?);

        Ok(found)
    }

    /// Open a link connection to a given URI
    ///
    /// If successful, the link [Connection] is returned.
    pub async fn open_link(&self, uri: &str) -> Result<Connection> {
        let connection: Option<Box<dyn ConnectionTrait + Send + Sync>> =
            if let Some(connection) = CrazyradioConnection::open(self, uri).await? {
                Some(Box::new(connection))
            } else if let Some(connection) = CrazyflieUSBConnection::open(self, uri).await? {
                Some(Box::new(connection))
            } else {
                None
            };

        let internal_connection = connection.ok_or(Error::InvalidUri)?;

        Ok(Connection::new(internal_connection))
    }
}
