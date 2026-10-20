//! UDP connection to a Crazyflie firmware simulator (Simmyflie)
//!
//! Each UDP datagram carries exactly one CRTP packet: the header byte followed by the data.
//! There are no acks on UDP, so the link probes the simulator with a null packet (`0xFF`),
//! which the simulator echoes back without passing it to the firmware. The probe is used
//! to check that the simulator answers before the connection is opened, and as a keepalive
//! to detect when it stops answering.
//!
//! Simmyflies also send beacons, carrying their UDP port, to a host-local multicast
//! group. Scanning listens for these beacons.

use crate::connection::{ConnectionTrait, RadioLinkStatistics};
use crate::error::{Error, Result};
use crate::{ConnectionStatus, LinkContext, Packet};
use async_trait::async_trait;
use futures_util::lock::Mutex;
use log::{debug, info, warn};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::time::{self, Instant, MissedTickBehavior};
use url::{Host, Url};

/// Null packet, echoed back by the simulator
const PROBE: [u8; 1] = [0xFF];
/// Interval between probes when nothing is received
const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(100);
/// How long to wait for an answer when probing a URI in `scan_selected()`
const SCAN_SELECTED_TIMEOUT: Duration = Duration::from_millis(300);

/// Default beacon address of the Simmyflie
const BEACON_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 50, 0);
const BEACON_PORT: u16 = 49999;
/// Beacon wire format, version 1: u8 version, u8 status, u16 UDP port (LE), ...
const BEACON_VERSION: u8 = 1;
const BEACON_SIZE: usize = 52;
/// Beacons are sent at 50 Hz, so this catches several from each simulator
const BEACON_LISTEN_TIME: Duration = Duration::from_millis(250);

/// A UDP connection to a Crazyflie firmware simulator
pub struct UdpConnection {
    /// Holds the current connection status
    status: Arc<Mutex<ConnectionStatus>>,
    /// Channel to send messages from the host to the Crazyflie
    uplink: flume::Sender<Vec<u8>>,
    /// Channel to receive messages from the Crazyflie to the host
    downlink: flume::Receiver<Vec<u8>>,
    /// Channel to signal the waiting callers that the connection has been closed
    disconnect_channel: flume::Receiver<()>,
    /// Flag to signal the user requested disconnection
    disconnect: Arc<AtomicBool>,
}

impl UdpConnection {
    pub async fn open(_link_context: &LinkContext, uri: &str) -> Result<Option<UdpConnection>> {
        let (addresses, timeout) = match Self::parse_uri(uri).await {
            Ok(parsed) => parsed,
            Err(Error::InvalidUriScheme) => return Ok(None),
            Err(e) => return Err(e),
        };

        // A host name can resolve to several addresses, use the first one that answers
        let mut result = Err(Error::InvalidUri);
        for address in addresses {
            result = probe(address, timeout).await;
            if result.is_ok() {
                break;
            }
            debug!("No answer from {}", address);
        }
        let socket = result?;

        Ok(Some(UdpConnection::new(socket, timeout)))
    }

    fn new(socket: UdpSocket, timeout: Duration) -> UdpConnection {
        // The simulator has answered the probe
        let status = Arc::new(Mutex::new(ConnectionStatus::Connected));

        let (disconnect_channel_tx, disconnect_channel_rx) = flume::bounded(0);
        let disconnect = Arc::new(AtomicBool::new(false));

        let (uplink_send, uplink_recv) = flume::unbounded::<Vec<u8>>();
        let (downlink_send, downlink_recv) = flume::unbounded::<Vec<u8>>();

        let mut thread = ConnectionThread {
            socket,
            uplink: uplink_recv,
            downlink: downlink_send,
            disconnect: disconnect.clone(),
            timeout,
        };
        let conn_status = status.clone();
        tokio::spawn(async move {
            let reason = match thread.run().await {
                Ok(reason) => reason,
                Err(e) => format!("Connection error: {}", e),
            };
            debug!("New status: Disconnected({:?})", reason);
            *conn_status.lock().await = ConnectionStatus::Disconnected(reason);
            drop(disconnect_channel_tx);
        });

        UdpConnection {
            status,
            uplink: uplink_send,
            downlink: downlink_recv,
            disconnect_channel: disconnect_channel_rx,
            disconnect,
        }
    }

