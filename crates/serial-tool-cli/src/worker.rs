use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use clap::{Args, ValueEnum};
use serde_json::{Map, Value, json};
use serial_tool_core::{
    DataBits, Error, FlowControl, Parity, RunError, Sequence, SerialSession, SerialSettings,
    SerialTransport, SimulationTransport, StepRunner, StopBits, Transport,
};
use uuid::Uuid;

use crate::{
    CliError, CliFlowControl, CliParity, hex, max_bytes, parse_bytes, sequence_result,
    settings_json, timestamp,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum WorkerMode {
    Live,
    Simulate,
}

#[derive(Debug, Args)]
pub(crate) struct WorkerArgs {
    #[arg(long, value_enum)]
    mode: WorkerMode,
    #[arg(long)]
    port: Option<String>,
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
    #[arg(long)]
    simulation_profile_id: Option<String>,
    #[arg(long, default_value_t = 0)]
    control_port: u16,
}

#[derive(Clone)]
struct JobIdentity {
    worker_job_id: String,
    command: String,
    job_id: Option<String>,
    state: &'static str,
    timestamp_utc: String,
}

struct State {
    active: Option<JobIdentity>,
    last_job: Option<Value>,
    stopping: bool,
    accepted: u64,
    succeeded: u64,
    failed: u64,
}

struct HttpContext<'a> {
    state: &'a Arc<Mutex<State>>,
    sender: &'a mpsc::Sender<Job>,
    output: &'a Mutex<io::Stdout>,
    run_id: &'a str,
    urls: &'a Value,
    mode: &'a str,
    serial_settings: &'a Value,
    settings: &'a SerialSettings,
    port: Option<&'a str>,
}

impl State {
    fn new() -> Self {
        Self {
            active: None,
            last_job: None,
            stopping: false,
            accepted: 0,
            succeeded: 0,
            failed: 0,
        }
    }
}

enum Operation {
    Send(Vec<u8>),
    SendFile(PathBuf),
    Receive(usize),
    Query(Vec<u8>, Vec<u8>, usize),
    RunSequence(Sequence),
}

#[derive(Debug)]
enum WorkerOperationError {
    Serial(Error),
    Sequence(RunError),
    SendFile {
        error: &'static str,
        message: String,
        result: Value,
    },
}

impl From<Error> for WorkerOperationError {
    fn from(error: Error) -> Self {
        Self::Serial(error)
    }
}

struct Job {
    identity: JobIdentity,
    operation: Operation,
}

enum WorkerSession {
    Live(SerialSession<SerialTransport>),
    Simulate(SerialSession<SimulationTransport>),
}

impl WorkerSession {
    fn run_jobs(
        self,
        receiver: mpsc::Receiver<Job>,
        state: Arc<Mutex<State>>,
        output: Arc<Mutex<io::Stdout>>,
        run_id: String,
    ) {
        match self {
            Self::Live(session) => run_jobs(session, receiver, state, output, run_id),
            Self::Simulate(session) => run_jobs(session, receiver, state, output, run_id),
        }
    }
}

fn execute<T: Transport>(
    session: &mut SerialSession<T>,
    operation: Operation,
    state: &Mutex<State>,
) -> Result<Value, WorkerOperationError> {
    match operation {
        Operation::Send(tx) => {
            session.write_all(&tx)?;
            session.flush()?;
            Ok(json!({ "tx_hex": hex(&tx), "tx_bytes": tx.len() }))
        }
        Operation::SendFile(path) => send_file(session, &path, state),
        Operation::Receive(limit) => {
            let mut rx = vec![0; limit];
            let count = session.read(&mut rx)?;
            rx.truncate(count);
            Ok(json!({ "rx_hex": hex(&rx), "rx_bytes": rx.len() }))
        }
        Operation::Query(tx, delimiter, limit) => {
            session.write_all(&tx)?;
            session.flush()?;
            let rx = session.read_until(&delimiter, limit)?;
            Ok(json!({
                "tx_hex": hex(&tx), "tx_bytes": tx.len(),
                "rx_hex": hex(&rx), "rx_bytes": rx.len(),
                "delimiter_hex": hex(&delimiter),
            }))
        }
        Operation::RunSequence(sequence) => {
            let report = StepRunner::new(session)
                .run(&sequence.steps)
                .map_err(WorkerOperationError::Sequence)?;
            Ok(json!({
                "sequence_version": sequence.sequence_version,
                "step_results": sequence_result::step_results_json(&report),
                "transcript": sequence_result::transcript_json(&report),
            }))
        }
    }
}

fn send_file_failure(
    error: &'static str,
    message: impl ToString,
    result: &Value,
) -> WorkerOperationError {
    WorkerOperationError::SendFile {
        error,
        message: message.to_string(),
        result: result.clone(),
    }
}

