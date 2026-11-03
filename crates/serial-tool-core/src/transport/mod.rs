mod serial;
mod simulation;
mod tcp;

pub use serial::SerialTransport;
pub use simulation::SimulationTransport;
pub use tcp::TcpTransport;

use std::io;

pub trait Transport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize>;
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
}
