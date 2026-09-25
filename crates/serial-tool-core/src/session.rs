use std::io;

use crate::{Error, Transport};

pub struct SerialSession<T: Transport> {
    transport: T,
    rx: Vec<u8>,
}

impl<T: Transport> SerialSession<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            rx: Vec::new(),
        }
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.transport.write_all(bytes).map_err(Error::WriteFailed)
    }

    pub fn flush(&mut self) -> Result<(), Error> {
        self.transport.flush().map_err(Error::FlushFailed)
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if !self.rx.is_empty() {
            let count = buffer.len().min(self.rx.len());
            buffer[..count].copy_from_slice(&self.rx[..count]);
            self.rx.drain(..count);
            return Ok(count);
        }
        self.transport
            .read(buffer)
            .map_err(|error| Self::map_read_error(error, Vec::new()))
    }

    /// Returns bytes through the delimiter. Partial bytes remain buffered on failure.
    pub fn read_until(&mut self, delimiter: &[u8], max_bytes: usize) -> Result<Vec<u8>, Error> {
        if delimiter.is_empty() {
            return Err(Error::InvalidConfiguration("delimiter must not be empty"));
        }
        if max_bytes == 0 || delimiter.len() > max_bytes {
            return Err(Error::InvalidConfiguration(
                "max bytes must be at least the delimiter length",
            ));
        }

        loop {
            if let Some(end) = self
                .rx
                .windows(delimiter.len())
                .position(|window| window == delimiter)
                .map(|start| start + delimiter.len())
                .filter(|end| *end <= max_bytes)
            {
                return Ok(self.rx.drain(..end).collect());
            }
            if self.rx.len() >= max_bytes {
                return Err(Error::ReadLimitExceeded {
                    max_bytes,
                    partial: self.rx[..max_bytes].to_vec(),
                });
            }

            let mut buffer = [0u8; 1024];
            let read_size = (max_bytes - self.rx.len()).min(buffer.len());
            let count = self
                .transport
                .read(&mut buffer[..read_size])
                .map_err(|error| Self::map_read_error(error, self.rx.clone()))?;
            if count == 0 {
                return Err(Error::ReadFailed(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "transport returned zero bytes",
                )));
            }
            self.rx.extend_from_slice(&buffer[..count]);
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn buffered_len(&self) -> usize {
        self.rx.len()
    }

    fn map_read_error(error: io::Error, partial: Vec<u8>) -> Error {
        if matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ) {
            Error::Timeout { partial }
        } else {
            Error::ReadFailed(error)
        }
    }
}
