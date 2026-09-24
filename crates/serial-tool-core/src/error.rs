use std::{error, fmt, io};

#[derive(Debug)]
pub enum Error {
    InvalidConfiguration(&'static str),
    PortEnumerationFailed(serialport::Error),
    PortOpenFailed(serialport::Error),
    ReadFailed(io::Error),
    WriteFailed(io::Error),
    FlushFailed(io::Error),
    Timeout { partial: Vec<u8> },
    ReadLimitExceeded { max_bytes: usize, partial: Vec<u8> },
}

impl Error {
    pub fn partial(&self) -> Option<&[u8]> {
        match self {
            Self::Timeout { partial } | Self::ReadLimitExceeded { partial, .. } => Some(partial),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => write!(f, "invalid configuration: {message}"),
            Self::PortEnumerationFailed(error) => write!(f, "port enumeration failed: {error}"),
            Self::PortOpenFailed(error) => write!(f, "port open failed: {error}"),
            Self::ReadFailed(error) => write!(f, "read failed: {error}"),
            Self::WriteFailed(error) => write!(f, "write failed: {error}"),
            Self::FlushFailed(error) => write!(f, "flush failed: {error}"),
            Self::Timeout { partial } => write!(f, "read timed out after {} bytes", partial.len()),
            Self::ReadLimitExceeded { max_bytes, .. } => {
                write!(f, "read limit of {max_bytes} bytes exceeded")
            }
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::PortEnumerationFailed(error) | Self::PortOpenFailed(error) => Some(error),
            Self::ReadFailed(error) | Self::WriteFailed(error) | Self::FlushFailed(error) => {
                Some(error)
            }
            _ => None,
        }
    }
}