    /// Listen for the beacons of the simulators running on this host
    pub(crate) async fn scan() -> Result<Vec<String>> {
        let socket = match open_beacon_socket() {
            Ok(socket) => socket,
            Err(e) => {
                debug!("Cannot listen for Simmyflie beacons: {}", e);
                return Ok(Vec::new());
            }
        };

        let mut found = Vec::new();
        let mut buffer = [0u8; 256];
        let deadline = Instant::now() + BEACON_LISTEN_TIME;
        while let Ok(received) = time::timeout_at(deadline, socket.recv_from(&mut buffer)).await {
            let Ok((n, from)) = received else {
                break;
            };
            if n != BEACON_SIZE || buffer[0] != BEACON_VERSION {
                continue;
            }
            let port = u16::from_le_bytes([buffer[2], buffer[3]]);
            let uri = format!("udp://{}:{}", from.ip(), port);
            if !found.contains(&uri) {
                found.push(uri);
            }
        }
        found.sort_by_key(|uri| uri.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()));

        Ok(found)
    }

    /// Probe the given UDP URIs and return the ones that answer, other URIs are skipped
    pub(crate) async fn scan_selected(uris: Vec<&str>) -> Result<Vec<String>> {
        let probes = uris.into_iter().map(|uri| async move {
            let (addresses, _) = match Self::parse_uri(uri).await {
                Ok(parsed) => parsed,
                Err(Error::InvalidUriScheme) => return Ok(None),
                Err(e) => return Err(e),
            };
            for address in addresses {
                if probe(address, SCAN_SELECTED_TIMEOUT).await.is_ok() {
                    return Ok(Some(uri.to_owned()));
                }
            }
            Ok(None)
        });

        let answered = futures::future::join_all(probes).await;
        answered
            .into_iter()
            .filter_map(Result::transpose)
            .collect()
    }

    /// Parse a `udp://<host>:<port>[?timeout=<ms>]` URI
    ///
    /// Returns the addresses the host resolves to, IPv4 first, and the timeout. Other query
    /// parameters, like the radio's `safelink` and `ackfilter`, are ignored.
    async fn parse_uri(uri: &str) -> Result<(Vec<SocketAddr>, Duration)> {
        let uri = Url::parse(uri)?;

        if uri.scheme() != "udp" {
            return Err(Error::InvalidUriScheme);
        }

        if !matches!(uri.path(), "" | "/") {
            return Err(Error::InvalidUri);
        }

        let port = uri.port().ok_or(Error::InvalidUri)?;

        let mut addresses: Vec<SocketAddr> = match uri.host().ok_or(Error::InvalidUri)? {
            Host::Domain(domain) => tokio::net::lookup_host((domain, port)).await?.collect(),
            Host::Ipv4(ip) => vec![(ip, port).into()],
            Host::Ipv6(ip) => vec![(ip, port).into()],
        };
        // The Simmyflie only listens on IPv4, while localhost often resolves to ::1 first
        addresses.sort_by_key(|address| address.is_ipv6());

        let mut timeout = Duration::from_millis(1000);
        for (key, value) in uri.query_pairs() {
            if key == "timeout" {
                let ms: u64 = value.parse().map_err(|_| Error::InvalidUri)?;
                timeout = Duration::from_millis(ms);
            }
        }

        Ok((addresses, timeout))
    }
}

/// Open a UDP socket to the address and wait for it to echo a probe
async fn probe(address: SocketAddr, timeout: Duration) -> Result<UdpSocket> {
    let local: SocketAddr = if address.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(local).await?;
    socket.connect(address).await?;

    let mut buffer = [0u8; 64];
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        socket.send(&PROBE).await?;

        let wait = (Instant::now() + KEEPALIVE_INTERVAL).min(deadline);
        // A refused probe means that nothing is listening on the port
        if let Ok(received) = time::timeout_at(wait, socket.recv(&mut buffer)).await {
            if buffer[..received?] == PROBE {
                return Ok(socket);
            }
        }
    }

    Err(Error::Timeout)
}

/// Socket that receives the Simmyflie beacons, sent to a multicast group on the loopback
/// interface
fn open_beacon_socket() -> std::io::Result<UdpSocket> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    // Several listeners on one host, like a simviewer, each get every beacon
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    // Binding to the group only hears that group, but Windows cannot bind to a group
    let bind_ip = if cfg!(windows) { Ipv4Addr::UNSPECIFIED } else { BEACON_GROUP };
    socket.bind(&SocketAddr::from((bind_ip, BEACON_PORT)).into())?;
    socket.join_multicast_v4(&BEACON_GROUP, &Ipv4Addr::LOCALHOST)?;

    UdpSocket::from_std(socket.into())
}

