use std::{io, path::PathBuf, time::Duration};

use chrono::{SecondsFormat, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use serial_tool_core::{
    DataBits, Error, ExecutionFailure, FlowControl, MAX_READ_BYTES, Parity, PortInfo, PortType,
    SequenceError, SerialSession, SerialSettings, SerialTransport, StopBits, TcpSettings,
    TcpTransport, Transport, list_ports,
};

mod auto_tx;
mod help;
mod sequence;
mod sequence_result;
mod terminal;
mod worker;
use sequence::SequenceArgs;
use worker::WorkerArgs;

#[derive(Debug, Parser)]
#[command(name = "serial-tool", version, about = "Serial port diagnostic CLI")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open the bundled offline user guide in the default browser.
    UserGuide(help::UserGuideArgs),
    /// Show static tool identity and Worker availability.
    Manifest(OutputArgs),
    /// Discover serial ports without opening them.
    ListPorts(OutputArgs),
    /// Write raw bytes and flush once.
    Send(SendArgs),
    /// Read raw bytes once, up to max-bytes.
    Receive(ReceiveArgs),
    /// Write raw bytes, then read through a delimiter.
    Query(QueryArgs),
    /// Validate or run a Serial Sequence file.
    Sequence(SequenceArgs),
    /// Run the local Serial Worker control plane.
    Worker(WorkerArgs),
    /// Open a persistent human-interactive serial terminal.
    Terminal(terminal::TerminalArgs),
    /// Repeatedly send a payload while continuously receiving until Ctrl+C.
    AutoTx(auto_tx::AutoTxArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum Format {
    #[default]
    Text,
    Json,
    Jsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum RxDisplay {
    #[default]
    Hex,
    Text,
    Both,
}

#[derive(Debug, Args)]
struct OutputArgs {
    #[arg(long, value_enum, default_value_t = Format::Text, conflicts_with = "json")]
    format: Format,
    #[arg(long, help = "Alias for --format json")]
    json: bool,
}

impl OutputArgs {
    fn machine(&self) -> bool {
        self.json || !matches!(self.format, Format::Text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(crate) enum TransportKind {
    #[default]
    Serial,
    Tcp,
}

#[derive(Debug, Args)]
pub(crate) struct SerialArgs {
    #[arg(long, value_enum, default_value_t = TransportKind::Serial)]
    transport: TransportKind,
    #[arg(long)]
    port: Option<String>,
    #[arg(long)]
    baud: Option<u32>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(5..=8))]
    data_bits: Option<u8>,
    #[arg(long, value_enum)]
    parity: Option<CliParity>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=2))]
    stop_bits: Option<u8>,
    #[arg(long, value_enum)]
    flow_control: Option<CliFlowControl>,
    #[arg(long)]
    timeout_ms: Option<u64>,
    #[arg(long)]
    host: Option<String>,
    #[arg(long)]
    tcp_port: Option<u16>,
    #[arg(long)]
    connect_timeout_ms: Option<u64>,
    #[arg(long)]
    io_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) enum ConnectionSettings {
    Serial(SerialSettings),
    Tcp(TcpSettings),
}

impl ConnectionSettings {
    pub(crate) fn target(&self) -> String {
        match self {
            Self::Serial(settings) => settings.port.clone(),
            Self::Tcp(settings) => format!("{}:{}", settings.host, settings.port),
        }
    }

    pub(crate) fn output(&self, simulated: bool) -> Value {
        match self {
            Self::Serial(settings) => {
                let mut value =
                    json!({"transport": "serial", "serial_settings": settings_json(settings)});
                if simulated {
                    value["serial_settings"]
                        .as_object_mut()
                        .unwrap()
                        .remove("port");
                } else {
                    value["port"] = json!(settings.port);
                }
                value
            }
            Self::Tcp(settings) => {
                let mut tcp = tcp_settings_json(settings);
                if simulated {
                    tcp.as_object_mut().unwrap().remove("host");
                    tcp.as_object_mut().unwrap().remove("port");
                }
                json!({"transport": "tcp", "tcp_settings": tcp})
            }
        }
    }

    fn one_shot_output(&self) -> Value {
        let mut value = self.output(false);
        if matches!(self, Self::Serial(_)) {
            value.as_object_mut().unwrap().remove("transport");
        }
        value
    }

    pub(crate) fn open(&self) -> Result<DeviceTransport, Error> {
        match self {
            Self::Serial(settings) => SerialTransport::open(settings).map(DeviceTransport::Serial),
            Self::Tcp(settings) => TcpTransport::connect(settings).map(DeviceTransport::Tcp),
        }
    }
}

pub(crate) enum DeviceTransport {
    Serial(SerialTransport),
    Tcp(TcpTransport),
}

impl Transport for DeviceTransport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Serial(t) => t.read(buffer),
            Self::Tcp(t) => t.read(buffer),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Self::Serial(t) => t.write_all(bytes),
            Self::Tcp(t) => t.write_all(bytes),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Serial(t) => t.flush(),
            Self::Tcp(t) => t.flush(),
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliParity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliFlowControl {
    None,
    Software,
    Hardware,
}

impl SerialArgs {
    pub(crate) fn settings(&self, simulated: bool) -> Result<ConnectionSettings, CliError> {
        match self.transport {
            TransportKind::Serial => {
                if self.host.is_some()
                    || self.tcp_port.is_some()
                    || self.connect_timeout_ms.is_some()
                    || self.io_timeout_ms.is_some()
                {
                    return Err(CliError::Input("serial transport forbids TCP options"));
                }
                let baud = self
                    .baud
                    .ok_or(CliError::Input("serial transport requires --baud"))?;
                let port = if simulated {
                    if self.port.is_some() {
                        return Err(CliError::Input("simulate forbids --port"));
                    }
                    "simulation".to_string()
                } else {
                    self.port
                        .clone()
                        .ok_or(CliError::Input("serial live requires --port"))?
                };
                let settings = SerialSettings {
                    port,
                    baud_rate: baud,
                    data_bits: match self.data_bits.unwrap_or(8) {
                        5 => DataBits::Five,
                        6 => DataBits::Six,
                        7 => DataBits::Seven,
                        _ => DataBits::Eight,
                    },
                    parity: match self.parity.unwrap_or(CliParity::None) {
                        CliParity::None => Parity::None,
                        CliParity::Odd => Parity::Odd,
                        CliParity::Even => Parity::Even,
                    },
                    stop_bits: if self.stop_bits.unwrap_or(1) == 1 {
                        StopBits::One
                    } else {
                        StopBits::Two
                    },
                    flow_control: match self.flow_control.unwrap_or(CliFlowControl::None) {
                        CliFlowControl::None => FlowControl::None,
                        CliFlowControl::Software => FlowControl::Software,
                        CliFlowControl::Hardware => FlowControl::Hardware,
                    },
                    timeout: Duration::from_millis(self.timeout_ms.unwrap_or(1000)),
                };
                settings.validate().map_err(CliError::Validation)?;
                Ok(ConnectionSettings::Serial(settings))
            }
            TransportKind::Tcp => {
                if self.port.is_some()
                    || self.baud.is_some()
                    || self.data_bits.is_some()
                    || self.parity.is_some()
                    || self.stop_bits.is_some()
                    || self.flow_control.is_some()
                    || self.timeout_ms.is_some()
                {
                    return Err(CliError::Input("TCP transport forbids UART options"));
                }
                let settings = TcpSettings {
                    host: if simulated {
                        if self.host.is_some() || self.tcp_port.is_some() {
                            return Err(CliError::Input(
                                "TCP simulation forbids --host and --tcp-port",
                            ));
                        }
                        String::new()
                    } else {
                        self.host
                            .clone()
                            .ok_or(CliError::Input("TCP live requires --host"))?
                    },
                    port: if simulated {
                        0
                    } else {
                        self.tcp_port
                            .ok_or(CliError::Input("TCP live requires --tcp-port"))?
                    },
                    connect_timeout: Duration::from_millis(self.connect_timeout_ms.unwrap_or(3000)),
                    io_timeout: Duration::from_millis(self.io_timeout_ms.unwrap_or(1000)),
                };
                if simulated {
                    if settings.connect_timeout.is_zero() || settings.io_timeout.is_zero() {
                        return Err(CliError::Input("TCP timeouts must be positive"));
                    }
                } else {
                    settings.validate().map_err(CliError::Validation)?;
                }
                Ok(ConnectionSettings::Tcp(settings))
            }
        }
    }
}

#[derive(Debug, Args)]
#[group(id = "tx", required = true, multiple = false, args = ["text", "hex"])]
struct TxArgs {
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    hex: Option<String>,
}

impl TxArgs {
    fn bytes(&self) -> Result<Vec<u8>, CliError> {
        parse_bytes(self.text.as_deref(), self.hex.as_deref())
    }
}

#[derive(Debug, Args)]
#[group(id = "payload", required = true, multiple = false, args = ["text", "hex", "file"])]
struct PayloadArgs {
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    hex: Option<String>,
    #[arg(long, help = "Read raw bytes from a nonempty file")]
    file: Option<PathBuf>,
}

impl PayloadArgs {
    fn bytes(&self) -> Result<Vec<u8>, CliError> {
        if let Some(path) = &self.file {
            let bytes = std::fs::read(path).map_err(|error| CliError::FileInput {
                path: path.clone(),
                error,
            })?;
            if bytes.is_empty() {
                return Err(CliError::Input("file input must not be empty"));
            }
            Ok(bytes)
        } else {
            parse_bytes(self.text.as_deref(), self.hex.as_deref())
        }
    }
}

#[derive(Debug, Args)]
#[group(id = "delimiter", required = true, multiple = false, args = ["until_text", "until_hex"])]
struct DelimiterArgs {
    #[arg(long)]
    until_text: Option<String>,
    #[arg(long)]
    until_hex: Option<String>,
}

impl DelimiterArgs {
    fn bytes(&self) -> Result<Vec<u8>, CliError> {
        parse_bytes(self.until_text.as_deref(), self.until_hex.as_deref())
    }
}

#[derive(Debug, Args)]
struct SendArgs {
    #[command(flatten)]
    serial: SerialArgs,
    #[command(flatten)]
    tx: PayloadArgs,
    #[command(flatten)]
    output: OutputArgs,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ReceiveArgs {
    #[command(flatten)]
    serial: SerialArgs,
    #[arg(long, default_value_t = 1024)]
    max_bytes: usize,
    #[arg(long, value_enum, default_value_t = RxDisplay::Hex)]
    rx_display: RxDisplay,
    #[command(flatten)]
    output: OutputArgs,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct QueryArgs {
    #[command(flatten)]
    serial: SerialArgs,
    #[command(flatten)]
    tx: TxArgs,
    #[command(flatten)]
    delimiter: DelimiterArgs,
    #[arg(long, default_value_t = 1024)]
    max_bytes: usize,
    #[arg(long, value_enum, default_value_t = RxDisplay::Hex)]
    rx_display: RxDisplay,
    #[command(flatten)]
    output: OutputArgs,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug)]
enum CliError {
    Validation(Error),
    Input(&'static str),
    FileInput {
        path: PathBuf,
        error: std::io::Error,
    },
    Sequence(SequenceError),
    SequenceExecution(ExecutionFailure),
    Runtime(Error),
}

impl CliError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Validation(_) | Self::Input(_) | Self::FileInput { .. } | Self::Sequence(_) => 2,
            Self::Runtime(_) | Self::SequenceExecution(_) => 3,
        }
    }

    fn partial(&self) -> Option<&[u8]> {
        match self {
            Self::Runtime(error) => error.partial(),
            Self::SequenceExecution(error) => error.error.partial(),
            _ => None,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(error) | Self::Runtime(error) => write!(f, "{error}"),
            Self::Input(message) => write!(f, "{message}"),
            Self::FileInput { path, error } => {
                write!(f, "Cannot read file {}: {error}", path.display())
            }
            Self::Sequence(error) => write!(f, "{error}"),
            Self::SequenceExecution(error) => write!(f, "{error}"),
        }
    }
}

