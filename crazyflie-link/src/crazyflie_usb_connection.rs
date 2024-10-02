use crate::connection::ConnectionTrait;
use crate::error::{Error, Result};
use crate::{ConnectionStatus, LinkContext, Packet};
use futures_channel::oneshot;
use futures_util::lock::Mutex;
use log::{debug, info, warn, error};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use url::Url;
use async_trait::async_trait;
use rusb::{DeviceHandle, DeviceList, GlobalContext};
use std::time::Duration;

/// Link connection
pub struct CrazyflieUSBConnection {
    status: Arc<Mutex<ConnectionStatus>>,
    uplink: flume::Sender<Vec<u8>>,
    downlink: flume::Receiver<Vec<u8>>,
    disconnect_channel: flume::Receiver<()>,
    disconnect: Arc<AtomicBool>
}

impl CrazyflieUSBConnection {
    pub async fn open(_link_context: &LinkContext, uri: &str) -> Result<Option<CrazyflieUSBConnection>> {
        let serial = Self::parse_uri(uri)?;

        let (
            _device_desc,
            handle,
            handle_ctrl
          ) = tokio::task::spawn_blocking(move || {
          for device in DeviceList::new()?.iter() {
            let device_desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };
  
            if device_desc.vendor_id() == 0x0483 && device_desc.product_id() == 0x5740 {
                let timeout = Duration::from_secs(1);

                let handle = match device.open() {
                  Ok(d) => d,
                  Err(_) => continue,
                };

                handle.claim_interface(0)?;

                let handle_ctrl = match device.open() {
                  Ok(d) => d,
                  Err(_) => continue,
                };
  
              let language = match handle.read_languages(timeout).unwrap_or_default().first() {
                Some(l) => *l,
                None => continue,
              };
  
              let detected_serial = handle.read_serial_number_string(language, &device_desc, timeout)?;
              
              if detected_serial == serial {
                return Ok((device_desc, handle, handle_ctrl));
              }
            }
          }
          Err(Error::InvalidUri)
        }).await.unwrap()?;

        let connection = CrazyflieUSBConnection::new(
          handle,
          handle_ctrl
        ).await?;

