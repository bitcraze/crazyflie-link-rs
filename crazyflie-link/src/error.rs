use std::num::ParseIntError;


pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid URI")]
    InvalidUri,
    #[error("Crazyradio error: {0:?}")]
    CrazyradioError(crazyradio::Error),
    #[error("Threading error: {0:?}")]
    CrossbeamRecvError(crossbeam_channel::RecvError),
    #[error("Threading error: {0:?}")]
    CrossbeamSendError(crossbeam_channel::SendError<Vec<u8>>),
}

impl From<crazyradio::Error> for Error {
    fn from(error: crazyradio::Error) -> Self {
        Error::CrazyradioError(error)
    }
}

impl From<crossbeam_channel::RecvError> for Error {
    fn from(error: crossbeam_channel::RecvError) -> Self {
        Error::CrossbeamRecvError(error)
    }
}

impl From<crossbeam_channel::SendError<Vec<u8>>> for Error {
    fn from(error: crossbeam_channel::SendError<Vec<u8>>) -> Self {
        Error::CrossbeamSendError(error)
    }
}

impl From<ParseIntError> for Error {
    fn from(_error: ParseIntError) -> Self {
        Error::InvalidUri
    }
}