fn send_file<T: Transport>(
    session: &mut SerialSession<T>,
    path: &Path,
    state: &Mutex<State>,
) -> Result<Value, WorkerOperationError> {
    let mut result = json!({ "bytes_written": 0 });
    if state.lock().unwrap().stopping {
        return Err(send_file_failure(
            "stopped_by_request",
            "send-file stopped by request",
            &result,
        ));
    }
    let mut file =
        File::open(path).map_err(|error| send_file_failure("file_error", error, &result))?;
    let metadata = file
        .metadata()
        .map_err(|error| send_file_failure("file_error", error, &result))?;
    if !metadata.is_file() {
        return Err(send_file_failure(
            "file_error",
            "path must refer to a regular file",
            &result,
        ));
    }
    result["file_bytes"] = json!(metadata.len());
    if metadata.len() == 0 {
        return Err(send_file_failure(
            "file_error",
            "file must not be empty",
            &result,
        ));
    }
    send_file_chunks(session, &mut file, state, result)
}

fn send_file_chunks<T: Transport, R: Read>(
    session: &mut SerialSession<T>,
    file: &mut R,
    state: &Mutex<State>,
    mut result: Value,
) -> Result<Value, WorkerOperationError> {
    let mut buffer = [0; 4096];
    let mut bytes_written = 0u64;
    loop {
        // Resolve EOF before stop so a completed final chunk remains successful.
        let count = match file.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            read => read.map_err(|error| send_file_failure("file_error", error, &result))?,
        };
        if count == 0 {
            return if bytes_written == 0 {
                Err(send_file_failure(
                    "file_error",
                    "file must not be empty",
                    &result,
                ))
            } else {
                Ok(result)
            };
        }
        if state.lock().unwrap().stopping {
            return Err(send_file_failure(
                "stopped_by_request",
                "send-file stopped by request",
                &result,
            ));
        }
        session
            .write_all(&buffer[..count])
            .and_then(|()| session.flush())
            .map_err(|error| send_file_failure("serial_error", error, &result))?;
        bytes_written += count as u64;
        result["bytes_written"] = json!(bytes_written);
    }
}

fn event(name: &str, run_id: &str) -> Value {
    json!({ "event": name, "schema_version": 3, "run_id": run_id, "timestamp_utc": timestamp() })
}

fn features() -> Value {
    json!({ "common.async_jobs": { "contract_version": 1 } })
}

fn common_error(category: &str, code: &str, message: &str) -> Value {
    json!({ "category": category, "code": code, "message": message, "details": {} })
}

fn emit<W: Write>(output: &Mutex<W>, value: &Value) {
    let mut stdout = output.lock().expect("stdout lock poisoned");
    writeln!(stdout, "{value}").expect("stdout write failed");
    stdout.flush().expect("stdout flush failed");
}

fn fatal(output: &Mutex<io::Stdout>, run_id: &str, message: &str, state: &State) -> i32 {
    let mut error = event("error", run_id);
    error["ok"] = json!(false);
    error["exit_code"] = json!(3);
    error["message"] = json!(message);
    error["scope"] = json!("worker");
    error["fatal"] = json!(true);
    error["error"] = common_error("worker", "runtime_error", message);
    emit(output, &error);
    let mut summary = summary(run_id, state, false);
    summary["fatal_error"] = error["error"].clone();
    emit(output, &summary);
    3
}

fn summary(run_id: &str, state: &State, ok: bool) -> Value {
    let mut value = event("summary", run_id);
    value["ok"] = json!(ok);
    value["exit_code"] = json!(if ok { 0 } else { 3 });
    value["accepted"] = json!(state.accepted);
    value["succeeded"] = json!(state.succeeded);
    value["failed"] = json!(state.failed);
    value["fatal_error"] = Value::Null;
    value
}