fn parse_bytes(text: Option<&str>, hex: Option<&str>) -> Result<Vec<u8>, CliError> {
    let bytes = match (text, hex) {
        (Some(text), None) => text.as_bytes().to_vec(),
        (None, Some(hex)) => {
            let digits: Vec<u8> = hex
                .bytes()
                .filter(|byte| !byte.is_ascii_whitespace())
                .collect();
            if !digits.len().is_multiple_of(2) {
                return Err(CliError::Input(
                    "hex input must contain an even number of digits",
                ));
            }
            let mut bytes = Vec::with_capacity(digits.len() / 2);
            for pair in digits.as_chunks::<2>().0 {
                let high = (pair[0] as char).to_digit(16);
                let low = (pair[1] as char).to_digit(16);
                match (high, low) {
                    (Some(high), Some(low)) => bytes.push(((high << 4) | low) as u8),
                    _ => return Err(CliError::Input("hex input contains a non-hex character")),
                }
            }
            bytes
        }
        _ => return Err(CliError::Input("exactly one byte input is required")),
    };
    if bytes.is_empty() {
        return Err(CliError::Input("byte input must not be empty"));
    }
    Ok(bytes)
}

fn max_bytes(value: usize) -> Result<usize, CliError> {
    if value == 0 || value > MAX_READ_BYTES {
        return Err(CliError::Input("max-bytes must be between 1 and 1048576"));
    }
    Ok(value)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(result, "{byte:02x}").expect("writing to String cannot fail");
    }
    result
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn error_json(command: &str, error: &CliError) -> Value {
    let mut value = json!({
        "event": "error", "schema_version": 2, "timestamp_utc": timestamp(),
        "ok": false, "command": command, "message": error.to_string(),
        "exit_code": error.exit_code(),
    });
    if let Some(partial) = error.partial() {
        value["partial_hex"] = json!(hex(partial));
        value["partial_bytes"] = json!(partial.len());
    }
    if let CliError::SequenceExecution(failure) = error {
        value["step_id"] = json!(failure.step_id.as_str());
        value["step_results"] = sequence_result::step_results_json(&failure.report);
        value["transcript"] = sequence_result::transcript_json(&failure.report);
    }
    value
}

