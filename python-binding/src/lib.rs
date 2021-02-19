
use pyo3::{exceptions::PyIOError, prelude::*};
use std::sync::RwLock;

#[pyclass]
struct LinkContext {
    context: crazyflie_link::LinkContext,
}

#[pymethods]
impl LinkContext {
    #[new]
    fn new() -> PyResult<Self> {
        let context = crazyflie_link::LinkContext::new().unwrap();
        
        Ok(LinkContext {
            context
        })
    }

    fn scan(&self) -> PyResult<Vec<String>> {
        let result = self.context.scan().unwrap();

        Ok(result)
    }

    fn open_link(&self, uri: &str) -> PyResult<Connection> {
        let connection = self.context.open_link(uri).unwrap();

        Ok(Connection{connection: RwLock::new(Some(connection))})
    }
}

#[pyclass]
struct Connection {
    connection: RwLock<Option<crazyflie_link::Connection>>,
}

#[pymethods]
impl Connection {
    fn send_packet(&self, py: Python, packet: Vec<u8>) {
        py.allow_threads(move || {
            let connection = self.connection.read().unwrap();

            if let Some(connection) = connection.as_ref() {
                connection.send_packet(packet).unwrap();
            }
        })
        
    }

    fn receive_packet(&self, py: Python) -> PyResult<Vec<u8>> {
        py.allow_threads(move || {
            let connection = self.connection.read().unwrap();

            if let Some(connection) = connection.as_ref() {
                Ok(connection.recv_packet().unwrap())
            } else {
                Err(PyIOError::new_err("Link closed"))
            }
        })
    }

    fn close(&self) {
        if let Some(connection) = self.connection.write().unwrap().take() {
            connection.disconnect()
        }
    }
}

#[pymodule]
fn cflinkrs(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<LinkContext>()?;

    Ok(())
}