impl WorkerArgs {
    fn settings(&self) -> Result<SerialSettings, CliError> {
        match self.mode {
            WorkerMode::Live if self.port.is_none() || self.simulation_profile_id.is_some() => {
                return Err(CliError::Input(
                    "live requires --port and forbids --simulation-profile-id",
                ));
            }
            WorkerMode::Simulate
                if self.port.is_some()
                    || self.simulation_profile_id.as_deref() != Some("loopback-v1") =>
            {
                return Err(CliError::Input(
                    "simulate requires --simulation-profile-id loopback-v1 and forbids --port",
                ));
            }
            _ => {}
        }
        let settings = SerialSettings {
            port: self.port.clone().unwrap_or_else(|| "simulation".into()),
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

    pub(crate) fn run(self) -> i32 {
        let output = Mutex::new(io::stdout());
        let run_id = Uuid::new_v4().to_string();
        let settings = match self.settings() {
            Ok(settings) => settings,
            Err(error) => {
                let mut value = event("error", &run_id);
                value["ok"] = json!(false);
                value["exit_code"] = json!(2);
                value["message"] = json!(error.to_string());
                value["scope"] = json!("request");
                value["fatal"] = json!(false);
                value["error"] = common_error("validation", "validation_error", &error.to_string());
                emit(&output, &value);
                return 2;
            }
        };
        let session = match self.mode {
            WorkerMode::Live => match SerialTransport::open(&settings) {
                Ok(transport) => WorkerSession::Live(SerialSession::new(transport)),
                Err(error) => return fatal(&output, &run_id, &error.to_string(), &State::new()),
            },
            WorkerMode::Simulate => {
                WorkerSession::Simulate(SerialSession::new(SimulationTransport::loopback()))
            }
        };
        let listener = match TcpListener::bind(("127.0.0.1", self.control_port)) {
            Ok(listener) => listener,
            Err(error) => {
                return fatal(
                    &output,
                    &run_id,
                    &format!("HTTP bind failed: {error}"),
                    &State::new(),
                );
            }
        };
        let port = listener
            .local_addr()
            .expect("bound listener has address")
            .port();
        let base = format!("http://127.0.0.1:{port}");
        let urls = json!({
            "status_url": format!("{base}/status"),
            "command_url": format!("{base}/command"),
            "stop_url": format!("{base}/stop"),
        });
        let mut serial_settings = settings_json(&settings);
        if matches!(self.mode, WorkerMode::Simulate) {
            serial_settings.as_object_mut().unwrap().remove("port");
        }
        let mode = match self.mode {
            WorkerMode::Live => "live",
            WorkerMode::Simulate => "simulate",
        };
        let state = Arc::new(Mutex::new(State::new()));
        let output = Arc::new(output);
        let (sender, receiver) = mpsc::channel::<Job>();
        let runner_state = Arc::clone(&state);
        let runner_output = Arc::clone(&output);
        let runner_id = run_id.clone();
        let runner = thread::spawn(move || {
            session.run_jobs(receiver, runner_state, runner_output, runner_id)
        });
        if let Err(error) = listener.set_nonblocking(true) {
            drop(sender);
            let _ = runner.join();
            return fatal(
                &output,
                &run_id,
                &format!("HTTP control failure: {error}"),
                &state.lock().unwrap(),
            );
        }
        let mut ready = event("ready", &run_id);
        ready["service"] = json!("serial-tool");
        ready["mode"] = json!(mode);
        ready["features"] = features();
        for (key, value) in urls.as_object().unwrap() {
            ready[key] = value.clone();
        }
        match self.mode {
            WorkerMode::Live => ready["port"] = json!(settings.port),
            WorkerMode::Simulate => ready["simulation_profile_id"] = json!("loopback-v1"),
        }
        emit(&output, &ready);
        let context = HttpContext {
            state: &state,
            sender: &sender,
            output: &output,
            run_id: &run_id,
            urls: &urls,
            mode,
            serial_settings: &serial_settings,
            settings: &settings,
            port: self.port.as_deref(),
        };
        let mut control_error = None;
        let mut shutdown_after = None;
        loop {
            if runner.is_finished() && !state.lock().unwrap().stopping {
                control_error = Some("runner stopped unexpectedly".to_string());
                break;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    if let Err(error) = handle_http(&mut stream, &context) {
                        eprintln!("HTTP request failed: {error}");
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => {
                    control_error = Some(format!("HTTP control failure: {error}"));
                    break;
                }
            }
            let current = state.lock().unwrap();
            if current.stopping && current.active.is_none() {
                let deadline = shutdown_after
                    .get_or_insert_with(|| Instant::now() + Duration::from_millis(200));
                if Instant::now() >= *deadline {
                    break;
                }
            }
            drop(current);
            thread::sleep(Duration::from_millis(10));
        }
        drop(sender);
        if runner.join().is_err() {
            control_error = Some("runner internal failure".into());
        }
        let current = state.lock().unwrap();
        if let Some(message) = control_error {
            fatal(&output, &run_id, &message, &current)
        } else {
            emit(&output, &summary(&run_id, &current, true));
            0
        }
    }
}

fn run_jobs<T: Transport, W: Write>(
    mut session: SerialSession<T>,
    receiver: mpsc::Receiver<Job>,
    state: Arc<Mutex<State>>,
    output: Arc<Mutex<W>>,
    run_id: String,
) {
    for job in receiver {
        let mut started = job_event("job_started", &run_id, &job.identity);
        started["status"] = json!("running");
        {
            let mut current = state.lock().unwrap();
            if let Some(active) = current.active.as_mut() {
                active.state = "running";
                active.timestamp_utc = started["timestamp_utc"].as_str().unwrap().to_owned();
            }
        }
        emit(&output, &started);
        let result = execute(&mut session, job.operation, &state);
        let mut terminal = job_event("job_finished", &run_id, &job.identity);
        terminal["result"] = Value::Null;
        terminal["error"] = Value::Null;
        match result {
            Ok(value) => {
                terminal["state"] = json!("succeeded");
                terminal["ok"] = json!(true);
                terminal["result"] = value;
            }
            Err(error) => {
                terminal["state"] = json!("failed");
                terminal["ok"] = json!(false);
                terminal["exit_code"] = json!(3);
                match error {
                    WorkerOperationError::Serial(error) => {
                        terminal["error"] =
                            common_error("execution", "serial_error", &error.to_string());
                        terminal["message"] = json!(error.to_string());
                        if let Some(partial) = error.partial() {
                            terminal["partial_hex"] = json!(hex(partial));
                            terminal["partial_bytes"] = json!(partial.len());
                        }
                    }
                    WorkerOperationError::SendFile {
                        error,
                        message,
                        result,
                    } => {
                        terminal["error"] = common_error("execution", error, &message);
                        terminal["message"] = json!(message);
                        terminal["result"] = result;
                    }
                    WorkerOperationError::Sequence(error) => {
                        terminal["error"] = common_error(
                            "execution",
                            "sequence_execution_error",
                            &error.to_string(),
                        );
                        terminal["message"] = json!(error.to_string());
                        if let RunError::Execution(failure) = error {
                            terminal["step_id"] = json!(failure.step_id.as_str());
                            terminal["step_results"] =
                                sequence_result::step_results_json(&failure.report);
                            terminal["transcript"] =
                                sequence_result::transcript_json(&failure.report);
                            if let Some(partial) = failure.error.partial() {
                                terminal["partial_hex"] = json!(hex(partial));
                                terminal["partial_bytes"] = json!(partial.len());
                            }
                        }
                    }
                }
            }
        }
        {
            let mut current = state.lock().unwrap();
            if terminal["ok"] == true {
                current.succeeded += 1;
            } else {
                current.failed += 1;
            }
            current.last_job = Some(terminal.clone());
            emit(&output, &terminal);
            current.active = None;
        }
    }
}

fn job_event(name: &str, run_id: &str, identity: &JobIdentity) -> Value {
    let mut value = event(name, run_id);
    value["worker_job_id"] = json!(identity.worker_job_id);
    value["command"] = json!(identity.command);
    value["job_id"] = json!(identity.job_id);
    value
}

fn handle_http(stream: &mut TcpStream, context: &HttpContext<'_>) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    let HttpContext {
        state,
        sender,
        output,
        run_id,
        urls,
        mode,
        serial_settings,
        settings,
        port,
    } = context;
    let port = *port;
    let (method, path, body) = match read_request(stream) {
        Ok(request) => request,
        Err(error) => {
            write_response(
                stream,
                400,
                &validation(run_id, None, None, &error.to_string()),
            )?;
            return Ok(());
        }
    };
    match (method.as_str(), path.as_str()) {
        ("GET", "/status") => {
            let current = state.lock().unwrap();
            let mut value = json!({
                "schema_version": 3, "service": "serial-tool", "run_id": run_id,
                "status": if current.stopping { "stopping" } else if current.active.is_some() { "busy" } else { "ready" },
                "worker_state": if current.stopping { "stopping" } else { "running" },
                "features": features(),
                "mode": mode, "active_job": current.active.as_ref().map(identity_json),
                "last_job": current.last_job, "fatal_error": null,
                "timestamp_utc": timestamp(), "serial_settings": serial_settings,
            });
            for (key, item) in urls.as_object().unwrap() {
                value[key] = item.clone();
            }
            if let Some(port) = port {
                value["port"] = json!(port);
            } else {
                value["simulation_profile_id"] = json!("loopback-v1");
            }
            write_response(stream, 200, &value)
        }
        ("POST", "/command") => {
            let (identity, operation) = match parse_command(&body, settings, run_id) {
                Ok(parsed) => parsed,
                Err(value) => return write_response(stream, 400, &value),
            };
            let mut current = state.lock().unwrap();
            let reason = if current.stopping {
                Some("stopping")
            } else if current.active.is_some() {
                Some("busy")
            } else {
                None
            };
            if let Some(reason) = reason {
                return write_response(
                    stream,
                    409,
                    &json!({
                        "schema_version": 3, "run_id": run_id, "status": "rejected",
                        "command": identity.command, "job_id": identity.job_id,
                        "kind": "async_job", "worker_job_id": null, "reason": reason,
                        "error": common_error("admission", reason, reason),
                    }),
                );
            }
            current.accepted += 1;
            let identity = JobIdentity {
                worker_job_id: format!("job-{}", current.accepted),
                timestamp_utc: timestamp(),
                ..identity
            };
            current.active = Some(identity.clone());
            let response = json!({
                "schema_version": 3, "run_id": run_id, "status": "accepted",
                "command": identity.command, "job_id": identity.job_id,
                "kind": "async_job", "worker_job_id": identity.worker_job_id, "error": null,
            });
            let mut accepted = job_event("job_accepted", run_id, &identity);
            accepted["status"] = json!("accepted");
            emit(output, &accepted);
            if sender
                .send(Job {
                    identity,
                    operation,
                })
                .is_err()
            {
                current.active = None;
                current.accepted -= 1;
                return write_response(
                    stream,
                    500,
                    &json!({
                        "schema_version": 3, "run_id": run_id, "status": "error",
                        "command": response["command"], "job_id": response["job_id"],
                        "kind": "async_job", "worker_job_id": null,
                        "error": common_error("worker", "runtime_error", "runner unavailable"),
                    }),
                );
            }
            drop(current);
            write_response(stream, 202, &response)
        }
        ("POST", "/stop") => {
            if !valid_stop_body(&body) {
                return write_response(
                    stream,
                    400,
                    &json!({
                        "schema_version": 3, "run_id": run_id, "status": "error",
                        "error": common_error("validation", "validation_error", "stop body must be empty or an object with optional integer schema_version 3 and string reason"),
                    }),
                );
            }
            let mut current = state.lock().unwrap();
            if !current.stopping {
                current.stopping = true;
                emit(output, &event("stop_requested", run_id));
            }
            write_response(
                stream,
                200,
                &json!({"schema_version": 3, "run_id": run_id, "status": "accepted", "error": null}),
            )
        }
        _ => write_response(
            stream,
            404,
            &json!({"schema_version": 3, "run_id": run_id, "status": "error", "error": common_error("validation", "not_found", "unknown endpoint")}),
        ),
    }
}

fn identity_json(identity: &JobIdentity) -> Value {
    json!({
        "worker_job_id": identity.worker_job_id, "command": identity.command, "job_id": identity.job_id,
        "state": identity.state, "timestamp_utc": identity.timestamp_utc,
    })
}

fn valid_stop_body(body: &[u8]) -> bool {
    if body.is_empty() {
        return true;
    }
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    object.iter().all(|(key, value)| match key.as_str() {
        "schema_version" => value.as_u64() == Some(3),
        "reason" => value.is_string(),
        _ => false,
    })
}

fn parse_command(
    body: &[u8],
    settings: &SerialSettings,
    run_id: &str,
) -> Result<(JobIdentity, Operation), Value> {
    let parsed: Value = serde_json::from_slice(body)
        .map_err(|_| validation(run_id, None, None, "malformed JSON"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| validation(run_id, None, None, "request must be an object"))?;
    let command = object.get("command").and_then(Value::as_str);
    let job_id = object.get("job_id").and_then(Value::as_str);
    let reject = |message| validation(run_id, command, job_id, message);
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "schema_version" | "command" | "arguments" | "job_id"
        )
    }) {
        return Err(reject("unknown top-level field"));
    }
    if object.get("schema_version").and_then(Value::as_u64) != Some(3) {
        return Err(reject("schema_version must be integer 3"));
    }
    let command = command
        .filter(|name| !name.is_empty())
        .ok_or_else(|| validation(run_id, None, job_id, "command must be a non-empty string"))?;
    if object.contains_key("job_id") && job_id.is_none() {
        return Err(validation(
            run_id,
            Some(command),
            None,
            "job_id must be a string",
        ));
    }
    let arguments = match object.get("arguments") {
        Some(value) => value
            .as_object()
            .ok_or_else(|| reject("arguments must be an object"))?,
        None => &Map::new(),
    };
    let operation = match command {
        "send" => {
            exact_keys(arguments, &["tx_hex"]).map_err(reject)?;
            Operation::Send(required_hex(arguments, "tx_hex").map_err(reject)?)
        }
        "send-file" => {
            exact_keys(arguments, &["path"]).map_err(reject)?;
            let value = arguments
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.trim().is_empty() && !path.contains('\0'))
                .ok_or_else(|| reject("path must be a non-empty string"))?;
            let path = Path::new(value);
            if !path.is_absolute() {
                return Err(reject("path must be an absolute local file path"));
            }
            #[cfg(windows)]
            if !matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
            {
                return Err(reject("path must be an absolute local file path"));
            }
            Operation::SendFile(path.to_owned())
        }
        "receive" => {
            exact_keys(arguments, &["max_bytes"]).map_err(reject)?;
            Operation::Receive(read_limit(arguments).map_err(reject)?)
        }
        "query" => {
            exact_keys(arguments, &["tx_hex", "delimiter_hex", "max_bytes"]).map_err(reject)?;
            let tx = required_hex(arguments, "tx_hex").map_err(reject)?;
            let delimiter = required_hex(arguments, "delimiter_hex").map_err(reject)?;
            let limit = read_limit(arguments).map_err(reject)?;
            if delimiter.len() > limit {
                return Err(reject("max_bytes must be at least delimiter length"));
            }
            Operation::Query(tx, delimiter, limit)
        }
        "run-sequence" => {
            exact_keys(arguments, &["sequence"]).map_err(reject)?;
            let value = arguments
                .get("sequence")
                .filter(|value| value.is_object())
                .ok_or_else(|| reject("sequence must be an object"))?;
            let sequence = Sequence::from_json_str(&value.to_string())
                .map_err(|error| validation(run_id, Some(command), job_id, &error.to_string()))?;
            if !sequence.serial.matches_serial_settings(settings) {
                return Err(reject(
                    "sequence serial settings must match Worker startup settings",
                ));
            }
            Operation::RunSequence(sequence)
        }
        _ => return Err(reject("unknown command")),
    };
    Ok((
        JobIdentity {
            worker_job_id: String::new(),
            command: command.into(),
            job_id: job_id.map(str::to_owned),
            state: "queued",
            timestamp_utc: String::new(),
        },
        operation,
    ))
}

