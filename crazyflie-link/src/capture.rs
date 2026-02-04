//! Wireshark packet capture support
//!
//! This module provides functionality to capture CRTP packets and send them
//! to a Wireshark extcap application via Unix socket.
//!
//! Enable with the `wireshark` feature flag.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Unix socket path for connecting to the extcap
const SOCKET_PATH: &str = "/tmp/crazyflie-wireshark.sock";

/// Link type for radio connections
pub const LINK_TYPE_RADIO: u8 = 1;
/// Link type for USB connections
pub const LINK_TYPE_USB: u8 = 2;

/// Direction: transmit (to Crazyflie)
pub const DIRECTION_TX: u8 = 0;
/// Direction: receive (from Crazyflie)
pub const DIRECTION_RX: u8 = 1;

/// Global capture socket (lazily initialized)
static CAPTURE_SOCKET: OnceLock<Mutex<Option<UnixStream>>> = OnceLock::new();

/// Initialize the capture connection
///
/// Attempts to connect to the Wireshark extcap Unix socket.
/// If the socket is not available (extcap not running), capture is silently disabled.
///
/// This also registers a callback with the crazyradio crate to capture radio packets.
pub fn init() {
    let socket = CAPTURE_SOCKET.get_or_init(|| {
        match UnixStream::connect(SOCKET_PATH) {
            Ok(stream) => {
                log::info!("Wireshark capture: connected to {}", SOCKET_PATH);
                Mutex::new(Some(stream))
            }
            Err(e) => {
                log::debug!("Wireshark capture: not available ({})", e);
                Mutex::new(None)
            }
        }
    });

    // If already initialized but disconnected, try to reconnect
    if let Ok(mut guard) = socket.lock() {
        if guard.is_none() {
            if let Ok(stream) = UnixStream::connect(SOCKET_PATH) {
                log::info!("Wireshark capture: reconnected to {}", SOCKET_PATH);
                *guard = Some(stream);
            }
        }
    }

    // Register callback with crazyradio crate
    crazyradio::capture::set_callback(Box::new(|direction, channel, address, radio_index, data| {
        send_packet(
            LINK_TYPE_RADIO,
            direction,
            address,
            channel,
            radio_index,
            data,
        );
    }));
}

/// Check if capture is available
pub fn is_available() -> bool {
    if let Some(socket) = CAPTURE_SOCKET.get() {
        if let Ok(guard) = socket.lock() {
            return guard.is_some();
        }
    }
    false
}

/// Send a captured packet to Wireshark
///
/// # Arguments
/// * `link_type` - LINK_TYPE_RADIO or LINK_TYPE_USB
/// * `direction` - DIRECTION_TX or DIRECTION_RX
/// * `address` - Device address (5 bytes for radio, up to 12 for USB)
/// * `channel` - Radio channel (0 for USB)
/// * `devid` - Device/radio index
/// * `data` - CRTP packet data (header + payload)
pub fn send_packet(
    link_type: u8,
    direction: u8,
    address: &[u8],
    channel: u8,
    devid: u8,
    data: &[u8],
) {
    let socket = match CAPTURE_SOCKET.get() {
        Some(s) => s,
        None => return,
    };

    let mut guard = match socket.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let stream = match guard.as_mut() {
        Some(s) => s,
        None => return,
    };

    // Get timestamp in microseconds
    let timestamp_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);

    // Build packet header:
    // | link_type(1) | direction(1) | address(12) | channel(1) | devid(1) | timestamp(8) | len(2) |
    let mut header = [0u8; 26];
    header[0] = link_type;
    header[1] = direction;

    // Copy address (pad to 12 bytes)
    let addr_len = address.len().min(12);
    header[2..2 + addr_len].copy_from_slice(&address[..addr_len]);

    header[14] = channel;
    header[15] = devid;
    header[16..24].copy_from_slice(&timestamp_us.to_le_bytes());
    header[24..26].copy_from_slice(&(data.len() as u16).to_le_bytes());

    // Write header and data
    if stream.write_all(&header).is_err() || stream.write_all(data).is_err() {
        // Connection lost, clear the socket
        log::debug!("Wireshark capture: connection lost");
        *guard = None;
    }
}
