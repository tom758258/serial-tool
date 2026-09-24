use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use serial_tool_core::{
    DataBits, Error, FlowControl, MAX_READ_BYTES, Parity, PortInfo, PortType, SerialSession,
    SerialSettings, SerialTransport, StopBits, Transport, list_ports,
};

mod worker;
use worker::WorkerArgs;

#[derive(Debug, Parser)]
#[command(name = "serial-tool", version, about = "Serial port diagnostic CLI")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    /// Run the local Serial Worker control plane.
    Worker(WorkerArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum Format {
    #[default]
    Text,
    Json,
    Jsonl,
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

#[derive(Debug, Args)]
struct SerialArgs {
    #[arg(long)]
    port: String,
    #[arg(long)]
    baud: u32,
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u8).range(5..=8), help = "Data bits: 5, 6, 7, or 8")]
    data_bits: u8,
    #[arg(long, value_enum, default_value_t = CliParity::None)]
    parity: CliParity,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2), help = "Stop bits: 1 or 2")]
    stop_bits: u8,
    #[arg(long, value_enum, default_value_t = CliFlowControl::None)]
    flow_control: CliFlowControl,
    #[arg(long, default_value_t = 1000)]
    timeout_ms: u64,
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
    fn settings(&self) -> Result<SerialSettings, CliError> {
        let settings = SerialSettings {
            port: self.port.clone(),
            baud_rate: self.baud,
            data_bits: match self.data_bits {
                5 => DataBits::Five,
                6 => DataBits::Six,
                7 => DataBits::Seven,
                _ => DataBits::Eight,
            },
            parity: match self.parity {
                CliParity::None => Parity::None,
                CliParity::Odd => Parity::Odd,
                CliParity::Even => Parity::Even,
            },
            stop_bits: if self.stop_bits == 1 {
                StopBits::One
            } else {
                StopBits::Two
            },
            flow_control: match self.flow_control {
                CliFlowControl::None => FlowControl::None,
                CliFlowControl::Software => FlowControl::Software,
                CliFlowControl::Hardware => FlowControl::Hardware,
            },
            timeout: Duration::from_millis(self.timeout_ms),
        };
        settings.validate().map_err(CliError::Validation)?;
        Ok(settings)
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
    tx: TxArgs,
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
    #[command(flatten)]
    output: OutputArgs,
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug)]
enum CliError {
    Validation(Error),
    Input(&'static str),
    Runtime(Error),
}

impl CliError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::Validation(_) | Self::Input(_) => 2,
            Self::Runtime(_) => 3,
        }
    }

    fn partial(&self) -> Option<&[u8]> {
        match self {
            Self::Runtime(error) => error.partial(),
            _ => None,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(error) | Self::Runtime(error) => write!(f, "{error}"),
            Self::Input(message) => write!(f, "{message}"),
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
            for pair in digits.chunks_exact(2) {
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
            Command::Worker(args) => return args.run(),
            command => command,
        };
        let self_ = Self { command };
        let (name, output) = match &self_.command {
            Command::Manifest(args) => ("manifest", args),
            Command::ListPorts(args) => ("list-ports", args),
            Command::Send(args) => ("send", &args.output),
            Command::Receive(args) => ("receive", &args.output),
            Command::Query(args) => ("query", &args.output),
            Command::Worker(_) => unreachable!(),
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
                    "worker_protocol": { "compatibility_policy": "v2-only", "schema_versions": [2] },
                });
                Ok((
                    value,
                    format!(
                        "serial {}\nWorker protocol: schema 2",
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
                let settings = args.serial.settings()?;
                let tx = args.tx.bytes()?;
                if !args.dry_run {
                    let transport = SerialTransport::open(&settings).map_err(CliError::Runtime)?;
                    send(&mut SerialSession::new(transport), &tx).map_err(CliError::Runtime)?;
                }
                let value = if args.dry_run {
                    json!({ "event": "dry_run", "schema_version": 2, "timestamp_utc": timestamp(), "ok": true, "command": "send", "performs_serial_io": false, "serial_settings": settings_json(&settings), "port": settings.port, "tx_hex": hex(&tx), "tx_bytes": tx.len() })
                } else {
                    json!({ "event": "send", "schema_version": 2, "timestamp_utc": timestamp(), "ok": true, "command": "send", "port": settings.port, "tx_hex": hex(&tx), "tx_bytes": tx.len() })
                };
                let text = if args.dry_run {
                    format!(
                        "Dry run: would send {} bytes to {} ({})",
                        tx.len(),
                        settings.port,
                        spaced_hex(&tx)
                    )
                } else {
                    format!(
                        "Send: {} bytes sent to {} ({})",
                        tx.len(),
                        settings.port,
                        spaced_hex(&tx)
                    )
                };
                Ok((value, text))
            }
            Command::Receive(args) => {
                let settings = args.serial.settings()?;
                let limit = max_bytes(args.max_bytes)?;
                if args.dry_run {
                    let value = json!({ "event": "dry_run", "schema_version": 2, "timestamp_utc": timestamp(), "ok": true, "command": "receive", "performs_serial_io": false, "serial_settings": settings_json(&settings), "port": settings.port, "max_bytes": limit });
                    return Ok((
                        value,
                        format!(
                            "Dry run: receive up to {limit} bytes from {}",
                            settings.port
                        ),
                    ));
                }
                let transport = SerialTransport::open(&settings).map_err(CliError::Runtime)?;
                let rx = receive(&mut SerialSession::new(transport), limit)
                    .map_err(CliError::Runtime)?;
                let value = json!({ "event": "receive", "schema_version": 2, "timestamp_utc": timestamp(), "ok": true, "command": "receive", "port": settings.port, "rx_hex": hex(&rx), "rx_bytes": rx.len() });
                Ok((
                    value,
                    format!(
                        "Receive: {} bytes from {} ({})",
                        rx.len(),
                        settings.port,
                        spaced_hex(&rx)
                    ),
                ))
            }
            Command::Query(args) => {
                let settings = args.serial.settings()?;
                let tx = args.tx.bytes()?;
                let delimiter = args.delimiter.bytes()?;
                let limit = max_bytes(args.max_bytes)?;
                if delimiter.len() > limit {
                    return Err(CliError::Input(
                        "max-bytes must be at least the delimiter length",
                    ));
                }
                if args.dry_run {
                    let value = json!({ "event": "dry_run", "schema_version": 2, "timestamp_utc": timestamp(), "ok": true, "command": "query", "performs_serial_io": false, "serial_settings": settings_json(&settings), "port": settings.port, "tx_hex": hex(&tx), "tx_bytes": tx.len(), "delimiter_hex": hex(&delimiter), "max_bytes": limit });
                    return Ok((
                        value,
                        format!(
                            "Dry run: query {} with TX {} until {} (max {limit} bytes)",
                            settings.port,
                            spaced_hex(&tx),
                            spaced_hex(&delimiter)
                        ),
                    ));
                }
                let transport = SerialTransport::open(&settings).map_err(CliError::Runtime)?;
                let rx = query(&mut SerialSession::new(transport), &tx, &delimiter, limit)
                    .map_err(CliError::Runtime)?;
                let value = json!({ "event": "query", "schema_version": 2, "timestamp_utc": timestamp(), "ok": true, "command": "query", "port": settings.port, "tx_hex": hex(&tx), "tx_bytes": tx.len(), "rx_hex": hex(&rx), "rx_bytes": rx.len(), "delimiter_hex": hex(&delimiter) });
                Ok((
                    value,
                    format!(
                        "Query {}: TX {} / RX {}",
                        settings.port,
                        spaced_hex(&tx),
                        spaced_hex(&rx)
                    ),
                ))
            }
            Command::Worker(_) => unreachable!(),
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
        assert_eq!(value["worker_protocol"]["schema_versions"], json!([2]));
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
