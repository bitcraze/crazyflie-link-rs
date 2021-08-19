// Connection handling code
use crate::crazyradio::{Channel, SharedCrazyradio};
use crate::error::Result;
use crate::Packet;
use async_executors::{JoinHandle, LocalSpawnHandleExt};
use async_executors::LocalSpawnHandle;
use futures_channel::oneshot;
use futures_util::lock::Mutex;
use log::{debug, info, warn};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use std::time;

#[cfg(feature = "native")]
use std::time::Instant;
#[cfg(feature = "webusb")]
use wasm_timer::Instant;

const EMPTY_PACKET_BEFORE_RELAX: u32 = 10;

bitflags! {
    pub struct ConnectionFlags: u32 {
        const SAFELINK = 0b00000001;
        const ACKFILTER = 0b00000010;
    }
}

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

pub struct Connection {
    status: Arc<Mutex<ConnectionStatus>>,
    uplink: flume::Sender<Vec<u8>>,
    downlink: flume::Receiver<Vec<u8>>,
    disconnect_channel: flume::Receiver<()>,
    disconnect: Arc<AtomicBool>,
    _thread_handle: JoinHandle<()>,
}

impl Connection {
    pub(crate) async fn new(
        executor: Arc<dyn LocalSpawnHandle<()> + Sync + Send>,
        radio: Arc<SharedCrazyradio>,
        channel: Channel,
        address: [u8; 5],
        flags: ConnectionFlags,
    ) -> Result<Connection> {
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
        let thread_handle = executor
            .spawn_handle_local(async move {
                if let Err(e) = thread.run(connection_initialized_send).await {
                    thread
                        .update_status(ConnectionStatus::Disconnected(format!(
                            "Connection error: {}",
                            e
                        )))
                        .await;
                }
                drop(thread.disconnect_channel);
            })
            .expect("Spawning connection task");

        // Wait for, either, the connection being established or failed initialization
        connection_initialized.await.unwrap();

        Ok(Connection {
            status,
            disconnect_channel: disconnect_channel_rx,
            uplink: uplink_send,
            downlink: downlink_recv,
            disconnect,
            _thread_handle: thread_handle,
        })
    }

    /// Close the connection and wait for the connection task to stop.
    ///
    /// The connection can also be closed by simply dropping the connection object.
    /// Though, if the connection task is currently processing a packet, it will continue running
    /// until the current packet has been processed. This function will wait for any ongoing packet
    /// to be processed and for the communication task to stop.
    pub async fn close(&self) {
        self.disconnect.store(true, Relaxed);
        let _ = self.disconnect_channel.recv_async().await;
    }

    /// Return the connection status
    pub async fn status(&self) -> ConnectionStatus {
        self.status.lock().await.clone()
    }

    /// Block until the connection is dropped. The `status()` function can be used to get the reason
    /// for the disconnection.
    pub async fn wait_disconnect(&self) {
        // The channel will return an error when the other side, in the connection thread, is dropped
        let _ = self.disconnect_channel.recv_async().await;
    }

    /// Send a packet to the connected Crazyflie
    ///
    /// This fundtion can return an error if the connection task is not active anymore.
    /// This can happen if the Crazyflie is disconnected due to a timeout
    pub async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.uplink.send_async(packet.into()).await?;
        Ok(())
    }

    /// Receive a packet from the connected Crazyflie
    ///
    /// This fundtion can return an error if the connection task is not active anymore.
    /// This can happen if the Crazyflie is disconnected due to a timeout
    pub async fn recv_packet(&self) -> Result<Packet> {
        let packet = self.downlink.recv_async().await?;
        Ok(packet.into())
    }
}

impl Drop for Connection {
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
                debug!("Canary dead, leaving connection loop.");
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