fn settings_json(settings: &SerialSettings) -> Value {
    json!({
        "port": settings.port,
        "baud_rate": settings.baud_rate,
        "data_bits": match settings.data_bits { DataBits::Five => 5, DataBits::Six => 6, DataBits::Seven => 7, DataBits::Eight => 8 },
        "parity": match settings.parity { Parity::None => "none", Parity::Odd => "odd", Parity::Even => "even" },
        "stop_bits": match settings.stop_bits { StopBits::One => 1, StopBits::Two => 2 },
        "flow_control": match settings.flow_control { FlowControl::None => "none", FlowControl::Software => "software", FlowControl::Hardware => "hardware" },
        "timeout_ms": settings.timeout.as_millis(),
    })
}

pub(crate) fn tcp_settings_json(settings: &TcpSettings) -> Value {
    json!({
        "host": settings.host, "port": settings.port,
        "connect_timeout_ms": settings.connect_timeout.as_millis(),
        "io_timeout_ms": settings.io_timeout.as_millis(),
    })
}

fn port_json(port: &PortInfo) -> Value {
    match &port.port_type {
        PortType::Usb(usb) => json!({
            "port": port.port_name, "type": "usb", "vid": usb.vid, "pid": usb.pid,
            "serial_number": usb.serial_number, "manufacturer": usb.manufacturer, "product": usb.product,
        }),
        other => json!({
            "port": port.port_name,
            "type": match other { PortType::Pci => "pci", PortType::Bluetooth => "bluetooth", _ => "unknown" },
        }),
    }
}

