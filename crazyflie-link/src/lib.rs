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
//! ## Cargo features
//!
//! - **packet_capture** - Enable packet capture via Unix socket (Unix only)
//!
//! ## Packet Capture
//!
//! When the `packet_capture` feature is enabled, CRTP packets can be captured and sent
//! to an external application for analysis. Call [`capture::init()`] at startup to
//! connect to the capture socket.
//!
//! The capture uses a Unix socket at `/tmp/crazyflie-capture.sock`. To view packets in
//! Wireshark, use the extcap plugin from <https://github.com/evoggy/wireshark-crazyflie>.
//!
//! ### Capture Format
//!
//! Each captured packet is sent with a 41-byte header followed by the CRTP packet data:
//!
//! | Offset | Size | Field       | Description                              |
//! |--------|------|-------------|------------------------------------------|
//! | 0      | 1    | link_type   | 1 = Radio, 2 = USB                       |
//! | 1      | 1    | direction   | 0 = TX (to Crazyflie), 1 = RX (from CF)  |
//! | 2      | 12   | address     | Radio address (5 bytes) or empty for USB |
//! | 14     | 1    | channel     | Radio channel (0 for USB)                |
//! | 15     | 16   | serial      | Radio/device serial number               |
//! | 31     | 8    | timestamp   | Microseconds since Unix epoch (LE)       |
//! | 39     | 2    | length      | CRTP packet length (LE)                  |
//! | 41     | N    | data        | CRTP packet (header + payload)           |

#[macro_use]
extern crate bitflags;

mod connection;
mod context;
mod crazyflie_usb_connection;
mod crazyradio_connection;
mod error;
mod packet;

#[cfg(feature = "packet_capture")]
pub mod capture;

pub(crate) use crazyradio;

pub use connection::{Connection, ConnectionStatus, RadioLinkStatistics};
pub use context::LinkContext;
pub use error::Error;
pub use packet::Packet;
pub use crazyradio::SharedCrazyradio;
