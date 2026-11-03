use std::time::Duration;

use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

impl DataBits {
    pub(crate) fn to_serialport(self) -> serialport::DataBits {
        match self {
            Self::Five => serialport::DataBits::Five,
            Self::Six => serialport::DataBits::Six,
            Self::Seven => serialport::DataBits::Seven,
            Self::Eight => serialport::DataBits::Eight,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
}

impl Parity {
    pub(crate) fn to_serialport(self) -> serialport::Parity {
        match self {
            Self::None => serialport::Parity::None,
            Self::Odd => serialport::Parity::Odd,
            Self::Even => serialport::Parity::Even,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

impl StopBits {
    pub(crate) fn to_serialport(self) -> serialport::StopBits {
        match self {
            Self::One => serialport::StopBits::One,
            Self::Two => serialport::StopBits::Two,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowControl {
    None,
    Software,
    Hardware,
}

impl FlowControl {
    pub(crate) fn to_serialport(self) -> serialport::FlowControl {
        match self {
            Self::None => serialport::FlowControl::None,
            Self::Software => serialport::FlowControl::Software,
            Self::Hardware => serialport::FlowControl::Hardware,
        }
    }
}

/// The timeout applies to serial port I/O as supported by the underlying driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialSettings {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow_control: FlowControl,
    pub timeout: Duration,
}

impl SerialSettings {
    pub fn validate(&self) -> Result<(), Error> {
        if self.port.trim().is_empty() {
            return Err(Error::InvalidConfiguration("port name must not be empty"));
        }
        if self.baud_rate == 0 {
            return Err(Error::InvalidConfiguration("baud rate must be nonzero"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpSettings {
    pub host: String,
    pub port: u16,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
}

impl Default for TcpSettings {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 0,
            connect_timeout: Duration::from_millis(3000),
            io_timeout: Duration::from_millis(1000),
        }
    }
}

impl TcpSettings {
    pub fn validate(&self) -> Result<(), Error> {
        if self.host.trim().is_empty() {
            return Err(Error::InvalidConfiguration("TCP host must not be empty"));
        }
        if self.host.contains("://") || self.host.contains('/') {
            return Err(Error::InvalidConfiguration("TCP host must not be a URL"));
        }
        if self.port == 0 {
            return Err(Error::InvalidConfiguration("TCP port must be 1..65535"));
        }
        self.validate_timeouts()
    }

    pub(crate) fn validate_timeouts(&self) -> Result<(), Error> {
        for timeout in [self.connect_timeout, self.io_timeout] {
            if timeout.is_zero()
                || !timeout.subsec_nanos().is_multiple_of(1_000_000)
                || u64::try_from(timeout.as_millis()).is_err()
            {
                return Err(Error::InvalidConfiguration(
                    "TCP timeouts must be positive integer milliseconds",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_settings_to_serialport() {
        assert_eq!(DataBits::Five.to_serialport(), serialport::DataBits::Five);
        assert_eq!(DataBits::Eight.to_serialport(), serialport::DataBits::Eight);
        assert_eq!(Parity::Odd.to_serialport(), serialport::Parity::Odd);
        assert_eq!(Parity::Even.to_serialport(), serialport::Parity::Even);
        assert_eq!(StopBits::Two.to_serialport(), serialport::StopBits::Two);
        assert_eq!(
            FlowControl::Software.to_serialport(),
            serialport::FlowControl::Software
        );
        assert_eq!(
            FlowControl::Hardware.to_serialport(),
            serialport::FlowControl::Hardware
        );
    }
}
