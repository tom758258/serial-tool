use std::io::{self, Read, Write};

use crate::{Error, SerialSettings};

use super::Transport;

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    pub fn open(settings: &SerialSettings) -> Result<Self, Error> {
        settings.validate()?;
        let port = serialport::new(&settings.port, settings.baud_rate)
            .data_bits(settings.data_bits.to_serialport())
            .parity(settings.parity.to_serialport())
            .stop_bits(settings.stop_bits.to_serialport())
            .flow_control(settings.flow_control.to_serialport())
            .timeout(settings.timeout)
            .open()
            .map_err(Error::PortOpenFailed)?;
        Ok(Self { port })
    }
}

impl Transport for SerialTransport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.port.read(buffer)
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.port.write_all(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.port.flush()
    }
}
