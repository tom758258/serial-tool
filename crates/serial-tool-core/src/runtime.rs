//! Shared persistent serial execution. Event callbacks run on the I/O owner;
//! they must return promptly and must not call blocking runtime methods.
use crate::{
    Direction, Error, RunError, RunReport, Sequence, SerialSession, SerialSettings,
    SerialTransport, SimulationTransport, StepRunner,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

const POLL_INTERVAL: Duration = Duration::from_millis(15);
const READ_CHUNK: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Live,
    Simulation,
}

#[derive(Clone, Debug)]
pub enum Event {
    Connected,
    Data {
        direction: Direction,
        bytes: Vec<u8>,
    },
    ConnectionError {
        message: String,
    },
    Disconnected,
}

#[derive(Debug)]
pub enum RuntimeError {
    Busy,
    Disconnected,
    Other(String),
    Sequence(Box<RunError>),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("Sequence running: connection busy"),
            Self::Disconnected => f.write_str("Not connected"),
            Self::Other(message) => f.write_str(message),
            Self::Sequence(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for RuntimeError {}

struct BusyReservation(Arc<Mutex<bool>>);
impl Drop for BusyReservation {
    fn drop(&mut self) {
        if let Ok(mut busy) = self.0.lock() {
            *busy = false;
        }
    }
}

enum Session {
    Live(SerialSession<SerialTransport>),
    Simulation(SerialSession<SimulationTransport>),
    #[cfg(test)]
    Test(SerialSession<tests::TestTransport>),
}

impl Session {
    fn send(&mut self, bytes: &[u8]) -> Result<(), Error> {
        match self {
            Self::Live(session) => session.write_all(bytes).and_then(|()| session.flush()),
            Self::Simulation(session) => session.write_all(bytes).and_then(|()| session.flush()),
            #[cfg(test)]
            Self::Test(session) => session.write_all(bytes).and_then(|()| session.flush()),
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
            #[cfg(test)]
            Self::Test(session) => Ok(usize::from(session.transport().read_gate.is_none())),
        }
    }

    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Error> {
        match self {
            Self::Live(session) => session.read(bytes),
            Self::Simulation(session) => session.read(bytes),
            #[cfg(test)]
            Self::Test(session) => session.read(bytes),
        }
    }

    fn run(&mut self, sequence: &Sequence) -> Result<RunReport, RunError> {
        match self {
            Self::Live(session) => StepRunner::new(session).run(&sequence.steps),
            Self::Simulation(session) => StepRunner::new(session).run(&sequence.steps),
            #[cfg(test)]
            Self::Test(session) => StepRunner::new(session).run(&sequence.steps),
        }
    }
}

enum Command {
    Send(Vec<u8>, mpsc::SyncSender<Result<(), RuntimeError>>),
    Run(Sequence, mpsc::SyncSender<Result<RunReport, RuntimeError>>),
    Disconnect(mpsc::SyncSender<()>),
}

struct Connection {
    commands: mpsc::Sender<Command>,
    thread: JoinHandle<()>,
    busy: Arc<Mutex<bool>>,
}

#[derive(Default)]
pub struct PersistentRuntime {
    connection: Mutex<Option<Connection>>,
}

impl PersistentRuntime {
    pub fn connect(
        &self,
        mode: Mode,
        settings: SerialSettings,
        events: impl Fn(Event) + Send + 'static,
    ) -> Result<(), RuntimeError> {
        settings
            .validate()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        let mut guard = self
            .connection
            .lock()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        Self::reap_finished(&mut guard);
        if guard.is_some() {
            return Err(RuntimeError::Other("Already connected".into()));
        }
        let session = match mode {
            Mode::Live => Session::Live(SerialSession::new(
                SerialTransport::open(&settings)
                    .map_err(|error| RuntimeError::Other(error.to_string()))?,
            )),
            Mode::Simulation => {
                Session::Simulation(SerialSession::new(SimulationTransport::loopback()))
            }
        };
        self.start(&mut guard, session, settings, events)
    }

    pub fn connect_simulation(
        &self,
        settings: SerialSettings,
        transport: SimulationTransport,
        events: impl Fn(Event) + Send + 'static,
    ) -> Result<(), RuntimeError> {
        settings
            .validate()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        let mut guard = self
            .connection
            .lock()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        Self::reap_finished(&mut guard);
        if guard.is_some() {
            return Err(RuntimeError::Other("Already connected".into()));
        }
        self.start(
            &mut guard,
            Session::Simulation(SerialSession::new(transport)),
            settings,
            events,
        )
    }

    fn start(
        &self,
        guard: &mut Option<Connection>,
        session: Session,
        settings: SerialSettings,
        events: impl Fn(Event) + Send + 'static,
    ) -> Result<(), RuntimeError> {
        let (commands, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("serial-session-owner".into())
            .spawn(move || owner_loop(session, settings, receiver, events))
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        *guard = Some(Connection {
            commands,
            thread,
            busy: Arc::new(Mutex::new(false)),
        });
        Ok(())
    }

    pub fn send(&self, bytes: Vec<u8>) -> Result<(), RuntimeError> {
        let (commands, busy) = self.sender()?;
        let (reply, response) = mpsc::sync_channel(1);
        let admission = busy
            .lock()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        if *admission {
            return Err(RuntimeError::Busy);
        }
        commands
            .send(Command::Send(bytes, reply))
            .map_err(|_| RuntimeError::Disconnected)?;
        drop(admission);
        response.recv().map_err(|_| RuntimeError::Disconnected)?
    }

    pub fn run(&self, sequence: Sequence) -> Result<RunReport, RuntimeError> {
        let (commands, busy) = self.sender()?;
        let mut admission = busy
            .lock()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        if *admission {
            return Err(RuntimeError::Busy);
        }
        // Reserve before enqueueing so concurrent sends cannot slip behind this run.
        *admission = true;
        drop(admission);
        let _reservation = BusyReservation(busy);
        let (reply, response) = mpsc::sync_channel(1);
        commands
            .send(Command::Run(sequence, reply))
            .map_err(|_| RuntimeError::Disconnected)?;
        response.recv().map_err(|_| RuntimeError::Disconnected)?
    }

    pub fn disconnect(&self) -> Result<(), RuntimeError> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        let connection = guard.take();
        let Some(connection) = connection else {
            return Err(RuntimeError::Disconnected);
        };
        if !connection.thread.is_finished() {
            let (reply, response) = mpsc::sync_channel(1);
            let _ = connection.commands.send(Command::Disconnect(reply));
            let _ = response.recv();
        }
        let result = connection
            .thread
            .join()
            .map_err(|_| RuntimeError::Other("Session owner panicked".into()));
        drop(guard);
        result
    }

    fn sender(&self) -> Result<(mpsc::Sender<Command>, Arc<Mutex<bool>>), RuntimeError> {
        let mut guard = self
            .connection
            .lock()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        Self::reap_finished(&mut guard);
        guard
            .as_ref()
            .map(|connection| (connection.commands.clone(), Arc::clone(&connection.busy)))
            .ok_or(RuntimeError::Disconnected)
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

impl Drop for PersistentRuntime {
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
                        direction: Direction::Rx,
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
            let result = session.send(&bytes);
            if result.is_ok() {
                events(Event::Data {
                    direction: Direction::Tx,
                    bytes,
                });
            }
            let failed = result.is_err();
            if let Err(error) = &result {
                events(Event::ConnectionError {
                    message: error.to_string(),
                });
            }
            let _ = reply.send(result.map_err(|error| RuntimeError::Other(error.to_string())));
            if failed {
                return true;
            }
        }
        Command::Run(sequence, reply) => {
            let result = if !sequence.serial.matches_serial_settings(settings) {
                Err(RuntimeError::Other(
                    "Sequence serial settings do not match the current connection".into(),
                ))
            } else {
                session
                    .run(&sequence)
                    .map_err(|error| RuntimeError::Sequence(Box::new(error)))
            };
            let failed = matches!(&result, Err(RuntimeError::Sequence(error))
                if matches!(error.as_ref(), RunError::Execution(failure)
                    if matches!(failure.error, Error::ReadFailed(_) | Error::ReadAvailabilityFailed(_)
                        | Error::WriteFailed(_) | Error::FlushFailed(_))));
            if failed {
                events(Event::ConnectionError {
                    message: result.as_ref().unwrap_err().to_string(),
                });
            }
            let _ = reply.send(result);
            if failed {
                return true;
            }
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
    use crate::{DataBits, FlowControl, Parity, StopBits, Transport};
    use std::io;

    pub(super) struct TestTransport {
        pub(super) read_gate: Option<(mpsc::SyncSender<()>, mpsc::Receiver<()>)>,
    }
    impl Transport for TestTransport {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if let Some((entered, release)) = &self.read_gate {
                let _ = entered.send(());
                release.recv().map_err(io::Error::other)?;
                bytes[0] = 0xaa;
                return Ok(1);
            }
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "disconnected test transport",
            ))
        }
        fn write_all(&mut self, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn settings() -> SerialSettings {
        SerialSettings {
            port: "simulation".into(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            timeout: Duration::from_millis(1000),
        }
    }

    fn sequence(steps: &str) -> Sequence {
        Sequence::from_json_str(&format!(r#"{{
            "sequence_version":1,
            "serial":{{"baud_rate":115200,"data_bits":8,"parity":"none","stop_bits":1,"flow_control":"none","timeout_ms":1000}},
            "steps":{steps}
        }}"#)).unwrap()
    }

    fn next(events: &mpsc::Receiver<Event>) -> Event {
        events.recv_timeout(Duration::from_secs(2)).unwrap()
    }

    fn connect(transport: SimulationTransport) -> (PersistentRuntime, mpsc::Receiver<Event>) {
        let runtime = PersistentRuntime::default();
        let (sender, events) = mpsc::channel();
        runtime
            .connect_simulation(settings(), transport, move |event| {
                let _ = sender.send(event);
            })
            .unwrap();
        assert!(matches!(next(&events), Event::Connected));
        (runtime, events)
    }

    fn data(events: &mpsc::Receiver<Event>, expected_direction: Direction, expected: &[u8]) {
        match next(events) {
            Event::Data { direction, bytes } => {
                assert_eq!(direction, expected_direction);
                assert_eq!(bytes, expected);
            }
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn receives_unsolicited_bytes_without_send_and_disconnects() {
        let (runtime, events) = connect(SimulationTransport::with_rx_chunks([vec![0, 255]]));
        data(&events, Direction::Rx, &[0, 255]);
        runtime.disconnect().unwrap();
        assert!(matches!(next(&events), Event::Disconnected));
        assert!(matches!(
            runtime.send(vec![1]),
            Err(RuntimeError::Disconnected)
        ));
    }

    #[test]
    fn manual_send_preserves_binary_loopback() {
        let (runtime, events) = connect(SimulationTransport::loopback());
        runtime.send(vec![0, 255]).unwrap();
        data(&events, Direction::Tx, &[0, 255]);
        data(&events, Direction::Rx, &[0, 255]);
        runtime.disconnect().unwrap();
        assert!(matches!(next(&events), Event::Disconnected));
    }

    #[test]
    fn sequence_owns_rx_without_duplicate_events_and_monitor_resumes() {
        let (runtime, events) = connect(SimulationTransport::loopback());
        let report = runtime
            .run(sequence(
                r#"[
            {"id":"send","type":"send_bytes","hex":"00ff0d0a"},
            {"id":"read","type":"read_until","delimiter_hex":"0d0a","max_bytes":64}
        ]"#,
            ))
            .unwrap();
        assert_eq!(report.step_results.len(), 2);
        assert_eq!(report.transcript.entries.len(), 2);
        assert_eq!(report.transcript.entries[0].direction, Direction::Tx);
        assert_eq!(report.transcript.entries[1].direction, Direction::Rx);
        assert_eq!(report.transcript.entries[0].bytes, [0, 255, 13, 10]);
        assert_eq!(report.transcript.entries[1].bytes, [0, 255, 13, 10]);
        assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));
        runtime.send(vec![0xaa]).unwrap();
        data(&events, Direction::Tx, &[0xaa]);
        data(&events, Direction::Rx, &[0xaa]);
        runtime.disconnect().unwrap();
    }

    #[test]
    fn sequence_rejects_send_and_second_run_without_queuing() {
        let runtime = Arc::new(PersistentRuntime::default());
        let (sender, events) = mpsc::channel();
        let (entered, reading) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::channel();
        {
            let mut connection = runtime.connection.lock().unwrap();
            runtime
                .start(
                    &mut connection,
                    Session::Test(SerialSession::new(TestTransport {
                        read_gate: Some((entered, gate)),
                    })),
                    settings(),
                    move |event| {
                        let _ = sender.send(event);
                    },
                )
                .unwrap();
        }
        assert!(matches!(next(&events), Event::Connected));
        let owner = Arc::clone(&runtime);
        let steps = sequence(r#"[{"id":"read","type":"read","max_bytes":1}]"#);
        let another = steps.clone();
        let run = thread::spawn(move || owner.run(steps));
        reading.recv_timeout(Duration::from_secs(2)).unwrap();
        let requester = Arc::clone(&runtime);
        let (replies, rejected) = mpsc::channel();
        let probe = thread::spawn(move || {
            let send = requester.send(vec![0xbb]);
            let sequence = requester.run(another);
            let _ = replies.send((send, sequence));
        });
        let (send, sequence) = rejected.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(send, Err(RuntimeError::Busy)));
        assert!(matches!(sequence, Err(RuntimeError::Busy)));
        probe.join().unwrap();
        release.send(()).unwrap();
        run.join().unwrap().unwrap();
        runtime.send(vec![0xaa]).unwrap();
        data(&events, Direction::Tx, &[0xaa]);
        runtime.disconnect().unwrap();
        assert!(matches!(next(&events), Event::Disconnected));
    }

    #[test]
    fn monitor_read_error_emits_error_then_disconnect_and_owner_exits() {
        let runtime = PersistentRuntime::default();
        let (sender, events) = mpsc::channel();
        {
            let mut connection = runtime.connection.lock().unwrap();
            runtime
                .start(
                    &mut connection,
                    Session::Test(SerialSession::new(TestTransport { read_gate: None })),
                    settings(),
                    move |event| {
                        let _ = sender.send(event);
                    },
                )
                .unwrap();
        }
        assert!(matches!(next(&events), Event::Connected));
        assert!(
            matches!(next(&events), Event::ConnectionError { message } if message.contains("read failed"))
        );
        assert!(matches!(next(&events), Event::Disconnected));
        runtime.disconnect().unwrap();
    }
}
