mod serial;
mod simulation;

pub use serial::SerialTransport;
pub use simulation::SimulationTransport;

use std::io;

pub trait Transport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize>;
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
}
