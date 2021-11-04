#![allow(clippy::single_component_path_imports)]

//! # Crazyflie Link
//!
//! This Crate implement the Crazyflie radio link connection using Crazyradio.
//! It allows to scan for Crazyflies and to open a safe bidirectional radio connection using a Crazyradio.
//!
//! The entry point to this Crazte is the [LinkContext], it keeps track of Crazyradio dongles
//! and provides functions to open a link [Connection].
//!
//! Since this crate spawns async tasks, it needs to know about what async executor to use.
//! The [async_executors] crate is used to support the common async executors and should be
//! passed to the context constructor.
//!
//! A connection can then be used to send and receive packet with the Crazyflie.
//!
//! Example:
//!
//! ``` no_run
//! # use std::error::Error;
//! # async fn test() -> Result<(), Box<dyn Error>> {
//! // Create a link Context
//! let context = crazyflie_link::LinkContext::new(async_executors::AsyncStd);
//!
//! // Scan for Crazyflies
//! let cf_found = context.scan([0xe7; 5]).await?;
//!
//! if !cf_found.is_empty() {
//!     let connection = context.open_link(&cf_found[0]).await?;
//!     let packet = connection.recv_packet().await?;
//!     println!("Packet received: {:?}", packet);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [async_executors]: https://crates.io/crates/async_executors

#[macro_use]
extern crate bitflags;

mod connection;
mod context;
mod error;
mod packet;

#[cfg(all(feature = "native", feature = "webusb"))]
compile_error!("feature \"native\" and feature \"webusb\" cannot be enabled at the same time");

#[cfg(feature = "native")]
pub(crate) use crazyradio;
#[cfg(feature = "webusb")]
pub(crate) use crazyradio_webusb as crazyradio;

pub use connection::{Connection, ConnectionStatus};
pub use context::LinkContext;
pub use error::Error;
pub use packet::Packet;