fn send<T: Transport>(session: &mut SerialSession<T>, bytes: &[u8]) -> Result<(), Error> {
    session.write_all(bytes)?;
    session.flush()
}

fn receive<T: Transport>(
    session: &mut SerialSession<T>,
    max_bytes: usize,
) -> Result<Vec<u8>, Error> {
    let mut bytes = vec![0; max_bytes];
    let count = session.read(&mut bytes)?;
    bytes.truncate(count);
    Ok(bytes)
}

fn query<T: Transport>(
    session: &mut SerialSession<T>,
    tx: &[u8],
    delimiter: &[u8],
    max_bytes: usize,
) -> Result<Vec<u8>, Error> {
    send(session, tx)?;
    session.read_until(delimiter, max_bytes)
}

impl Cli {
    pub fn run(self) -> i32 {
        let command = match self.command {
            Command::UserGuide(args) => return args.run(),
            Command::Worker(args) => return args.run(),
            Command::Terminal(args) => return args.run(),
            Command::AutoTx(args) => return args.run(),
            command => command,
        };
        let self_ = Self { command };
        let (name, output) = match &self_.command {
            Command::Manifest(args) => ("manifest", args),
            Command::ListPorts(args) => ("list-ports", args),
            Command::Send(args) => ("send", &args.output),
            Command::Receive(args) => ("receive", &args.output),
            Command::Query(args) => ("query", &args.output),
            Command::Sequence(args) => (args.command_name(), args.output()),
            Command::UserGuide(_) | Command::Worker(_) | Command::Terminal(_) | Command::AutoTx(_) => unreachable!(),
        };
        let machine = output.machine();
        match self_.execute() {
            Ok((value, text)) => {
                if machine {
                    println!("{value}");
                } else {
                    println!("{text}");
                }
                0
            }
            Err(error) => {
                let code = error.exit_code();
                if machine {
                    println!("{}", error_json(name, &error));
                } else {
                    eprintln!("{error}");
                }
                code
            }
        }
    }

