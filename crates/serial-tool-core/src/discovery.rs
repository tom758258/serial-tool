use crate::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    pub port_name: String,
    pub port_type: PortType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortType {
    Usb(UsbInfo),
    Pci,
    Bluetooth,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbInfo {
    pub vid: u16,
    pub pid: u16,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

pub fn list_ports() -> Result<Vec<PortInfo>, Error> {
    serialport::available_ports()
        .map_err(Error::PortEnumerationFailed)
        .map(|ports| {
            ports
                .into_iter()
                .map(|port| PortInfo {
                    port_name: port.port_name,
                    port_type: match port.port_type {
                        serialport::SerialPortType::UsbPort(usb) => PortType::Usb(UsbInfo {
                            vid: usb.vid,
                            pid: usb.pid,
                            serial_number: usb.serial_number,
                            manufacturer: usb.manufacturer,
                            product: usb.product,
                        }),
                        serialport::SerialPortType::PciPort => PortType::Pci,
                        serialport::SerialPortType::BluetoothPort => PortType::Bluetooth,
                        serialport::SerialPortType::Unknown => PortType::Unknown,
                    },
                })
                .collect()
        })
}
