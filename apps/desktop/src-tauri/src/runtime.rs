use std::{
    sync::{Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

use serde::Serialize;
use serial_tool_core::{
    Direction, Error, RunError, RunReport, Sequence, SerialSession, SerialSettings,
    SerialTransport, SimulationTransport, StepOutcome, StepRunner,
};

const POLL_INTERVAL: Duration = Duration::from_millis(15);
const READ_CHUNK: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Live,
    Simulation,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Connected,
    Data {
        direction: &'static str,
        bytes: Vec<u8>,
    },
    Disconnected,
    ConnectionError {
        message: String,
    },
}

#[derive(Debug, Serialize)]
pub struct StepResultDto {
    pub step_id: String,
    pub kind: &'static str,
    pub bytes_written: Option<usize>,
    pub requested_duration_ms: Option<u128>,
    pub bytes: Option<Vec<u8>>,
    pub completed_iterations: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TranscriptDto {
    pub step_id: String,
    pub direction: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub status: &'static str,
    pub failing_step_id: Option<String>,
    pub error: Option<String>,
    pub partial_bytes: Option<Vec<u8>>,
    pub step_results: Vec<StepResultDto>,
    pub transcript: Vec<TranscriptDto>,
}

impl RunResult {
    fn from_report(report: RunReport, failure: Option<(String, Error)>) -> Self {
        let (status, failing_step_id, error, partial_bytes) = match failure {
            Some((id, error)) => (
                "failed",
                Some(id),
                Some(error.to_string()),
                error.partial().map(ToOwned::to_owned),
            ),
            None => ("success", None, None, None),
        };
        Self {
            status,
            failing_step_id,
            error,
            partial_bytes,
            step_results: report
                .step_results
                .into_iter()
                .map(|result| {
                    let mut dto = StepResultDto {
                        step_id: result.step_id.as_str().to_owned(),
                        kind: "",
                        bytes_written: None,
                        requested_duration_ms: None,
                        bytes: None,
                        completed_iterations: None,
                    };
                    match result.outcome {
                        StepOutcome::SendText { bytes_written } => {
                            dto.kind = "send_text";
                            dto.bytes_written = Some(bytes_written);
                        }
                        StepOutcome::SendBytes { bytes_written } => {
                            dto.kind = "send_bytes";
                            dto.bytes_written = Some(bytes_written);
                        }
                        StepOutcome::Wait { requested_duration } => {
                            dto.kind = "wait";
                            dto.requested_duration_ms = Some(requested_duration.as_millis());
                        }
                        StepOutcome::Read { bytes } => {
                            dto.kind = "read";
                            dto.bytes = Some(bytes);
                        }
                        StepOutcome::ReadUntil { bytes } => {
                            dto.kind = "read_until";
                            dto.bytes = Some(bytes);
                        }
                        StepOutcome::Repeat {
                            completed_iterations,
                        } => {
                            dto.kind = "repeat";
                            dto.completed_iterations = Some(completed_iterations);
                        }
                    }
                    dto
                })
                .collect(),
            transcript: report
                .transcript
                .entries
                .into_iter()
                .map(|entry| TranscriptDto {
                    step_id: entry.step_id.as_str().to_owned(),
                    direction: match entry.direction {
                        Direction::Tx => "tx",
                        Direction::Rx => "rx",
                    },
                    bytes: entry.bytes,
                })
                .collect(),
        }
    }
}

enum Session {
    Live(SerialSession<SerialTransport>),
    Simulation(SerialSession<SimulationTransport>),
}

impl Session {
    fn send(&mut self, bytes: &[u8]) -> Result<(), Error> {
        match self {
            Self::Live(session) => session.write_all(bytes).and_then(|()| session.flush()),
            Self::Simulation(session) => session.write_all(bytes).and_then(|()| session.flush()),
        }
    }

    fn pending(&self) -> Result<usize, Error> {
        match self {
            Self::Live(session) => {
                Ok(session.buffered_len() + session.transport().bytes_to_read()?)
            }
            Self::Simulation(session) => {
                Ok(session.buffered_len() + session.transport().bytes_to_read())
            }
        }
    }

    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Error> {
        match self {
            Self::Live(session) => session.read(bytes),
            Self::Simulation(session) => session.read(bytes),
        }
    }

    fn run(&mut self, sequence: &Sequence) -> Result<RunReport, RunError> {
        match self {
            Self::Live(session) => StepRunner::new(session).run(&sequence.steps),
            Self::Simulation(session) => StepRunner::new(session).run(&sequence.steps),
        }
    }
}

enum Command {
    Send(Vec<u8>, mpsc::SyncSender<Result<(), String>>),
    Run(Sequence, mpsc::SyncSender<Result<RunResult, String>>),
    Disconnect(mpsc::SyncSender<()>),
}

struct Connection {
    commands: mpsc::Sender<Command>,
    thread: JoinHandle<()>,
}

#[derive(Default)]
pub struct SessionManager {
    connection: Mutex<Option<Connection>>,
}

impl SessionManager {
    pub fn connect(
        &self,
        mode: Mode,
        settings: SerialSettings,
        events: impl Fn(Event) + Send + 'static,
    ) -> Result<(), String> {
        settings.validate().map_err(|error| error.to_string())?;
        let mut guard = self.connection.lock().map_err(|error| error.to_string())?;
        Self::reap_finished(&mut guard);
        if guard.is_some() {
            return Err("Already connected".into());
        }
        let session = match mode {
            Mode::Live => Session::Live(SerialSession::new(
                SerialTransport::open(&settings).map_err(|error| error.to_string())?,
            )),
            Mode::Simulation => {
                Session::Simulation(SerialSession::new(SimulationTransport::loopback()))
            }
        };
        let (commands, receiver) = mpsc::channel();
        let owner_settings = settings.clone();
        let thread = thread::Builder::new()
            .name("serial-session-owner".into())
            .spawn(move || owner_loop(session, owner_settings, receiver, events))
            .map_err(|error| error.to_string())?;
        *guard = Some(Connection { commands, thread });
        Ok(())
    }

    pub fn send(&self, bytes: Vec<u8>) -> Result<(), String> {
        let commands = self.sender()?;
        let (reply, response) = mpsc::sync_channel(1);
        commands
            .send(Command::Send(bytes, reply))
            .map_err(|_| "Connection closed")?;
        response.recv().map_err(|_| "Connection closed")?
    }

    pub fn run(&self, sequence: Sequence) -> Result<RunResult, String> {
        let commands = self.sender()?;
        let (reply, response) = mpsc::sync_channel(1);
        commands
            .send(Command::Run(sequence, reply))
            .map_err(|_| "Connection closed")?;
        response.recv().map_err(|_| "Connection closed")?
    }

    pub fn disconnect(&self) -> Result<(), String> {
        let mut guard = self.connection.lock().map_err(|error| error.to_string())?;
        let connection = guard.take();
        let Some(connection) = connection else {
            return Err("Not connected".into());
        };
        if !connection.thread.is_finished() {
            let (reply, response) = mpsc::sync_channel(1);
            let _ = connection.commands.send(Command::Disconnect(reply));
            let _ = response.recv();
        }
        let result = connection
            .thread
            .join()
            .map_err(|_| "Session owner panicked".to_owned());
        drop(guard);
        result
    }

    fn sender(&self) -> Result<mpsc::Sender<Command>, String> {
        let mut guard = self.connection.lock().map_err(|error| error.to_string())?;
        Self::reap_finished(&mut guard);
        guard
            .as_ref()
            .map(|connection| connection.commands.clone())
            .ok_or_else(|| "Not connected".into())
    }

    fn reap_finished(guard: &mut Option<Connection>) {
        if guard
            .as_ref()
            .is_some_and(|connection| connection.thread.is_finished())
            && let Some(connection) = guard.take()
        {
            let _ = connection.thread.join();
        }
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        if self.connection.get_mut().is_ok_and(|value| value.is_some()) {
            let _ = self.disconnect();
        }
    }
}

fn owner_loop(
    mut session: Session,
    settings: SerialSettings,
    commands: mpsc::Receiver<Command>,
    events: impl Fn(Event),
) {
    events(Event::Connected);
    loop {
        let command = match commands.try_recv() {
            Ok(command) => Some(command),
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => None,
        };
        if let Some(command) = command {
            if handle_command(&mut session, &settings, command, &events) {
                break;
            }
            continue;
        }
        match session.pending() {
            Ok(0) => match commands.recv_timeout(POLL_INTERVAL) {
                Ok(command) => {
                    if handle_command(&mut session, &settings, command, &events) {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            },
            Ok(pending) => {
                let mut buffer = [0u8; READ_CHUNK];
                match session.read(&mut buffer[..pending.min(READ_CHUNK)]) {
                    Ok(count) if count > 0 => events(Event::Data {
                        direction: "rx",
                        bytes: buffer[..count].to_vec(),
                    }),
                    Ok(_) | Err(Error::Timeout { .. }) => {}
                    Err(error) => {
                        events(Event::ConnectionError {
                            message: error.to_string(),
                        });
                        break;
                    }
                }
            }
            Err(error) => {
                events(Event::ConnectionError {
                    message: error.to_string(),
                });
                break;
            }
        }
    }
    events(Event::Disconnected);
}

fn handle_command(
    session: &mut Session,
    settings: &SerialSettings,
    command: Command,
    events: &impl Fn(Event),
) -> bool {
    match command {
        Command::Send(bytes, reply) => {
            let result = session.send(&bytes).map_err(|error| error.to_string());
            if result.is_ok() {
                events(Event::Data {
                    direction: "tx",
                    bytes,
                });
            }
            let _ = reply.send(result);
        }
        Command::Run(sequence, reply) => {
            let result = if !sequence.serial.matches_serial_settings(settings) {
                Err("Sequence serial settings do not match the current connection".into())
            } else {
                match session.run(&sequence) {
                    Ok(report) => Ok(RunResult::from_report(report, None)),
                    Err(RunError::Execution(failure)) => Ok(RunResult::from_report(
                        failure.report,
                        Some((failure.step_id.as_str().to_owned(), failure.error)),
                    )),
                    Err(RunError::Validation(error)) => Err(error.to_string()),
                }
            };
            let _ = reply.send(result);
        }
        Command::Disconnect(reply) => {
            let _ = reply.send(());
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_tool_core::{DataBits, FlowControl, Parity, StopBits};

    fn settings() -> SerialSettings {
        SerialSettings {
            port: "simulation".into(),
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            timeout: Duration::from_millis(1000),
        }
    }

    fn connect() -> (SessionManager, mpsc::Receiver<Event>) {
        let manager = SessionManager::default();
        let (sender, events) = mpsc::channel();
        manager
            .connect(Mode::Simulation, settings(), move |event| {
                sender.send(event).unwrap();
            })
            .unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::Connected
        ));
        (manager, events)
    }

    fn next_data(events: &mpsc::Receiver<Event>) -> (&'static str, Vec<u8>) {
        match events.recv_timeout(Duration::from_secs(1)).unwrap() {
            Event::Data { direction, bytes } => (direction, bytes),
            other => panic!("expected data event, got {other:?}"),
        }
    }

    #[test]
    fn simulation_session_send_rx_and_disconnect() {
        let (manager, events) = connect();
        manager.send(b"OK\r\n".to_vec()).unwrap();
        assert_eq!(next_data(&events), ("tx", b"OK\r\n".to_vec()));
        assert_eq!(next_data(&events), ("rx", b"OK\r\n".to_vec()));
        manager.disconnect().unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::Disconnected
        ));
        let (sender, reconnect_events) = mpsc::channel();
        manager
            .connect(Mode::Simulation, settings(), move |event| {
                sender.send(event).unwrap();
            })
            .unwrap();
        manager.disconnect().unwrap();
        assert!(matches!(
            reconnect_events
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            Event::Connected
        ));
    }

    #[test]
    fn sequence_exclusively_reads_and_monitor_resumes() {
        let (manager, events) = connect();
        let sequence = Sequence::from_json_str(r#"{
            "sequence_version": 1,
            "serial": {"baud_rate":115200,"data_bits":8,"parity":"none","stop_bits":1,"flow_control":"none","timeout_ms":1000},
            "steps": [
                {"id":"send-frame","type":"send_bytes","hex":"0055aaff0d0a"},
                {"id":"read-frame","type":"read_until","delimiter_hex":"0d0a","max_bytes":64}
            ]
        }"#).unwrap();
        let result = manager.run(sequence).unwrap();
        assert_eq!(result.status, "success");
        assert_eq!(result.step_results.len(), 2);
        assert_eq!(result.step_results[0].step_id, "send-frame");
        assert_eq!(result.step_results[0].bytes_written, Some(6));
        assert_eq!(result.step_results[1].step_id, "read-frame");
        assert_eq!(
            result.step_results[1].bytes,
            Some(vec![0, 0x55, 0xaa, 0xff, 0x0d, 0x0a])
        );
        assert_eq!(result.transcript.len(), 2);
        assert_eq!(result.transcript[0].direction, "tx");
        assert_eq!(result.transcript[1].direction, "rx");
        assert_eq!(
            result.transcript[1].bytes,
            vec![0, 0x55, 0xaa, 0xff, 0x0d, 0x0a]
        );
        manager.send(vec![0xaa]).unwrap();
        assert_eq!(next_data(&events), ("tx", vec![0xaa]));
        assert_eq!(next_data(&events), ("rx", vec![0xaa]));
        manager.disconnect().unwrap();
    }

    #[test]
    fn mismatched_sequence_keeps_connection_usable() {
        let (manager, events) = connect();
        let sequence = Sequence::from_json_str(r#"{
            "sequence_version": 1,
            "serial": {"baud_rate":9600,"data_bits":8,"parity":"none","stop_bits":1,"flow_control":"none","timeout_ms":1000},
            "steps": [{"id":"send-byte","type":"send_bytes","hex":"aa"}]
        }"#).unwrap();
        assert!(manager.run(sequence).unwrap_err().contains("do not match"));
        manager.send(vec![0xbb]).unwrap();
        assert_eq!(next_data(&events), ("tx", vec![0xbb]));
        assert_eq!(next_data(&events), ("rx", vec![0xbb]));
        manager.disconnect().unwrap();
    }
}