fn exact_keys(arguments: &Map<String, Value>, allowed: &[&str]) -> Result<(), &'static str> {
    if arguments.keys().any(|key| !allowed.contains(&key.as_str())) {
        Err("unknown argument field")
    } else {
        Ok(())
    }
}

fn required_hex(arguments: &Map<String, Value>, name: &str) -> Result<Vec<u8>, &'static str> {
    let value = arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or("required hex argument must be a string")?;
    parse_bytes(None, Some(value)).map_err(|_| "invalid hex bytes")
}

fn read_limit(arguments: &Map<String, Value>) -> Result<usize, &'static str> {
    match arguments.get("max_bytes") {
        None => Ok(1024),
        Some(value) => {
            let number = value.as_u64().ok_or("max_bytes must be an integer")?;
            let number = usize::try_from(number).map_err(|_| "invalid max_bytes")?;
            max_bytes(number).map_err(|_| "max_bytes must be between 1 and 1048576")
        }
    }
}

fn validation(run_id: &str, command: Option<&str>, job_id: Option<&str>, message: &str) -> Value {
    let kind = match command {
        Some("send" | "send-file" | "receive" | "query" | "run-sequence") => Some("async_job"),
        _ => None,
    };
    json!({
        "schema_version": 3, "run_id": run_id, "status": "error", "command": command,
        "job_id": job_id, "kind": kind, "worker_job_id": null,
        "error": common_error("validation", "validation_error", message),
    })
}

