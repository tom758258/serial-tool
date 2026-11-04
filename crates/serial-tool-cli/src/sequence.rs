use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};
use serial_tool_core::{
    Direction, RunError, RunReport, Sequence, SequenceTransport, SerialSession, SerialTransport,
    SimulationTransport, Step, StepRunner, TcpSettings, TcpTransport, Transport,
};

use crate::{
    CliError, OutputArgs, RxDisplay, TransportKind, render_rx, sequence_result, tcp_settings_json,
    timestamp,
};

#[derive(Debug, Args)]
pub(super) struct SequenceArgs {
    #[command(subcommand)]
    command: SequenceCommand,
}

#[derive(Debug, Subcommand)]
enum SequenceCommand {
    /// Check a Sequence file without opening a port or executing steps.
    Validate(ValidateArgs),
    /// Execute a Sequence using an explicit runtime resource.
    Run(RunArgs),
}

#[derive(Debug, Args)]
struct ValidateArgs {
    #[arg(long)]
    file: std::path::PathBuf,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RunMode {
    Live,
    Simulate,
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long)]
    file: std::path::PathBuf,
    #[arg(long, value_enum)]
    mode: RunMode,
    #[arg(long, value_enum, default_value_t = TransportKind::Serial)]
    transport: TransportKind,
    #[arg(long)]
    port: Option<String>,
    #[arg(long)]
    host: Option<String>,
    #[arg(long)]
    tcp_port: Option<u16>,
    #[arg(long)]
    connect_timeout_ms: Option<u64>,
    #[arg(long)]
    simulation_profile_id: Option<String>,
    #[arg(long, value_enum, default_value_t = RxDisplay::Hex)]
    rx_display: RxDisplay,
    #[command(flatten)]
    output: OutputArgs,
}

impl SequenceArgs {
    pub(super) fn command_name(&self) -> &'static str {
        match self.command {
            SequenceCommand::Validate(_) => "sequence-validate",
            SequenceCommand::Run(_) => "sequence-run",
        }
    }

    pub(super) fn output(&self) -> &OutputArgs {
        match &self.command {
            SequenceCommand::Validate(args) => &args.output,
            SequenceCommand::Run(args) => &args.output,
        }
    }

    pub(super) fn execute(self) -> Result<(Value, String), CliError> {
        match self.command {
            SequenceCommand::Validate(args) => {
                let sequence = Sequence::load(&args.file).map_err(CliError::Sequence)?;
                let defined_steps = count_steps(&sequence.steps);
                Ok((
                    json!({ "event": "sequence_validate", "schema_version": 2, "timestamp_utc": timestamp(), "ok": true, "sequence_version": sequence.sequence_version, "defined_steps": defined_steps }),
                    format!(
                        "Sequence valid: version {}, {defined_steps} defined steps",
                        sequence.sequence_version
                    ),
                ))
            }
            SequenceCommand::Run(args) => args.execute(),
        }
    }
}

