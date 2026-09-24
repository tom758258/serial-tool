# Serial Worker Contract

The Serial Worker implements schema `2` with `v2-only` compatibility. It uses
the lifecycle and machine output rules in the [Common Worker Protocol](common-worker-protocol.md)
and [Common CLI JSON / JSONL contract](common-cli-jsonl-contract.md). This
document defines the Serial-specific behavior.

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

`GET /status` returns a memory-only, non-mutating schema-2 object with
`service`, `run_id`, `status`, `mode`, absolute `status_url`, `command_url`,
`stop_url`, `serial_settings`, `active_job`, `last_job`, `fatal_error`, and
`timestamp_utc`. Live status includes `port`; simulation status includes
`simulation_profile_id`. Status is `ready`, `busy`, or `stopping` while the
control plane is reachable. `last_job` holds only the most recent terminal
event; there is no persistent history.

`POST /command` accepts only `schema_version`, `command`, `arguments`, and
`job_id` at the top level. Request-level `context` and unknown fields are
forbidden. `schema_version` must be the exact JSON integer `2`; `command` must
be a non-empty string; `arguments` may be omitted or must be an object; and
`job_id`, when present, must be a string. Commands cannot override startup
context. Machine arguments use raw bytes encoded as hex. ASCII whitespace in
hex is ignored; an empty value, odd digit count, or non-hex digit is rejected.

| Command | Arguments | Result |
| --- | --- | --- |
| `send` | required `tx_hex` string | `tx_hex`, `tx_bytes` |
| `receive` | optional `max_bytes` integer | `rx_hex`, `rx_bytes` |
| `query` | required `tx_hex`, `delimiter_hex` strings; optional `max_bytes` integer | `tx_hex`, `tx_bytes`, `rx_hex`, `rx_bytes`, `delimiter_hex` |

`max_bytes` defaults to `1024` and must be in `1..=1048576`; the query
delimiter must fit within it. Unknown argument fields, wrong types, and
missing required arguments are rejected before admission or serial I/O.
Requests over 16 MiB or with headers over 16 KiB are rejected. The control
plane uses one active slot and does not queue further commands.

Valid admitted commands return HTTP `202` with Common fields
`schema_version: 2`, `status: "accepted"`, echoed `command` and `job_id`, and
`worker_job_id` (`job-1`, `job-2`, and so on). A second command while a job is
queued or running returns HTTP `409`, `status: "rejected"`, and `reason:
"busy"`. During stopping, the reason is `"stopping"`. Invalid requests return
HTTP `400`, `status: "error"`, `error: "validation_error"`, and `message`.
The response echoes a safely identifiable string `command` or `job_id` and
otherwise uses `null`.

The runner uses Core `write_all` and `flush` for `send`, Core `read` for
`receive`, and `write_all`, `flush`, and Core `read_until` for `query`. An
accepted command's HTTP response remains `202` even if execution later fails.
Timeouts and serial read/write/flush failures emit `job_failed` and update
`last_job`; the Worker remains available. Partial RX supplied by Core appears
as `partial_hex` and `partial_bytes`.

`POST /stop` accepts an empty body or empty JSON object and returns a
structured `stopping` response. Repeated calls succeed while the endpoint is
reachable. New commands are rejected. An active command is allowed to finish
or time out before the runner and control plane close. The session is then
dropped and a final summary is emitted. Stop performs no device-specific
command, reset, RX clear, or output-off action. Normal stop exits `0`.

## Runtime JSONL

Worker stdout contains only JSON object lines. Human diagnostics go to
stderr. Every runtime event has `event`, exact integer `schema_version: 2`,
the same non-empty `run_id`, and ISO 8601 `timestamp_utc`. `ready` includes
absolute endpoint URLs, `service: "serial-tool"`, `mode`, and the live `port`
or simulation profile. Job events `job_accepted`, `job_started`,
`job_finished`, and `job_failed` include `worker_job_id`, `command`, and
`job_id`. Success includes `ok: true` and `result`; failure includes
`ok: false`, `exit_code: 3`, `error`, and `message`. Other events are
`stop_requested`, fatal `error`, and final `summary`. The summary reports
`ok`, `exit_code`, and accepted, succeeded, and failed counts. Ordinary job
failure does not make the summary fatal. Fatal Worker failure emits `error`
and `summary` with `ok: false`, then exits `3`.