#[async_trait]
impl ConnectionTrait for UdpConnection {
    /// Wait for the connection to be closed. Returns the message stored in the
    /// disconnected connection status that indicate the reason for the disconnection
    async fn wait_close(&self) -> String {
        // Wait for the connection task to drop the disconnect channel
        let _ = self.disconnect_channel.recv_async().await;
        if let ConnectionStatus::Disconnected(reason) = self.status().await {
            reason
        } else {
            "Still connected!".to_owned()
        }
    }

    /// Close the connection and wait for the connection task to stop.
    ///
    /// The connection can also be closed by simply dropping the connection object.
    async fn close(&self) {
        self.disconnect.store(true, Relaxed);
        let _ = self.disconnect_channel.recv_async().await;
    }

    /// Return the connection status
    async fn status(&self) -> ConnectionStatus {
        self.status.lock().await.clone()
    }

    /// Block until the connection is dropped. The `status()` function can be used to get the reason
    /// for the disconnection.
    async fn wait_disconnect(&self) {
        // The channel will return an error when the other side, in the connection task, is dropped
        let _ = self.disconnect_channel.recv_async().await;
    }

    /// Send a packet to the connected Crazyflie
    ///
    /// This function can return an error if the connection task is not active anymore.
    /// This can happen if the simulator stopped answering
    async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.uplink.send_async(packet.into()).await?;
        Ok(())
    }

    /// Receive a packet from the connected Crazyflie
    ///
    /// This function can return an error if the connection task is not active anymore.
    /// This can happen if the simulator stopped answering
    async fn recv_packet(&self) -> Result<Packet> {
        let packet = self.downlink.recv_async().await?;
        Ok(packet.into())
    }

    async fn link_statistics(&self) -> Option<RadioLinkStatistics> {
        None
    }
}

impl Drop for UdpConnection {
    fn drop(&mut self) {
        self.disconnect.store(true, Relaxed);
    }
}

struct ConnectionThread {
    socket: UdpSocket,
    uplink: flume::Receiver<Vec<u8>>,
    downlink: flume::Sender<Vec<u8>>,
    disconnect: Arc<AtomicBool>,
    timeout: Duration,
}

impl ConnectionThread {
    /// Run the connection until it is closed or lost, returns the reason
    async fn run(&mut self) -> Result<String> {
        let peer = self.socket.peer_addr()?;
        info!("Connected to {}", peer);

        let mut keepalive = time::interval(KEEPALIVE_INTERVAL);
        keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut last_received = Instant::now();
        let mut buffer = [0u8; 64];

        loop {
            if self.disconnect.load(Relaxed) {
                debug!("Disconnect requested, leaving connection loop.");
                return Ok("Connection closed".to_owned());
            }

            tokio::select! {
                packet = self.uplink.recv_async() => {
                    // The connection object has been dropped
                    let Ok(packet) = packet else {
                        return Ok("Connection closed".to_owned());
                    };
                    #[cfg(feature = "packet_capture")]
                    capture(&peer, crate::capture::DIRECTION_TX, &packet);
                    self.socket.send(&packet).await?;
                }
                received = self.socket.recv(&mut buffer) => {
                    let n = received?;
                    last_received = Instant::now();
                    // Probe echoes only show that the simulator is alive
                    if n == 0 || buffer[..n] == PROBE {
                        continue;
                    }
                    #[cfg(feature = "packet_capture")]
                    capture(&peer, crate::capture::DIRECTION_RX, &buffer[..n]);
                    if self.downlink.send(buffer[..n].to_vec()).is_err() {
                        return Ok("Connection closed".to_owned());
                    }
                }
                _ = keepalive.tick() => {
                    if last_received.elapsed() >= self.timeout {
                        warn!("Connection to {} lost (timeout)", peer);
                        return Ok("Connection timeout".to_owned());
                    }
                    if last_received.elapsed() >= KEEPALIVE_INTERVAL {
                        self.socket.send(&PROBE).await?;
                    }
                }
            }
        }
    }
}

