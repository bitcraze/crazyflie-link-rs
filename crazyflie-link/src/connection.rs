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

#[async_trait]
pub trait ConnectionTrait {

 async fn wait_close(&self) -> String;

 async fn close(&self);

 async fn status(&self) -> ConnectionStatus;

 async fn wait_disconnect(&self);

 async fn send_packet(&self, packet: Packet) -> Result<()>;

 async fn recv_packet(&self) -> Result<Packet>;

}

pub struct Connection {
    internal_connection: Box<dyn ConnectionTrait + Send + Sync>
}

impl Connection {
    pub fn new(internal_connection: Box<dyn ConnectionTrait + Send + Sync>) -> Self {
        Self {
            internal_connection
        }
    }

    pub async fn wait_close(&self) -> String {
        self.internal_connection.wait_close().await
    }

    pub async fn close(&self) {
        self.internal_connection.close().await
    }

    pub async fn status(&self) -> ConnectionStatus {
        self.internal_connection.status().await
    }

    pub async fn wait_disconnect(&self) {
        self.internal_connection.wait_disconnect().await
    }

    pub async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.internal_connection.send_packet(packet).await
    }

    pub async fn recv_packet(&self) -> Result<Packet> {
        self.internal_connection.recv_packet().await
    }
}