use std::{error, fmt, io};

#[derive(Debug)]
pub enum Error {
    InvalidConfiguration(&'static str),
    PortEnumerationFailed(serialport::Error),
    PortOpenFailed {
        port: Box<str>,
        source: serialport::Error,
    },
    ReadFailed(io::Error),
    ReadAvailabilityFailed(serialport::Error),
    WriteFailed(io::Error),
    FlushFailed(io::Error),
    Timeout {
        partial: Vec<u8>,
    },
    ReadLimitExceeded {
        max_bytes: usize,
        partial: Vec<u8>,
    },
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
            Self::PortOpenFailed { port, source } => {
                if matches!(
                    source.kind(),
                    serialport::ErrorKind::NoDevice
                        | serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied)
                ) {
                    write!(
                        f,
                        "Cannot open {port}. The port may be unavailable, already in use by another application, or blocked by permissions. Close other serial terminals, check the device connection, and try again."
                    )
                } else {
                    write!(f, "Cannot open {port}: {source}")
                }
            }
            Self::ReadFailed(error) => write!(f, "read failed: {error}"),
            Self::ReadAvailabilityFailed(error) => write!(f, "read availability failed: {error}"),
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
            Self::PortOpenFailed { source, .. } => Some(source),
            Self::PortEnumerationFailed(error) => Some(error),
            Self::ReadAvailabilityFailed(error) => Some(error),
            Self::ReadFailed(error) | Self::WriteFailed(error) | Self::FlushFailed(error) => {
                Some(error)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_open_unavailable_or_permission_denied_is_actionable() {
        for kind in [
            serialport::ErrorKind::NoDevice,
            serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied),
        ] {
            let error = Error::PortOpenFailed {
                port: "COM4".into(),
                source: serialport::Error::new(kind, "denied by OS"),
            };

            assert_eq!(
                error.to_string(),
                "Cannot open COM4. The port may be unavailable, already in use by another application, or blocked by permissions. Close other serial terminals, check the device connection, and try again."
            );
            let source = error::Error::source(&error)
                .unwrap()
                .downcast_ref::<serialport::Error>()
                .unwrap();
            assert_eq!(source.kind(), kind);
            assert_eq!(source.to_string(), "denied by OS");
        }
    }

    #[test]
    fn port_open_other_error_preserves_detail() {
        let kind = serialport::ErrorKind::InvalidInput;
        let error = Error::PortOpenFailed {
            port: "COM4".into(),
            source: serialport::Error::new(kind, "invalid setting"),
        };

        assert_eq!(error.to_string(), "Cannot open COM4: invalid setting");
        let source = error::Error::source(&error)
            .unwrap()
            .downcast_ref::<serialport::Error>()
            .unwrap();
        assert_eq!(source.kind(), kind);
        assert_eq!(source.to_string(), "invalid setting");
    }
}
