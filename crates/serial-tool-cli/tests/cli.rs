use std::process::{Command, Output};

use serde_json::Value;

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

#[test]
fn one_shot_machine_modes_emit_one_object() {
    for args in [
        vec!["manifest", "--json"],
        vec!["manifest", "--format", "jsonl"],
        vec![
            "send",
            "--port",
            "COM_DOES_NOT_EXIST",
            "--baud",
            "9600",
            "--hex",
            "00 FF",
            "--dry-run",
            "--json",
        ],
        vec![
            "receive",
            "--port",
            "COM_DOES_NOT_EXIST",
            "--baud",
            "9600",
            "--max-bytes",
            "8",
            "--dry-run",
            "--format",
            "jsonl",
        ],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(object(&output)["schema_version"].as_i64(), Some(2));
    }
}

#[test]
fn validation_and_connection_errors_have_distinct_exit_codes() {
    let validation = run(&[
        "send",
        "--port",
        "COM_DOES_NOT_EXIST",
        "--baud",
        "9600",
        "--hex",
        "0g",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(validation.status.code(), Some(2));
    assert_eq!(object(&validation)["event"], "error");

    let runtime = run(&[
        "send",
        "--port",
        "COM_DOES_NOT_EXIST",
        "--baud",
        "9600",
        "--text",
        "X",
        "--json",
    ]);
    assert_eq!(runtime.status.code(), Some(3));
    assert_eq!(object(&runtime)["exit_code"], 3);
}
