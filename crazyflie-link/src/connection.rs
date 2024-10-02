use crate::Packet;
use crate::error::Result;
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

}

/// Connection to a Crazyflie
pub struct Connection {
    /// Reference to the internal connection object 
    internal_connection: Box<dyn ConnectionTrait + Send + Sync>
}

impl Connection {
    /// Create a new connection object
    pub fn new(internal_connection: Box<dyn ConnectionTrait + Send + Sync>) -> Self {
        Self {
            internal_connection
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
}