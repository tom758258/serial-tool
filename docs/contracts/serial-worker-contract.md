# Serial Worker Contract

The Serial Worker implements schema `3` with `v3-only` compatibility. It uses
the lifecycle and machine output rules in the [Common Worker Protocol](common-worker-protocol.md)
and [Common CLI JSON / JSONL contract](common-cli-jsonl-contract.md). This
document defines the Serial-specific behavior.

`manifest --json` keeps Manifest schema `2`, declares Worker schemas `[3]`,
`execution.model: "multi_job"`, and `execution.context_binding: "startup"`.
Its five commands below all have `kind: "async_job"`. Manifest and runtime
features contain only `common.async_jobs` with `contract_version: 1`.
Cancellation and streaming are not adopted. Multi-job means repeated jobs on
one session, with one active slot and no additional queue or parallel jobs.
Ordinary CLI results, dry-run output, and Sequence v1 retain their independent
versions.

## Startup and ownership

`serial-tool worker --mode live --port COM4 --baud 115200 --control-port 0`
claims the explicit port before emitting `ready`. A failed open or HTTP bind
emits a structured `error` and failed `summary`, then exits `3` without `ready`.
`ready` means the local HTTP control plane is accepting requests and the port
has been claimed; it does not assert device protocol readiness.

`serial-tool worker --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --control-port 0`
creates a session without touching physical ports. Live requires `--port` and
forbids `--simulation-profile-id`. Simulate requires the profile and forbids
`--port`. Invalid startup settings exit `2`. The only simulation identity is
`simulation_profile_id: "loopback-v1"`; it is not a physical model ID. Every
write's exact bytes become pending RX bytes in order. Reading without pending
RX deterministically times out.

Each Worker owns one persistent `SerialSession` for its entire lifetime. The
session alone owns the serial transport and shared RX buffer. The sole runner
thread performs serial operations; the HTTP control plane does not touch the
transport. The startup mode, port or simulation profile, baud, data bits,
parity, stop bits, flow control, and timeout are fixed for that runtime.
Defaults for optional line settings match the one-shot CLI: 8 data bits, no
parity, 1 stop bit, no flow control, and 1000 ms timeout. `--baud` is required.
In simulation these settings describe the logical session and do not represent
hardware configuration. `--control-port` defaults to `0` (OS-selected) and
accepts `0..65535`. HTTP binds only to `127.0.0.1`.

## HTTP control plane

`GET /status` returns a memory-only, non-mutating schema-3 object with
`service`, `run_id`, `status`, `mode`, absolute `status_url`, `command_url`,
`stop_url`, `features`, `worker_state`, `serial_settings`, `active_job`,
`last_job`, `fatal_error`, and `timestamp_utc`. Live status includes `port`; simulation status includes
`simulation_profile_id`. Status is `ready`, `busy`, or `stopping` while the
control plane is reachable. This `status` is Serial-specific; Common
`worker_state` is `running` even while a job is busy, and `stopping` after
stop acceptance. `fatal_error` is null during normal operation. Status never
queries hardware or changes the session.

`active_job` is null or contains `worker_job_id`, `job_id`, `command`,
`state` (`queued` or `running`), and `timestamp_utc` when that state was established.
`last_job` is null or the complete most recent terminal, including `state`,
`ok`, `result`, `error`, domain diagnostics, and a fixed `timestamp_utc`.
Polling changes only the outer observation time, not the terminal timestamp
or outcome. JSONL and status retain the same terminal identity and result;
there is no persistent history.

`POST /command` accepts only `schema_version`, `command`, `arguments`, and
`job_id` at the top level. Request-level `context` and unknown fields are
forbidden. `schema_version` must be the exact JSON integer token `3`;
`2`, `"3"`, `true`, `3.0`, and `3e0` are invalid. `command` must
be a non-empty string; `arguments` may be omitted or must be an object; and
`job_id`, when present, must be a string. Commands cannot override startup
context. Inline byte arguments use raw bytes encoded as hex. ASCII whitespace in
hex is ignored; an empty value, odd digit count, or non-hex digit is rejected.

| Command | Arguments | Result |
| --- | --- | --- |
| `send` | required `tx_hex` string | `tx_hex`, `tx_bytes` |
| `send-file` | required absolute local `path` string | `file_bytes`, `bytes_written` |
| `receive` | optional `max_bytes` integer | `rx_hex`, `rx_bytes` |
| `query` | required `tx_hex`, `delimiter_hex` strings; optional `max_bytes` integer | `tx_hex`, `tx_bytes`, `rx_hex`, `rx_bytes`, `delimiter_hex` |
| `run-sequence` | required `sequence` inline Sequence v1 JSON object | `sequence_version`, `step_results`, `transcript` |

