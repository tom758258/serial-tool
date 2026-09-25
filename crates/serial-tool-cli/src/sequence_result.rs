use serde_json::{Value, json};
use serial_tool_core::{Direction, RunReport, StepOutcome};

use crate::hex;

pub(super) fn step_results_json(report: &RunReport) -> Value {
    Value::Array(report.step_results.iter().map(|result| {
        let mut value = match &result.outcome {
            StepOutcome::SendText { bytes_written } => json!({ "type": "send_text", "bytes_written": bytes_written }),
            StepOutcome::SendBytes { bytes_written } => json!({ "type": "send_bytes", "bytes_written": bytes_written }),
            StepOutcome::Wait { requested_duration } => json!({ "type": "wait", "requested_duration_ms": requested_duration.as_millis() }),
            StepOutcome::Read { bytes } => json!({ "type": "read", "rx_hex": hex(bytes), "rx_bytes": bytes.len() }),
            StepOutcome::ReadUntil { bytes } => json!({ "type": "read_until", "rx_hex": hex(bytes), "rx_bytes": bytes.len() }),
            StepOutcome::Repeat { completed_iterations } => json!({ "type": "repeat", "completed_iterations": completed_iterations }),
        };
        value["step_id"] = json!(result.step_id.as_str());
        value
    }).collect())
}

pub(super) fn transcript_json(report: &RunReport) -> Value {
    Value::Array(report.transcript.entries.iter().map(|entry| json!({
        "step_id": entry.step_id.as_str(),
        "direction": match entry.direction { Direction::Tx => "tx", Direction::Rx => "rx" },
        "hex": hex(&entry.bytes), "bytes": entry.bytes.len(),
    })).collect())
}
