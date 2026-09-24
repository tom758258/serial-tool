use std::{io, time::Duration};

use serial_tool_core::{
    Direction, Error, MAX_READ_BYTES, RunError, SerialSession, SimulationTransport, Step, StepId,
    StepKind, StepOutcome, StepRunner, Transport, ValidationReason,
};

fn step(id: &str, kind: StepKind) -> Step {
    Step {
        id: StepId::new(id).unwrap(),
        kind,
    }
}

#[test]
fn step_ids_are_stable_validated_strings() {
    for id in ["send-reset", "step1", "uart-check-2"] {
        let value = StepId::new(id).unwrap();
        assert_eq!(value.as_str(), id);
        assert_eq!(value.to_string(), id);
    }
    for id in ["", "Send", "send_reset", "-send", "send-"] {
        assert!(StepId::new(id).is_err(), "{id}");
    }
}

#[test]
fn invalid_definitions_stop_before_any_io() {
    let invalid = [
        (
            step(
                "repeat-a",
                StepKind::Repeat {
                    count: 1,
                    steps: vec![step("send-a", StepKind::Wait(Duration::ZERO))],
                },
            ),
            ValidationReason::DuplicateId,
        ),
        (
            step("bad", StepKind::SendText(String::new())),
            ValidationReason::EmptySend,
        ),
        (
            step("bad", StepKind::SendBytes(vec![])),
            ValidationReason::EmptySend,
        ),
        (
            step("bad", StepKind::Read { max_bytes: 0 }),
            ValidationReason::InvalidReadLimit,
        ),
        (
            step(
                "bad",
                StepKind::Read {
                    max_bytes: MAX_READ_BYTES + 1,
                },
            ),
            ValidationReason::InvalidReadLimit,
        ),
        (
            step(
                "bad",
                StepKind::ReadUntil {
                    delimiter: vec![],
                    max_bytes: 1,
                },
            ),
            ValidationReason::EmptyDelimiter,
        ),
        (
            step(
                "bad",
                StepKind::ReadUntil {
                    delimiter: vec![0, 1],
                    max_bytes: 1,
                },
            ),
            ValidationReason::DelimiterExceedsReadLimit,
        ),
        (
            step(
                "bad",
                StepKind::Repeat {
                    count: 0,
                    steps: vec![],
                },
            ),
            ValidationReason::ZeroRepeatCount,
        ),
    ];

    for (bad, reason) in invalid {
        let mut session = SerialSession::new(SimulationTransport::loopback());
        let steps = [step("send-a", StepKind::SendBytes(vec![0xaa])), bad];
        let error = StepRunner::new(&mut session).run(&steps).unwrap_err();
        assert!(matches!(error, RunError::Validation(found) if found.reason == reason));
        assert!(session.transport().captured_tx().is_empty());
    }
}

#[test]
fn linear_execution_keeps_exact_bytes_and_logical_io_order() {
    let mut session = SerialSession::new(SimulationTransport::with_rx_chunks([
        vec![0xff, 0],
        b"OK\r\n".to_vec(),
    ]));
    let steps = [
        step("send-text", StepKind::SendText("é".into())),
        step("send-bytes", StepKind::SendBytes(vec![0, 0xff])),
        step("read-raw", StepKind::Read { max_bytes: 2 }),
        step(
            "read-until",
            StepKind::ReadUntil {
                delimiter: b"\r\n".to_vec(),
                max_bytes: 8,
            },
        ),
        step("wait", StepKind::Wait(Duration::ZERO)),
    ];
    let report = StepRunner::new(&mut session).run(&steps).unwrap();

    assert_eq!(session.transport().captured_tx(), &[0xc3, 0xa9, 0, 0xff]);
    assert_eq!(
        report
            .step_results
            .iter()
            .map(|result| result.step_id.as_str())
            .collect::<Vec<_>>(),
        ["send-text", "send-bytes", "read-raw", "read-until", "wait"]
    );
    assert_eq!(
        report.step_results[0].outcome,
        StepOutcome::SendText { bytes_written: 2 }
    );
    assert_eq!(
        report.step_results[1].outcome,
        StepOutcome::SendBytes { bytes_written: 2 }
    );
    assert_eq!(
        report.step_results[2].outcome,
        StepOutcome::Read {
            bytes: vec![0xff, 0]
        }
    );
    assert_eq!(
        report.step_results[3].outcome,
        StepOutcome::ReadUntil {
            bytes: b"OK\r\n".to_vec()
        }
    );
    assert_eq!(
        report.step_results[4].outcome,
        StepOutcome::Wait {
            requested_duration: Duration::ZERO
        }
    );
    assert_eq!(
        report
            .transcript
            .entries
            .iter()
            .map(|entry| (
                entry.step_id.as_str(),
                entry.direction,
                entry.bytes.as_slice()
            ))
            .collect::<Vec<_>>(),
        [
            ("send-text", Direction::Tx, &b"\xc3\xa9"[..]),
            ("send-bytes", Direction::Tx, &[0, 0xff][..]),
            ("read-raw", Direction::Rx, &[0xff, 0][..]),
            ("read-until", Direction::Rx, &b"OK\r\n"[..]),
        ]
    );
}

