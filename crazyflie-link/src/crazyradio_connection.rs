use crate::connection::ConnectionTrait;
// Connection handling code
use crate::crazyradio::{Channel, SharedCrazyradio};
use crate::error::{Error, Result};
use crate::{ConnectionStatus, LinkContext, Packet};
use async_trait::async_trait;
use futures_channel::oneshot;
use futures_util::lock::Mutex;
use hex::FromHex;
use log::{debug, info, warn};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use url::Url;

use std::time;

use std::time::Instant;

const EMPTY_PACKET_BEFORE_RELAX: u32 = 10;

bitflags! {
    pub struct ConnectionFlags: u32 {
        const SAFELINK = 0b00000001;
        const ACKFILTER = 0b00000010;
    }
}

/// Link connection
pub struct CrazyradioConnection {
    status: Arc<Mutex<ConnectionStatus>>,
    uplink: flume::Sender<Vec<u8>>,
    downlink: flume::Receiver<Vec<u8>>,
    disconnect_channel: flume::Receiver<()>,
    disconnect: Arc<AtomicBool>,
}

impl CrazyradioConnection {
    pub async fn open(
        link_context: &LinkContext,
        uri: &str,
    ) -> Result<Option<CrazyradioConnection>> {
        let (radio, channel, address, flags) = match Self::parse_uri(uri) {
            Ok((radio, channel, address, flags)) => (radio, channel, address, flags),
            Err(Error::InvalidUriScheme) => {
                return Ok(None);
            }
            Err(e) => {
                return Err(e);
            }
        };

        let radio = link_context.get_radio(radio).await?;
        let connection = CrazyradioConnection::new(radio, channel, address, flags).await?;

        Ok(Some(connection))
    }

    async fn new(
        radio: Arc<SharedCrazyradio>,
        channel: Channel,
        address: [u8; 5],
        flags: ConnectionFlags,
    ) -> Result<CrazyradioConnection> {
        let status = Arc::new(Mutex::new(ConnectionStatus::Connecting));

        let (disconnect_channel_tx, disconnect_channel_rx) = flume::bounded(0);
        let disconnect = Arc::new(AtomicBool::new(false));

        let (uplink_send, uplink_recv) = flume::bounded(1000);
        let (downlink_send, downlink_recv) = flume::bounded(1000);

        let (connection_initialized_send, connection_initialized) = oneshot::channel();

        let mut thread = ConnectionThread {
            radio,
            status: status.clone(),
            disconnect_channel: disconnect_channel_tx,
            safelink_down_ctr: 0,
            safelink_up_ctr: 0,
            uplink: uplink_recv,
            downlink: downlink_send,
            channel,
            address,
            flags,
            disconnect: disconnect.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = thread.run(connection_initialized_send).await {
                thread
                    .update_status(ConnectionStatus::Disconnected(format!(
                        "Connection error: {}",
                        e
                    )))
                    .await;
            }
            drop(thread.disconnect_channel);
        });

        // Wait for, either, the connection being established or failed initialization
        connection_initialized.await.unwrap();

