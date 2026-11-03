use std::{fs, time::Duration};

use serial_tool_core::{
    DataBits, FlowControl, Parity, Sequence, SequenceError, SequenceSerialConfig,
    SequenceTcpConfig, SequenceTransport, Step, StepId, StepKind, StopBits, TcpSettings,
    ValidationReason,
};

fn step(id: &str, kind: StepKind) -> Step {
    Step {
        id: StepId::new(id).unwrap(),
        kind,
    }
}

fn sample() -> Sequence {
    Sequence {
        sequence_version: 1,
        transport: SequenceTransport::Serial(SequenceSerialConfig {
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            timeout: Duration::from_millis(1000),
        }),
        steps: vec![
            step("send-text", StepKind::SendText("é".into())),
            step("send-bytes", StepKind::SendBytes(vec![0, 0x55, 0xaa, 0xff])),
            step("wait", StepKind::Wait(Duration::from_millis(100))),
            step("read", StepKind::Read { max_bytes: 2 }),
            step(
                "read-until",
                StepKind::ReadUntil {
                    delimiter: vec![0x0d, 0x0a],
                    max_bytes: 64,
                },
            ),
            step(
                "repeat",
                StepKind::Repeat {
                    count: 2,
                    steps: vec![step("nested", StepKind::SendBytes(vec![0xff]))],
                },
            ),
        ],
    }
}

#[test]
fn all_step_types_round_trip_with_canonical_hex_and_serial_settings() {
    let sequence = sample();
    let json = sequence.to_json_pretty().unwrap();
    assert!(json.contains("\"hex\": \"0055aaff\""));
    assert!(json.ends_with('\n'));
    assert!(!json.contains("\"port\""));
    assert_eq!(Sequence::from_json_str(&json).unwrap(), sequence);
    let spaced = json.replace("0055aaff", "00 55 AA FF");
    assert_eq!(
        Sequence::from_json_str(&spaced)
            .unwrap()
            .to_json_pretty()
            .unwrap(),
        json
    );
    let SequenceTransport::Serial(config) = &sequence.transport else {
        panic!("expected UART Sequence");
    };
    assert_eq!(config.serial_settings_for_port("COM4").port, "COM4");
}

#[test]
fn strict_schema_rejects_unrecognized_or_invalid_input() {
    let base = sample().to_json_pretty().unwrap();
    for bad in [
        base.replace("\"sequence_version\": 1", "\"sequence_version\": 2"),
        base.replace("\"sequence_version\": 1", "\"sequence_version\": 1.0"),
        base.replace(
            "\"sequence_version\": 1,",
            "\"sequence_version\": 1, \"future_magic\": true,",
        ),
        base.replace(
            "\"baud_rate\": 115200,",
            "\"baud_rate\": 115200, \"port\": \"COM4\",",
        ),
        base.replace("\"text\": \"é\"", "\"text\": \"é\", \"unexpected\": 1"),
        base.replace("\"type\": \"send_text\"", "\"type\": \"mystery\""),
        base.replace("0055aaff", "0055xxff"),
        base.replace("\"parity\": \"none\"", "\"parity\": \"mark\""),
    ] {
        assert!(Sequence::from_json_str(&bad).is_err(), "accepted: {bad}");
    }
    assert!(matches!(
        Sequence::from_json_str(
            &base.replace("\"sequence_version\": 1", "\"sequence_version\": 2")
        ),
        Err(SequenceError::UnsupportedVersion(2))
    ));
}

#[test]
fn tcp_round_trip_and_settings_match_without_endpoint() {
    let mut sequence = sample();
    sequence.transport = SequenceTransport::Tcp(SequenceTcpConfig {
        io_timeout: Duration::from_millis(1000),
    });
    let json = sequence.to_json_pretty().unwrap();
    assert!(json.contains("\"type\": \"tcp\""));
    assert!(!json.contains("\"host\""));
    assert!(!json.contains("\"port\""));
    assert!(!json.contains("\"connect_timeout_ms\""));
    assert_eq!(Sequence::from_json_str(&json).unwrap(), sequence);
    let tcp = TcpSettings::default();
    assert!(sequence.transport.matches_tcp_settings(&tcp));
    assert!(
        !sequence
            .transport
            .matches_serial_settings(&sample_serial_settings())
    );
    let mut different = tcp;
    different.io_timeout = Duration::from_millis(2000);
    assert!(!sequence.transport.matches_tcp_settings(&different));
}

