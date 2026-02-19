use crate::error::Result;
use crate::Packet;
use async_trait::async_trait;

/// Describe the current link connection status
#[derive(Clone, Debug)]
pub enum ConnectionStatus {
    /// The link is connecting (ie. Safelink not enabled yet)
    Connecting,
    /// The link is connected and active (Safelink enabled and no timeout)
    Connected,
    /// The link is disconnected, the string contains the human-readable reason
    Disconnected(String),
}

/// Radio link statistics snapshot
///
/// Contains metrics about the radio link quality collected over the last measurement window.
/// Only available for radio connections (not USB).
#[derive(Clone, Debug, Default)]
pub struct RadioLinkStatistics {
    /// ACK success rate (0.0 to 1.0). Ratio of acknowledged packets to total packets sent.
    pub link_quality: f32,
    /// Data packets sent per second (excludes null/keepalive packets)
    pub uplink_rate: f32,
    /// Packets received per second (non-empty ACK payloads)
    pub downlink_rate: f32,
    /// Total radio packets sent per second (data + null/keepalive packets)
    pub radio_send_rate: f32,
    /// Average number of retries per acknowledged packet (0 = first attempt always succeeds)
    pub avg_retries: f32,
    /// Fraction of ACKs where the nRF24 power detector triggered (0.0 to 1.0)
    pub power_detector_rate: f32,
    /// Average uplink RSSI reported by the Crazyflie firmware. `None` if no RSSI data was received.
    pub uplink_rssi: Option<f32>,
}

// Describes the interface for a connection to a Crazyflie
#[async_trait]
pub trait ConnectionTrait {
    /// Wait for the connection to be closed. Returns the message stored in the
    /// disconnected connection status that indicate the reason for the disconnection
    async fn wait_close(&self) -> String;

    /// Close the connection and wait for the connection task to stop.
    ///
    /// The connection can also be closed by simply dropping the connection object.
    /// Though, if the connection task is currently processing a packet, it will continue running
    /// until the current packet has been processed. This function will wait for any ongoing packet
    /// to be processed and for the communication task to stop.
    async fn close(&self);

    /// Return the connection status
    async fn status(&self) -> ConnectionStatus;

    /// Block until the connection is dropped. The `status()` function can be used to get the reason
    /// for the disconnection.
    async fn wait_disconnect(&self);

    /// Send a packet to the connected Crazyflie
    ///
    /// This function can return an error if the connection task is not active anymore.
    /// This can happen if the Crazyflie is disconnected due to a timeout
    async fn send_packet(&self, packet: Packet) -> Result<()>;

    /// Receive a packet from the connected Crazyflie
    ///
    /// This function can return an error if the connection task is not active anymore.
    /// This can happen if the Crazyflie is disconnected due to a timeout
    async fn recv_packet(&self) -> Result<Packet>;

    /// Get radio link statistics
    ///
    /// Returns `Some(RadioLinkStatistics)` for radio connections with link quality,
    /// uplink/downlink rates. Returns `None` for USB connections where these metrics
    /// are not available.
    async fn link_statistics(&self) -> Option<RadioLinkStatistics>;
}

/// Connection to a Crazyflie
pub struct Connection {
    /// Reference to the internal connection object
    internal_connection: Box<dyn ConnectionTrait + Send + Sync>,
}

impl Connection {
    /// Create a new connection object
    pub fn new(internal_connection: Box<dyn ConnectionTrait + Send + Sync>) -> Self {
        Self {
            internal_connection,
        }
    }

    /// Wait for the connection to be closed. Returns the message stored in the
    /// disconnected connection status that indicate the reason for the disconnection
    pub async fn wait_close(&self) -> String {
        self.internal_connection.wait_close().await
    }

    /// Close the connection and wait for the connection task to stop.
    ///
    /// The connection can also be closed by simply dropping the connection object.
    /// Though, if the connection task is currently processing a packet, it will continue running
    /// until the current packet has been processed. This function will wait for any ongoing packet
    /// to be processed and for the communication task to stop.
    pub async fn close(&self) {
        self.internal_connection.close().await
    }

    /// Return the connection status
    pub async fn status(&self) -> ConnectionStatus {
        self.internal_connection.status().await
    }

    /// Block until the connection is dropped. The `status()` function can be used to get the reason
    /// for the disconnection.
    pub async fn wait_disconnect(&self) {
        self.internal_connection.wait_disconnect().await
    }

    /// Send a packet to the connected Crazyflie
    ///
    /// This function can return an error if the connection task is not active anymore.
    /// This can happen if the Crazyflie is disconnected due to a timeout
    pub async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.internal_connection.send_packet(packet).await
    }

    /// Receive a packet from the connected Crazyflie
    ///
    /// This function can return an error if the connection task is not active anymore.
    /// This can happen if the Crazyflie is disconnected due to a timeout
    pub async fn recv_packet(&self) -> Result<Packet> {
        self.internal_connection.recv_packet().await
    }

    /// Get radio link statistics
    ///
    /// Returns `Some(RadioLinkStatistics)` for radio connections with link quality,
    /// uplink/downlink rates. Returns `None` for USB connections where these metrics
    /// are not available.
    pub async fn link_statistics(&self) -> Option<RadioLinkStatistics> {
        self.internal_connection.link_statistics().await
    }
}
