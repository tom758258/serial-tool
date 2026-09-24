use std::{collections::VecDeque, io};

use super::Transport;

#[derive(Debug, Default)]
pub struct SimulationTransport {
    rx_chunks: VecDeque<Vec<u8>>,
    tx: Vec<u8>,
    loopback: bool,
}

impl SimulationTransport {
    pub fn with_rx_chunks(chunks: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            rx_chunks: chunks.into_iter().collect(),
            tx: Vec::new(),
            loopback: false,
        }
    }

    pub fn loopback() -> Self {
        Self {
            loopback: true,
            ..Self::default()
        }
    }

    pub fn captured_tx(&self) -> &[u8] {
        &self.tx
    }
}

impl Transport for SimulationTransport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        while self.rx_chunks.front().is_some_and(Vec::is_empty) {
            self.rx_chunks.pop_front();
        }
        let chunk = self
            .rx_chunks
            .front_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "simulation RX exhausted"))?;
        let count = buffer.len().min(chunk.len());
        buffer[..count].copy_from_slice(&chunk[..count]);
        chunk.drain(..count);
        if chunk.is_empty() {
            self.rx_chunks.pop_front();
        }
        Ok(count)
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.tx.extend_from_slice(bytes);
        if self.loopback {
            self.rx_chunks.push_back(bytes.to_vec());
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