fn read_request(stream: &mut TcpStream) -> io::Result<(String, String, Vec<u8>)> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing method"))?
        .to_owned();
    let path = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing path"))?
        .to_owned();
    if parts.next() != Some("HTTP/1.1") && !line.ends_with("HTTP/1.0\r\n") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP version",
        ));
    }
    let mut length = 0usize;
    let mut header_bytes = line.len();
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        header_bytes += line.len();
        if header_bytes > 16 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "headers too large",
            ));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if line.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete headers",
            ));
        }
        if let Some(value) = line
            .split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        {
            length = value.1.trim().parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid content length")
            })?;
        }
    }
    if length > 16 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok((method, path, body))
}

fn write_response(stream: &mut TcpStream, status: u16, value: &Value) -> io::Result<()> {
    let body = value.to_string();
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_slot_rejects_another_command() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let body = json!({"schema_version": 3, "command": "send", "arguments": {"tx_hex": "01"}})
            .to_string();
        write!(
            client,
            "POST /command HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let state = Arc::new(Mutex::new(State::new()));
        state.lock().unwrap().active = Some(JobIdentity {
            worker_job_id: "job-1".into(),
            command: "receive".into(),
            job_id: None,
            state: "running",
            timestamp_utc: timestamp(),
        });
        let (sender, receiver) = mpsc::channel();
        let output = Mutex::new(io::stdout());
        let urls = json!({});
        let settings = SerialSettings {
            port: "simulation".into(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            timeout: Duration::from_millis(1000),
        };
        let settings_json = settings_json(&settings);
        let context = HttpContext {
            state: &state,
            sender: &sender,
            output: &output,
            run_id: "run",
            urls: &urls,
            mode: "simulate",
            serial_settings: &settings_json,
            settings: &settings,
            port: None,
        };
        handle_http(&mut server, &context).unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 409 Conflict"));
        let (_, body) = response.split_once("\r\n\r\n").unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap()["reason"],
            "busy"
        );
        assert!(receiver.try_recv().is_err());
        assert_eq!(state.lock().unwrap().accepted, 0);
    }

    struct SendFileFixture(PathBuf);

    impl SendFileFixture {
        fn new(size: usize) -> Self {
            let path = std::env::temp_dir().join(format!("serial-worker-{}.bin", Uuid::new_v4()));
            let mut file = File::create(&path).unwrap();
            let buffer: [u8; 4096] = std::array::from_fn(|index| index as u8);
            let mut remaining = size;
            while remaining > 0 {
                let count = remaining.min(buffer.len());
                file.write_all(&buffer[..count]).unwrap();
                remaining -= count;
            }
            Self(path)
        }
    }

    impl Drop for SendFileFixture {
        fn drop(&mut self) {
            std::fs::remove_file(&self.0).unwrap();
        }
    }

    struct SendFileGate {
        call: usize,
        on_flush: bool,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    #[derive(Default)]
    struct SendFileTransport {
        written: usize,
        writes: usize,
        flushes: usize,
        fail_write: Option<usize>,
        fail_flush: Option<usize>,
        gate: Option<SendFileGate>,
        state: Option<Arc<Mutex<State>>>,
        dropped: Option<Arc<std::sync::atomic::AtomicBool>>,
    }

    impl SendFileTransport {
        fn gate(&self, call: usize, on_flush: bool) {
            if let Some(state) = &self.state {
                assert!(
                    state.try_lock().is_ok(),
                    "State mutex held during serial I/O"
                );
            }
            if let Some(gate) = &self.gate
                && gate.call == call
                && gate.on_flush == on_flush
            {
                gate.entered.send(()).unwrap();
                gate.release.recv_timeout(Duration::from_secs(5)).unwrap();
            }
        }
    }

    impl Transport for SendFileTransport {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("send-file must not read the serial session")
        }

        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.writes += 1;
            self.gate(self.writes, false);
            assert!(!bytes.is_empty() && bytes.len() <= 4096);
            for (index, byte) in bytes.iter().enumerate() {
                assert_eq!(*byte, (self.written + index) as u8);
            }
            if self.fail_write == Some(self.writes) {
                // An unconfirmed partial write must never enter bytes_written.
                self.written += bytes.len() / 2;
                return Err(io::Error::other("test write failure"));
            }
            self.written += bytes.len();
            Ok(())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            self.gate(self.flushes, true);
            if self.fail_flush == Some(self.flushes) {
                return Err(io::Error::other("test flush failure"));
            }
            Ok(())
        }
    }

    impl Drop for SendFileTransport {
        fn drop(&mut self) {
            if let Some(dropped) = &self.dropped {
                dropped.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    #[test]
    fn send_file_chunks_and_large_file_use_bounded_transport() {
        for size in [6, 4095, 4096, 4097, 12299, 8 * 1024 * 1024 + 17] {
            let fixture = SendFileFixture::new(size);
            let state = Arc::new(Mutex::new(State::new()));
            let mut session = SerialSession::new(SendFileTransport {
                state: Some(state.clone()),
                written: 0,
                writes: 0,
                flushes: 0,
                fail_write: None,
                fail_flush: None,
                gate: None,
                dropped: None,
            });
            let result =
                execute(&mut session, Operation::SendFile(fixture.0.clone()), &state).unwrap();
            assert_eq!(result, json!({"file_bytes": size, "bytes_written": size}));
            assert_eq!(session.transport().written, size);
            assert_eq!(session.transport().writes, size.div_ceil(4096));
            assert_eq!(session.transport().flushes, size.div_ceil(4096));
        }
    }

    #[test]
    fn send_file_serial_failures_count_only_flushed_chunks() {
        let fixture = SendFileFixture::new(3 * 4096);
        for (fail_write, fail_flush, expected) in [
            (Some(1), None, 0),
            (None, Some(1), 0),
            (Some(2), None, 4096),
            (None, Some(2), 4096),
        ] {
            let mut session = SerialSession::new(SendFileTransport {
                fail_write,
                fail_flush,
                written: 0,
                writes: 0,
                flushes: 0,
                gate: None,
                state: None,
                dropped: None,
            });
            let error = send_file(&mut session, &fixture.0, &Mutex::new(State::new())).unwrap_err();
            let WorkerOperationError::SendFile {
                error,
                message,
                result,
            } = error
            else {
                panic!("expected send-file error");
            };
            assert_eq!(error, "serial_error");
            assert!(message.contains("test"));
            assert_eq!(
                result,
                json!({"file_bytes": 12288, "bytes_written": expected})
            );
            assert_eq!(
                session.transport().writes,
                if expected == 0 { 1 } else { 2 }
            );
        }
    }

    #[test]
    fn send_file_read_failure_preserves_confirmed_bytes() {
        struct FailingReader {
            read_once: bool,
            state: Arc<Mutex<State>>,
        }
        impl Read for FailingReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                assert!(
                    self.state.try_lock().is_ok(),
                    "State mutex held during file I/O"
                );
                if self.read_once {
                    return Err(io::Error::other("test file read failure"));
                }
                self.read_once = true;
                for (index, byte) in buffer.iter_mut().enumerate() {
                    *byte = index as u8;
                }
                Ok(buffer.len())
            }
        }
        let state = Arc::new(Mutex::new(State::new()));
        let mut reader = FailingReader {
            read_once: false,
            state: state.clone(),
        };
        let mut session = SerialSession::new(SendFileTransport::default());
        let error = send_file_chunks(
            &mut session,
            &mut reader,
            &state,
            json!({"file_bytes": 8192, "bytes_written": 0}),
        )
        .unwrap_err();
        let WorkerOperationError::SendFile { error, result, .. } = error else {
            panic!("expected file error");
        };
        assert_eq!(error, "file_error");
        assert_eq!(result, json!({"file_bytes": 8192, "bytes_written": 4096}));
        assert_eq!(session.transport().writes, 1);
    }

    fn send_file_http(
        state: &Arc<Mutex<State>>,
        sender: &mpsc::Sender<Job>,
        path: &str,
        body: Value,
    ) -> (u16, Value) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let body = body.to_string();
        write!(
            client,
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let output = Mutex::new(io::stdout());
        let urls = json!({});
        let settings = SerialSettings {
            port: "simulation".into(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            timeout: Duration::from_millis(1000),
        };
        let serial_settings = settings_json(&settings);
        handle_http(
            &mut server,
            &HttpContext {
                state,
                sender,
                output: &output,
                run_id: "send-file-run",
                urls: &urls,
                mode: "simulate",
                serial_settings: &serial_settings,
                settings: &settings,
                port: None,
            },
        )
        .unwrap();
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        (
            head.split_whitespace().nth(1).unwrap().parse().unwrap(),
            serde_json::from_str(body).unwrap(),
        )
    }

    #[test]
    fn send_file_http_stop_at_write_flush_and_completion_boundaries() {
        for (size, call, on_flush, stopped) in [
            (12288, 1, false, true),
            (12288, 2, true, true),
            (4096, 1, true, false),
            (4097, 2, true, false),
        ] {
            let fixture = SendFileFixture::new(size);
            let state = Arc::new(Mutex::new(State::new()));
            let output = Arc::new(Mutex::new(Vec::<u8>::new()));
            let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let session = SerialSession::new(SendFileTransport {
                gate: Some(SendFileGate {
                    call,
                    on_flush,
                    entered: entered_tx,
                    release: release_rx,
                }),
                state: Some(state.clone()),
                dropped: Some(dropped.clone()),
                ..Default::default()
            });
            let (sender, receiver) = mpsc::channel();
            let runner_state = state.clone();
            let runner_output = output.clone();
            let runner = thread::spawn(move || {
                run_jobs(
                    session,
                    receiver,
                    runner_state,
                    runner_output,
                    "send-file-run".into(),
                )
            });
            let body = json!({"schema_version": 3, "command": "send-file", "arguments": {"path": fixture.0}, "job_id": "client-file"});
            let (code, accepted) = send_file_http(&state, &sender, "/command", body.clone());
            assert_eq!(code, 202);
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let (code, busy) = send_file_http(&state, &sender, "/command", body.clone());
            assert_eq!(code, 409);
            assert_eq!(busy["reason"], "busy");
            let (code, response) = send_file_http(&state, &sender, "/stop", json!({}));
            assert_eq!(code, 200);
            assert_eq!(response["status"], "accepted");
            let (code, response) = send_file_http(&state, &sender, "/stop", json!({}));
            assert_eq!(code, 200);
            assert_eq!(response["status"], "accepted");
            let (code, rejected) = send_file_http(&state, &sender, "/command", body);
            assert_eq!(code, 409);
            assert_eq!(rejected["reason"], "stopping");
            assert!(!runner.is_finished());
            release_tx.send(()).unwrap();
            drop(sender);
            runner.join().unwrap();
            assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
            let current = state.lock().unwrap();
            assert!(current.active.is_none());
            assert_eq!(current.accepted, 1);
            assert_eq!(current.failed, u64::from(stopped));
            assert_eq!(current.succeeded, u64::from(!stopped));
            emit(&output, &summary("send-file-run", &current, true));
            let bytes = output.lock().unwrap();
            let events: Vec<Value> = std::str::from_utf8(&bytes)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(events.len(), 3);
            assert_eq!(events[0]["event"], "job_started");
            let terminal = &events[1];
            assert_eq!(terminal, current.last_job.as_ref().unwrap());
            assert_eq!(terminal["worker_job_id"], accepted["worker_job_id"]);
            assert_eq!(terminal["job_id"], "client-file");
            assert_eq!(terminal["run_id"], "send-file-run");
            assert_eq!(terminal["schema_version"], 3);
            assert_eq!(terminal["event"], "job_finished");
            assert_eq!(
                terminal["state"],
                if stopped { "failed" } else { "succeeded" }
            );
            assert_eq!(terminal["ok"], !stopped);
            if !stopped {
                assert_eq!(terminal["error"], Value::Null);
            }
            assert_eq!(
                terminal["result"],
                json!({"file_bytes": size, "bytes_written": if stopped {call * 4096} else {size}})
            );
            if stopped {
                assert_eq!(terminal["error"]["code"], "stopped_by_request");
                assert_eq!(terminal["exit_code"], 3);
            }
            assert_eq!(events[2]["event"], "summary");
            assert_eq!(events[2]["ok"], true);
            assert_eq!(events[2]["exit_code"], 0);
            assert!(terminal.get("tx_hex").is_none());
        }
    }

    #[test]
    fn send_file_stop_before_runner_starts_emits_one_failure() {
        let fixture = SendFileFixture::new(4097);
        let state = Arc::new(Mutex::new(State::new()));
        let (sender, receiver) = mpsc::channel();
        let body =
            json!({"schema_version": 3, "command": "send-file", "arguments": {"path": fixture.0}});
        assert_eq!(send_file_http(&state, &sender, "/command", body).0, 202);
        assert_eq!(send_file_http(&state, &sender, "/stop", json!({})).0, 200);
        drop(sender);
        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        // Failing the first write would expose any I/O after this stop.
        let session = SerialSession::new(SendFileTransport {
            fail_write: Some(1),
            fail_flush: None,
            written: 0,
            writes: 0,
            flushes: 0,
            gate: None,
            state: None,
            dropped: None,
        });
        run_jobs(
            session,
            receiver,
            state.clone(),
            output.clone(),
            "send-file-run".into(),
        );
        let current = state.lock().unwrap();
        assert_eq!(current.failed, 1);
        assert_eq!(current.succeeded, 0);
        let terminal = current.last_job.as_ref().unwrap();
        assert_eq!(terminal["event"], "job_finished");
        assert_eq!(terminal["error"]["code"], "stopped_by_request");
        assert_eq!(terminal["result"], json!({"bytes_written": 0}));
        assert_eq!(
            output
                .lock()
                .unwrap()
                .iter()
                .filter(|byte| **byte == b'\n')
                .count(),
            2
        );
    }
}
