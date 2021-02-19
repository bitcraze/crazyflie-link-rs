
use pyo3::prelude::*;
use pyo3::wrap_pyfunction;

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

        Ok(Connection{connection})
    }
}

#[pyclass]
struct Connection {
    connection: crazyflie_link::Connection,
}

#[pymethods]
impl Connection {
    fn send_packet(&self, py: Python, packet: Vec<u8>) {
        py.allow_threads(move || {
            self.connection.send_packet(packet).unwrap();
        })
        
    }

    fn receive_packet(&self, py: Python) -> PyResult<Vec<u8>> {
        py.allow_threads(move || {
            Ok(self.connection.recv_packet().unwrap())
        })
    }

    // TODO: Implement closing the connection
}

#[pymodule]
fn cflinkrs(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<LinkContext>()?;

    Ok(())
}