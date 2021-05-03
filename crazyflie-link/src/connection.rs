// Connection handling code
use crate::error::{Error, Result};
use crate::Packet;
use crazyradio::{SharedCrazyradio, Channel};
use log::{debug, info, warn};
use std::sync::{Arc, Weak};
use std::time;
use std::time::Duration;
use async_executors::{SpawnHandle, JoinHandle, SpawnHandleExt};
use futures_util::lock::Mutex;
use futures::channel::oneshot;

const EMPTY_PACKET_BEFORE_RELAX: u32 = 10;
const RELAX_DELAY: Duration = Duration::from_millis(0);

bitflags! {
    pub struct ConnectionFlags: u32 {
        const SAFELINK = 0b00000001;
        const ACKFILTER = 0b00000010;
    }
}

#[derive(Clone, Debug)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected(String),
}

pub struct Connection {
    status: Arc<Mutex<ConnectionStatus>>,
    uplink: flume::Sender<Vec<u8>>,
    downlink: flume::Receiver<Vec<u8>>,
    disconnect_canary: Arc<()>,
    thread_handle: JoinHandle<()>,
}

impl Connection {
    pub async fn new(
        executor: Arc<dyn SpawnHandle<()> + Sync + Send>,
        radio: Arc<SharedCrazyradio>,
        channel: Channel,
        address: [u8; 5],
        flags: ConnectionFlags,
    ) -> Result<Connection> {
        let status = Arc::new(Mutex::new(ConnectionStatus::Connecting));

        let disconnect_canary = Arc::new(());

        let (uplink_send, uplink_recv) = flume::unbounded();
        let (downlink_send, downlink_recv) = flume::unbounded();

        let (connection_initialized_send, connection_initialized) = oneshot::channel();

        let mut thread = ConnectionThread::new(
            radio,
            status.clone(),
            Arc::downgrade(&disconnect_canary),
            uplink_recv,
            downlink_send,
            channel,
            address,
            flags,
        );
        let thread_handle = executor.spawn_handle (async move {
            if let Err(e) = thread.run(connection_initialized_send).await {
                thread.update_status(ConnectionStatus::Disconnected(format!(
                    "Connection error: {}",
                    e
                ))).await;
            }
        }).expect("Spawning connection task");

        // Wait for, either, the connection being established or failed initialization
        connection_initialized.await.unwrap();

        Ok(Connection {
            status,
            disconnect_canary,
            uplink: uplink_send,
            downlink: downlink_recv,
            thread_handle,
        })
    }

    pub async fn close(self) {
        drop(self.disconnect_canary);
        self.thread_handle.await;
    }

    pub async fn status(&self) -> ConnectionStatus {
        self.status.lock().await.clone()
    }

    pub async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.uplink.send_async(packet.into()).await?;
        Ok(())
    }

    pub async fn recv_packet(&self) -> Result<Packet> {
        let packet = self.downlink.recv_async().await?;
        Ok(packet.into())
    }
}

struct ConnectionThread {
    radio: Arc<SharedCrazyradio>,
    status: Arc<Mutex<ConnectionStatus>>,
    disconnect_canary: Weak<()>,
    safelink_up_ctr: u8,
    safelink_down_ctr: u8,
    uplink: flume::Receiver<Vec<u8>>,
    downlink: flume::Sender<Vec<u8>>,
    channel: Channel,
    address: [u8; 5],
    flags: ConnectionFlags,
}

impl ConnectionThread {
    fn new(
        radio: Arc<SharedCrazyradio>,
        status: Arc<Mutex<ConnectionStatus>>,
        disconnect_canary: Weak<()>,
        uplink: flume::Receiver<Vec<u8>>,
        downlink: flume::Sender<Vec<u8>>,
        channel: Channel,
        address: [u8; 5],
        flags: ConnectionFlags,
    ) -> Self {
        ConnectionThread {
            radio,
            status,
            disconnect_canary,
            safelink_up_ctr: 0,
            safelink_down_ctr: 0,
            uplink,
            downlink,
            channel,
            address,
            flags,
        }
    }

    async fn update_status(&self, new_status: ConnectionStatus) {
        debug!("New status: {:?}", &new_status);
        let mut status = self.status.lock().await;
        *status = new_status;
    }

    async fn enable_safelink(&mut self) -> Result<bool> {
        // Tying 10 times to reset safelink
        for _ in 0..10 {
            let (ack, payload) =
                self.radio
                    .send_packet_async(self.channel, self.address, vec![0xff, 0x05, 0x01]).await?;

            if ack.received && payload == [0xff, 0x05, 0x01] {
                self.safelink_down_ctr = 0;
                self.safelink_up_ctr = 0;

                // Safelink enabled!
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn send_packet_safe(&mut self, packet: Vec<u8>) -> Result<(crazyradio::Ack, Vec<u8>)> {
        let mut packet = packet;
        packet[0] &= 0xF3;
        packet[0] |= (self.safelink_up_ctr << 3) | (self.safelink_down_ctr << 2);

        let (ack, mut ack_payload) = self.radio.send_packet(self.channel, self.address, packet)?;

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

    fn send_packet(&mut self, packet: Vec<u8>, safe: bool) -> Result<(crazyradio::Ack, Vec<u8>)> {
        let result = if safe {
            self.send_packet_safe(packet)?
        } else {
            self.radio.send_packet(self.channel, self.address, packet)?
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
        let mut last_pk_time = time::Instant::now();
        let mut relax_timeout = RELAX_DELAY;
        let mut n_empty_packets = 0;
        let mut packet = vec![0xff];
        let mut needs_resend = false;
        while last_pk_time.elapsed() < time::Duration::from_millis(1000) {
            if !needs_resend {
                packet = match self.uplink.recv_timeout(relax_timeout) {
                    Ok(pk) => pk,
                    Err(flume::RecvTimeoutError::Timeout) => vec![0xff], // Null packet
                    Err(flume::RecvTimeoutError::Disconnected) => {
                        return Err(Error::ChannelRecvError(flume::RecvError::Disconnected))
                    }
                }
            }

            let (ack, mut ack_payload) = self.send_packet(packet.clone(), safelink)?;

            if ack.received {
                last_pk_time = time::Instant::now();
                needs_resend = false;

                // Add some relaxation time if the Crazyflie has nothing to send back
                // for a while

                let should_handle = self.preprocess_ack(&mut ack_payload, safelink);
                if should_handle {
                    self.downlink.send(ack_payload)?;
                    relax_timeout = Duration::from_nanos(0);
                } else if n_empty_packets > EMPTY_PACKET_BEFORE_RELAX {
                    // If no packet received for a while, relax packet pulling
                    relax_timeout = RELAX_DELAY;
                } else {
                    relax_timeout = Duration::from_nanos(0);
                    n_empty_packets += 1;
                }
            } else {
                debug!("Lost packet!");
                needs_resend = true;
            }

            // If the connection object has been dropped, leave the thread
            if Weak::upgrade(&self.disconnect_canary).is_none() {
                return Ok(());
            }
        }

        self.update_status(ConnectionStatus::Disconnected(
            "Connection timeout".to_string(),
        )).await;

        warn!(
            "Connection to {:?}/{:X?} lost (timeout)",
            self.channel, self.address
        );

        Ok(())
    }
}
