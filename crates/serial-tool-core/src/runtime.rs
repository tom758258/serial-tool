//! Shared persistent serial execution. Event callbacks run on the I/O owner;
//! they must return promptly and must not call blocking runtime methods.
use crate::{
    Direction, Error, RunError, RunReport, Sequence, SerialSession, SerialSettings,
    SerialTransport, SimulationTransport, StepRunner,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
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
            Self::Busy => f.write_str("Connection busy"),
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
            Self::Test(session) => Ok(usize::from(session.transport().monitor_error)),
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
    Send(
        Vec<u8>,
        BusyReservation,
        mpsc::SyncSender<Result<(), RuntimeError>>,
    ),
    StartPeriodic(PeriodicTx, mpsc::SyncSender<Result<(), RuntimeError>>),
    StopPeriodic(mpsc::SyncSender<()>),
    Run(Sequence, mpsc::SyncSender<Result<RunReport, RuntimeError>>),
    Disconnect(mpsc::SyncSender<()>),
}

struct PeriodicTx {
    bytes: Vec<u8>,
    interval: Duration,
    next_send: Instant,
    _reservation: BusyReservation,
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
        let mut admission = busy
            .lock()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        if *admission {
            return Err(RuntimeError::Busy);
        }
        *admission = true;
        drop(admission);
        commands
            .send(Command::Send(bytes, BusyReservation(busy), reply))
            .map_err(|_| RuntimeError::Disconnected)?;
        response.recv().map_err(|_| RuntimeError::Disconnected)?
    }

    pub fn start_periodic(&self, bytes: Vec<u8>, interval: Duration) -> Result<(), RuntimeError> {
        if bytes.is_empty() {
            return Err(RuntimeError::Other(
                "Periodic send data must not be empty".into(),
            ));
        }
        if interval.is_zero()
            || interval.as_millis() == 0
            || !interval.subsec_nanos().is_multiple_of(1_000_000)
        {
            return Err(RuntimeError::Other(
                "Interval must be integer milliseconds greater than zero".into(),
            ));
        }
        let next_send = Instant::now()
            .checked_add(interval)
            .ok_or_else(|| RuntimeError::Other("Interval is too large".into()))?;
        let (commands, busy) = self.sender()?;
        let mut admission = busy
            .lock()
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        if *admission {
            return Err(RuntimeError::Busy);
        }
        *admission = true;
        drop(admission);
        let periodic = PeriodicTx {
            bytes,
            interval,
            next_send,
            _reservation: BusyReservation(busy),
        };
        let (reply, response) = mpsc::sync_channel(1);
        commands
            .send(Command::StartPeriodic(periodic, reply))
            .map_err(|_| RuntimeError::Disconnected)?;
        response.recv().map_err(|_| RuntimeError::Disconnected)?
    }

    pub fn stop_periodic(&self) -> Result<(), RuntimeError> {
        let (commands, _) = self.sender()?;
        let (reply, response) = mpsc::sync_channel(1);
        commands
            .send(Command::StopPeriodic(reply))
            .map_err(|_| RuntimeError::Disconnected)?;
        response.recv().map_err(|_| RuntimeError::Disconnected)
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
    let mut periodic: Option<PeriodicTx> = None;
    loop {
        let command = match commands.try_recv() {
            Ok(command) => Some(command),
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => None,
        };
        if let Some(command) = command {
            if handle_command(&mut session, &settings, command, &events, &mut periodic) {
                break;
            }
            continue;
        }
        if let Some(tx) = periodic.as_mut()
            && Instant::now() >= tx.next_send
        {
            if transmit(&mut session, &tx.bytes, &events).is_err() {
                break;
            }
            tx.next_send = Instant::now() + tx.interval;
        }
        let wait = periodic.as_ref().map_or(POLL_INTERVAL, |tx| {
            POLL_INTERVAL.min(tx.next_send.saturating_duration_since(Instant::now()))
        });
        match session.pending() {
            Ok(0) => match commands.recv_timeout(wait) {
                Ok(command) => {
                    if handle_command(&mut session, &settings, command, &events, &mut periodic) {
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
    drop(periodic);
    events(Event::Disconnected);
}

fn transmit(
    session: &mut Session,
    bytes: &[u8],
    events: &impl Fn(Event),
) -> Result<(), RuntimeError> {
    match session.send(bytes) {
        Ok(()) => {
            events(Event::Data {
                direction: Direction::Tx,
                bytes: bytes.to_vec(),
            });
            Ok(())
        }
        Err(error) => {
            events(Event::ConnectionError {
                message: error.to_string(),
            });
            Err(RuntimeError::Other(error.to_string()))
        }
    }
}

fn handle_command(
    session: &mut Session,
    settings: &SerialSettings,
    command: Command,
    events: &impl Fn(Event),
    periodic: &mut Option<PeriodicTx>,
) -> bool {
    match command {
        Command::Send(bytes, reservation, reply) => {
            let result = transmit(session, &bytes, events);
            let failed = result.is_err();
            drop(reservation);
            let _ = reply.send(result);
            if failed {
                return true;
            }
        }
        Command::StartPeriodic(mut tx, reply) => {
            let result = transmit(session, &tx.bytes, events);
            let failed = result.is_err();
            if !failed {
                tx.next_send = Instant::now() + tx.interval;
                *periodic = Some(tx);
            }
            let _ = reply.send(result);
            if failed {
                return true;
            }
        }
        Command::StopPeriodic(reply) => {
            *periodic = None;
            let _ = reply.send(());
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

    #[derive(Default)]
    pub(super) struct TestTransport {
        pub(super) read_gate: Option<(mpsc::SyncSender<()>, mpsc::Receiver<()>)>,
        write_gate: Option<(mpsc::SyncSender<()>, mpsc::Receiver<()>)>,
        writes_left: Option<usize>,
        pub(super) monitor_error: bool,
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
            if let Some((entered, release)) = self.write_gate.take() {
                let _ = entered.send(());
                release.recv().map_err(io::Error::other)?;
            }
            if let Some(left) = &mut self.writes_left {
                if *left == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "test write failure",
                    ));
                }
                *left -= 1;
            }
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
    fn periodic_repeats_with_rx_then_stops_and_releases_connection() {
        let (runtime, events) = connect(SimulationTransport::loopback());
        let bytes = vec![0, 255, 13, 10];
        runtime
            .start_periodic(bytes.clone(), Duration::from_millis(40))
            .unwrap();
        for _ in 0..3 {
            data(&events, Direction::Tx, &bytes);
            data(&events, Direction::Rx, &bytes);
        }
        assert!(matches!(runtime.send(vec![1]), Err(RuntimeError::Busy)));
        assert!(matches!(
            runtime.run(sequence(
                r#"[{"id":"send","type":"send_text","text":"OK"}]"#
            )),
            Err(RuntimeError::Busy)
        ));
        assert!(matches!(
            runtime.start_periodic(vec![2], Duration::from_millis(1)),
            Err(RuntimeError::Busy)
        ));
        runtime.stop_periodic().unwrap();
        // Events already emitted before Stop may still be queued.
        while events.try_recv().is_ok() {}
        assert!(matches!(
            events.recv_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        runtime.send(vec![0xaa]).unwrap();
        data(&events, Direction::Tx, &[0xaa]);
        data(&events, Direction::Rx, &[0xaa]);
        runtime
            .run(sequence(r#"[{"id":"wait","type":"wait","duration_ms":1}]"#))
            .unwrap();
        runtime.disconnect().unwrap();
    }

    #[test]
    fn periodic_disconnect_drops_scheduler_and_reconnects_idle() {
        let (runtime, events) = connect(SimulationTransport::loopback());
        runtime
            .start_periodic(b"STATUS?".to_vec(), Duration::from_millis(40))
            .unwrap();
        data(&events, Direction::Tx, b"STATUS?");
        runtime.disconnect().unwrap();
        while !matches!(next(&events), Event::Disconnected) {}
        assert!(matches!(
            events.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected)
        ));
        let (sender, reconnected) = mpsc::channel();
        runtime
            .connect(Mode::Simulation, settings(), move |event| {
                let _ = sender.send(event);
            })
            .unwrap();
        assert!(matches!(next(&reconnected), Event::Connected));
        assert!(matches!(
            reconnected.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        runtime.send(vec![1]).unwrap();
        data(&reconnected, Direction::Tx, &[1]);
        data(&reconnected, Direction::Rx, &[1]);
        runtime.disconnect().unwrap();
    }

    #[test]
    fn invalid_periodic_input_sends_nothing_and_does_not_reserve() {
        let (runtime, events) = connect(SimulationTransport::loopback());
        for (bytes, interval) in [
            (vec![1], Duration::ZERO),
            (vec![], Duration::from_millis(1)),
            (vec![1], Duration::from_micros(1500)),
            (vec![1], Duration::from_secs(u64::MAX)),
        ] {
            assert!(matches!(
                runtime.start_periodic(bytes, interval),
                Err(RuntimeError::Other(_))
            ));
        }
        assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));
        runtime
            .start_periodic(vec![1], Duration::from_millis(1))
            .unwrap();
        data(&events, Direction::Tx, &[1]);
        runtime.stop_periodic().unwrap();
        runtime.disconnect().unwrap();
    }

    #[test]
    fn send_reserves_connection_until_write_completes() {
        let runtime = Arc::new(PersistentRuntime::default());
        let (entered, writing) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::channel();
        {
            let mut connection = runtime.connection.lock().unwrap();
            runtime
                .start(
                    &mut connection,
                    Session::Test(SerialSession::new(TestTransport {
                        write_gate: Some((entered, gate)),
                        ..Default::default()
                    })),
                    settings(),
                    |_| {},
                )
                .unwrap();
        }
        let owner = Arc::clone(&runtime);
        let send = thread::spawn(move || owner.send(vec![0, 255, 128, 13, 10]));
        writing.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(runtime.send(vec![1]), Err(RuntimeError::Busy)));
        assert!(matches!(
            runtime.start_periodic(vec![1], Duration::from_millis(1)),
            Err(RuntimeError::Busy)
        ));
        assert!(matches!(
            runtime.run(sequence(r#"[{"id":"wait","type":"wait","duration_ms":1}]"#)),
            Err(RuntimeError::Busy)
        ));
        release.send(()).unwrap();
        send.join().unwrap().unwrap();
        runtime.send(vec![2]).unwrap();
        runtime.disconnect().unwrap();
    }

    #[test]
    fn periodic_write_failure_disconnects_and_releases_scheduler() {
        for successful_writes in [0, 1] {
            let runtime = PersistentRuntime::default();
            let (sender, events) = mpsc::channel();
            {
                let mut connection = runtime.connection.lock().unwrap();
                runtime
                    .start(
                        &mut connection,
                        Session::Test(SerialSession::new(TestTransport {
                            writes_left: Some(successful_writes),
                            ..Default::default()
                        })),
                        settings(),
                        move |event| {
                            let _ = sender.send(event);
                        },
                    )
                    .unwrap();
            }
            assert!(matches!(next(&events), Event::Connected));
            let start = runtime.start_periodic(vec![1], Duration::from_millis(1));
            if successful_writes == 0 {
                assert!(start.is_err());
            } else {
                start.unwrap();
                data(&events, Direction::Tx, &[1]);
            }
            assert!(matches!(next(&events), Event::ConnectionError { .. }));
            assert!(matches!(next(&events), Event::Disconnected));
            runtime.disconnect().unwrap();
            runtime
                .connect(Mode::Simulation, settings(), |_| {})
                .unwrap();
            runtime.send(vec![2]).unwrap();
            runtime.disconnect().unwrap();
        }
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
                        ..Default::default()
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
            let periodic = requester.start_periodic(vec![1], Duration::from_millis(1));
            let _ = replies.send((send, sequence, periodic));
        });
        let (send, sequence, periodic) = rejected.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(send, Err(RuntimeError::Busy)));
        assert!(matches!(sequence, Err(RuntimeError::Busy)));
        assert!(matches!(periodic, Err(RuntimeError::Busy)));
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
                    Session::Test(SerialSession::new(TestTransport {
                        monitor_error: true,
                        ..Default::default()
                    })),
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