        Ok(CrazyradioConnection {
            status,
            disconnect_channel: disconnect_channel_rx,
            uplink: uplink_send,
            downlink: downlink_recv,
            disconnect,
        })
    }

    pub(crate) async fn scan(link_context: &LinkContext, address: [u8; 5]) -> Result<Vec<String>> {
        let channels = match link_context.get_radio(0).await {
            Ok(radio) => {
                radio
                    .scan_async(
                        Channel::from_number(0)?,
                        Channel::from_number(125)?,
                        address,
                        vec![0xff],
                    )
                    .await?
            }
            Err(_) => Vec::new(),
        };

        Ok(channels
            .iter()
            .map(|channel| {
                let channel: u8 = (*channel).into();
                let address = hex::encode(address.to_vec()).to_uppercase();
                format!("radio://0/{}/2M/{}", channel, address)
            })
            .collect())
    }

    pub(crate) async fn scan_selected(
        link_context: &LinkContext,
        uris: Vec<&str>,
    ) -> Result<Vec<String>> {
        let mut found = Vec::new();
        for uri in uris {
            let (radio_nth, channel, address, _) = Self::parse_uri(uri)?;
            let radio = link_context.get_radio(radio_nth).await?;
            let (ack, _) = radio
                .send_packet_async(channel, address, vec![0xFF, 0xFF, 0xFF])
                .await?;
            if ack.received {
                found.push(format!("{}{}", uri, "?safelink=0&ackfilter=0"));
            }
        }
        Ok(found)
    }

    fn parse_uri(uri: &str) -> Result<(usize, Channel, [u8; 5], ConnectionFlags)> {
        let uri = Url::parse(uri)?;

        if uri.scheme() != "radio" {
            return Err(Error::InvalidUriScheme);
        }

        let radio: usize = uri.domain().ok_or(Error::InvalidUri)?.parse()?;

        let path: Vec<&str> = uri.path_segments().ok_or(Error::InvalidUri)?.collect();
        if path.len() != 3 {
            return Err(Error::InvalidUri);
        }
        let channel = Channel::from_number(path[0].parse()?)?;
        let _rate = path[1];
        let addr_str = path[2];

        if addr_str.len() > 10 {
            return Err(Error::InvalidUri);
        }

        let address = match <[u8; 5]>::from_hex(format!("{:0>10}", addr_str)) {
            Ok(address) => address,
            Err(_) => return Err(Error::InvalidUri),
        };

        let mut flags = ConnectionFlags::SAFELINK | ConnectionFlags::ACKFILTER;
        for (key, value) in uri.query_pairs() {
            match key.as_ref() {
                "safelink" => {
                    if value == "0" {
                        flags.remove(ConnectionFlags::SAFELINK);
                    }
                }
                "ackfilter" => {
                    if value == "0" {
                        flags.remove(ConnectionFlags::ACKFILTER);
                    }
                }
                _ => continue,
            };
        }

        Ok((radio, channel, address, flags))
    }
}

#[async_trait]
impl ConnectionTrait for CrazyradioConnection {
    async fn wait_close(&self) -> String {
        // Wait for the connection thread to drop the disconnect channel
        let _ = self.disconnect_channel.recv_async().await;
        if let ConnectionStatus::Disconnected(reason) = self.status().await {
            reason
        } else {
            "Still connected!".to_owned()
        }
    }

    async fn close(&self) {
        self.disconnect.store(true, Relaxed);
        let _ = self.disconnect_channel.recv_async().await;
    }

    /// Return the connection status
    async fn status(&self) -> ConnectionStatus {
        self.status.lock().await.clone()
    }

    async fn wait_disconnect(&self) {
        // The channel will return an error when the other side, in the connection thread, is dropped
        let _ = self.disconnect_channel.recv_async().await;
    }

    async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.uplink.send_async(packet.into()).await?;
        Ok(())
    }

    async fn recv_packet(&self) -> Result<Packet> {
        let packet = self.downlink.recv_async().await?;
        Ok(packet.into())
    }
}

impl Drop for CrazyradioConnection {
    fn drop(&mut self) {
        self.disconnect.store(true, Relaxed);
    }
}

struct ConnectionThread {
    radio: Arc<SharedCrazyradio>,
    status: Arc<Mutex<ConnectionStatus>>,
    disconnect_channel: flume::Sender<()>,
    safelink_up_ctr: u8,
    safelink_down_ctr: u8,
    uplink: flume::Receiver<Vec<u8>>,
    downlink: flume::Sender<Vec<u8>>,
    channel: Channel,
    address: [u8; 5],
    flags: ConnectionFlags,
    disconnect: Arc<AtomicBool>,
}

impl ConnectionThread {
    async fn update_status(&self, new_status: ConnectionStatus) {
        debug!("New status: {:?}", &new_status);
        let mut status = self.status.lock().await;
        *status = new_status;
    }

