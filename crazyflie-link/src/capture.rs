//! Packet capture support
//!
//! This module provides functionality to capture CRTP packets and send them
//! via Unix socket for analysis in Wireshark.
//!
//! Enable with the `packet_capture` feature flag and call [`init()`] at startup.
//!
//! To view captured packets in Wireshark, use the extcap plugin from
//! <https://github.com/evoggy/wireshark-crazyflie>.
//!
//! See the crate-level documentation for the capture format specification.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Unix socket path for connecting to the extcap
const SOCKET_PATH: &str = "/tmp/crazyflie-capture.sock";

/// Link type for radio connections
pub const LINK_TYPE_RADIO: u8 = 1;
/// Link type for USB connections
pub const LINK_TYPE_USB: u8 = 2;

/// Direction: transmit (to Crazyflie)
pub const DIRECTION_TX: u8 = 0;
/// Direction: receive (from Crazyflie)
pub const DIRECTION_RX: u8 = 1;

/// Global capture socket (lazily initialized)
static CAPTURE_SOCKET: OnceLock<Mutex<UnixStream>> = OnceLock::new();

/// Initialize the capture connection
///
/// Attempts to connect to the Unix socket. If the socket is not available the
/// capture is silently disabled.
///
/// This also registers a callback with the crazyradio crate to capture radio packets.
pub fn init() {
    if let Ok(stream) = UnixStream::connect(SOCKET_PATH) {
        log::info!("Packet capture: connected to {}", SOCKET_PATH);
        let _ = CAPTURE_SOCKET.set(Mutex::new(stream));

        // Only register callback if connected
        crazyradio::capture::set_callback(Box::new(|event| {
            send_packet(
                LINK_TYPE_RADIO,
                event.direction,
                event.address,
                event.channel,
                event.serial,
                event.data,
            );
        }));
    } else {
        log::debug!("Packet capture: not available");
    }
}

/// Check if capture is available
pub fn is_available() -> bool {
    CAPTURE_SOCKET.get().is_some()
}

/// Send a captured packet to Wireshark
///
/// # Arguments
/// * `link_type` - LINK_TYPE_RADIO or LINK_TYPE_USB
/// * `direction` - DIRECTION_TX or DIRECTION_RX
/// * `address` - Device address (5 bytes for radio, up to 12 for USB)
/// * `channel` - Radio channel (0 for USB)
/// * `serial` - Device/radio serial number (up to 16 bytes)
/// * `data` - CRTP packet data (header + payload)
pub fn send_packet(
    link_type: u8,
    direction: u8,
    address: &[u8],
    channel: u8,
    serial: &str,
    data: &[u8],
) {
    let Some(socket) = CAPTURE_SOCKET.get() else { return };
    let Ok(mut stream) = socket.lock() else { return };

    // Get timestamp in microseconds
    let timestamp_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);

    // Build packet header:
    // | link_type(1) | direction(1) | address(12) | channel(1) | serial(16) | timestamp(8) | len(2) |
    let mut header = [0u8; 41];
    header[0] = link_type;
    header[1] = direction;

    // Copy address (pad to 12 bytes)
    let addr_len = address.len().min(12);
    header[2..2 + addr_len].copy_from_slice(&address[..addr_len]);

    header[14] = channel;

    // Copy serial (pad to 16 bytes)
    let serial_bytes = serial.as_bytes();
    let serial_len = serial_bytes.len().min(16);
    header[15..15 + serial_len].copy_from_slice(&serial_bytes[..serial_len]);

    header[31..39].copy_from_slice(&timestamp_us.to_le_bytes());
    header[39..41].copy_from_slice(&(data.len() as u16).to_le_bytes());

    // Write header and data, ignore errors
    let _ = stream.write_all(&header);
    let _ = stream.write_all(data);
}
