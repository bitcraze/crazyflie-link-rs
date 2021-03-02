// Connection handling code
use crate::error::{Error, Result};
use crate::radio_thread::RadioThread;
use crazyradio::Channel;
use crossbeam_utils::sync::{ShardedLock, WaitGroup};
use log::{debug, info, warn};
use std::sync::Arc;
use std::sync::RwLock;
use std::time;
use std::time::Duration;

const EMPTY_PACKET_BEFORE_RELAX: u32 = 10;
const RELAX_DELAY: Duration = Duration::from_millis(10);

#[derive(Clone, Debug)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected(String),
}

pub struct Connection {
    status: Arc<RwLock<ConnectionStatus>>,
    disconnect: Arc<ShardedLock<bool>>,
    thread: std::thread::JoinHandle<()>,
    uplink: crossbeam_channel::Sender<Vec<u8>>,
    downlink: crossbeam_channel::Receiver<Vec<u8>>,
}

impl Connection {
    pub fn new(radio: Arc<RadioThread>, channel: Channel, address: [u8; 5]) -> Result<Connection> {
        let status = Arc::new(RwLock::new(ConnectionStatus::Connecting));
        let disconnect = Arc::new(ShardedLock::new(false));

        let (uplink_send, uplink_recv) = crossbeam_channel::unbounded();
        let (downlink_send, downlink_recv) = crossbeam_channel::unbounded();

        let connection_initialized = WaitGroup::new();

        let ci = connection_initialized.clone();
        let mut thread = ConnectionThread::new(
            radio,
            status.clone(),
            disconnect.clone(),
            uplink_recv,
            downlink_send,
            channel,
            address,
        );
        let thread = std::thread::spawn(move || match thread.run(ci) {
            Err(e) => thread.update_status(ConnectionStatus::Disconnected(format!(
                "Connection error: {}",
                e
            ))),
            _ => {}
        });

        // Wait for, either, the connection being established or failed initialization
        connection_initialized.wait();

        Ok(Connection {
            status,
            disconnect,
            thread,
            uplink: uplink_send,
            downlink: downlink_recv,
        })
    }

    pub fn status(&self) -> ConnectionStatus {
        self.status.read().unwrap().clone()
    }

    pub fn disconnect(self) {
        *self.disconnect.write().unwrap() = true;
        debug!("Closing the connection!");
        self.thread.join().unwrap();
    }

    pub fn send_packet(&self, packet: Vec<u8>) -> Result<()> {
        self.uplink.send(packet)?;
        Ok(())
    }

    pub fn recv_packet_timeout(&self, timeout: Duration) -> Result<Vec<u8>> {
        let packet = self.downlink.recv_timeout(timeout)?;
        Ok(packet)
    }
}

struct ConnectionThread {
    radio: Arc<RadioThread>,
    status: Arc<RwLock<ConnectionStatus>>,
    disconnect: Arc<ShardedLock<bool>>,
    safelink_up_ctr: u8,
    safelink_down_ctr: u8,
    uplink: crossbeam_channel::Receiver<Vec<u8>>,
    downlink: crossbeam_channel::Sender<Vec<u8>>,
    channel: Channel,
    address: [u8; 5],
}

impl ConnectionThread {
    fn new(
        radio: Arc<RadioThread>,
        status: Arc<RwLock<ConnectionStatus>>,
        disconnect: Arc<ShardedLock<bool>>,
        uplink: crossbeam_channel::Receiver<Vec<u8>>,
        downlink: crossbeam_channel::Sender<Vec<u8>>,
        channel: Channel,
        address: [u8; 5],
    ) -> Self {
        ConnectionThread {
            radio,
            status,
            disconnect,
            safelink_up_ctr: 0,
            safelink_down_ctr: 0,
            uplink,
            downlink,
            channel,
            address,
        }
    }

    fn update_status(&self, new_status: ConnectionStatus) {
        debug!("New status: {:?}", &new_status);
        let mut status = self.status.write().unwrap();
        *status = new_status;
    }

    fn enable_safelink(&mut self) -> Result<bool> {
        // Tying 10 times to reset safelink
        for _ in 0..10 {
            let (ack, payload) =
                self.radio
                    .send_packet(self.channel, self.address, vec![0xff, 0x05, 0x01])?;

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

        let (ack, ack_payload) = self.radio.send_packet(self.channel, self.address, packet)?;

        if ack.received && ack_payload.len() > 0 {
            let received_down_ctr = (ack_payload[0] & 0x04) >> 2;
            if received_down_ctr == self.safelink_down_ctr {
                self.safelink_down_ctr = 1 - self.safelink_down_ctr;
            }
        }

        if ack.received {
            self.safelink_up_ctr = 1 - self.safelink_up_ctr;
        }

        Ok((ack, ack_payload))
    }

    fn run(&mut self, connection_initialized: WaitGroup) -> Result<()> {
        info!("Connecting to {:?}/{:X?} ...", self.channel, self.address);

        // Try to initialize safelink
        // This server only supports safelink, if it cannot be enabled
        // the Crazyflie is deemed not connectable
        if self.enable_safelink()? == false {
            self.update_status(ConnectionStatus::Disconnected(
                "Cannot initialize connection".to_string(),
            ));
            warn!(
                "Cannot initialize connection with {:?}/{:X?}",
                self.channel, self.address
            );
            return Ok(());
        }

        // Safelink is initialized, we are connected!
        self.update_status(ConnectionStatus::Connected);
        drop(connection_initialized);

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
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => vec![0xff],  // Null packet
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Err(Error::CrossbeamRecvError(crossbeam_channel::RecvError))
                }
            }

            let (ack, mut ack_payload) = self.send_packet_safe(packet.clone())?;

            if ack.received {
                last_pk_time = time::Instant::now();
                needs_resend = false;

                // Add some relaxation time if the Crazyflie has nothing to send back
                // for a while
                if ack_payload.len() > 0 && (ack_payload[0] & 0xF3) != 0xF3 {
                    ack_payload[0] &= 0xF3;
                    self.downlink.send(ack_payload)?;
                    relax_timeout = Duration::from_nanos(0);
                } else {
                    if n_empty_packets > EMPTY_PACKET_BEFORE_RELAX {
                        // If no packet received for a while, relax packet pulling
                        relax_timeout = RELAX_DELAY;
                    } else {
                        relax_timeout = Duration::from_nanos(0);
                        n_empty_packets += 1;
                    }
                    
                }
            } else {
                debug!("Lost packet!");
                needs_resend = true;
            }

            if *self.disconnect.read().unwrap() {
                self.update_status(ConnectionStatus::Disconnected(
                    "Disconnect requested".to_string(),
                ));
                info!(
                    "Closing connection to {:?}/{:X?} as requested",
                    self.channel, self.address
                );
                return Ok(());
            }
        }

        self.update_status(ConnectionStatus::Disconnected(
            "Connection timeout".to_string(),
        ));

        warn!(
            "Connection to {:?}/{:X?} lost (timeout)",
            self.channel, self.address
        );

        Ok(())
    }
}
