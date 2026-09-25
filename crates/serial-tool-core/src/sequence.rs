use std::{error, fmt, fs, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    DataBits, FlowControl, InvalidStepId, Parity, SerialSettings, Step, StepId, StepKind, StopBits,
    ValidationError, validate_steps,
};

pub const SEQUENCE_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceSerialConfig {
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow_control: FlowControl,
    pub timeout: Duration,
}

impl SequenceSerialConfig {
    pub fn matches_serial_settings(&self, settings: &SerialSettings) -> bool {
        self.baud_rate == settings.baud_rate
            && self.data_bits == settings.data_bits
            && self.parity == settings.parity
            && self.stop_bits == settings.stop_bits
            && self.flow_control == settings.flow_control
            && self.timeout == settings.timeout
    }

    pub fn serial_settings_for_port(&self, port: impl Into<String>) -> SerialSettings {
        SerialSettings {
            port: port.into(),
            baud_rate: self.baud_rate,
            data_bits: self.data_bits,
            parity: self.parity,
            stop_bits: self.stop_bits,
            flow_control: self.flow_control,
            timeout: self.timeout,
        }
    }

    fn validate(&self) -> Result<(), SequenceError> {
        if self.baud_rate == 0 {
            return Err(SequenceError::InvalidSerial("baud_rate must be nonzero"));
        }
        duration_ms(self.timeout).map_err(|_| {
            SequenceError::InvalidSerial("timeout must be representable as u64 milliseconds")
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub sequence_version: u64,
    pub serial: SequenceSerialConfig,
    pub steps: Vec<Step>,
}

impl Sequence {
    pub fn validate(&self) -> Result<(), SequenceError> {
        if self.sequence_version != SEQUENCE_VERSION {
            return Err(SequenceError::UnsupportedVersion(self.sequence_version));
        }
        self.serial.validate()?;
        if self.steps.is_empty() {
            return Err(SequenceError::EmptySteps);
        }
        validate_steps(&self.steps).map_err(SequenceError::StepValidation)?;
        validate_durations(&self.steps)?;
        Ok(())
    }

    pub fn from_json_str(json: &str) -> Result<Self, SequenceError> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(SequenceError::Json)?;
        match value
            .get("sequence_version")
            .and_then(serde_json::Value::as_u64)
        {
            Some(SEQUENCE_VERSION) => {}
            Some(version) => return Err(SequenceError::UnsupportedVersion(version)),
            None => return Err(SequenceError::InvalidVersion),
        }
        let wire: WireSequence = serde_json::from_value(value).map_err(SequenceError::Json)?;
        let sequence = Self {
            sequence_version: wire.sequence_version,
            serial: wire.serial.try_into()?,
            steps: wire
                .steps
                .into_iter()
                .map(Step::try_from)
                .collect::<Result<_, _>>()?,
        };
        sequence.validate()?;
        Ok(sequence)
    }

    pub fn to_json_pretty(&self) -> Result<String, SequenceError> {
        self.validate()?;
        let wire = WireSequence {
            sequence_version: self.sequence_version,
            serial: WireSerial::try_from(&self.serial)?,
            steps: self
                .steps
                .iter()
                .map(WireStep::try_from)
                .collect::<Result<_, _>>()?,
        };
        let mut json = serde_json::to_string_pretty(&wire).map_err(SequenceError::Serialization)?;
        json.push('\n');
        Ok(json)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, SequenceError> {
        let json = fs::read_to_string(path).map_err(SequenceError::Io)?;
        Self::from_json_str(&json)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), SequenceError> {
        let json = self.to_json_pretty()?;
        fs::write(path, json).map_err(SequenceError::Io)
    }
}

#[derive(Debug)]
pub enum SequenceError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidVersion,
    UnsupportedVersion(u64),
    InvalidSerial(&'static str),
    InvalidStepId(InvalidStepId),
    InvalidHex {
        step_id: String,
        field: &'static str,
    },
    EmptySteps,
    StepValidation(ValidationError),
    InvalidDuration {
        step_id: StepId,
    },
    Serialization(serde_json::Error),
}

impl fmt::Display for SequenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "sequence file I/O failed: {error}"),
            Self::Json(error) => write!(f, "invalid sequence JSON/schema: {error}"),
            Self::InvalidVersion => f.write_str("invalid sequence_version: expected integer 1"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported sequence_version: {version}")
            }
            Self::InvalidSerial(message) => write!(f, "invalid sequence serial config: {message}"),
            Self::InvalidStepId(error) => error.fmt(f),
            Self::InvalidHex { step_id, field } => {
                write!(f, "invalid {field} hex for step {step_id}")
            }
            Self::EmptySteps => f.write_str("sequence steps must not be empty"),
            Self::StepValidation(error) => error.fmt(f),
            Self::InvalidDuration { step_id } => write!(
                f,
                "step {step_id} duration is not representable as u64 milliseconds"
            ),
            Self::Serialization(error) => write!(f, "sequence serialization failed: {error}"),
        }
    }
}

