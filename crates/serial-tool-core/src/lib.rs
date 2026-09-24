pub mod config;
pub mod discovery;
pub mod error;
pub mod session;
pub mod transport;

pub use config::{DataBits, FlowControl, Parity, SerialSettings, StopBits};
pub use discovery::{PortInfo, PortType, UsbInfo, list_ports};
pub use error::Error;
pub use session::SerialSession;
pub use transport::{SerialTransport, SimulationTransport, Transport};