#[cfg(feature = "packet_capture")]
fn capture(peer: &SocketAddr, direction: u8, packet: &[u8]) {
    crate::capture::send_packet(
        crate::capture::LINK_TYPE_UDP,
        direction,
        &[],
        0,
        &peer.to_string(),
        packet,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    /// A stand-in for the simulator: echoes probes and sends `reply` for every other packet,
    /// until `silent` is set
    async fn fake_simulator(
        reply: Vec<u8>,
    ) -> (SocketAddr, Arc<AtomicBool>, tokio::task::JoinHandle<()>) {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = socket.local_addr().unwrap();
        let silent = Arc::new(AtomicBool::new(false));
        let task_silent = silent.clone();
        let task = tokio::spawn(async move {
            let mut buffer = [0u8; 64];
            loop {
                let (n, from) = socket.recv_from(&mut buffer).await.unwrap();
                if task_silent.load(Relaxed) {
                    continue;
                }
                let answer = if buffer[..n] == PROBE { &PROBE[..] } else { &reply[..] };
                socket.send_to(answer, from).await.unwrap();
            }
        });
        (address, silent, task)
    }

    #[tokio::test]
    async fn parse_uri() {
        let (addresses, timeout) = UdpConnection::parse_uri("udp://127.0.0.1:50000").await.unwrap();
        assert_eq!(addresses, vec![SocketAddr::from((Ipv4Addr::LOCALHOST, 50000))]);
        assert_eq!(timeout, Duration::from_millis(1000));

        let (addresses, timeout) =
            UdpConnection::parse_uri("udp://localhost:50000/?safelink=0&timeout=250").await.unwrap();
        assert!(addresses[0].is_ipv4());
        assert_eq!(timeout, Duration::from_millis(250));

        assert!(matches!(
            UdpConnection::parse_uri("radio://0/80/2M/E7E7E7E7E7").await,
            Err(Error::InvalidUriScheme)
        ));
        for uri in ["udp://localhost", "udp://localhost:50000/1", "udp://localhost:50000?timeout=x"] {
            assert!(matches!(UdpConnection::parse_uri(uri).await, Err(Error::InvalidUri)), "{}", uri);
        }
    }

    #[tokio::test]
    async fn send_and_receive() {
        let (address, _, _simulator) = fake_simulator(vec![0x00, b'h', b'i']).await;
        let context = LinkContext::new();
        let link = context.open_link(&format!("udp://{}", address)).await.unwrap();
        assert!(matches!(link.status().await, ConnectionStatus::Connected));

        link.send_packet(Packet::new(0, 0, vec![1, 2, 3])).await.unwrap();
        let packet = time::timeout(Duration::from_secs(1), link.recv_packet()).await.unwrap().unwrap();
        assert_eq!(packet.get_header(), 0x00);
        assert_eq!(packet.get_data(), &vec![b'h', b'i']);

        // Probe echoes are not passed on
        time::sleep(3 * KEEPALIVE_INTERVAL).await;
        assert!(link.recv_packet().now_or_never().is_none());

        link.close().await;
        assert_eq!(link.wait_close().await, "Connection closed");
    }

    #[tokio::test]
    async fn open_fails_without_simulator() {
        // Bound but silent, so the probe gets no answer
        let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let uri = format!("udp://{}?timeout=300", silent.local_addr().unwrap());
        assert!(matches!(LinkContext::new().open_link(&uri).await, Err(Error::Timeout)));

        // Nothing bound, so the probe is refused
        let port = silent.local_addr().unwrap().port();
        drop(silent);
        let uri = format!("udp://127.0.0.1:{}", port);
        assert!(matches!(LinkContext::new().open_link(&uri).await, Err(Error::IoError(_))));
    }

    #[tokio::test]
    async fn disconnects_when_simulator_stops_answering() {
        let (address, silent, _simulator) = fake_simulator(vec![]).await;
        let context = LinkContext::new();
        let link = context.open_link(&format!("udp://{}?timeout=300", address)).await.unwrap();

        silent.store(true, Relaxed);
        let reason = time::timeout(Duration::from_secs(2), link.wait_close()).await.unwrap();
        assert_eq!(reason, "Connection timeout");
    }

    #[tokio::test]
    async fn disconnects_when_simulator_exits() {
        let (address, _, simulator) = fake_simulator(vec![]).await;
        let context = LinkContext::new();
        let link = context.open_link(&format!("udp://{}?timeout=5000", address)).await.unwrap();

        // The closed port refuses the next probe, long before the timeout
        simulator.abort();
        let reason = time::timeout(Duration::from_secs(2), link.wait_close()).await.unwrap();
        assert!(reason.starts_with("Connection error"), "{}", reason);
    }

    #[tokio::test]
    async fn scan_selected() {
        let (address, _, _simulator) = fake_simulator(vec![]).await;
        let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let present = format!("udp://{}", address);
        let absent = format!("udp://{}", silent.local_addr().unwrap());

        let found = UdpConnection::scan_selected(vec![&absent, &present, "usb://123"]).await.unwrap();
        assert_eq!(found, vec![present]);
    }
}
