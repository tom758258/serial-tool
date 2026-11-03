use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use crate::{Error, TcpSettings};

use super::Transport;

pub struct TcpTransport {
    stream: TcpStream,
    io_timeout: Duration,
}

impl TcpTransport {
    pub fn connect(settings: &TcpSettings) -> Result<Self, Error> {
        settings.validate()?;
        let target = (settings.host.as_str(), settings.port);
        let addresses = target
            .to_socket_addrs()
            .map_err(|source| Error::TcpConnectFailed {
                host: settings.host.clone().into_boxed_str(),
                port: settings.port,
                source,
            })?;
        let mut last_error =
            io::Error::new(io::ErrorKind::AddrNotAvailable, "no TCP address resolved");
        for address in addresses {
            match TcpStream::connect_timeout(&address, settings.connect_timeout) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(settings.io_timeout))
                        .and_then(|()| stream.set_write_timeout(Some(settings.io_timeout)))
                        .map_err(|source| Error::TcpConnectFailed {
                            host: settings.host.clone().into_boxed_str(),
                            port: settings.port,
                            source,
                        })?;
                    return Ok(Self {
                        stream,
                        io_timeout: settings.io_timeout,
                    });
                }
                Err(error) => last_error = error,
            }
        }
        Err(Error::TcpConnectFailed {
            host: settings.host.clone().into_boxed_str(),
            port: settings.port,
            source: last_error,
        })
    }

    /// Probe a connected socket without consuming RX or blocking the single I/O owner
    /// for the configured (potentially long) normal I/O timeout.
    pub(crate) fn pending(&self) -> Result<usize, Error> {
        self.stream
            .set_read_timeout(Some(Duration::from_millis(1)))
            .map_err(Error::ReadFailed)?;
        let mut buffer = [0u8; 4096];
        let peek_result = self.stream.peek(&mut buffer);
        // Restore the user-selected timeout even if peek failed.
        self.stream
            .set_read_timeout(Some(self.io_timeout))
            .map_err(Error::ReadFailed)?;
        match peek_result {
            Ok(0) => Err(Error::ReadFailed(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "TCP peer closed the connection",
            ))),
            Ok(count) => Ok(count),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(0)
            }
            Err(error) => Err(Error::ReadFailed(error)),
        }
    }
}

impl Transport for TcpTransport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        match self.stream.read(buffer)? {
            0 => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "TCP peer closed the connection",
            )),
            count => Ok(count),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stream.write_all(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}
