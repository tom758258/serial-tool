use std::{fs, time::Duration};

use serial_tool_core::{
    DataBits, FlowControl, Parity, Sequence, SequenceError, SequenceSerialConfig, Step, StepId,
    StepKind, StopBits, ValidationReason,
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
        serial: SequenceSerialConfig {
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            timeout: Duration::from_millis(1000),
        },
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
    assert_eq!(
        sequence.serial.serial_settings_for_port("COM4").port,
        "COM4"
    );
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