fn sample_serial_settings() -> serial_tool_core::SerialSettings {
    let SequenceTransport::Serial(serial) = sample().transport else {
        unreachable!()
    };
    serial.serial_settings_for_port("COM4")
}

#[test]
fn tcp_invalid_fields_and_legacy_root_are_rejected() {
    let mut sequence = sample();
    sequence.transport = SequenceTransport::Tcp(SequenceTcpConfig {
        io_timeout: Duration::from_millis(1000),
    });
    let json = sequence.to_json_pretty().unwrap();
    let mut old: serde_json::Value =
        serde_json::from_str(&sample().to_json_pretty().unwrap()).unwrap();
    let serial = old["transport"]["settings"].take();
    old.as_object_mut().unwrap().remove("transport");
    old["serial"] = serial;
    let legacy = old.to_string();
    assert!(Sequence::from_json_str(&legacy).is_err());
    for bad in [
        json.replace("\"io_timeout_ms\": 1000", "\"io_timeout_ms\": 0"),
        json.replace("\"io_timeout_ms\": 1000", "\"io_timeout_ms\": -1"),
        json.replace("\"io_timeout_ms\": 1000", "\"io_timeout_ms\": 1.5"),
        json.replace(
            "\"io_timeout_ms\": 1000",
            "\"io_timeout_ms\": 1000, \"host\": \"localhost\"",
        ),
        json.replace("\"io_timeout_ms\": 1000", "\"port\": 5025"),
        json.replace("\"io_timeout_ms\": 1000", "\"connect_timeout_ms\": 3000"),
        json.replace("\"type\": \"tcp\"", "\"type\": \"udp\""),
        json.replace("\"type\": \"tcp\"", "\"type\": \"tcp\", \"extra\": 1"),
        json.replace("\"settings\": {", "\"settings\": {\"baud_rate\": 9600,"),
    ] {
        assert!(Sequence::from_json_str(&bad).is_err(), "accepted: {bad}");
    }
    sequence.transport = SequenceTransport::Tcp(SequenceTcpConfig {
        io_timeout: Duration::ZERO,
    });
    assert!(sequence.validate().is_err());
}

#[test]
fn sequence_uses_shared_step_validation() {
    let mut sequence = sample();
    for (steps, reason) in [
        (
            vec![step(
                "outer",
                StepKind::Repeat {
                    count: 1,
                    steps: vec![step("outer", StepKind::Wait(Duration::ZERO))],
                },
            )],
            ValidationReason::DuplicateId,
        ),
        (
            vec![step("read", StepKind::Read { max_bytes: 0 })],
            ValidationReason::InvalidReadLimit,
        ),
        (
            vec![step(
                "repeat",
                StepKind::Repeat {
                    count: 0,
                    steps: vec![],
                },
            )],
            ValidationReason::ZeroRepeatCount,
        ),
    ] {
        sequence.steps = steps;
        assert!(
            matches!(sequence.validate(), Err(SequenceError::StepValidation(error)) if error.reason == reason)
        );
        assert!(sequence.to_json_pretty().is_err());
    }
    sequence.steps.clear();
    assert!(matches!(
        sequence.validate(),
        Err(SequenceError::EmptySteps)
    ));
    let invalid_path = std::env::temp_dir().join(format!(
        "serial-sequence-invalid-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    assert!(sequence.save(&invalid_path).is_err());
    assert!(!invalid_path.exists());
    sequence = sample();
    sequence.steps[2] = step("wait", StepKind::Wait(Duration::from_nanos(1)));
    assert!(matches!(
        sequence.to_json_pretty(),
        Err(SequenceError::InvalidDuration { .. })
    ));
}

#[test]
fn save_then_load_file_and_clean_up() {
    let path = std::env::temp_dir().join(format!(
        "serial-sequence-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let sequence = sample();
    sequence.save(&path).unwrap();
    let loaded = Sequence::load(&path);
    let saved = fs::read_to_string(&path);
    fs::remove_file(&path).unwrap();
    assert_eq!(loaded.unwrap(), sequence);
    assert_eq!(saved.unwrap(), sequence.to_json_pretty().unwrap());
}