impl error::Error for SequenceError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) | Self::Serialization(error) => Some(error),
            Self::InvalidStepId(error) => Some(error),
            Self::StepValidation(error) => Some(error),
            _ => None,
        }
    }
}

fn duration_ms(duration: Duration) -> Result<u64, ()> {
    if !duration.subsec_nanos().is_multiple_of(1_000_000) {
        return Err(());
    }
    u64::try_from(duration.as_millis()).map_err(|_| ())
}

fn validate_durations(steps: &[Step]) -> Result<(), SequenceError> {
    for step in steps {
        match &step.kind {
            StepKind::Wait(duration) if duration_ms(*duration).is_err() => {
                return Err(SequenceError::InvalidDuration {
                    step_id: step.id.clone(),
                });
            }
            StepKind::Repeat { steps, .. } => validate_durations(steps)?,
            _ => {}
        }
    }
    Ok(())
}

fn parse_hex(hex: &str, step_id: &str, field: &'static str) -> Result<Vec<u8>, SequenceError> {
    let digits: Vec<_> = hex
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if !digits.len().is_multiple_of(2) {
        return Err(SequenceError::InvalidHex {
            step_id: step_id.into(),
            field,
        });
    }
    digits
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16);
            let low = (pair[1] as char).to_digit(16);
            match (high, low) {
                (Some(high), Some(low)) => Ok(((high << 4) | low) as u8),
                _ => Err(SequenceError::InvalidHex {
                    step_id: step_id.into(),
                    field,
                }),
            }
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSequence {
    sequence_version: u64,
    serial: WireSerial,
    steps: Vec<WireStep>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSerial {
    baud_rate: u32,
    data_bits: u8,
    parity: String,
    stop_bits: u8,
    flow_control: String,
    timeout_ms: u64,
}

impl TryFrom<WireSerial> for SequenceSerialConfig {
    type Error = SequenceError;

    fn try_from(wire: WireSerial) -> Result<Self, Self::Error> {
        Ok(Self {
            baud_rate: wire.baud_rate,
            data_bits: match wire.data_bits {
                5 => DataBits::Five,
                6 => DataBits::Six,
                7 => DataBits::Seven,
                8 => DataBits::Eight,
                _ => {
                    return Err(SequenceError::InvalidSerial(
                        "data_bits must be 5, 6, 7, or 8",
                    ));
                }
            },
            parity: match wire.parity.as_str() {
                "none" => Parity::None,
                "odd" => Parity::Odd,
                "even" => Parity::Even,
                _ => return Err(SequenceError::InvalidSerial("invalid parity")),
            },
            stop_bits: match wire.stop_bits {
                1 => StopBits::One,
                2 => StopBits::Two,
                _ => return Err(SequenceError::InvalidSerial("stop_bits must be 1 or 2")),
            },
            flow_control: match wire.flow_control.as_str() {
                "none" => FlowControl::None,
                "software" => FlowControl::Software,
                "hardware" => FlowControl::Hardware,
                _ => return Err(SequenceError::InvalidSerial("invalid flow_control")),
            },
            timeout: Duration::from_millis(wire.timeout_ms),
        })
    }
}

impl TryFrom<&SequenceSerialConfig> for WireSerial {
    type Error = SequenceError;

    fn try_from(config: &SequenceSerialConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            baud_rate: config.baud_rate,
            data_bits: match config.data_bits {
                DataBits::Five => 5,
                DataBits::Six => 6,
                DataBits::Seven => 7,
                DataBits::Eight => 8,
            },
            parity: match config.parity {
                Parity::None => "none",
                Parity::Odd => "odd",
                Parity::Even => "even",
            }
            .into(),
            stop_bits: match config.stop_bits {
                StopBits::One => 1,
                StopBits::Two => 2,
            },
            flow_control: match config.flow_control {
                FlowControl::None => "none",
                FlowControl::Software => "software",
                FlowControl::Hardware => "hardware",
            }
            .into(),
            timeout_ms: duration_ms(config.timeout).map_err(|_| {
                SequenceError::InvalidSerial("timeout must be representable as u64 milliseconds")
            })?,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WireStep {
    SendText {
        id: String,
        text: String,
    },
    SendBytes {
        id: String,
        hex: String,
    },
    Wait {
        id: String,
        duration_ms: u64,
    },
    Read {
        id: String,
        max_bytes: usize,
    },
    ReadUntil {
        id: String,
        delimiter_hex: String,
        max_bytes: usize,
    },
    Repeat {
        id: String,
        count: usize,
        steps: Vec<WireStep>,
    },
}

impl TryFrom<WireStep> for Step {
    type Error = SequenceError;

    fn try_from(wire: WireStep) -> Result<Self, Self::Error> {
        let (id, kind) = match wire {
            WireStep::SendText { id, text } => (id, StepKind::SendText(text)),
            WireStep::SendBytes { id, hex } => {
                let bytes = parse_hex(&hex, &id, "hex")?;
                (id, StepKind::SendBytes(bytes))
            }
            WireStep::Wait { id, duration_ms } => {
                (id, StepKind::Wait(Duration::from_millis(duration_ms)))
            }
            WireStep::Read { id, max_bytes } => (id, StepKind::Read { max_bytes }),
            WireStep::ReadUntil {
                id,
                delimiter_hex,
                max_bytes,
            } => {
                let delimiter = parse_hex(&delimiter_hex, &id, "delimiter_hex")?;
                (
                    id,
                    StepKind::ReadUntil {
                        delimiter,
                        max_bytes,
                    },
                )
            }
            WireStep::Repeat { id, count, steps } => {
                let steps = steps
                    .into_iter()
                    .map(Step::try_from)
                    .collect::<Result<_, _>>()?;
                (id, StepKind::Repeat { count, steps })
            }
        };
        Ok(Step {
            id: StepId::new(id).map_err(SequenceError::InvalidStepId)?,
            kind,
        })
    }
}

impl TryFrom<&Step> for WireStep {
    type Error = SequenceError;

    fn try_from(step: &Step) -> Result<Self, Self::Error> {
        let id = step.id.as_str().to_owned();
        Ok(match &step.kind {
            StepKind::SendText(text) => Self::SendText {
                id,
                text: text.clone(),
            },
            StepKind::SendBytes(bytes) => Self::SendBytes {
                id,
                hex: hex(bytes),
            },
            StepKind::Wait(duration) => Self::Wait {
                id,
                duration_ms: duration_ms(*duration).map_err(|_| {
                    SequenceError::InvalidDuration {
                        step_id: step.id.clone(),
                    }
                })?,
            },
            StepKind::Read { max_bytes } => Self::Read {
                id,
                max_bytes: *max_bytes,
            },
            StepKind::ReadUntil {
                delimiter,
                max_bytes,
            } => Self::ReadUntil {
                id,
                delimiter_hex: hex(delimiter),
                max_bytes: *max_bytes,
            },
            StepKind::Repeat { count, steps } => Self::Repeat {
                id,
                count: *count,
                steps: steps.iter().map(Self::try_from).collect::<Result<_, _>>()?,
            },
        })
    }
}
