use std::{
    io::{self, BufRead},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use clap::{Args, ValueEnum};
use serial_tool_core::{
    Direction, SerialSettings,
    runtime::{Event, Mode, PersistentRuntime},
};

use crate::{
    CliError, CliFlowControl, CliParity, RxDisplay, SerialArgs, parse_bytes, render_rx, spaced_hex,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TerminalMode {
    Live,
    Simulate,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TxFormat {
    Text,
    Hex,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LineEnding {
    None,
    Lf,
    Crlf,
}

#[derive(Debug, Args)]
pub(super) struct TerminalArgs {
    #[arg(long, value_enum)]
    mode: TerminalMode,
    #[arg(long)]
    port: Option<String>,
    #[arg(long)]
    simulation_profile_id: Option<String>,
    #[arg(long)]
    baud: u32,
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u8).range(5..=8))]
    data_bits: u8,
    #[arg(long, value_enum, default_value_t = CliParity::None)]
    parity: CliParity,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
    stop_bits: u8,
    #[arg(long, value_enum, default_value_t = CliFlowControl::None)]
    flow_control: CliFlowControl,
    #[arg(long, default_value_t = 1000)]
    timeout_ms: u64,
    #[arg(long, value_enum, default_value_t = RxDisplay::Hex)]
    rx_display: RxDisplay,
    #[arg(long, value_enum, default_value_t = TxFormat::Text)]
    tx_format: TxFormat,
    #[arg(long, value_enum, default_value_t = LineEnding::None)]
    line_ending: LineEnding,
}

enum Message {
    Event(Event),
    Input(io::Result<String>),
    Eof,
    Stop,
}

impl TerminalArgs {
    fn settings(&self) -> Result<(Mode, SerialSettings), CliError> {
        let (mode, port) = match (
            self.mode,
            self.port.as_deref(),
            self.simulation_profile_id.as_deref(),
        ) {
            (TerminalMode::Live, Some(port), None) if !port.trim().is_empty() => (Mode::Live, port),
            (TerminalMode::Simulate, None, Some("loopback-v1")) => {
                (Mode::Simulation, "loopback-v1")
            }
            (TerminalMode::Simulate, None, Some(_)) => {
                return Err(CliError::Input("unknown simulation profile"));
            }
            (TerminalMode::Live, _, _) => {
                return Err(CliError::Input(
                    "live mode requires --port and forbids --simulation-profile-id",
                ));
            }
            (TerminalMode::Simulate, _, _) => {
                return Err(CliError::Input(
                    "simulate mode requires --simulation-profile-id and forbids --port",
                ));
            }
        };
        let settings = SerialArgs {
            port: port.into(),
            baud: self.baud,
            data_bits: self.data_bits,
            parity: self.parity,
            stop_bits: self.stop_bits,
            flow_control: self.flow_control,
            timeout_ms: self.timeout_ms,
        }
        .settings()?;
        Ok((mode, settings))
    }

    pub(super) fn run(self) -> i32 {
        let (mode, settings) = match self.settings() {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!("{error}");
                return error.exit_code();
            }
        };
        let (sender, receiver) = mpsc::channel();
        let signals = sender.clone();
        if let Err(error) = ctrlc::set_handler(move || {
            let _ = signals.send(Message::Stop);
        }) {
            eprintln!("Cannot install Ctrl+C handler: {error}");
            return 3;
        }
        let runtime = PersistentRuntime::default();
        let events = sender.clone();
        if let Err(error) = runtime.connect(mode, settings, move |event| {
            let _ = events.send(Message::Event(event));
        }) {
            eprintln!("{error}");
            return 3;
        }
        let input = thread::Builder::new()
            .name("terminal-stdin".into())
            .spawn(move || {
                let stdin = io::stdin();
                let mut stdin = stdin.lock();
                loop {
                    let mut line = String::new();
                    match stdin.read_line(&mut line) {
                        Ok(0) => {
                            let _ = sender.send(Message::Eof);
                            break;
                        }
                        Ok(_) => {
                            if sender.send(Message::Input(Ok(line))).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(Message::Input(Err(error)));
                            break;
                        }
                    }
                }
            });
        if let Err(error) = input {
            eprintln!("Cannot read stdin: {error}");
            let _ = runtime.disconnect();
            return 3;
        }
        self.run_connected(&runtime, &receiver, mode, None)
    }

    fn run_connected(
        &self,
        runtime: &PersistentRuntime,
        receiver: &mpsc::Receiver<Message>,
        mode: Mode,
        mut eof_deadline: Option<Instant>,
    ) -> i32 {
        let mut code = 0;
        loop {
            if eof_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                break;
            }
            let message = match eof_deadline {
                Some(deadline) => receiver
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .ok(),
                None => receiver.recv().ok(),
            };
            match message {
                Some(Message::Input(Ok(line))) => {
                    match tx_bytes(&line, self.tx_format, self.line_ending) {
                        Ok(bytes) => {
                            if let Err(error) = runtime.send(bytes) {
                                eprintln!("{error}");
                                code = 3;
                                break;
                            }
                        }
                        Err(error) => eprintln!("Invalid input: {error}"),
                    }
                }
                Some(Message::Input(Err(error))) => {
                    eprintln!("stdin error: {error}");
                    code = 3;
                    break;
                }
                Some(Message::Eof) => {
                    // EOF allows one short monitor window for the last submitted line's RX.
                    eof_deadline = Some(Instant::now() + Duration::from_millis(50))
                }
                Some(Message::Stop) | None => break,
                Some(Message::Event(Event::Disconnected)) => {
                    println!("Disconnected");
                    return code;
                }
                Some(Message::Event(event)) => {
                    if !self.display(event, mode) {
                        code = 3;
                        break;
                    }
                }
            }
        }
        if let Err(error) = runtime.disconnect() {
            eprintln!("{error}");
            code = 3;
        }
        for message in receiver.try_iter() {
            if let Message::Event(event) = message
                && !self.display(event, mode)
            {
                code = 3;
            }
        }
        code
    }

    fn display(&self, event: Event, mode: Mode) -> bool {
        match event {
            Event::Connected => println!(
                "Connected ({}) — enter one line to send; EOF or Ctrl+C disconnects",
                if mode == Mode::Live {
                    "live"
                } else {
                    "simulate: loopback-v1"
                }
            ),
            Event::Disconnected => println!("Disconnected"),
            Event::Data {
                direction: Direction::Tx,
                bytes,
            } => println!("TX: {}", spaced_hex(&bytes)),
            Event::Data {
                direction: Direction::Rx,
                bytes,
            } => {
                let rendered = render_rx(&bytes, self.rx_display, "RX ");
                if self.rx_display == RxDisplay::Both {
                    println!("{rendered}");
                } else {
                    println!("RX: {rendered}");
                }
            }
            Event::ConnectionError { message } => {
                eprintln!("Connection error: {message}");
                return false;
            }
        }
        true
    }
}

