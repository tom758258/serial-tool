use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use clap::{Args, ValueEnum};
use serde_json::{Map, Value, json};
use serial_tool_core::{
    DataBits, Error, FlowControl, Parity, SerialSession, SerialSettings, SerialTransport,
    SimulationTransport, StopBits, Transport,
};
use uuid::Uuid;

use crate::{
    CliError, CliFlowControl, CliParity, hex, max_bytes, parse_bytes, settings_json, timestamp,
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
    Receive(usize),
    Query(Vec<u8>, Vec<u8>, usize),
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
    fn execute(&mut self, operation: Operation) -> Result<Value, Error> {
        match self {
            Self::Live(session) => execute(session, operation),
            Self::Simulate(session) => execute(session, operation),
        }
    }
}

fn execute<T: Transport>(
    session: &mut SerialSession<T>,
    operation: Operation,
) -> Result<Value, Error> {
    match operation {
        Operation::Send(tx) => {
            session.write_all(&tx)?;
            session.flush()?;
            Ok(json!({ "tx_hex": hex(&tx), "tx_bytes": tx.len() }))
        }
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
    }
}

fn event(name: &str, run_id: &str) -> Value {
    json!({ "event": name, "schema_version": 2, "run_id": run_id, "timestamp_utc": timestamp() })
}

fn emit(output: &Mutex<io::Stdout>, value: &Value) {
    let mut stdout = output.lock().expect("stdout lock poisoned");
    writeln!(stdout, "{value}").expect("stdout write failed");
    stdout.flush().expect("stdout flush failed");
}

fn fatal(output: &Mutex<io::Stdout>, run_id: &str, message: &str, state: &State) -> i32 {
    let mut error = event("error", run_id);
    error["ok"] = json!(false);
    error["exit_code"] = json!(3);
    error["message"] = json!(message);
    emit(output, &error);
    let mut summary = summary(run_id, state, false);
    summary["fatal_error"] = json!(message);
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
            run_jobs(session, receiver, runner_state, runner_output, runner_id)
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

fn run_jobs(
    mut session: WorkerSession,
    receiver: mpsc::Receiver<Job>,
    state: Arc<Mutex<State>>,
    output: Arc<Mutex<io::Stdout>>,
    run_id: String,
) {
    for job in receiver {
        let mut started = job_event("job_started", &run_id, &job.identity);
        started["status"] = json!("running");
        emit(&output, &started);
        let result = session.execute(job.operation);
        let mut terminal = job_event(
            if result.is_ok() {
                "job_finished"
            } else {
                "job_failed"
            },
            &run_id,
            &job.identity,
        );
        match result {
            Ok(value) => {
                terminal["ok"] = json!(true);
                terminal["result"] = value;
            }
            Err(error) => {
                terminal["ok"] = json!(false);
                terminal["exit_code"] = json!(3);
                terminal["error"] = json!("serial_error");
                terminal["message"] = json!(error.to_string());
                if let Some(partial) = error.partial() {
                    terminal["partial_hex"] = json!(hex(partial));
                    terminal["partial_bytes"] = json!(partial.len());
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
        port,
    } = context;
    let port = *port;
    let (method, path, body) = match read_request(stream) {
        Ok(request) => request,
        Err(error) => {
            write_response(
                stream,
                400,
                &json!({"schema_version": 2, "status": "error", "command": null, "job_id": null, "error": "validation_error", "message": error.to_string()}),
            )?;
            return Ok(());
        }
    };
    match (method.as_str(), path.as_str()) {
        ("GET", "/status") => {
            let current = state.lock().unwrap();
            let mut value = json!({
                "schema_version": 2, "service": "serial-tool", "run_id": run_id,
                "status": if current.stopping { "stopping" } else if current.active.is_some() { "busy" } else { "ready" },
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
            let (identity, operation) = match parse_command(&body) {
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
                    &json!({"schema_version": 2, "status": "rejected", "command": identity.command, "job_id": identity.job_id, "reason": reason}),
                );
            }
            current.accepted += 1;
            let identity = JobIdentity {
                worker_job_id: format!("job-{}", current.accepted),
                ..identity
            };
            current.active = Some(identity.clone());
            let response = json!({"schema_version": 2, "status": "accepted", "command": identity.command, "job_id": identity.job_id, "worker_job_id": identity.worker_job_id});
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
                    &json!({"schema_version": 2, "status": "error", "command": response["command"], "job_id": response["job_id"], "error": "runtime_error", "message": "runner unavailable"}),
                );
            }
            drop(current);
            write_response(stream, 202, &response)
        }
        ("POST", "/stop") => {
            if !body.is_empty()
                && !matches!(serde_json::from_slice::<Value>(&body), Ok(Value::Object(ref object)) if object.is_empty())
            {
                return write_response(
                    stream,
                    400,
                    &json!({"schema_version": 2, "status": "error", "command": null, "job_id": null, "error": "validation_error", "message": "stop body must be empty or {}"}),
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
                &json!({"schema_version": 2, "status": "stopping"}),
            )
        }
        _ => write_response(
            stream,
            404,
            &json!({"schema_version": 2, "status": "error", "command": null, "job_id": null, "error": "not_found", "message": "unknown endpoint"}),
        ),
    }
}

fn identity_json(identity: &JobIdentity) -> Value {
    json!({"worker_job_id": identity.worker_job_id, "command": identity.command, "job_id": identity.job_id})
}

fn parse_command(body: &[u8]) -> Result<(JobIdentity, Operation), Value> {
    let parsed: Value =
        serde_json::from_slice(body).map_err(|_| validation(None, None, "malformed JSON"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| validation(None, None, "request must be an object"))?;
    let command = object.get("command").and_then(Value::as_str);
    let job_id = object.get("job_id").and_then(Value::as_str);
    let reject = |message| validation(command, job_id, message);
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "schema_version" | "command" | "arguments" | "job_id"
        )
    }) {
        return Err(reject("unknown top-level field"));
    }
    if object.get("schema_version").and_then(Value::as_u64) != Some(2) {
        return Err(reject("schema_version must be integer 2"));
    }
    let command = command
        .filter(|name| !name.is_empty())
        .ok_or_else(|| validation(None, job_id, "command must be a non-empty string"))?;
    if object.contains_key("job_id") && job_id.is_none() {
        return Err(validation(Some(command), None, "job_id must be a string"));
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
        _ => return Err(reject("unknown command")),
    };
    Ok((
        JobIdentity {
            worker_job_id: String::new(),
            command: command.into(),
            job_id: job_id.map(str::to_owned),
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

fn validation(command: Option<&str>, job_id: Option<&str>, message: &str) -> Value {
    json!({"schema_version": 2, "status": "error", "command": command, "job_id": job_id, "error": "validation_error", "message": message})
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
        let body = json!({"schema_version": 2, "command": "send", "arguments": {"tx_hex": "01"}})
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
        });
        let (sender, receiver) = mpsc::channel();
        let output = Mutex::new(io::stdout());
        let urls = json!({});
        let settings = json!({});
        let context = HttpContext {
            state: &state,
            sender: &sender,
            output: &output,
            run_id: "run",
            urls: &urls,
            mode: "simulate",
            serial_settings: &settings,
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
}
