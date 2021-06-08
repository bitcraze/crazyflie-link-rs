use flume::RecvTimeoutError;
use std::num::ParseIntError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid URI")]
    InvalidUri,
    #[error("Timeout")]
    Timeout,
    #[error("Crazyradio error: {0:?}")]
    CrazyradioError(crate::crazyradio::Error),
    #[error("Threading error: {0:?}")]
    ChannelRecvError(flume::RecvError),
    #[error("Threading error: {0:?}")]
    ChannelSendError(flume::SendError<Vec<u8>>),
}

impl From<crate::crazyradio::Error> for Error {
    fn from(error: crate::crazyradio::Error) -> Self {
        Error::CrazyradioError(error)
    }
}

impl From<flume::RecvError> for Error {
    fn from(error: flume::RecvError) -> Self {
        Error::ChannelRecvError(error)
    }
}

impl From<flume::SendError<Vec<u8>>> for Error {
    fn from(error: flume::SendError<Vec<u8>>) -> Self {
        Error::ChannelSendError(error)
    }
}

impl From<url::ParseError> for Error {
    fn from(_error: url::ParseError) -> Self {
        Error::InvalidUri
    }
}

impl From<ParseIntError> for Error {
    fn from(_error: ParseIntError) -> Self {
        Error::InvalidUri
    }
}

impl From<RecvTimeoutError> for Error {
    fn from(_error: RecvTimeoutError) -> Self {
        Error::Timeout
    }
}