    fn execute(self) -> Result<(Value, String), CliError> {
        match self.command {
            Command::Manifest(_) => {
                let value = json!({
                    "event": "tool_manifest", "schema_version": 2, "tool_id": "serial",
                    "tool_version": env!("CARGO_PKG_VERSION"),
                    "worker_protocol": { "compatibility_policy": "v3-only", "schema_versions": [3] },
                    "execution": { "model": "multi_job", "context_binding": "startup" },
                    "features": {
                        "common.async_jobs": { "contract_version": 1 },
                        "common.streaming": { "contract_version": 1, "lifecycles": ["run_bound"] }
                    },
                    "commands": {
                        "send": { "kind": "async_job" },
                        "send-file": { "kind": "async_job" },
                        "receive": { "kind": "async_job" },
                        "query": { "kind": "async_job" },
                        "run-sequence": { "kind": "async_job" },
                    },
                });
                Ok((
                    value,
                    format!(
                        "serial {}\nWorker protocol: schema 3",
                        env!("CARGO_PKG_VERSION")
                    ),
                ))
            }
            Command::ListPorts(_) => {
                let ports = list_ports().map_err(CliError::Runtime)?;
                let lines: Vec<String> = ports
                    .iter()
                    .map(|port| {
                        let mut line = format!(
                            "{}  {}",
                            port.port_name,
                            match port.port_type {
                                PortType::Usb(_) => "usb",
                                PortType::Pci => "pci",
                                PortType::Bluetooth => "bluetooth",
                                PortType::Unknown => "unknown",
                            }
                        );
                        if let PortType::Usb(usb) = &port.port_type {
                            line.push_str(&format!("  VID={:04x} PID={:04x}", usb.vid, usb.pid));
                            for (label, value) in [
                                ("serial", &usb.serial_number),
                                ("manufacturer", &usb.manufacturer),
                                ("product", &usb.product),
                            ] {
                                if let Some(value) = value {
                                    line.push_str(&format!("  {label}={value}"));
                                }
                            }
                        }
                        line
                    })
                    .collect();
                let text = if lines.is_empty() {
                    "No serial ports found".into()
                } else {
                    format!("PORT  TYPE\n{}", lines.join("\n"))
                };
                let value = json!({ "event": "list-ports", "schema_version": 2, "timestamp_utc": timestamp(), "ok": true, "count": ports.len(), "ports": ports.iter().map(port_json).collect::<Vec<_>>() });
                Ok((value, text))
            }
            Command::Send(args) => {
                let settings = args.serial.settings(false)?;
                let tx = args.tx.bytes()?;
                if !args.dry_run {
                    let transport = settings.open().map_err(CliError::Runtime)?;
                    send(&mut SerialSession::new(transport), &tx).map_err(CliError::Runtime)?;
                }
                let target = settings.target();
                let mut value = json!({
                    "event": if args.dry_run { "dry_run" } else { "send" },
                    "schema_version": 2, "timestamp_utc": timestamp(), "ok": true,
                    "command": "send", "tx_hex": hex(&tx), "tx_bytes": tx.len(),
                });
                let details = settings.one_shot_output();
                for (key, item) in details.as_object().unwrap() {
                    value[key] = item.clone();
                }
                if args.dry_run {
                    value["performs_serial_io"] = json!(false);
                    if matches!(settings, ConnectionSettings::Tcp(_)) {
                        value["performs_tcp_io"] = json!(false);
                    }
                } else {
                    value.as_object_mut().unwrap().remove("serial_settings");
                }
                let text = if args.dry_run {
                    format!(
                        "Dry run: would send {} bytes to {} ({})",
                        tx.len(),
                        target,
                        spaced_hex(&tx)
                    )
                } else {
                    format!(
                        "Send: {} bytes sent to {} ({})",
                        tx.len(),
                        target,
                        spaced_hex(&tx)
                    )
                };
                Ok((value, text))
            }
            Command::Receive(args) => {
                let settings = args.serial.settings(false)?;
                let limit = max_bytes(args.max_bytes)?;
                let target = settings.target();
                let mut value = json!({
                    "event": if args.dry_run { "dry_run" } else { "receive" },
                    "schema_version": 2, "timestamp_utc": timestamp(), "ok": true,
                    "command": "receive",
                });
                for (key, item) in settings.one_shot_output().as_object().unwrap() {
                    value[key] = item.clone();
                }
                if args.dry_run {
                    value["performs_serial_io"] = json!(false);
                    if matches!(settings, ConnectionSettings::Tcp(_)) {
                        value["performs_tcp_io"] = json!(false);
                    }
                    value["max_bytes"] = json!(limit);
                    return Ok((
                        value,
                        format!("Dry run: receive up to {limit} bytes from {target}"),
                    ));
                }
                let transport = settings.open().map_err(CliError::Runtime)?;
                let rx = receive(&mut SerialSession::new(transport), limit)
                    .map_err(CliError::Runtime)?;
                value["rx_hex"] = json!(hex(&rx));
                value["rx_bytes"] = json!(rx.len());
                value.as_object_mut().unwrap().remove("serial_settings");
                let rendered = render_rx(&rx, args.rx_display, "");
                let text = if args.rx_display == RxDisplay::Both {
                    format!("Receive: {} bytes from {}\n{rendered}", rx.len(), target)
                } else {
                    format!("Receive: {} bytes from {} ({rendered})", rx.len(), target)
                };
                Ok((value, text))
            }
            Command::Query(args) => {
                let settings = args.serial.settings(false)?;
                let tx = args.tx.bytes()?;
                let delimiter = args.delimiter.bytes()?;
                let limit = max_bytes(args.max_bytes)?;
                if delimiter.len() > limit {
                    return Err(CliError::Input(
                        "max-bytes must be at least the delimiter length",
                    ));
                }
                let target = settings.target();
                let mut value = json!({
                    "event": if args.dry_run { "dry_run" } else { "query" },
                    "schema_version": 2, "timestamp_utc": timestamp(), "ok": true,
                    "command": "query", "tx_hex": hex(&tx), "tx_bytes": tx.len(),
                    "delimiter_hex": hex(&delimiter),
                });
                for (key, item) in settings.one_shot_output().as_object().unwrap() {
                    value[key] = item.clone();
                }
                if args.dry_run {
                    value["performs_serial_io"] = json!(false);
                    if matches!(settings, ConnectionSettings::Tcp(_)) {
                        value["performs_tcp_io"] = json!(false);
                    }
                    value["max_bytes"] = json!(limit);
                    return Ok((
                        value,
                        format!(
                            "Dry run: query {target} with TX {} until {} (max {limit} bytes)",
                            spaced_hex(&tx),
                            spaced_hex(&delimiter)
                        ),
                    ));
                }
                let transport = settings.open().map_err(CliError::Runtime)?;
                let rx = query(&mut SerialSession::new(transport), &tx, &delimiter, limit)
                    .map_err(CliError::Runtime)?;
                value["rx_hex"] = json!(hex(&rx));
                value["rx_bytes"] = json!(rx.len());
                value.as_object_mut().unwrap().remove("serial_settings");
                let text = if args.rx_display == RxDisplay::Both {
                    format!(
                        "Query {target}: TX {}\n{}",
                        spaced_hex(&tx),
                        render_rx(&rx, args.rx_display, "RX ")
                    )
                } else {
                    format!(
                        "Query {target}: TX {} / RX {}",
                        spaced_hex(&tx),
                        render_rx(&rx, args.rx_display, "")
                    )
                };
                Ok((value, text))
            }
            Command::Sequence(args) => args.execute(),
            Command::UserGuide(_) | Command::Worker(_) | Command::Terminal(_) | Command::AutoTx(_) => unreachable!(),
        }
    }
}