impl RunArgs {
    fn execute(self) -> Result<(Value, String), CliError> {
        let sequence = Sequence::load(&self.file).map_err(CliError::Sequence)?;
        let simulated = matches!(self.mode, RunMode::Simulate);
        let profile = self.simulation_profile_id.as_deref();
        if (simulated && profile != Some("loopback-v1")) || (!simulated && profile.is_some()) {
            return Err(CliError::Input(
                "simulate requires loopback-v1; live forbids simulation profile",
            ));
        }

        let (report, resource, tcp_settings) = match (&sequence.transport, self.transport) {
            (SequenceTransport::Serial(serial), TransportKind::Serial) => {
                if self.host.is_some()
                    || self.tcp_port.is_some()
                    || self.connect_timeout_ms.is_some()
                {
                    return Err(CliError::Input("serial transport forbids TCP options"));
                }
                let resource = if simulated {
                    if self.port.is_some() {
                        return Err(CliError::Input("simulate forbids --port"));
                    }
                    "loopback-v1".to_string()
                } else {
                    self.port
                        .clone()
                        .ok_or(CliError::Input("serial live requires --port"))?
                };
                let report = if simulated {
                    run_steps(
                        &mut SerialSession::new(SimulationTransport::loopback()),
                        &sequence.steps,
                    )?
                } else {
                    let settings = serial.serial_settings_for_port(&resource);
                    settings.validate().map_err(CliError::Validation)?;
                    let transport = SerialTransport::open(&settings).map_err(CliError::Runtime)?;
                    run_steps(&mut SerialSession::new(transport), &sequence.steps)?
                };
                (report, resource, None)
            }
            (SequenceTransport::Tcp(tcp), TransportKind::Tcp) => {
                if self.port.is_some() {
                    return Err(CliError::Input("TCP transport forbids --port"));
                }
                let (host, port) = if simulated {
                    if self.host.is_some() || self.tcp_port.is_some() {
                        return Err(CliError::Input(
                            "TCP simulation forbids --host and --tcp-port",
                        ));
                    }
                    (String::new(), 0)
                } else {
                    (
                        self.host
                            .clone()
                            .ok_or(CliError::Input("TCP live requires --host"))?,
                        self.tcp_port
                            .ok_or(CliError::Input("TCP live requires --tcp-port"))?,
                    )
                };
                let settings = TcpSettings {
                    host,
                    port,
                    connect_timeout: std::time::Duration::from_millis(
                        self.connect_timeout_ms.unwrap_or(3000),
                    ),
                    io_timeout: tcp.io_timeout,
                };
                if simulated {
                    if settings.connect_timeout.is_zero() {
                        return Err(CliError::Input("TCP connect timeout must be positive"));
                    }
                } else {
                    settings.validate().map_err(CliError::Validation)?;
                }
                let resource = if simulated {
                    "loopback-v1".to_string()
                } else {
                    format!("{}:{}", settings.host, settings.port)
                };
                let report = if simulated {
                    run_steps(
                        &mut SerialSession::new(SimulationTransport::loopback()),
                        &sequence.steps,
                    )?
                } else {
                    let transport = TcpTransport::connect(&settings).map_err(CliError::Runtime)?;
                    run_steps(&mut SerialSession::new(transport), &sequence.steps)?
                };
                (report, resource, Some(settings))
            }
            _ => return Err(CliError::Input("sequence transport must match --transport")),
        };
        let mut value = json!({
            "event": "sequence_run", "schema_version": 2, "timestamp_utc": timestamp(),
            "ok": true, "sequence_version": sequence.sequence_version,
            "transport": match self.transport {
                TransportKind::Serial => "serial",
                TransportKind::Tcp => "tcp",
            },
            "mode": match self.mode { RunMode::Live => "live", RunMode::Simulate => "simulate" },
            "step_results": sequence_result::step_results_json(&report),
            "transcript": sequence_result::transcript_json(&report),
        });
        if simulated {
            value["simulation_profile_id"] = json!(resource);
        } else if let Some(settings) = &tcp_settings {
            value["tcp_settings"] = tcp_settings_json(settings);
        } else {
            value["port"] = json!(resource);
        }
        let mut text = format!(
            "Sequence run complete: {} step results",
            report.step_results.len()
        );
        for entry in &report.transcript.entries {
            if entry.direction == Direction::Rx {
                let rendered = render_rx(&entry.bytes, self.rx_display, "");
                if self.rx_display == RxDisplay::Both {
                    text.push_str(&format!("\nRX {}\n{rendered}", entry.step_id));
                } else {
                    text.push_str(&format!("\nRX {}: {rendered}", entry.step_id));
                }
            }
        }
        Ok((value, text))
    }
}

fn run_steps<T: Transport>(
    session: &mut SerialSession<T>,
    steps: &[Step],
) -> Result<RunReport, CliError> {
    StepRunner::new(session)
        .run(steps)
        .map_err(|error| match error {
            RunError::Validation(error) => {
                CliError::Sequence(serial_tool_core::SequenceError::StepValidation(error))
            }
            RunError::Execution(error) => CliError::SequenceExecution(error),
        })
}

fn count_steps(steps: &[Step]) -> usize {
    steps
        .iter()
        .map(|step| {
            1 + match &step.kind {
                serial_tool_core::StepKind::Repeat { steps, .. } => count_steps(steps),
                _ => 0,
            }
        })
        .sum()
}
