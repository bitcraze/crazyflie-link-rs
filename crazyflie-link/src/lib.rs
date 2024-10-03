#![allow(clippy::single_component_path_imports)]

//! # Crazyflie Link
//!
//! This Crate implement the Crazyflie radio link connection using Crazyradio.
//! It allows to scan for Crazyflies and to open a safe bidirectional radio connection using a Crazyradio.
//!
//! The entry point to this Crate is the [LinkContext], it keeps track of Crazyradio dongles
//! and provides functions to open a link [Connection].
//!
//! A connection can then be used to send and receive packet with the Crazyflie.
//!
//! Example:
//!
//! ``` no_run
//! # use std::error::Error;
//! # async fn test() -> Result<(), Box<dyn Error>> {
//! // Create a link Context
//! let context = crazyflie_link::LinkContext::new();
//!
//! // Scan for Crazyflies
//! let cf_found = context.scan([0xe7; 5]).await?;
//!
//! if let Some(uri) = cf_found.first() {
//!     let connection = context.open_link(uri).await?;
//!     let packet = connection.recv_packet().await?;
//!     println!("Packet received: {:?}", packet);
//! }
//! # Ok(())
//! # }
//! ```
//!

#[macro_use]
extern crate bitflags;

mod connection;
mod context;
mod crazyflie_usb_connection;
mod crazyradio_connection;
mod error;
mod packet;

pub(crate) use crazyradio;

pub use connection::{Connection, ConnectionStatus};
pub use context::LinkContext;
pub use error::Error;
pub use packet::Packet;
