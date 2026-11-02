use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant},
};

use super::Transport;

#[derive(Debug, Default)]
pub struct SimulationTransport {
    rx_chunks: VecDeque<Vec<u8>>,
    tx: Vec<u8>,
    loopback: bool,
    periodic_rx: Option<PeriodicRx>,
}

#[derive(Debug)]
struct PeriodicRx {
    next_due: Instant,
    next_value: u32,
}

impl SimulationTransport {
    pub fn with_rx_chunks(chunks: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            rx_chunks: chunks.into_iter().collect(),
            tx: Vec::new(),
            loopback: false,
            periodic_rx: None,
        }
    }

    pub fn loopback() -> Self {
        Self {
            loopback: true,
            ..Self::default()
        }
    }

    pub fn periodic_rx() -> Self {
        Self {
            periodic_rx: Some(PeriodicRx {
                next_due: Instant::now() + Duration::from_millis(100),
                next_value: 1,
            }),
            ..Self::default()
        }
    }

    pub fn captured_tx(&self) -> &[u8] {
        &self.tx
    }

    pub fn bytes_to_read(&self) -> usize {
        let pending: usize = self.rx_chunks.iter().map(Vec::len).sum();
        if pending == 0
            && self
                .periodic_rx
                .as_ref()
                .is_some_and(|periodic| Instant::now() >= periodic.next_due)
        {
            4
        } else {
            pending
        }
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
        if self.rx_chunks.is_empty()
            && let Some(periodic) = self.periodic_rx.as_mut()
            && Instant::now() >= periodic.next_due
        {
            self.rx_chunks
                .push_back(periodic.next_value.to_be_bytes().to_vec());
            periodic.next_value = periodic.next_value.wrapping_add(1);
            periodic.next_due = Instant::now() + Duration::from_millis(100);
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