fn tx_bytes(line: &str, format: TxFormat, ending: LineEnding) -> Result<Vec<u8>, CliError> {
    let line = match line.strip_suffix('\n') {
        Some(line) => line.strip_suffix('\r').unwrap_or(line),
        None => line,
    };
    match format {
        TxFormat::Hex => parse_bytes(None, Some(line)),
        TxFormat::Text => {
            let mut bytes = line.as_bytes().to_vec();
            match ending {
                LineEnding::None => {}
                LineEnding::Lf => bytes.push(b'\n'),
                LineEnding::Crlf => bytes.extend_from_slice(b"\r\n"),
            }
            if bytes.is_empty() {
                return Err(CliError::Input("byte input must not be empty"));
            }
            Ok(bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_eof_final_drain_connection_error_exits_three() {
        use clap::Parser;

        let crate::Command::Terminal(args) = crate::Cli::parse_from([
            "serial-tool",
            "terminal",
            "--mode",
            "simulate",
            "--baud",
            "115200",
            "--simulation-profile-id",
            "loopback-v1",
        ])
        .command
        else {
            unreachable!();
        };
        let (mode, settings) = args.settings().unwrap();
        // An expired EOF deadline skips the main loop, leaving the error for final drain.
        for eof_deadline in [None, Some(Instant::now())] {
            let runtime = PersistentRuntime::default();
            let (sender, receiver) = mpsc::channel();
            runtime.connect(mode, settings.clone(), |_| {}).unwrap();
            sender
                .send(Message::Event(Event::ConnectionError {
                    message: "queued connection failure".into(),
                }))
                .unwrap();
            assert_eq!(
                args.run_connected(&runtime, &receiver, mode, eof_deadline),
                3
            );
        }
    }

    #[test]
    fn terminal_tx_preserves_payload_and_applies_only_requested_ending() {
        assert_eq!(
            tx_bytes("STATUS?\r\n", TxFormat::Text, LineEnding::None).unwrap(),
            b"STATUS?"
        );
        assert_eq!(
            tx_bytes("STATUS?\n", TxFormat::Text, LineEnding::Crlf).unwrap(),
            b"STATUS?\r\n"
        );
        assert_eq!(
            tx_bytes("00 FF\r\n", TxFormat::Hex, LineEnding::Crlf).unwrap(),
            [0, 255]
        );
        assert!(tx_bytes("0G\n", TxFormat::Hex, LineEnding::None).is_err());
    }
}
