use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

use serde_json::Value;

fn file(steps: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "serial-cli-sequence-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    fs::write(&path, format!(r#"{{"sequence_version":1,"serial":{{"baud_rate":115200,"data_bits":8,"parity":"none","stop_bits":1,"flow_control":"none","timeout_ms":1000}},"steps":[{steps}]}}"#)).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_serial-tool"))
        .args(args)
        .output()
        .unwrap()
}

fn object(output: &Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    serde_json::from_str(&stdout).unwrap()
}

const SUCCESS_STEPS: &str = r#"{"id":"send-frame","type":"send_bytes","hex":"0055aaff0d0a"},{"id":"read-frame","type":"read_until","delimiter_hex":"0d0a","max_bytes":64}"#;

#[test]
fn validate_is_hardware_free_and_classifies_invalid_input() {
    let path = file(SUCCESS_STEPS);
    let name = path.to_str().unwrap();
    let output = run(&["sequence", "validate", "--file", name, "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value = object(&output);
    assert_eq!(value["event"], "sequence_validate");
    assert_eq!(value["schema_version"].as_i64(), Some(2));
    assert_eq!(value["sequence_version"].as_i64(), Some(1));
    assert_eq!(value["defined_steps"], 2);
    let text = run(&["sequence", "validate", "--file", name]);
    assert_eq!(text.status.code(), Some(0));
    assert!(
        String::from_utf8(text.stdout)
            .unwrap()
            .contains("Sequence valid")
    );
    fs::write(&path, "{}").unwrap();
    let invalid = run(&["sequence", "validate", "--file", name, "--format", "jsonl"]);
    let invalid_live = run(&[
        "sequence",
        "run",
        "--file",
        name,
        "--mode",
        "live",
        "--port",
        "COM_DOES_NOT_EXIST",
        "--json",
    ]);
    fs::remove_file(&path).unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(object(&invalid)["event"], "error");
    assert_eq!(invalid_live.status.code(), Some(2));
    assert_eq!(object(&invalid_live)["command"], "sequence-run");
}

#[test]
fn simulation_preserves_exact_results_and_transcript() {
    let path = file(SUCCESS_STEPS);
    let name = path.to_str().unwrap();
    for format in ["json", "jsonl"] {
        let output = run(&[
            "sequence",
            "run",
            "--file",
            name,
            "--mode",
            "simulate",
            "--simulation-profile-id",
            "loopback-v1",
            "--format",
            format,
        ]);
        assert_eq!(output.status.code(), Some(0));
        let value = object(&output);
        assert_eq!(value["event"], "sequence_run");
        assert_eq!(value["schema_version"].as_i64(), Some(2));
        assert_eq!(value["sequence_version"].as_i64(), Some(1));
        assert_eq!(value["mode"], "simulate");
        assert_eq!(value["step_results"][0]["bytes_written"], 6);
        assert_eq!(value["step_results"][1]["rx_hex"], "0055aaff0d0a");
        assert_eq!(value["transcript"][0]["direction"], "tx");
        assert_eq!(value["transcript"][0]["hex"], "0055aaff0d0a");
        assert_eq!(value["transcript"][1]["direction"], "rx");
        assert_eq!(value["transcript"][1]["hex"], "0055aaff0d0a");
        assert!(value.get("run_id").is_none());
    }
    let text = run(&[
        "sequence",
        "run",
        "--file",
        name,
        "--mode",
        "simulate",
        "--simulation-profile-id",
        "loopback-v1",
    ]);
    fs::remove_file(&path).unwrap();
    assert_eq!(text.status.code(), Some(0));
}

#[test]
fn simulation_failure_keeps_completed_work_and_partial_rx() {
    let path = file(
        r#"{"id":"send-byte","type":"send_bytes","hex":"aa"},{"id":"read-reply","type":"read_until","delimiter_hex":"0d0a","max_bytes":8}"#,
    );
    let output = run(&[
        "sequence",
        "run",
        "--file",
        path.to_str().unwrap(),
        "--mode",
        "simulate",
        "--simulation-profile-id",
        "loopback-v1",
        "--json",
    ]);
    fs::remove_file(&path).unwrap();
    assert_eq!(output.status.code(), Some(3));
    let value = object(&output);
    assert_eq!(value["event"], "error");
    assert_eq!(value["command"], "sequence-run");
    assert_eq!(value["step_id"], "read-reply");
    assert_eq!(value["partial_hex"], "aa");
    assert_eq!(value["partial_bytes"], 1);
    assert_eq!(value["step_results"][0]["step_id"], "send-byte");
    assert_eq!(value["transcript"][0]["hex"], "aa");
}

#[test]
fn runtime_mode_admission_is_explicit() {
    let path = file(SUCCESS_STEPS);
    let name = path.to_str().unwrap();
    for args in [
        vec!["--mode", "live"],
        vec![
            "--mode",
            "live",
            "--port",
            "COM4",
            "--simulation-profile-id",
            "loopback-v1",
        ],
        vec!["--mode", "simulate"],
        vec![
            "--mode",
            "simulate",
            "--port",
            "COM4",
            "--simulation-profile-id",
            "loopback-v1",
        ],
        vec!["--mode", "simulate", "--simulation-profile-id", "unknown"],
    ] {
        let mut command = vec!["sequence", "run", "--file", name, "--json"];
        command.extend(args);
        let output = run(&command);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(object(&output)["exit_code"], 2);
    }
    fs::remove_file(&path).unwrap();
}
