use pyo3::{exceptions::PyIOError, prelude::*};
use std::{sync::RwLock, time::Duration};

#[pyclass]
struct LinkContext {
    context: crazyflie_link::LinkContext,
}

#[pymethods]
impl LinkContext {
    #[new]
    fn new() -> PyResult<Self> {
        let context = crazyflie_link::LinkContext::new();

        Ok(LinkContext { context })
    }

    fn scan(&self, address: [u8; 5]) -> PyResult<Vec<String>> {
        self.context
            .scan(address)
            .map_err(|e| PyErr::new::<PyIOError, _>(format!("{:?}", e)))
    }

    fn open_link(&self, uri: &str) -> PyResult<Connection> {
        let connection = self
            .context
            .open_link(uri)
            .map_err(|e| PyErr::new::<PyIOError, _>(format!("{:?}", e)))?;

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
                connection
                    .send_packet(crazyflie_link::Packet::from(packet))
                    .map_err(|e| PyErr::new::<PyIOError, _>(format!("Error: {:?}", e)))
            } else {
                Err(PyErr::new::<PyIOError, _>("Link closed"))
            }
        })
    }

    fn receive_packet(&self, py: Python) -> PyResult<Vec<u8>> {
        py.allow_threads(move || loop {
            let connection = self.connection.read().unwrap();

            if let Some(connection) = connection.as_ref() {
                match connection.recv_packet_timeout(Duration::from_millis(100)) {
                    Ok(pk) => return Ok(pk.into()),
                    _ => continue,
                }
            } else {
                return Err(PyIOError::new_err("Link closed"));
            }
        })
    }

    fn close(&self) {
        if let Some(connection) = self.connection.write().unwrap().take() {
            connection.close()
        }
    }
}

#[pymodule]
fn cflinkrs(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<LinkContext>()?;

    Ok(())
}
