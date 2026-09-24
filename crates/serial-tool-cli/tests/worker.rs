use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use serde_json::{Value, json};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_serial-tool"))
}

fn spawn() -> (Child, Receiver<Value>, Value) {
    let mut child = binary()
        .args([
            "worker",
            "--mode",
            "simulate",
            "--baud",
            "115200",
            "--simulation-profile-id",
            "loopback-v1",
            "--control-port",
            "0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if line.is_empty() {
                continue;
            }
            let value: Value =
                serde_json::from_str(&line).expect("stdout must contain JSON objects");
            assert!(value.is_object());
            assert_eq!(value["schema_version"].as_u64(), Some(2));
            sender.send(value).unwrap();
        }
    });
    let ready = next(&receiver);
    assert_eq!(ready["event"], "ready");
    assert_eq!(ready["mode"], "simulate");
    assert_eq!(ready["simulation_profile_id"], "loopback-v1");
    assert!(!ready["run_id"].as_str().unwrap().is_empty());
    for key in ["status_url", "command_url", "stop_url"] {
        assert!(
            ready[key]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
    }
    (child, receiver, ready)
}

fn next(receiver: &Receiver<Value>) -> Value {
    receiver.recv_timeout(Duration::from_secs(5)).unwrap()
}

fn request(url: &str, method: &str, body: &str) -> (u16, Value) {
    let address = url.strip_prefix("http://").unwrap();
    let (host, path) = address.split_once('/').unwrap();
    let mut stream = TcpStream::connect(host).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    thread::sleep(Duration::from_millis(20));
    write!(stream, "{method} /{path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

fn wait_terminal(receiver: &Receiver<Value>, run_id: &str) -> Value {
    loop {
        let event = next(receiver);
        assert_eq!(event["run_id"], run_id);
        if event["event"] == "job_finished" || event["event"] == "job_failed" {
            return event;
        }
    }
}

fn stop(child: &mut Child, receiver: &Receiver<Value>, ready: &Value, body: &str) {
    let (code, response) = request(ready["stop_url"].as_str().unwrap(), "POST", body);
    assert_eq!(code, 200);
    assert_eq!(response["status"], "stopping");
    let (code, _) = request(ready["stop_url"].as_str().unwrap(), "POST", "{}");
    assert_eq!(code, 200);
    let (code, rejected) = request(
        ready["command_url"].as_str().unwrap(),
        "POST",
        &json!({"schema_version":2,"command":"send","arguments":{"tx_hex":"00"}}).to_string(),
    );
    assert_eq!(code, 409);
    assert_eq!(rejected["reason"], "stopping");
    let mut summary = None;
    while let Ok(event) = receiver.recv_timeout(Duration::from_secs(5)) {
        assert_eq!(event["run_id"], ready["run_id"]);
        if event["event"] == "summary" {
            summary = Some(event);
            break;
        }
    }
    let summary = summary.expect("summary event");
    assert_eq!(summary["ok"], true);
    assert_eq!(summary["exit_code"], 0);
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

#[test]
fn startup_validation_and_bind_failure() {
    for args in [
        vec!["worker", "--mode", "live", "--baud", "115200"],
        vec!["worker", "--mode", "simulate", "--baud", "115200"],
        vec![
            "worker",
            "--mode",
            "simulate",
            "--baud",
            "115200",
            "--simulation-profile-id",
            "unknown",
        ],
        vec![
            "worker",
            "--mode",
            "live",
            "--baud",
            "115200",
            "--port",
            "COM4",
            "--simulation-profile-id",
            "loopback-v1",
        ],
        vec![
            "worker",
            "--mode",
            "simulate",
            "--baud",
            "115200",
            "--simulation-profile-id",
            "loopback-v1",
            "--port",
            "COM4",
        ],
    ] {
        let output = binary().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("\"event\":\"ready\""));
    }
    let guard = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = guard.local_addr().unwrap().port().to_string();
    let output = binary()
        .args([
            "worker",
            "--mode",
            "simulate",
            "--baud",
            "115200",
            "--simulation-profile-id",
            "loopback-v1",
            "--control-port",
            &port,
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let lines: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines[0]["event"], "error");
    assert_eq!(lines[1]["event"], "summary");
    assert_eq!(lines[1]["ok"], false);
}

#[test]
fn simulation_session_lifecycle() {
    let (mut child, receiver, ready) = spawn();
    let run_id = ready["run_id"].as_str().unwrap();
    let status_url = ready["status_url"].as_str().unwrap();
    let command_url = ready["command_url"].as_str().unwrap();
    let (code, status) = request(status_url, "GET", "");
    assert_eq!(code, 200);
    assert_eq!(status["status"], "ready");
    assert_eq!(status["run_id"], run_id);
    assert_eq!(status["active_job"], Value::Null);
    assert_eq!(status["last_job"], Value::Null);
    for key in ["status_url", "command_url", "stop_url"] {
        assert_eq!(status[key], ready[key]);
    }

    let (code, accepted) = request(command_url, "POST", &json!({"schema_version":2,"command":"send","arguments":{"tx_hex":"00 55 AA FF 0D 0A"},"job_id":"client-1"}).to_string());
    assert_eq!(code, 202);
    assert_eq!(accepted["worker_job_id"], "job-1");
    assert_eq!(accepted["job_id"], "client-1");
    let finished = wait_terminal(&receiver, run_id);
    assert_eq!(finished["event"], "job_finished");
    assert_eq!(finished["result"]["tx_hex"], "0055aaff0d0a");
    let (_, status) = request(status_url, "GET", "");
    assert_eq!(status["last_job"]["worker_job_id"], "job-1");
    let (_, status_again) = request(status_url, "GET", "");
    assert_eq!(status_again["last_job"], status["last_job"]);

    let (code, _) = request(
        command_url,
        "POST",
        &json!({"schema_version":2,"command":"receive"}).to_string(),
    );
    assert_eq!(code, 202);
    let finished = wait_terminal(&receiver, run_id);
    assert_eq!(finished["result"]["rx_hex"], "0055aaff0d0a");

    let (code, _) = request(command_url, "POST", &json!({"schema_version":2,"command":"query","arguments":{"tx_hex":"00 55 AA FF 0D 0A","delimiter_hex":"0D 0A"}}).to_string());
    assert_eq!(code, 202);
    let finished = wait_terminal(&receiver, run_id);
    assert_eq!(finished["result"]["rx_hex"], "0055aaff0d0a");
    assert_eq!(finished["result"]["delimiter_hex"], "0d0a");

    let (code, _) = request(
        command_url,
        "POST",
        &json!({"schema_version":2,"command":"receive"}).to_string(),
    );
    assert_eq!(code, 202);
    let failed = wait_terminal(&receiver, run_id);
    assert_eq!(failed["event"], "job_failed");
    assert_eq!(failed["exit_code"], 3);
    assert_eq!(failed["partial_hex"], "");

    let (code, _) = request(command_url, "POST", &json!({"schema_version":2,"command":"query","arguments":{"tx_hex":"aa","delimiter_hex":"0d0a"}}).to_string());
    assert_eq!(code, 202);
    let failed = wait_terminal(&receiver, run_id);
    assert_eq!(failed["partial_hex"], "aa");
    assert_eq!(failed["partial_bytes"], 1);
    assert!(child.try_wait().unwrap().is_none());
    let (_, status) = request(status_url, "GET", "");
    assert_eq!(status["status"], "ready");
    assert_eq!(status["last_job"]["event"], "job_failed");
    stop(&mut child, &receiver, &ready, "");
}

#[test]
fn strict_command_envelope_and_argument_validation() {
    let (mut child, receiver, ready) = spawn();
    let url = ready["command_url"].as_str().unwrap();
    for body in [
        "{}",
        r#"{"schema_version":1,"command":"send"}"#,
        r#"{"schema_version":"2","command":"send"}"#,
        r#"{"schema_version":2.0,"command":"send"}"#,
        r#"{"schema_version":true,"command":"send"}"#,
        r#"{"schema_version":2,"command":"send","arguments":{"tx_hex":"00"},"extra":1}"#,
        r#"{"schema_version":2,"command":"send","arguments":{"tx_hex":"00"},"context":{}}"#,
        r#"{"schema_version":2,"command":"send","arguments":[]}"#,
        r#"{"schema_version":2,"command":"send","job_id":3}"#,
        r#"{"schema_version":2,"command":"unknown"}"#,
        r#"{"schema_version":2,"command":"send","arguments":{"tx_hex":"00","extra":1}}"#,
        r#"{"schema_version":2,"command":"send","arguments":{"tx_hex":"0g"}}"#,
        r#"{"schema_version":2,"command":"receive","arguments":{"max_bytes":true}}"#,
        r#"{"schema_version":2,"command":"receive","arguments":{"max_bytes":0}}"#,
        r#"{"schema_version":2,"command":"query","arguments":{"tx_hex":"00","delimiter_hex":"0001","max_bytes":1}}"#,
        "not json",
        "[]",
    ] {
        let (code, result) = request(url, "POST", body);
        assert_eq!(code, 400, "{body}");
        assert_eq!(result["status"], "error");
        assert_eq!(result["error"], "validation_error");
    }
    let (_, status) = request(ready["status_url"].as_str().unwrap(), "GET", "");
    assert_eq!(status["active_job"], Value::Null);
    assert_eq!(status["last_job"], Value::Null);
    stop(&mut child, &receiver, &ready, "{}");
}