#[test]
fn each_successful_send_flushes_the_session() {
    #[derive(Default)]
    struct FlushTransport {
        tx: Vec<u8>,
        flushes: usize,
    }

    impl Transport for FlushTransport {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            unreachable!()
        }

        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.tx.extend_from_slice(bytes);
            Ok(())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    let mut session = SerialSession::new(FlushTransport::default());
    let steps = [
        step("text", StepKind::SendText("A".into())),
        step("bytes", StepKind::SendBytes(vec![0, 0xff])),
    ];
    StepRunner::new(&mut session).run(&steps).unwrap();
    assert_eq!(session.transport().tx, &[b'A', 0, 0xff]);
    assert_eq!(session.transport().flushes, 2);
}

#[test]
fn repeat_reuses_child_ids_and_session_state() {
    let mut session = SerialSession::new(SimulationTransport::loopback());
    let steps = [step(
        "repeat",
        StepKind::Repeat {
            count: 2,
            steps: vec![
                step("send-byte", StepKind::SendBytes(vec![0x55])),
                step("read-byte", StepKind::Read { max_bytes: 1 }),
            ],
        },
    )];
    let report = StepRunner::new(&mut session).run(&steps).unwrap();
    assert_eq!(session.transport().captured_tx(), &[0x55, 0x55]);
    assert_eq!(
        report
            .step_results
            .iter()
            .map(|result| result.step_id.as_str())
            .collect::<Vec<_>>(),
        ["send-byte", "read-byte", "send-byte", "read-byte", "repeat"]
    );
    assert_eq!(
        report.step_results[4].outcome,
        StepOutcome::Repeat {
            completed_iterations: 2
        }
    );
    assert_eq!(
        report
            .transcript
            .entries
            .iter()
            .map(|entry| (entry.direction, entry.bytes.as_slice()))
            .collect::<Vec<_>>(),
        [
            (Direction::Tx, &[0x55][..]),
            (Direction::Rx, &[0x55][..]),
            (Direction::Tx, &[0x55][..]),
            (Direction::Rx, &[0x55][..]),
        ]
    );
}

#[test]
fn failure_stops_later_steps_and_keeps_core_partial() {
    let mut session = SerialSession::new(SimulationTransport::loopback());
    let steps = [
        step("send-a", StepKind::SendBytes(vec![0xaa])),
        step(
            "read-reply",
            StepKind::ReadUntil {
                delimiter: b"\n".to_vec(),
                max_bytes: 8,
            },
        ),
        step("send-b", StepKind::SendBytes(vec![0xbb])),
    ];
    let error = StepRunner::new(&mut session).run(&steps).unwrap_err();
    let RunError::Execution(failure) = error else {
        panic!("expected execution failure")
    };
    assert_eq!(failure.step_id.as_str(), "read-reply");
    assert!(matches!(failure.error, Error::Timeout { .. }));
    assert_eq!(failure.error.partial(), Some(&[0xaa][..]));
    assert_eq!(failure.report.step_results.len(), 1);
    assert_eq!(failure.report.step_results[0].step_id.as_str(), "send-a");
    assert_eq!(failure.report.transcript.entries.len(), 1);
    assert_eq!(failure.report.transcript.entries[0].bytes, vec![0xaa]);
    assert_eq!(session.transport().captured_tx(), &[0xaa]);
    let mut buffered = [0];
    assert_eq!(session.read(&mut buffered).unwrap(), 1);
    assert_eq!(buffered, [0xaa]);
}

#[test]
fn repeat_failure_keeps_completed_iterations_and_stops_outer_steps() {
    let mut session = SerialSession::new(SimulationTransport::with_rx_chunks([b"OK\n".to_vec()]));
    let steps = [
        step(
            "repeat",
            StepKind::Repeat {
                count: 3,
                steps: vec![
                    step("send-byte", StepKind::SendBytes(vec![0x55])),
                    step(
                        "read-reply",
                        StepKind::ReadUntil {
                            delimiter: b"\n".to_vec(),
                            max_bytes: 8,
                        },
                    ),
                ],
            },
        ),
        step("later", StepKind::SendBytes(vec![0xff])),
    ];
    let error = StepRunner::new(&mut session).run(&steps).unwrap_err();
    let RunError::Execution(failure) = error else {
        panic!("expected execution failure")
    };
    assert_eq!(failure.step_id.as_str(), "read-reply");
    assert!(matches!(failure.error, Error::Timeout { .. }));
    assert_eq!(session.transport().captured_tx(), &[0x55, 0x55]);
    assert_eq!(
        failure
            .report
            .step_results
            .iter()
            .map(|result| result.step_id.as_str())
            .collect::<Vec<_>>(),
        ["send-byte", "read-reply", "send-byte"]
    );
    assert_eq!(failure.report.transcript.entries.len(), 3);
}