`max_bytes` defaults to `1024` and must be in `1..=1048576`; the query
delimiter must fit within it. Unknown argument fields, wrong types, and
missing required arguments are rejected before admission or serial I/O.
`run-sequence` accepts only `arguments.sequence`; paths, file names, and
serialized Sequence strings are not accepted. The object follows the
[Serial Sequence contract](serial-sequence-contract.md). Core parses and
validates the complete Sequence before admission. Its baud rate, data bits,
parity, stop bits, flow control, and timeout must exactly match the Worker
startup settings in both live and simulation mode; the port is a separate
runtime resource. A mismatch or invalid definition returns HTTP `400` with
`status: "error"` and Common Error code `validation_error` before admission
or any serial I/O. The Worker never reconfigures or replaces its session for a
Sequence.
Requests over 16 MiB or with headers over 16 KiB are rejected. The control
plane uses one active slot and does not queue further commands.

Every command response contains `schema_version: 3`, `run_id`, `status`,
`command`, `job_id`, `kind`, `worker_job_id`, and `error`. Valid admitted
commands return HTTP `202`, `status: "accepted"`, `kind: "async_job"`,
`error: null`, and a unique `worker_job_id` (`job-1`, `job-2`, and so on).
A second command while a job is queued or running returns HTTP `409`,
`status: "rejected"`, and Common Error category `admission`, with code and
Serial-specific `reason` set to `busy` or `stopping`. Invalid requests return
HTTP `400`, `status: "error"`, and Common Error category `validation`, code
`validation_error`. Runner handling failures use HTTP `500`, category
`worker`, code `runtime_error`. Rejections and errors have
`worker_job_id: null`; they create no job. A safely identifiable string
`command` or `job_id` is echoed, otherwise null; `kind` is null when unknown.
Common Error always contains `category`, `code`, non-empty `message`, and an
object `details`. Never automatically retry a command with possible serial
side effects after a timeout, disconnect, missing response, or HTTP `500`.

The runner uses Core `write_all` and `flush` for `send`, Core `read` for
`receive`, and `write_all`, `flush`, and Core `read_until` for `query`. An
accepted command's HTTP response remains `202` even if execution later fails.
Timeouts and serial read/write/flush failures emit `job_finished` with
`state: "failed"`, Common Error category `execution`, code `serial_error`,
and update `last_job`; the Worker remains available. Partial RX supplied by Core appears
as `partial_hex` and `partial_bytes`.
For `run-sequence`, the same runner thread executes the existing Core
`StepRunner` on the persistent session. Its success result uses the same
`step_results` and logical `transcript` machine representation as one-shot
`sequence run --json`. A Step failure emits `job_finished` with
`state: "failed"`, Common Error code `sequence_execution_error`,
`exit_code: 3`, `step_id`, completed `step_results`, and completed `transcript`.
When Core supplies partial RX,
`partial_hex` and `partial_bytes` are also present. Ordinary execution failure
leaves the Worker ready for another command. The single active slot and busy
rejection apply to `run-sequence` and all other commands alike.

## Raw binary file sending

Submit a Worker-local path through the existing `POST /command` envelope:

```json
{
  "schema_version": 3,
  "command": "send-file",
  "arguments": { "path": "C:\\TestData\\payload.bin" },
  "job_id": "send-file-001"
}
```

`arguments` accepts only `path`, a nonempty, non-whitespace string containing
an absolute local file path. Relative paths, wrong types, embedded NULs, and
unknown fields return HTTP `400` without creating a job. On Windows, drive
paths (including verbatim drive paths) are accepted; UNC shares and device
namespace paths are rejected. The file must exist on the Worker host and be
accessible under the Worker's operating-system permissions. There is no
upload endpoint, path expansion, or file snapshot. Do not modify the file
while sending; a local path is not an access-control sandbox or a guarantee
that a mapped drive or symbolic link points to local storage.

Syntactically valid paths return HTTP `202`. Only the runner opens the file,
checks metadata and reads it; nonexistent, inaccessible, non-regular, empty,
or unreadable files fail as `job_finished`, `state: "failed"`, with Common
Error code `file_error`.
Every extension is raw binary: no decoding, hex conversion, line-ending
conversion, or appended bytes occur. The runner uses one fixed 4 KiB buffer,
reads chunks of up to 4096 bytes until EOF, and calls the existing Core
`SerialSession.write_all()` followed by `flush()` for each chunk. It owns the
session exclusively and never holds the State mutex during file or serial
I/O. There are no progress events, retransmissions, retries, or delivery
acknowledgments. The 16 MiB HTTP body limit is unchanged.

Success is one `job_finished` event with `state: "succeeded"`, `ok: true`,
`error: null`, and, for example, this `result`:

```json
{ "file_bytes": 20971520, "bytes_written": 20971520 }
```

`file_bytes` is the regular file's size from the initial opened-file metadata,
not a snapshot or a promised transfer length. Reads continue to EOF; changes
or truncation during transmission may make `bytes_written` differ from
`file_bytes`. `bytes_written` counts only chunks whose entire Write and Flush
both succeeded. It confirms Worker-side completion, not device receipt or
processing. A failed Write or Flush may have sent some bytes, but no bytes
from that unconfirmed chunk enter the count.

