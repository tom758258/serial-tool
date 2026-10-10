use std::{
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

fn run(args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_serial-tool"))
        .args(["auto-tx", "--baud", "115200"])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!(
                "auto-tx did not reject input: {:?}",
                child.wait_with_output().unwrap()
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn auto_tx_validates_modes_payload_interval_and_rejects_machine_options() {
    for resource in [
        vec!["--mode", "live"],
        vec!["--mode", "simulate"],
        vec!["--mode", "simulate", "--simulation-profile-id", "unknown"],
        vec![
            "--mode",
            "live",
            "--port",
            "COM4",
            "--simulation-profile-id",
            "loopback-v1",
        ],
        vec![
            "--mode",
            "simulate",
            "--port",
            "COM4",
            "--simulation-profile-id",
            "loopback-v1",
        ],
    ] {
        let output = run(&resource
            .into_iter()
            .chain(["--text", "X", "--interval-ms", "40"])
            .collect::<Vec<_>>());
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    let resource = [
        "--mode",
        "simulate",
        "--simulation-profile-id",
        "loopback-v1",
    ];
    for payload in [
        vec![],
        vec!["--text", ""],
        vec!["--hex", ""],
        vec!["--hex", "0G"],
        vec!["--text", "X", "--hex", "58"],
        vec!["--text", "X", "--file", "unused"],
        vec!["--hex", "58", "--file", "unused"],
    ] {
        let output = run(&resource
            .into_iter()
            .chain(["--interval-ms", "40"])
            .chain(payload)
            .collect::<Vec<_>>());
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    for interval in ["0", "-1", "1.5", "invalid", "18446744073709551616"] {
        let output = run(&resource
            .into_iter()
            .chain(["--text", "X", "--interval-ms", interval])
            .collect::<Vec<_>>());
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    for option in [
        vec!["--json"],
        vec!["--format", "jsonl"],
        vec!["--count", "2"],
    ] {
        let output = run(&resource
            .into_iter()
            .chain(["--text", "X", "--interval-ms", "40"])
            .chain(option)
            .collect::<Vec<_>>());
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
}

#[test]
fn auto_tx_live_connection_failure_exits_three() {
    let output = run(&[
        "--mode",
        "live",
        "--port",
        "COM_DOES_NOT_EXIST",
        "--text",
        "X",
        "--interval-ms",
        "40",
    ]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn cli_help_lists_new_commands_sources_and_display_modes() {
    for (command, expected) in [
        (None, vec!["auto-tx"]),
        (Some("send"), vec!["--text", "--hex", "--file"]),
        (Some("terminal"), vec!["hex-stream"]),
        (
            Some("auto-tx"),
            vec![
                "--mode",
                "--simulation-profile-id",
                "--file",
                "--interval-ms",
                "hex-stream",
                "--tx-display",
            ],
        ),
    ] {
        let mut args = Vec::new();
        if let Some(command) = command {
            args.push(command);
        }
        args.push("--help");
        let output = Command::new(env!("CARGO_BIN_EXE_serial-tool"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let help = String::from_utf8(output.stdout).unwrap();
        for text in expected {
            assert!(help.contains(text), "{help}");
        }
    }
}
