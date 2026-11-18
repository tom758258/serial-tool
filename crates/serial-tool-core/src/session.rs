use std::{io, time::Duration};

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

    /// Read exactly the requested byte count, across any number of transport reads.
    /// A failed read reports the bytes accumulated so far, leaving them buffered.
    pub fn read_fixed_length(&mut self, expected_bytes: usize) -> Result<Vec<u8>, Error> {
        if expected_bytes == 0 || expected_bytes > 1_048_576 {
            return Err(Error::InvalidConfiguration(
                "expected bytes must be in 1..=1048576",
            ));
        }
        while self.rx.len() < expected_bytes {
            let mut buffer = [0u8; 1024];
            let read_size = (expected_bytes - self.rx.len()).min(buffer.len());
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
        Ok(self.rx.drain(..expected_bytes).collect())
    }

    /// Wait for the first byte using the configured read timeout, then complete
    /// only after a full idle gap without another transport read returning data.
    pub fn read_until_idle(&mut self, idle_gap: Duration, max_bytes: usize) -> Result<Vec<u8>, Error> {
        if idle_gap.is_zero() || max_bytes == 0 || max_bytes > 1_048_576 {
            return Err(Error::InvalidConfiguration("invalid idle gap or max bytes"));
        }
        let mut received_any = !self.rx.is_empty();
        loop {
            if self.rx.len() >= max_bytes {
                return Err(Error::ReadLimitExceeded {
                    max_bytes,
                    partial: self.rx[..max_bytes].to_vec(),
                });
            }
            let mut buffer = [0u8; 1024];
            let read_size = (max_bytes - self.rx.len()).min(buffer.len());
            let read_result = if received_any {
                self.transport.read_with_timeout(&mut buffer[..read_size], idle_gap)
            } else {
                self.transport.read(&mut buffer[..read_size])
            };
            match read_result {
                Ok(0) => {
                    return Err(Error::ReadFailed(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "transport returned zero bytes",
                    )));
                }
                Ok(count) => {
                    self.rx.extend_from_slice(&buffer[..count]);
                    received_any = true;
                }
                Err(error)
                    if received_any
                        && matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) =>
                {
                    return Ok(self.rx.drain(..).collect());
                }
                Err(error) => return Err(Self::map_read_error(error, self.rx.clone())),
            }
        }
    }

    /// Consume only the partial bytes already reported by an ordinary Worker Job.
    /// Trailing bytes (if any) remain available for the next operation.
    pub fn consume_reported_rx(&mut self, partial: &[u8]) -> Result<(), Error> {
        if !self.rx.starts_with(partial) {
            return Err(Error::InvalidConfiguration(
                "reported RX does not match buffered RX",
            ));
        }
        self.rx.drain(..partial.len());
        Ok(())
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn buffered_len(&self) -> usize {
        self.rx.len()
    }

    /// Discard bytes already reported for an ordinary Worker Query read timeout.
    /// Does not touch the transport's pending RX or the normal read_until retry policy.
    pub fn discard_buffered_rx(&mut self) {
        self.rx.clear();
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