Failures use `job_finished` with `state: "failed"`, `ok: false`,
`exit_code: 3`, `error`, `message`, and `result`. Its `result` always contains
`bytes_written` and includes `file_bytes` only after regular-file metadata
was obtained. For example, a stop after one confirmed chunk may report:

```json
{
  "state": "failed",
  "ok": false,
  "error": {
    "category": "execution",
    "code": "stopped_by_request",
    "message": "send-file stopped by request",
    "details": {}
  },
  "message": "send-file stopped by request",
  "result": { "file_bytes": 20971520, "bytes_written": 4096 }
}
```

| Outcome | Terminal event / state | Common Error code |
| --- | --- | --- |
| EOF after all read chunks were written and flushed | `job_finished` / `succeeded` | none |
| Stop before sending or before a remaining chunk | `job_finished` / `failed` | `stopped_by_request` |
| File open, metadata, empty-file, or read failure | `job_finished` / `failed` | `file_error` |
| Serial Write or Flush failure | `job_finished` / `failed` | `serial_error` |

Results, status, and JSONL never contain the file payload or a complete
`tx_hex`. The transfer buffer and counters have bounded memory usage. The
existing `loopback-v1` SimulationTransport still captures TX and retains
pending RX in memory; its storage grows with bytes sent. Use small simulation
files for loopback checks. Large-file validation uses a bounded test transport
that checks byte order without accumulating TX or RX. Live transmission does
not retain the complete payload.

`POST /stop` accepts an empty body, `{}`, or an object containing only optional
`schema_version: 3` (exact integer token) and `reason` (string). Unknown fields
and invalid types return HTTP `400`, `status: "error"`, with a validation
Common Error and no stop side effects. Success remains HTTP `200`, with
`schema_version: 3`, `run_id`, `status: "accepted"`, and `error: null`.
Acceptance does not establish job or cleanup completion.
Repeated calls cause no duplicate stop side effects and succeed while the
endpoint is reachable. New commands are rejected. An active `send`, `receive`, `query`,
or `run-sequence` is allowed to finish or time out before the runner and
control plane close. The session is then
dropped and a final summary is emitted. Stop performs no device-specific
command, reset, RX clear, or output-off action. Normal stop exits `0`.
`/stop` does not interrupt an active Sequence; it waits for the currently
admitted job to terminate under existing StepRunner/Serial timeout semantics.

For `send-file`, stop is cooperative: it is checked before file access and at
each chunk boundary before Write. Already started file/serial I/O is allowed
to return; stop cannot forcibly interrupt a blocking read, Write, or Flush.
After the current chunk completes, the runner reads the next chunk to resolve
EOF before checking stop. No remaining chunk is sent once stop is observed.
EOF after a successful final Flush remains `job_finished`, even if stop
arrived during that Flush; this also applies to exact multiples of 4096
bytes. A remaining chunk instead produces `stopped_by_request` with confirmed
partial bytes. An actual I/O error remains `file_error` or `serial_error`,
even when stop is also pending. Each admitted job emits exactly one terminal
event. Stopped or failed file jobs count as ordinary job failures; successful
Worker cleanup still emits a normal summary and exits `0`. Stop is not job
cancellation. The other commands' stop behavior remains unchanged.

## Runtime JSONL

Worker stdout contains only JSON object lines. Human diagnostics go to
stderr. Every runtime event has `event`, exact integer `schema_version: 3`,
the same non-empty `run_id`, and ISO 8601 `timestamp_utc`. `ready` includes
absolute endpoint URLs, `service: "serial-tool"`, `mode`, and the live `port`
or simulation profile, plus runtime `features`. Serial-specific `job_accepted`
and `job_started` are non-terminal. Every normally controlled job termination
emits exactly one `job_finished` with `worker_job_id`, `command`, `job_id`,
`state`, `ok`, `result`, and `error`. Success uses `state: "succeeded"`,
`ok: true`, and `error: null`. Failure uses `state: "failed"`, `ok: false`,
`exit_code: 3`, and a Common Error of category `execution`; `result` is null
when there is no domain result. Existing top-level `message`, `partial_hex`,
`partial_bytes`, `step_id`, `step_results`, and `transcript` diagnostics are
retained when available. `job_failed` is no longer emitted.

Other events are `stop_requested`, `error`, and final `summary`. Startup
validation errors have `scope: "request"`, `fatal: false`, category
`validation`, and exit `2`. Fatal Worker errors have `scope: "worker"`,
`fatal: true`, category `worker`, and code `runtime_error`; they emit a failed
summary and exit `3`. Early failures may omit `ready`.

Summary reports `ok`, `fatal_error` (Common Error or null), intended `exit_code`,
and accepted, succeeded, and failed counts. Ordinary job failure does not make
the summary fatal. Consumers must drain stdout to EOF and observe actual
process exit, retaining any later error or cleanup evidence. Stop acceptance,
summary, or EOF alone does not prove successful shutdown. An already confirmed
job result survives later Worker failure. Session close and cleanup order are
unchanged; no extra device I/O is performed to collect shutdown evidence.