        Ok(Some(connection))
    }

    async fn new(
        usb_handle: DeviceHandle<GlobalContext>,
        usb_handle_ctrl: DeviceHandle<GlobalContext>,
    ) -> Result<CrazyflieUSBConnection> {
        let status = Arc::new(Mutex::new(ConnectionStatus::Connecting));

        let (disconnect_channel_tx, disconnect_channel_rx) = flume::bounded(0);
        let disconnect = Arc::new(AtomicBool::new(false));

        let (uplink_send, uplink_recv) = flume::bounded::<Vec<u8>>(1000);
        let (downlink_send, downlink_recv) = flume::bounded::<Vec<u8>>(1000);

        let (connection_initialized_send, connection_initialized) = oneshot::channel();

        let conn_disconnect = disconnect.clone();
        let conn_status = status.clone();

        tokio::task::spawn_blocking(move || {

                // Switch the Crazyflie into USB communication mode
                match usb_handle_ctrl.write_control(64, 0x01,  0x01, 0x01, &[], Duration::from_secs(1)) {
                  Ok(_) => debug!("Switched to USB communication mode"),
                  Err(e) => {
                    error!("Error switching to USB communication mode: {:?}", e);
                    return
                  }
                }

                let inner_status = conn_status.clone();
                tokio::runtime::Handle::current().block_on(async move {
                  let mut status = inner_status.lock().await;
                  *status = ConnectionStatus::Connected;
                });

                connection_initialized_send.send(()).unwrap();

                let thread_handle = std::thread::spawn::<_, Result<_>>(move || {
                    info!("Communication thread started");
                    loop {
                      let mut buf = vec![0; 64];
                      match usb_handle.read_bulk(0x81, &mut buf, Duration::from_millis(20)) {
                        Ok(n) => {
                          if n > 0 {
                            let packet = buf[0..n].to_vec();
                            downlink_send.send(packet)?;
                          }
                        },
                        Err(rusb::Error::Timeout) => {},
                        Err(e) => {
                          warn!("Downlink thread error: {:?}", e);
                          return Err(e.into());
                        }
                      }

                      while !uplink_recv.is_empty() {
                        let packet = uplink_recv.recv()?;
                        match usb_handle.write_bulk(0x01, &packet, Duration::from_millis(100)) {
                          Ok(_) => {},
                          Err(e) => {
                            warn!("Uplink thread error: {:?}", e);
                            return Err(e.into());
                          }
                      }
                    }

                    // If the connection object has been dropped, leave the thread
                    if conn_disconnect.load(Relaxed) {
                      debug!("Disconnect requested, leaving connection loop.");
                      return Ok(());
                    }
                  }
                });

                let disconnect_message = match thread_handle.join() {
                  Ok(Ok(())) => format!("Connection closed"),
                  Ok(Err(e)) => {
                    error!("Connection thread error: {:?}", e);
                    format!("USB error: {:?}", e)
                  },
                  Err(e) => {
                    error!("Connection thread panicked: {:?}", e);
                    format!("Connection thread panicked: {:?}", e)
                  }
                };

                let inner_status = conn_status.clone();
                tokio::runtime::Handle::current().block_on(async move {
                  let mut status = inner_status.lock().await;
                  *status = ConnectionStatus::Disconnected(disconnect_message)
                });
                
                // Best effort, do not care if the other side is dropped
                let _ =  disconnect_channel_tx.send(());

                match usb_handle_ctrl.write_control(64, 0x01,  0x01, 0x00, &[], Duration::from_secs(1)) {
                  Ok(_) => debug!("Switched back to CR mode in CF"),
                  Err(e) => {
                    error!("Error switching back to CR mode in CF: {:?}", e);
                  }
                }
            });

        // Wait for, either, the connection being established or failed initialization
        connection_initialized.await.unwrap();

        Ok(CrazyflieUSBConnection {
            status,
            disconnect_channel: disconnect_channel_rx,
            uplink: uplink_send,
            downlink: downlink_recv,
            disconnect
        })
    }

    fn parse_uri(uri: &str) -> Result<String> {
      let uri = Url::parse(uri)?;

      if uri.scheme() != "usb" {
          return Err(Error::InvalidUriScheme);
      }

      let serial = uri.domain().ok_or(Error::InvalidUri)?;

      if uri.path_segments().is_some() {
        return Err(Error::InvalidUri);
      }

      Ok(serial.to_owned())
  }
  }

  #[async_trait]
  impl ConnectionTrait for CrazyflieUSBConnection {
    /// Wait for the connection to be closed. Returns the message stored in the
    /// disconnected connection status that indicate the reason for the disconnection
    async fn wait_close(&self) -> String {
        // Wait for the connection thread to drop the disconnect channel
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
    /// Though, if the connection task is currently processing a packet, it will continue running
    /// until the current packet has been processed. This function will wait for any ongoing packet
    /// to be processed and for the communication task to stop.
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
        // The channel will return an error when the other side, in the connection thread, is dropped
        let _ = self.disconnect_channel.recv_async().await;
    }

    /// Send a packet to the connected Crazyflie
    ///
    /// This fundtion can return an error if the connection task is not active anymore.
    /// This can happen if the Crazyflie is disconnected due to a timeout
    async fn send_packet(&self, packet: Packet) -> Result<()> {
        self.uplink.send_async(packet.into()).await?;
        Ok(())
    }

    /// Receive a packet from the connected Crazyflie
    ///
    /// This fundtion can return an error if the connection task is not active anymore.
    /// This can happen if the Crazyflie is disconnected due to a timeout
    async fn recv_packet(&self) -> Result<Packet> {
        let packet = self.downlink.recv_async().await?;
        Ok(packet.into())
    }
}

impl Drop for CrazyflieUSBConnection {
    fn drop(&mut self) {
        self.disconnect.store(true, Relaxed);
    }
}
