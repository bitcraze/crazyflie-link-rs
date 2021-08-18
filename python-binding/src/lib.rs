use async_std::future;
use async_std::task;
use pyo3::{exceptions::PyIOError, prelude::*};
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
            crazyflie_link::LinkContext::new(std::sync::Arc::new(async_executors::AsyncStd));

        LinkContext { context }
    }

    #[args(address = "[0xe7; 5]")]
    fn scan(&self, address: [u8; 5]) -> PyResult<Vec<String>> {
        task::block_on(async {
            self.context
                .scan(address)
                .await
                .map_err(|e| PyErr::new::<PyIOError, _>(format!("{:?}", e)))
        })
    }

    fn scan_selected(&self, uris: Vec<&str>) -> PyResult<Vec<String>> {
        task::block_on(async {
            self.context
                .scan_selected(uris)
                .await
                .map_err(|e| PyErr::new::<PyIOError, _>(format!("{:?}", e)))
        })
    }

    fn open_link(&self, uri: &str) -> PyResult<Connection> {
        let connection = task::block_on(async {
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
                task::block_on(async {
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

    #[args(timeout = "100")]
    fn receive_packet(&self, py: Python, timeout: u64) -> PyResult<Option<Vec<u8>>> {
        py.allow_threads(|| {
            let connection = self.connection.read().unwrap();

            if let Some(connection) = connection.as_ref() {
                let timeout = Duration::from_millis(timeout);
                let pk = task::block_on(async {
                    match future::timeout(timeout, connection.recv_packet()).await {
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
            task::block_on(async { connection.close().await })
        }
    }
}

#[pymodule]
fn cflinkrs(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<LinkContext>()?;

    Ok(())
}