    async fn enable_safelink(&mut self) -> Result<bool> {
        // Tying 10 times to reset safelink
        for _ in 0..10 {
            let (ack, payload) = self
                .radio
                .send_packet_async(self.channel, self.address, vec![0xff, 0x05, 0x01])
                .await?;

            if ack.received && payload == [0xff, 0x05, 0x01] {
                self.safelink_down_ctr = 0;
                self.safelink_up_ctr = 0;

                // Safelink enabled!
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn send_packet_safe(
        &mut self,
        packet: Vec<u8>,
    ) -> Result<(crate::crazyradio::Ack, Vec<u8>)> {
        let mut packet = packet;
        packet[0] &= 0xF3;
        packet[0] |= (self.safelink_up_ctr << 3) | (self.safelink_down_ctr << 2);

        let (ack, mut ack_payload) = self
            .radio
            .send_packet_async(self.channel, self.address, packet)
            .await?;

        if ack.received {
            self.safelink_up_ctr = 1 - self.safelink_up_ctr;
        }

        if ack.received
            && !ack_payload.is_empty()
            && (ack_payload[0] & 0x04) >> 2 == self.safelink_down_ctr
        {
            self.safelink_down_ctr = 1 - self.safelink_down_ctr;
        } else {
            // If the down counter does not match, this is a reapeted ack and the payload needs to be dropped
            ack_payload.clear();
        }

        Ok((ack, ack_payload))
    }

    async fn send_packet(
        &mut self,
        packet: Vec<u8>,
        safe: bool,
    ) -> Result<(crate::crazyradio::Ack, Vec<u8>)> {
        let result = if safe {
            self.send_packet_safe(packet).await?
        } else {
            self.radio
                .send_packet_async(self.channel, self.address, packet)
                .await?
        };

        Ok(result)
    }

    fn use_safelink(&self) -> bool {
        self.flags.contains(ConnectionFlags::SAFELINK)
    }

    //
    // In order to know whether or not to handle and send this ack back to
    // the receiver we need to check some stuff.
    //
    // In safelink we deem a payload empty if it only containx 0xF3, and we do
    // not handle it.
    //
    // When not using safelink (perhaps communicating with bootloader) we check
    // if the ackfilter flag is not set. If we have no ack filter then we
    // handle payload-less acks as well.
    //
    fn preprocess_ack(&self, payload: &mut Vec<u8>, safelink: bool) -> bool {
        if safelink {
            if !payload.is_empty() && payload[0] & 0xF3 != 0xF3 {
                payload[0] &= 0xF3;
                return true;
            }
            return false;
        }

        // not safelink

        if !payload.is_empty() {
            return true;
        }

        if !self.flags.contains(ConnectionFlags::ACKFILTER) {
            *payload = vec![0xFF];
            return true;
        }

        false
    }

    async fn run(&mut self, connection_initialized: oneshot::Sender<()>) -> Result<()> {
        info!("Connecting to {:?}/{:X?} ...", self.channel, self.address);

        // Try to initialize safelink if in use
        let safelink = self.use_safelink() && self.enable_safelink().await?;

        self.update_status(ConnectionStatus::Connected).await;
        connection_initialized.send(()).unwrap();

        // Communication loop ...
        let mut last_pk_time = Instant::now();
        let mut n_empty_packets = 0;
        let mut packet = vec![0xff];
        let mut needs_resend = false;
        while last_pk_time.elapsed() < time::Duration::from_millis(1000) {
            if !needs_resend {
                packet = if self.uplink.is_empty() {
                    vec![0xff]
                } else {
                    self.uplink.recv_async().await?
                }
            }

            let (ack, mut ack_payload) = self.send_packet(packet.clone(), safelink).await?;

            debug!("Packet sent!");

            if ack.received {
                last_pk_time = Instant::now();
                needs_resend = false;

                // Add some relaxation time if the Crazyflie has nothing to send back
                // for a while

                let should_handle = self.preprocess_ack(&mut ack_payload, safelink);
                if should_handle {
                    self.downlink.send(ack_payload)?;
                } else if n_empty_packets > EMPTY_PACKET_BEFORE_RELAX {
                    // If no packet received for a while, relax packet pulling
                } else {
                    n_empty_packets += 1;
                }
            } else {
                debug!("Lost packet!");
                needs_resend = true;
            }

            // If the connection object has been dropped, leave the thread
            if self.disconnect.load(Relaxed) {
                debug!("Disconnect requested, leaving connection loop.");
                self.update_status(ConnectionStatus::Disconnected(
                    "Connection closed".to_owned(),
                ))
                .await;
                return Ok(());
            }
        }

        self.update_status(ConnectionStatus::Disconnected(
            "Connection timeout".to_string(),
        ))
        .await;

        warn!(
            "Connection to {:?}/{:X?} lost (timeout)",
            self.channel, self.address
        );

        Ok(())
    }
}
