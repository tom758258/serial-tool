mod serial;
mod simulation;
mod tcp;

pub use serial::SerialTransport;
pub use simulation::SimulationTransport;
pub use tcp::TcpTransport;

use std::{io, time::Duration};

pub trait Transport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize>;

    /// A bounded read for Idle Gap reception. Implementations must honor the timeout.
    fn read_with_timeout(&mut self, _buffer: &mut [u8], _timeout: Duration) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "timed read is unsupported"))
    }
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
}
