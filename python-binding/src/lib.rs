use pyo3::{exceptions::PyIOError, prelude::*};
use tokio::runtime::Handle;
use std::{sync::RwLock, time::Duration};

#[pyclass]
struct LinkContext {
    context: crazyflie_link::LinkContext,
}

#[pymethods]
impl LinkContext {
    #[new]
    fn new() -> Self {
        let context =
            crazyflie_link::LinkContext::new();

        LinkContext { context }
    }

    #[pyo3(signature = (address = [0xe7; 5]))]
    fn scan(&self, address: [u8; 5]) -> PyResult<Vec<String>> {
        Handle::current().block_on(async {
            self.context
                .scan(address)
                .await
                .map_err(|e| PyErr::new::<PyIOError, _>(format!("{:?}", e)))
        })
    }

    fn scan_selected(&self, uris: Vec<String>) -> PyResult<Vec<String>> {
        Handle::current().block_on(async {
            self.context
                .scan_selected(uris.iter().map(|s| s.as_str()).collect())
                .await
                .map_err(|e| PyErr::new::<PyIOError, _>(format!("{:?}", e)))
        })
    }

    fn open_link(&self, uri: &str) -> PyResult<Connection> {
        let connection = Handle::current().block_on(async {
            self.context
                .open_link(uri)
                .await
                .map_err(|e| PyErr::new::<PyIOError, _>(format!("{:?}", e)))
        })?;

        Ok(Connection {
            connection: RwLock::new(Some(connection)),
        })
    }
}

#[pyclass]
struct Connection {
    connection: RwLock<Option<crazyflie_link::Connection>>,
}

#[pymethods]
impl Connection {
    fn send_packet(&self, py: Python, packet: Vec<u8>) -> PyResult<()> {
        py.allow_threads(move || {
            let connection = self.connection.read().unwrap();

            if let Some(connection) = connection.as_ref() {
                Handle::current().block_on(async {
                    connection
                        .send_packet(crazyflie_link::Packet::from(packet))
                        .await
                        .map_err(|e| PyErr::new::<PyIOError, _>(format!("Error: {:?}", e)))
                })
            } else {
                Err(PyErr::new::<PyIOError, _>("Link closed"))
            }
        })
    }

    #[pyo3(signature = (timeout = 100))]
    fn receive_packet(&self, py: Python, timeout: u64) -> PyResult<Option<Vec<u8>>> {
        py.allow_threads(|| {
            let connection = self.connection.read().unwrap();

            if let Some(connection) = connection.as_ref() {
                let timeout = Duration::from_millis(timeout);
                let pk = Handle::current().block_on(async {
                    match tokio::time::timeout(timeout, connection.recv_packet()).await {
                        Ok(packet) => {
                            if let Ok(packet) = packet {
                                Ok(packet.into())
                            } else {
                                Err(PyIOError::new_err("Link closed"))
                            }
                        }
                        Err(_) => Ok(None),
                    }
                });
                pk.map(|p| p.map(|p| p.into()))
            } else {
                Err(PyIOError::new_err("Link closed"))
            }
        })
    }

    fn close(&self) {
        if let Some(connection) = self.connection.write().unwrap().take() {
            Handle::current().block_on(async { connection.close().await })
        }
    }
}

#[pymodule]
fn cflinkrs(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LinkContext>()?;

    Ok(())
}
