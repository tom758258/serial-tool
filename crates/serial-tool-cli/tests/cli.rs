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
        let value = object(&output);
        assert_eq!(value["schema_version"].as_i64(), Some(2));
        if args[0] == "manifest" {
            assert_eq!(value["event"], "tool_manifest");
            assert_eq!(value["tool_id"], "serial");
            assert!(!value["tool_version"].as_str().unwrap().is_empty());
            assert_eq!(value["worker_protocol"]["compatibility_policy"], "v3-only");
            assert_eq!(
                value["worker_protocol"]["schema_versions"],
                serde_json::json!([3])
            );
            assert_eq!(
                value["execution"],
                serde_json::json!({"model": "multi_job", "context_binding": "startup"})
            );
            assert_eq!(
                value["features"],
                serde_json::json!({
                    "common.async_jobs": {"contract_version": 1},
                    "common.streaming": {"contract_version": 1, "lifecycles": ["run_bound"]}
                })
            );
            assert_eq!(
                value["commands"],
                serde_json::json!({
                    "send": {"kind": "async_job"}, "send-file": {"kind": "async_job"},
                    "receive": {"kind": "async_job"}, "query": {"kind": "async_job"},
                    "run-sequence": {"kind": "async_job"}
                })
            );
        }
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

#[test]
fn dry_run_send_text_describes_planned_transmission() {
    let output = run(&[
        "send",
        "--port",
        "COM_DOES_NOT_EXIST",
        "--baud",
        "115200",
        "--hex",
        "00 FF",
        "--dry-run",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Dry run: would send 2 bytes to COM_DOES_NOT_EXIST")
    );
}

#[test]
fn send_file_dry_run_preserves_raw_bytes_and_machine_fields() {
    let path = std::env::temp_dir().join(format!("serial-cli-file-{}.bin", std::process::id()));
    for bytes in [b"\x00\xff\r\n".as_slice(), b"AA 55\r\n".as_slice()] {
        std::fs::write(&path, bytes).unwrap();
        for format in ["json", "jsonl"] {
            let output = run(&[
                "send",
                "--port",
                "COM_DOES_NOT_EXIST",
                "--baud",
                "115200",
                "--file",
                path.to_str().unwrap(),
                "--dry-run",
                "--format",
                format,
            ]);
            assert_eq!(output.status.code(), Some(0), "{output:?}");
            let value = object(&output);
            assert_eq!(value["schema_version"], 2);
            assert_eq!(value["event"], "dry_run");
            assert_eq!(value["command"], "send");
            assert_eq!(value["performs_serial_io"], false);
            assert_eq!(value["tx_bytes"], bytes.len());
            let expected: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(value["tx_hex"], expected);
            let mut keys: Vec<_> = value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                [
                    "command",
                    "event",
                    "ok",
                    "performs_serial_io",
                    "port",
                    "schema_version",
                    "serial_settings",
                    "timestamp_utc",
                    "tx_bytes",
                    "tx_hex"
                ]
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn file_input_errors_are_validation_errors_before_port_open() {
    let path = std::env::temp_dir().join(format!("serial-cli-empty-{}.bin", std::process::id()));
    std::fs::write(&path, []).unwrap();
    let missing = path.with_extension("missing");
    for (file, expected) in [
        (path.as_path(), "file input must not be empty"),
        (missing.as_path(), "Cannot read file"),
        (std::env::temp_dir().as_path(), "Cannot read file"),
    ] {
        for dry_run in [false, true] {
            let mut args = vec![
                "send",
                "--port",
                "COM_DOES_NOT_EXIST",
                "--baud",
                "9600",
                "--file",
                file.to_str().unwrap(),
                "--json",
            ];
            if dry_run {
                args.push("--dry-run");
            }
            let output = run(&args);
            assert_eq!(output.status.code(), Some(2), "{output:?}");
            let value = object(&output);
            assert_eq!(value["event"], "error");
            assert_eq!(value["schema_version"], 2);
            assert_eq!(value["exit_code"], 2);
            assert!(
                value["message"].as_str().unwrap().contains(expected),
                "{value}"
            );
        }
        let output = run(&[
            "auto-tx",
            "--mode",
            "simulate",
            "--baud",
            "9600",
            "--simulation-profile-id",
            "loopback-v1",
            "--interval-ms",
            "40",
            "--file",
            file.to_str().unwrap(),
        ]);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(expected));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn send_sources_are_exclusive_and_query_keeps_text_hex_only() {
    let base = [
        "send",
        "--port",
        "COM_DOES_NOT_EXIST",
        "--baud",
        "9600",
        "--dry-run",
    ];
    for sources in [
        vec![],
        vec!["--text", "X", "--hex", "58"],
        vec!["--text", "X", "--file", "unused"],
        vec!["--hex", "58", "--file", "unused"],
    ] {
        let output = run(&base.iter().copied().chain(sources).collect::<Vec<_>>());
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    for source in ["--text", "--hex"] {
        let output = run(&[
            "query",
            "--port",
            "COM_DOES_NOT_EXIST",
            "--baud",
            "9600",
            source,
            if source == "--text" { "X" } else { "58" },
            "--until-hex",
            "0D 0A",
            "--dry-run",
            "--json",
        ]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(object(&output)["tx_hex"], "58");
    }
    let output = run(&[
        "query",
        "--port",
        "COM_DOES_NOT_EXIST",
        "--baud",
        "9600",
        "--text",
        "X",
        "--file",
        "unused",
        "--until-text",
        "OK",
        "--dry-run",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unexpected argument '--file'")
    );
}
