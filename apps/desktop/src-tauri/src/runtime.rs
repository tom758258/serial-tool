use serde::Serialize;
pub use serial_tool_core::runtime::Mode;
use serial_tool_core::runtime::{Event as CoreEvent, PersistentRuntime, RuntimeError};
use serial_tool_core::{
    Direction, Error, RunError, RunReport, Sequence, SerialSettings, StepOutcome,
};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Connected,
    Data {
        direction: &'static str,
        bytes: Vec<u8>,
    },
    Disconnected,
    ConnectionError {
        message: String,
    },
}

#[derive(Debug, Serialize)]
pub struct StepResultDto {
    pub step_id: String,
    pub kind: &'static str,
    pub bytes_written: Option<usize>,
    pub requested_duration_ms: Option<u128>,
    pub bytes: Option<Vec<u8>>,
    pub completed_iterations: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TranscriptDto {
    pub step_id: String,
    pub direction: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub status: &'static str,
    pub failing_step_id: Option<String>,
    pub error: Option<String>,
    pub partial_bytes: Option<Vec<u8>>,
    pub step_results: Vec<StepResultDto>,
    pub transcript: Vec<TranscriptDto>,
}

impl RunResult {
    fn from_report(report: RunReport, failure: Option<(String, Error)>) -> Self {
        let (status, failing_step_id, error, partial_bytes) = match failure {
            Some((id, error)) => (
                "failed",
                Some(id),
                Some(error.to_string()),
                error.partial().map(ToOwned::to_owned),
            ),
            None => ("success", None, None, None),
        };
        Self {
            status,
            failing_step_id,
            error,
            partial_bytes,
            step_results: report
                .step_results
                .into_iter()
                .map(|result| {
                    let mut dto = StepResultDto {
                        step_id: result.step_id.as_str().to_owned(),
                        kind: "",
                        bytes_written: None,
                        requested_duration_ms: None,
                        bytes: None,
                        completed_iterations: None,
                    };
                    match result.outcome {
                        StepOutcome::SendText { bytes_written } => {
                            dto.kind = "send_text";
                            dto.bytes_written = Some(bytes_written);
                        }
                        StepOutcome::SendBytes { bytes_written } => {
                            dto.kind = "send_bytes";
                            dto.bytes_written = Some(bytes_written);
                        }
                        StepOutcome::Wait { requested_duration } => {
                            dto.kind = "wait";
                            dto.requested_duration_ms = Some(requested_duration.as_millis());
                        }
                        StepOutcome::Read { bytes } => {
                            dto.kind = "read";
                            dto.bytes = Some(bytes);
                        }
                        StepOutcome::ReadUntil { bytes } => {
                            dto.kind = "read_until";
                            dto.bytes = Some(bytes);
                        }
                        StepOutcome::Repeat {
                            completed_iterations,
                        } => {
                            dto.kind = "repeat";
                            dto.completed_iterations = Some(completed_iterations);
                        }
                    }
                    dto
                })
                .collect(),
            transcript: report
                .transcript
                .entries
                .into_iter()
                .map(|entry| TranscriptDto {
                    step_id: entry.step_id.as_str().to_owned(),
                    direction: match entry.direction {
                        Direction::Tx => "tx",
                        Direction::Rx => "rx",
                    },
                    bytes: entry.bytes,
                })
                .collect(),
        }
    }
}

#[derive(Default)]
pub struct SessionManager {
    runtime: PersistentRuntime,
}

impl SessionManager {
    pub fn connect(
        &self,
        mode: Mode,
        settings: SerialSettings,
        events: impl Fn(Event) + Send + 'static,
    ) -> Result<(), String> {
        self.runtime
            .connect(mode, settings, move |event| {
                events(match event {
                    CoreEvent::Connected => Event::Connected,
                    CoreEvent::Disconnected => Event::Disconnected,
                    CoreEvent::ConnectionError { message } => Event::ConnectionError { message },
                    CoreEvent::Data { direction, bytes } => Event::Data {
                        direction: match direction {
                            Direction::Tx => "tx",
                            Direction::Rx => "rx",
                        },
                        bytes,
                    },
                })
            })
            .map_err(|error| error.to_string())
    }

    pub fn send(&self, bytes: Vec<u8>) -> Result<(), String> {
        self.runtime.send(bytes).map_err(|error| error.to_string())
    }

    pub fn start_periodic(
        &self,
        bytes: Vec<u8>,
        interval: std::time::Duration,
    ) -> Result<(), String> {
        self.runtime
            .start_periodic(bytes, interval)
            .map_err(|error| error.to_string())
    }

    pub fn stop_periodic(&self) -> Result<(), String> {
        self.runtime
            .stop_periodic()
            .map_err(|error| error.to_string())
    }

