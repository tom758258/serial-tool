use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};
use serial_tool_core::{
    Direction, RunError, RunReport, Sequence, SerialSession, SerialTransport, SimulationTransport,
    Step, StepRunner, Transport,
};

use crate::{CliError, OutputArgs, RxDisplay, render_rx, sequence_result, timestamp};

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
    #[arg(long)]
    port: Option<String>,
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
        let resource = match (
            self.mode,
            self.port.as_deref(),
            self.simulation_profile_id.as_deref(),
        ) {
            (RunMode::Live, Some(port), None) if !port.trim().is_empty() => port,
            (RunMode::Simulate, None, Some("loopback-v1")) => "loopback-v1",
            (RunMode::Simulate, None, Some(_)) => {
                return Err(CliError::Input("unknown simulation profile"));
            }
            (RunMode::Live, _, _) => {
                return Err(CliError::Input(
                    "live mode requires --port and forbids --simulation-profile-id",
                ));
            }
            (RunMode::Simulate, _, _) => {
                return Err(CliError::Input(
                    "simulate mode requires --simulation-profile-id and forbids --port",
                ));
            }
        };
        let sequence = Sequence::load(&self.file).map_err(CliError::Sequence)?;
        let report = match self.mode {
            RunMode::Live => {
                let settings = sequence.serial.serial_settings_for_port(resource);
                settings.validate().map_err(CliError::Validation)?;
                let transport = SerialTransport::open(&settings).map_err(CliError::Runtime)?;
                run_steps(&mut SerialSession::new(transport), &sequence.steps)?
            }
            RunMode::Simulate => run_steps(
                &mut SerialSession::new(SimulationTransport::loopback()),
                &sequence.steps,
            )?,
        };
        let mut value = json!({
            "event": "sequence_run", "schema_version": 2, "timestamp_utc": timestamp(),
            "ok": true, "sequence_version": sequence.sequence_version,
            "mode": match self.mode { RunMode::Live => "live", RunMode::Simulate => "simulate" },
            "step_results": sequence_result::step_results_json(&report), "transcript": sequence_result::transcript_json(&report),
        });
        match self.mode {
            RunMode::Live => value["port"] = json!(resource),
            RunMode::Simulate => value["simulation_profile_id"] = json!(resource),
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
