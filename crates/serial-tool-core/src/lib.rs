pub mod config;
pub mod discovery;
pub mod error;
pub mod session;
pub mod step;
pub mod transport;

pub use config::{DataBits, FlowControl, Parity, SerialSettings, StopBits};
pub use discovery::{PortInfo, PortType, UsbInfo, list_ports};
pub use error::Error;
pub use session::SerialSession;
pub use step::{
    Direction, ExecutionFailure, InvalidStepId, MAX_READ_BYTES, RunError, RunReport, Step, StepId,
    StepKind, StepOutcome, StepResult, StepRunner, Transcript, TranscriptEntry, ValidationError,
    ValidationReason,
};
pub use transport::{SerialTransport, SimulationTransport, Transport};