    pub fn run(&self, sequence: Sequence) -> Result<RunResult, String> {
        match self.runtime.run(sequence) {
            Ok(report) => Ok(RunResult::from_report(report, None)),
            Err(RuntimeError::Sequence(error)) => match *error {
                RunError::Execution(failure) => Ok(RunResult::from_report(
                    failure.report,
                    Some((failure.step_id.as_str().to_owned(), failure.error)),
                )),
                RunError::Validation(error) => Err(error.to_string()),
            },
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn disconnect(&self) -> Result<(), String> {
        self.runtime.disconnect().map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_tool_core::{DataBits, FlowControl, Parity, StopBits};
    use std::{sync::mpsc, time::Duration};

    fn settings() -> SerialSettings {
        SerialSettings {
            port: "simulation".into(),
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: FlowControl::None,
            timeout: Duration::from_millis(1000),
        }
    }

    fn connect() -> (SessionManager, mpsc::Receiver<Event>) {
        let manager = SessionManager::default();
        let (sender, events) = mpsc::channel();
        manager
            .connect(Mode::Simulation, settings(), move |event| {
                sender.send(event).unwrap();
            })
            .unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::Connected
        ));
        (manager, events)
    }

    fn next_data(events: &mpsc::Receiver<Event>) -> (&'static str, Vec<u8>) {
        match events.recv_timeout(Duration::from_secs(1)).unwrap() {
            Event::Data { direction, bytes } => (direction, bytes),
            other => panic!("expected data event, got {other:?}"),
        }
    }

    #[test]
    fn simulation_session_send_rx_and_disconnect() {
        let (manager, events) = connect();
        manager.send(b"OK\r\n".to_vec()).unwrap();
        assert_eq!(next_data(&events), ("tx", b"OK\r\n".to_vec()));
        assert_eq!(next_data(&events), ("rx", b"OK\r\n".to_vec()));
        manager.disconnect().unwrap();
        assert!(matches!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::Disconnected
        ));
        let (sender, reconnect_events) = mpsc::channel();
        manager
            .connect(Mode::Simulation, settings(), move |event| {
                sender.send(event).unwrap();
            })
            .unwrap();
        manager.disconnect().unwrap();
        assert!(matches!(
            reconnect_events
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            Event::Connected
        ));
    }

    #[test]
    fn sequence_exclusively_reads_and_monitor_resumes() {
        let (manager, events) = connect();
        let sequence = Sequence::from_json_str(r#"{
            "sequence_version": 1,
            "serial": {"baud_rate":115200,"data_bits":8,"parity":"none","stop_bits":1,"flow_control":"none","timeout_ms":1000},
            "steps": [
                {"id":"send-frame","type":"send_bytes","hex":"0055aaff0d0a"},
                {"id":"read-frame","type":"read_until","delimiter_hex":"0d0a","max_bytes":64}
            ]
        }"#).unwrap();
        let result = manager.run(sequence).unwrap();
        assert_eq!(result.status, "success");
        assert_eq!(result.step_results.len(), 2);
        assert_eq!(result.step_results[0].step_id, "send-frame");
        assert_eq!(result.step_results[0].bytes_written, Some(6));
        assert_eq!(result.step_results[1].step_id, "read-frame");
        assert_eq!(
            result.step_results[1].bytes,
            Some(vec![0, 0x55, 0xaa, 0xff, 0x0d, 0x0a])
        );
        assert_eq!(result.transcript.len(), 2);
        assert_eq!(result.transcript[0].direction, "tx");
        assert_eq!(result.transcript[1].direction, "rx");
        assert_eq!(
            result.transcript[1].bytes,
            vec![0, 0x55, 0xaa, 0xff, 0x0d, 0x0a]
        );
        manager.send(vec![0xaa]).unwrap();
        assert_eq!(next_data(&events), ("tx", vec![0xaa]));
        assert_eq!(next_data(&events), ("rx", vec![0xaa]));
        manager.disconnect().unwrap();
    }

    #[test]
    fn periodic_adapter_preserves_binary_rx_and_busy_errors() {
        let (manager, events) = connect();
        let bytes = vec![0, 255, 128, 13, 10];
        manager
            .start_periodic(bytes.clone(), Duration::from_millis(40))
            .unwrap();
        for _ in 0..2 {
            assert_eq!(next_data(&events), ("tx", bytes.clone()));
            assert_eq!(next_data(&events), ("rx", bytes.clone()));
        }
        assert_eq!(manager.send(vec![1]).unwrap_err(), "Connection busy");
        manager.stop_periodic().unwrap();
        while events.try_recv().is_ok() {}
        manager.send(vec![2]).unwrap();
        assert_eq!(next_data(&events), ("tx", vec![2]));
        assert_eq!(next_data(&events), ("rx", vec![2]));
        manager.disconnect().unwrap();
    }

    #[test]
    fn failed_sequence_json_retains_partial_bytes_and_completed_transcript() {
        let (manager, _events) = connect();
        let sequence = Sequence::from_json_str(r#"{
            "sequence_version": 1,
            "serial": {"baud_rate":115200,"data_bits":8,"parity":"none","stop_bits":1,"flow_control":"none","timeout_ms":1000},
            "steps": [
                {"id":"send","type":"send_bytes","hex":"00ff80"},
                {"id":"read","type":"read_until","delimiter_hex":"0d0a","max_bytes":64}
            ]
        }"#).unwrap();
        let result = manager.run(sequence).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string_pretty(&result).unwrap()).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["failing_step_id"], "read");
        assert!(json["error"].as_str().unwrap().contains("timed out"));
        assert_eq!(json["partial_bytes"], serde_json::json!([0, 255, 128]));
        assert_eq!(json["step_results"].as_array().unwrap().len(), 1);
        assert_eq!(
            json["transcript"][0]["bytes"],
            serde_json::json!([0, 255, 128])
        );
        assert_eq!(json["transcript"].as_array().unwrap().len(), 1);
        manager.disconnect().unwrap();
    }

    #[test]
    fn mismatched_sequence_keeps_connection_usable() {
        let (manager, events) = connect();
        let sequence = Sequence::from_json_str(r#"{
            "sequence_version": 1,
            "serial": {"baud_rate":9600,"data_bits":8,"parity":"none","stop_bits":1,"flow_control":"none","timeout_ms":1000},
            "steps": [{"id":"send-byte","type":"send_bytes","hex":"aa"}]
        }"#).unwrap();
        assert!(manager.run(sequence).unwrap_err().contains("do not match"));
        manager.send(vec![0xbb]).unwrap();
        assert_eq!(next_data(&events), ("tx", vec![0xbb]));
        assert_eq!(next_data(&events), ("rx", vec![0xbb]));
        manager.disconnect().unwrap();
    }
}
