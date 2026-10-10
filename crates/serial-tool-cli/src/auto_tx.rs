use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use clap::Args;
use serial_tool_core::runtime::{Event, Mode, PersistentRuntime, RuntimeError};

use crate::{
    CliError, PayloadArgs,
    terminal::{ConnectionArgs, DisplayArgs},
};

#[derive(Debug, Args)]
pub(super) struct AutoTxArgs {
    #[command(flatten)]
    connection: ConnectionArgs,
    #[command(flatten)]
    tx: PayloadArgs,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    interval_ms: u64,
    #[command(flatten)]
    display: DisplayArgs,
}

enum Message {
    Event(Event),
    Stop,
}

impl AutoTxArgs {
    fn interval(&self) -> Result<Duration, CliError> {
        let interval = Duration::from_millis(self.interval_ms);
        if Instant::now().checked_add(interval).is_none() {
            return Err(CliError::Input("interval-ms is too large"));
        }
        Ok(interval)
    }

    pub(super) fn run(self) -> i32 {
        let plan = (|| {
            Ok::<_, CliError>((
                self.connection.settings()?,
                self.tx.bytes()?,
                self.interval()?,
            ))
        })();
        let ((mode, settings), bytes, interval) = match plan {
            Ok(plan) => plan,
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
        if let Err(error) = runtime.connect(mode, settings, move |event| {
            let _ = sender.send(Message::Event(event));
        }) {
            eprintln!("{error}");
            return 3;
        }
        self.run_connected(&runtime, &receiver, mode, bytes, interval)
    }

    fn run_connected(
        &self,
        runtime: &PersistentRuntime,
        receiver: &mpsc::Receiver<Message>,
        mode: Mode,
        bytes: Vec<u8>,
        interval: Duration,
    ) -> i32 {
        let mut display = self.display.renderer();
        let hint = "Auto TX active; Ctrl+C stops and disconnects";
        let mut code = 0;
        if let Err(error) = runtime.start_periodic(bytes, interval) {
            eprintln!("{error}");
            code = 3;
        } else {
            loop {
                match receiver.recv() {
                    Ok(Message::Stop) => break,
                    Ok(Message::Event(Event::Disconnected)) => {
                        display.show(Event::Disconnected, mode, hint);
                        code = 3;
                        break;
                    }
                    Ok(Message::Event(event)) => {
                        if !display.show(event, mode, hint) {
                            code = 3;
                            break;
                        }
                    }
                    Err(_) => {
                        code = 3;
                        break;
                    }
                }
            }
        }
        // The owner may already have closed the connection after an I/O failure.
        if let Err(error) = runtime.stop_periodic()
            && !matches!(error, RuntimeError::Disconnected)
        {
            eprintln!("{error}");
            code = 3;
        }
        if let Err(error) = runtime.disconnect()
            && !matches!(error, RuntimeError::Disconnected)
        {
            eprintln!("{error}");
            code = 3;
        }
        for message in receiver.try_iter() {
            if let Message::Event(event) = message
                && !display.show(event, mode, hint)
            {
                code = 3;
            }
        }
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serial_tool_core::Direction;
    use std::thread;

    fn args(extra: &[&str]) -> AutoTxArgs {
        let crate::Command::AutoTx(args) = crate::Cli::parse_from(
            [
                "serial-tool",
                "auto-tx",
                "--mode",
                "simulate",
                "--baud",
                "115200",
                "--simulation-profile-id",
                "loopback-v1",
                "--rx-display",
                "hex-stream",
            ]
            .into_iter()
            .chain(extra.iter().copied()),
        )
        .command
        else {
            unreachable!()
        };
        args
    }

    fn simulate(
        args: AutoTxArgs,
        expected: &[u8],
        repetitions: usize,
        file: Option<&std::path::Path>,
    ) {
        let (mode, settings) = args.connection.settings().unwrap();
        let bytes = args.tx.bytes().unwrap();
        let interval = args.interval().unwrap();
        let runtime = PersistentRuntime::default();
        let (sender, receiver) = mpsc::channel();
        let (observed, observations) = mpsc::channel();
        let events = sender.clone();
        runtime
            .connect(mode, settings.clone(), move |event| {
                let _ = observed.send(event.clone());
                let _ = events.send(Message::Event(event));
            })
            .unwrap();
        let (code, data) = thread::scope(|scope| {
            let controller = scope.spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut data = Vec::new();
                let mut rx_count = 0;
                while Instant::now() < deadline && rx_count < repetitions {
                    let Ok(event) = observations
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    else {
                        break;
                    };
                    if let Event::Data { direction, bytes } = event {
                        if direction == Direction::Tx
                            && data.is_empty()
                            && let Some(path) = file
                        {
                            if std::fs::write(path, b"changed").is_err() {
                                break;
                            }
                        }
                        if direction == Direction::Rx {
                            rx_count += 1;
                        }
                        data.push((direction, bytes));
                    }
                }
                let _ = sender.send(Message::Stop);
                data
            });
            let code = args.run_connected(&runtime, &receiver, mode, bytes, interval);
            (code, controller.join().unwrap())
        });
        assert_eq!(code, 0);
        for direction in [Direction::Tx, Direction::Rx] {
            let matching: Vec<_> = data
                .iter()
                .filter(|(found, _)| *found == direction)
                .collect();
            assert!(matching.len() >= repetitions, "{data:?}");
            assert!(
                matching.iter().all(|(_, bytes)| bytes == expected),
                "{data:?}"
            );
        }
        assert!(matches!(
            runtime.send(vec![1]),
            Err(RuntimeError::Disconnected)
        ));
        runtime.connect(mode, settings, |_| {}).unwrap();
        runtime.send(vec![1]).unwrap();
        runtime.disconnect().unwrap();
    }

