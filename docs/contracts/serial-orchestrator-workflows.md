# Serial Orchestrator Workflows

This tool-specific contract applies the [Common Orchestrator Workflows](common-orchestrator-workflows.md)
to the [Serial Worker](serial-worker-contract.md). The Sequence format is
defined by the [Serial Sequence contract](serial-sequence-contract.md).

## Worker startup

1. Read and validate the Serial Sequence. Use its baud rate, data bits,
   parity, stop bits, flow control, and timeout as the Worker startup line
   settings. Do not add orchestration defaults.
2. For live execution, use a COM resource explicitly selected by an operator,
   saved orchestrator configuration, or prior explicit discovery and selection.
   The Sequence does not contain a port. Do not scan and guess, cycle through
   ports, or automatically choose the first COM port.
3. For simulation, use `simulation_profile_id: "loopback-v1"`. This is the
   Worker-specific deterministic simulation identity; do not synthesize a
   physical `planning_model_id`.
4. Start the Worker and wait for `ready` with its `run_id` and endpoint URLs.
   Representative launches for a Sequence with 115200, 8-N-1, no flow control,
   and a 1000 ms timeout are:

```text
serial-tool worker --mode live --port COM4 --baud 115200 --data-bits 8 --parity none --stop-bits 1 --flow-control none --timeout-ms 1000 --control-port 0
serial-tool worker --mode simulate --baud 115200 --data-bits 8 --parity none --stop-bits 1 --flow-control none --timeout-ms 1000 --simulation-profile-id loopback-v1 --control-port 0
```

## Execute and clean up

Submit the canonical inline Sequence object to `POST /command`:

```json
{
  "schema_version": 2,
  "command": "run-sequence",
  "arguments": {
    "sequence": {
      "sequence_version": 1,
      "serial": {
        "baud_rate": 115200,
        "data_bits": 8,
        "parity": "none",
        "stop_bits": 1,
        "flow_control": "none",
        "timeout_ms": 1000
      },
      "steps": [
        { "id": "send-frame", "type": "send_bytes", "hex": "0055aaff0d0a" },
        { "id": "read-frame", "type": "read_until", "delimiter_hex": "0d0a", "max_bytes": 64 }
      ]
    }
  },
  "job_id": "serial-job-1"
}
```

Omit request-level `context` because Serial execution context is bound at
Worker startup. HTTP `202` means admission only. Save the accepted
`worker_job_id`, then poll non-mutating `GET /status` or consume stdout JSONL
until a matching `job_finished` or `job_failed` terminal event arrives. Match
the event's `run_id` and `worker_job_id`. Request cooperative `POST /stop`,
then require a final `summary` and normal process exit. Stop waits for an
active Sequence to finish or fail; it does not interrupt its steps.
