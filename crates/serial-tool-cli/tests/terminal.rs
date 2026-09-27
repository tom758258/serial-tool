use std::{
    io::Write,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

fn terminal(extra: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_serial-tool"))
        .args([
            "terminal",
            "--mode",
            "simulate",
            "--baud",
            "115200",
            "--simulation-profile-id",
            "loopback-v1",
        ])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("terminal did not exit on EOF: {output:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

#[test]
fn simulation_terminal_recovers_from_invalid_hex_and_exits_on_eof() {
    let output = terminal(
        &["--tx-format", "hex", "--line-ending", "crlf"],
        b"0G\r\n00 FF\r\n",
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("Connected (simulate: loopback-v1)"),
        "{stdout}"
    );
    assert!(stdout.contains("TX: 00 FF"), "{stdout}");
    assert!(stdout.contains("RX: 00 FF"), "{stdout}");
    assert!(stdout.contains("Disconnected"), "{stdout}");
    assert!(
        !stdout.contains("00 FF 0D 0A"),
        "hex must ignore text line ending"
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Invalid input")
    );
}

#[test]
fn text_terminal_applies_crlf_and_uses_existing_safe_rx_display() {
    let output = terminal(
        &["--line-ending", "crlf", "--rx-display", "text"],
        b"STATUS?\r\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("TX: 53 54 41 54 55 53 3F 0D 0A"),
        "{stdout}"
    );
    assert!(stdout.contains("RX: STATUS?\\r\\n"), "{stdout}");
}

#[test]
fn stream_terminal_joins_rx_without_lf_and_hides_tx_without_stopping_send() {
    let output = terminal(
        &[
            "--rx-display",
            "stream",
            "--tx-display",
            "off",
            "--line-ending",
            "none",
        ],
        b"ABC\nDEF\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"ABCDEF");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("Connected (simulate: loopback-v1)"),
        "{stderr}"
    );
    assert!(stderr.contains("Disconnected"), "{stderr}");
    assert!(!stderr.contains("TX:"), "{stderr}");
}

#[test]
fn stream_terminal_crlf_creates_only_one_real_newline() {
    let output = terminal(
        &[
            "--rx-display",
            "stream",
            "--tx-display",
            "off",
            "--line-ending",
            "crlf",
        ],
        b"Hello\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"Hello\n");
    assert!(!String::from_utf8(output.stderr).unwrap().contains("TX:"));
}

#[test]
fn terminal_validates_mode_resources_and_rejects_machine_output() {
    for args in [
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
        vec![
            "--mode",
            "simulate",
            "--simulation-profile-id",
            "loopback-v1",
            "--json",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_serial-tool"))
            .args(["terminal", "--baud", "115200"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}