    #[test]
    fn auto_tx_first_send_is_immediate_and_stop_releases_connection() {
        simulate(
            args(&["--text", "STATUS?", "--interval-ms", "60000"]),
            b"STATUS?",
            1,
            None,
        );
    }

    #[test]
    fn auto_tx_repeats_text_hex_and_file_snapshot_with_continuous_rx() {
        simulate(
            args(&["--text", "STATUS?", "--interval-ms", "40"]),
            b"STATUS?",
            2,
            None,
        );
        simulate(
            args(&["--hex", "00 FF 0D 0A", "--interval-ms", "40"]),
            &[0, 255, 13, 10],
            2,
            None,
        );
        let path = std::env::temp_dir().join(format!(
            "serial-cli-auto-snapshot-{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, [0, 255, 13, 10]).unwrap();
        simulate(
            args(&["--file", path.to_str().unwrap(), "--interval-ms", "40"]),
            &[0, 255, 13, 10],
            2,
            Some(&path),
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"changed");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn auto_tx_connection_errors_exit_three_and_release_connection() {
        let args = args(&["--text", "X", "--interval-ms", "40"]);
        let (mode, settings) = args.connection.settings().unwrap();
        for stop_first in [false, true] {
            let runtime = PersistentRuntime::default();
            let (sender, receiver) = mpsc::channel();
            let events = sender.clone();
            runtime
                .connect(mode, settings.clone(), move |event| {
                    let _ = events.send(Message::Event(event));
                })
                .unwrap();
            if stop_first {
                sender.send(Message::Stop).unwrap();
            }
            sender
                .send(Message::Event(Event::ConnectionError {
                    message: "injected I/O failure".into(),
                }))
                .unwrap();
            assert_eq!(
                args.run_connected(
                    &runtime,
                    &receiver,
                    mode,
                    vec![1],
                    Duration::from_millis(40)
                ),
                3
            );
            runtime.connect(mode, settings.clone(), |_| {}).unwrap();
            runtime.disconnect().unwrap();
        }
        let runtime = PersistentRuntime::default();
        let (_, receiver) = mpsc::channel();
        assert_eq!(
            args.run_connected(
                &runtime,
                &receiver,
                mode,
                vec![1],
                Duration::from_millis(40)
            ),
            3
        );
    }
}
