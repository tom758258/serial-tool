# Serial Orchestrator Workflows

This tool-specific contract applies the [Common Orchestrator Workflows](common-orchestrator-workflows.md)
to the [Serial Worker](serial-worker-contract.md). The Sequence format is
defined by the [Serial Sequence contract](serial-sequence-contract.md).

## Worker startup

Validate the actual `manifest --json` output first: Manifest schema 2,
`v3-only` Worker schemas `[3]`, `multi_job` / `startup` execution, and
`common.async_jobs` contract version 1. The five declared commands are `send`,
`send-file`, `receive`, `query`, and `run-sequence`, all `async_job`.
Cancellation is not adopted. Optional `common.streaming` contract version 1
with `run_bound` is supported only when the Worker is started with
`--streaming`. That mode continuously emits Serial-specific `rx` JSONL
events while allowing only `send` and `send-file` Async Jobs. There is no
`receive`, `query` or `run-sequence` job in an opted-in runtime.
The ordinary mode retains the five existing commands. Apply the Common loopback URL
validation and same-origin rules before contacting any control endpoint.

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
4. Start the Worker and wait for schema-3 `ready` with its `run_id`, endpoint
   URLs, and matching runtime `common.async_jobs` feature. HTTP readiness
   fallback requires matching `run_id`, validated URLs/features,
   `worker_state: "running"`, and `fatal_error: null`.
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
  "schema_version": 3,
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
Worker startup. HTTP `202` means admission only; the response includes
`schema_version: 3`, `run_id`, `kind: "async_job"`, and `error: null`.
Save the accepted `worker_job_id`, then poll non-mutating `GET /status` or consume stdout JSONL
until a matching `job_finished` terminal or complete `last_job` snapshot
arrives. Match `run_id`, `worker_job_id`, `job_id`, and command. Check `state`
(`succeeded` or `failed`), `ok`, `result`, and Common Error; retain partial RX,
Sequence results/transcript, and partial file counts. A normal job failure
does not make this multi-job Worker fatal. Common job correlation and terminal
tracking belong in the shared Orchestrator layer, with Serial domain results
handled by a thin adapter; this repository does not implement that tracker.

Request cooperative `POST /stop` with an empty body, `{}`, or optional
`schema_version: 3` and string `reason`. HTTP `200`, `status: "accepted"`,
`error: null` acknowledges stop only. Require summary, stdout EOF, and actual
process exit, retaining fatal or cleanup failure evidence even after a
successful summary. Stop waits for an active Sequence to finish or fail; it
does not interrupt its steps or cancel jobs. `send-file` retains its existing
chunk-boundary cooperative stop and `stopped_by_request` failure. Do not retry
side-effecting commands after a missing response, HTTP `500`, or disconnect.

## Optional RX Streaming Worker

For a continuous Serial receiver, explicitly add `--streaming` to Worker
startup. Verify Manifest Streaming capability and runtime adoption, then
wait for JSONL status `stream.state: "active"` before relying on RX events
or submitting TX. The Worker outputs only RX data events (`rx_hex`,
`rx_bytes`); TX is sent with the existing asynchronous `send` or
`send-file` commands and observed using their normal `job_finished`
terminals. `run_bound` needs no separate source-start command.

Stop and correlate events as required by Common v3, including ended,
summary, stdout EOF and actual process exit. RX event chunks are raw
observations, not protocol frames or a query response API. Any Workflow
TX/RX FIFO, Background capture and Foreground-to-Background routing belong
to Orchestrator, not Serial Tool. No Orchestrator integration is implemented
by this contract update.