fn spaced_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn safe_rx_text(bytes: &[u8]) -> String {
    let mut result = String::new();
    for character in String::from_utf8_lossy(bytes).chars() {
        match character {
            '\r' => result.push_str("\\r"),
            '\n' => result.push_str("\\n"),
            '\t' => result.push_str("\\t"),
            control if control.is_control() => result.extend(control.escape_default()),
            printable => result.push(printable),
        }
    }
    result
}

fn render_rx(bytes: &[u8], display: RxDisplay, prefix: &str) -> String {
    match display {
        RxDisplay::Hex => spaced_hex(bytes),
        RxDisplay::Text => safe_rx_text(bytes),
        RxDisplay::Both => format!(
            "{prefix}HEX: {}\n{prefix}TEXT: {}",
            spaced_hex(bytes),
            safe_rx_text(bytes)
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use serial_tool_core::SimulationTransport;

    use super::*;

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("serial-tool").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn parses_exact_tx_and_delimiter_bytes() {
        assert_eq!(parse_bytes(Some("é"), None).unwrap(), "é".as_bytes());
        assert_eq!(
            parse_bytes(None, Some("00 55\tAA\nFF")).unwrap(),
            [0, 0x55, 0xaa, 0xff]
        );
        assert_eq!(
            parse_bytes(None, Some("0055AAFF")).unwrap(),
            [0, 0x55, 0xaa, 0xff]
        );
        assert_eq!(parse_bytes(Some("OK"), None).unwrap(), b"OK");
        assert_eq!(parse_bytes(None, Some("0D 0A")).unwrap(), b"\r\n");
        for bad in ["0", "0g", " "] {
            assert!(parse_bytes(None, Some(bad)).is_err());
        }
        assert!(parse_bytes(Some(""), None).is_err());
        assert!(parse_bytes(None, Some("")).is_err());
    }

    #[test]
    fn rx_presentation_keeps_raw_bytes_and_escapes_controls() {
        let bytes = b"OK\r\n";
        assert_eq!(render_rx(bytes, RxDisplay::Hex, ""), "4F 4B 0D 0A");
        assert_eq!(render_rx(bytes, RxDisplay::Text, ""), "OK\\r\\n");
        assert_eq!(safe_rx_text("你好".as_bytes()), "你好");

        let invalid = vec![b'O', b'K', 0xff, 0x1b, b'\t'];
        assert_eq!(safe_rx_text(&invalid), "OK�\\u{1b}\\t");
        assert_eq!(invalid, [b'O', b'K', 0xff, 0x1b, b'\t']);
    }

    #[test]
    fn parses_rx_display_only_for_rx_human_commands() {
        let Command::Receive(receive) =
            cli(&["receive", "--port", "COM1", "--baud", "9600"]).command
        else {
            panic!("expected receive")
        };
        assert_eq!(receive.rx_display, RxDisplay::Hex);

        let Command::Receive(receive) = cli(&[
            "receive",
            "--port",
            "COM1",
            "--baud",
            "9600",
            "--rx-display",
            "text",
        ])
        .command
        else {
            panic!("expected receive")
        };
        assert_eq!(receive.rx_display, RxDisplay::Text);

        let Command::Query(query) = cli(&[
            "query",
            "--port",
            "COM1",
            "--baud",
            "9600",
            "--text",
            "X",
            "--until-text",
            "Y",
            "--rx-display",
            "both",
        ])
        .command
        else {
            panic!("expected query")
        };
        assert_eq!(query.rx_display, RxDisplay::Both);

        let Command::Sequence(sequence) = cli(&[
            "sequence",
            "run",
            "--file",
            "sequence.json",
            "--mode",
            "simulate",
            "--simulation-profile-id",
            "loopback-v1",
            "--rx-display",
            "text",
        ])
        .command
        else {
            panic!("expected sequence")
        };
        assert_eq!(sequence.command_name(), "sequence-run");

        assert!(
            Cli::try_parse_from([
                "serial-tool",
                "receive",
                "--port",
                "COM1",
                "--baud",
                "9600",
                "--rx-display",
                "unknown",
            ])
            .is_err()
        );
    }

    #[test]
    fn clap_requires_exactly_one_input_of_each_kind() {
        let base = ["serial-tool", "query", "--port", "COM1", "--baud", "9600"];
        assert!(
            Cli::try_parse_from(base.into_iter().chain([
                "--text",
                "X",
                "--hex",
                "58",
                "--until-text",
                "Y"
            ]))
            .is_err()
        );
        assert!(
            Cli::try_parse_from(base.into_iter().chain([
                "--text",
                "X",
                "--until-text",
                "Y",
                "--until-hex",
                "59"
            ]))
            .is_err()
        );
        assert!(
            Cli::try_parse_from(base.into_iter().chain(["--text", "X", "--until-text", "Y"]))
                .is_ok()
        );
    }

    #[test]
    fn manifest_is_static_and_advertises_worker() {
        let (value, _) = cli(&["manifest", "--json"]).execute().unwrap();
        assert_eq!(value["event"], "tool_manifest");
        assert_eq!(value["schema_version"].as_i64(), Some(2));
        assert_eq!(value["tool_id"], "serial");
        assert_eq!(value["worker_protocol"]["schema_versions"], json!([3]));
    }

    #[test]
    fn dry_runs_validate_without_opening_a_port() {
        let (send, _) = cli(&[
            "send",
            "--port",
            "COM_DOES_NOT_EXIST",
            "--baud",
            "115200",
            "--hex",
            "00 FF",
            "--dry-run",
            "--json",
        ])
        .execute()
        .unwrap();
        assert_eq!(send["event"], "dry_run");
        assert_eq!(send["schema_version"].as_i64(), Some(2));
        assert_eq!(send["performs_serial_io"], false);
        assert_eq!(send["tx_hex"], "00ff");
        assert!(send.get("run_id").is_none());

        let (query, _) = cli(&[
            "query",
            "--port",
            "COM_DOES_NOT_EXIST",
            "--baud",
            "115200",
            "--text",
            "X",
            "--until-hex",
            "0D 0A",
            "--max-bytes",
            "16",
            "--dry-run",
            "--json",
        ])
        .execute()
        .unwrap();
        assert_eq!(query["event"], "dry_run");
        assert_eq!(query["delimiter_hex"], "0d0a");
        assert!(query.get("run_id").is_none());
        assert_eq!(
            cli(&[
                "send",
                "--port",
                "COM1",
                "--baud",
                "0",
                "--text",
                "X",
                "--dry-run"
            ])
            .execute()
            .unwrap_err()
            .exit_code(),
            2
        );
        assert_eq!(
            cli(&[
                "query",
                "--port",
                "COM1",
                "--baud",
                "9600",
                "--text",
                "X",
                "--until-text",
                "Y",
                "--max-bytes",
                "0",
                "--dry-run"
            ])
            .execute()
            .unwrap_err()
            .exit_code(),
            2
        );
    }

    #[test]
    fn core_session_operations_preserve_raw_bytes() {
        let mut sent = SerialSession::new(SimulationTransport::default());
        send(&mut sent, &[0, 0xff]).unwrap();
        assert_eq!(sent.transport().captured_tx(), &[0, 0xff]);

        let mut queried = SerialSession::new(SimulationTransport::with_rx_chunks([vec![
            0xff, 0x0d, 0x0a,
        ]]));
        let rx = query(&mut queried, &[0, 0x55], b"\r\n", 8).unwrap();
        assert_eq!(queried.transport().captured_tx(), &[0, 0x55]);
        assert_eq!(rx, [0xff, 0x0d, 0x0a]);
        assert_eq!(hex(&rx), "ff0d0a");

        let mut receiving =
            SerialSession::new(SimulationTransport::with_rx_chunks([vec![0, 0xff]]));
        assert_eq!(receive(&mut receiving, 4).unwrap(), [0, 0xff]);
    }

    struct FailedWrite;

    impl Transport for FailedWrite {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            unreachable!()
        }
        fn write_all(&mut self, _: &[u8]) -> io::Result<()> {
            Err(io::Error::other("injected failure"))
        }
        fn flush(&mut self) -> io::Result<()> {
            unreachable!()
        }
    }

    #[test]
    fn machine_errors_classify_failures_and_preserve_partial_rx() {
        let validation = cli(&[
            "send", "--port", "COM1", "--baud", "9600", "--hex", "0g", "--json",
        ])
        .execute()
        .unwrap_err();
        let value = error_json("send", &validation);
        assert_eq!(value["exit_code"], 2);
        assert_eq!(value["event"], "error");
        assert_eq!(value["schema_version"].as_i64(), Some(2));

        let mut failed = SerialSession::new(FailedWrite);
        let runtime = CliError::Runtime(send(&mut failed, b"X").unwrap_err());
        assert_eq!(error_json("send", &runtime)["exit_code"], 3);

        let mut partial = SerialSession::new(SimulationTransport::with_rx_chunks([vec![0, 0xff]]));
        let timeout = CliError::Runtime(query(&mut partial, b"X", b"\r\n", 8).unwrap_err());
        let value = error_json("query", &timeout);
        assert_eq!(value["exit_code"], 3);
        assert_eq!(value["partial_hex"], "00ff");
        assert_eq!(value["partial_bytes"], 2);
    }
}
