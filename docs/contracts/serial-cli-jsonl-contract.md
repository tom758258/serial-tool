# Serial CLI JSON / JSONL Contract

This document defines the current `serial-tool` command-line machine surface.
Ordinary one-shot results and Manifest keep exact integer `schema_version: 2`.
Worker runtime JSONL and HTTP use exact integer `schema_version: 3` under the
[Common CLI JSON / JSONL contract](common-cli-jsonl-contract.md) revision 3.0.
Dry-run, Sequence v1, and other independent contracts are unchanged.
Consumers should ignore unknown optional fields.

## Commands and output

The one-shot commands are `manifest`, `list-ports`, `send`, `receive`, `query`,
`sequence validate`, and `sequence run`. The Sequence file format is defined by
the [Serial Sequence contract](serial-sequence-contract.md). The persistent
`worker` runtime is defined by the Serial Worker Contract.
Each accepts `--format text|json|jsonl` (default `text`) or `--json` as an alias
for `--format json`. Each one-shot machine command writes one complete JSON
object to stdout. JSONL output has exactly one object line. Text mode writes
human-readable output to stdout and errors to stderr.

`receive`, `query`, and `sequence run` accept `--rx-display hex|text|both`
with default `hex`. This runtime presentation option affects only text-mode
stdout. Raw RX bytes remain authoritative, and JSON/JSONL machine fields keep
their canonical lowercase hex representation regardless of the option. Human
hex is uppercase and space-separated. Human text uses lossy UTF-8 decoding,
preserves printable Unicode, and escapes terminal control characters rather
than sending them to the terminal. `both` presents hex and text together.

Runtime result objects include `event`, `schema_version`, UTC ISO 8601
`timestamp_utc`, and `ok: true`. `send`, `receive`, and `query` also include
`command` and `port`; `send` and `query` include `tx_hex` and
`tx_bytes`; `receive` and `query` include `rx_hex` and `rx_bytes`; `query`
includes `delimiter_hex`. Hex is lowercase, has two digits per byte, and has
no separators. Raw bytes remain authoritative; UTF-8 decoding is not required.

`send` accepts exactly one of `--text`, `--hex`, or `--file`. File input reads
a complete nonempty file as raw bytes, without text decoding, hex parsing,
or newline conversion, then uses the existing send-and-flush operation.
Dry-run may read and validate the file but performs no serial I/O. Empty,
missing, or unreadable files are input errors (exit 2). File input does not add
machine fields or change Schema 2; `tx_hex` and `tx_bytes` describe the loaded
bytes exactly. `query` continues to accept only `--text` or `--hex` for TX.

`terminal` and `auto-tx` are human-facing persistent commands. Neither accepts
`--json` or `--format`; both are outside this one-shot JSON/JSONL contract.
Both accept `--rx-display hex|text|both|stream|hex-stream` and
`--tx-display hex|off`. Hex Stream emits uppercase two-digit RX bytes to
stdout with exactly one space between bytes across events, no prefix or
event-boundary newline, and flushes each RX event. Stream and Hex Stream send
lifecycle diagnostics and TX logs to stderr; `--tx-display off` hides only TX
presentation.

`auto-tx` requires Live mode with `--port`, or Simulation mode with
`--simulation-profile-id loopback-v1`, the serial settings used by Terminal,
one of `--text`, `--hex`, or `--file`, and a positive integer `--interval-ms`.
It reads file input once before connecting and repeats the loaded bytes.
Core's existing periodic runtime sends immediately, then waits the interval
after each completed send and flush, without catch-up sends. RX monitoring
continues during Auto TX. Ctrl+C stops periodic TX and disconnects. Input
errors exit 2; connection or serial I/O errors exit 3.
The Worker machine contract remains unchanged.

## Events

- `tool_manifest`: static tool identity with `tool_id: "serial"`,
  `tool_version`, and `worker_protocol` containing
  `compatibility_policy: "v3-only"` and `schema_versions: [3]`,
  `execution: {"model":"multi_job","context_binding":"startup"}`,
  and `features: {"common.async_jobs":{"contract_version":1}}`.
  `commands` contains exactly `send`, `send-file`, `receive`, `query`, and
  `run-sequence`, each with `kind: "async_job"`. Manifest schema remains 2.
  This command performs no port discovery, HTTP bind, or device I/O. The Worker surface is
  defined in the [Serial Worker contract](serial-worker-contract.md).
- `list-ports`: discovery result with `count` and `ports`. Each port has `port`
  and `type` (`usb`, `pci`, `bluetooth`, or `unknown`). USB ports also have
  numeric `vid` and `pid` and optional `serial_number`, `manufacturer`, and
  `product` values. Discovery does not open ports.
- `send`, `receive`, `query`: completed serial operations. `receive` performs
  one bounded raw read. `query` writes and flushes TX, then reads through a raw
  delimiter using Core's session buffer.
- `dry_run`: plan validation for `send`, `receive`, or `query`, with `command`,
  `performs_serial_io: false`, `serial_settings`, `port`, and applicable TX,
  delimiter, or `max_bytes` fields. It neither checks port existence nor opens
  the port, and omits `run_id`.
- `sequence_validate`: successful no-I/O validation with `ok: true`,
  `sequence_version: 1`, and `defined_steps`, the recursively defined Step
  count (not the runtime execution count).
- `sequence_run`: successful one-shot execution with `ok: true`,
  `sequence_version: 1`, `mode` (`live` or `simulate`), `step_results`, and
  `transcript`. Live results contain `port`; simulation results contain
  `simulation_profile_id`. Neither has `run_id` or `worker_job_id`.
  `step_results` preserve completion order, including each repeat child
  execution. Each result has `step_id`, `type`, and a type-specific field:
  `bytes_written` for sends, `requested_duration_ms` for wait, `rx_hex` and
  `rx_bytes` for reads, or `completed_iterations` for repeat. Transcript
  entries have `step_id`, `direction` (`tx` or `rx`), `hex`, and `bytes`. They
  represent logical I/O from completed steps, not a physical-wire capture.
- `error`: after machine handling starts, a validation or runtime failure
  contains `ok: false`, `command`, `message`, and `exit_code`. When Core
  supplies partial RX bytes, it also contains `partial_hex` and
  `partial_bytes`.
  A `sequence run` StepRunner failure also includes the failing `step_id`,
  completed `step_results`, and completed logical `transcript`. The underlying
  error remains in `message`; Core partial RX appears when available.

## Worker runtime

Worker uses `ready` with runtime features and control URLs, Common Status
with `worker_state` and job snapshots, and one `job_finished` terminal for
each normally controlled job termination. Its `state` is `succeeded` or
`failed`, with required `ok`, `result`, and Common `error` fields. Domain
partial results and diagnostics remain available. Serial-specific
`job_accepted`, `job_started`, and `stop_requested` are non-terminal.
Worker fatal errors use Common `scope`, `fatal`, and Error fields, and
summary includes `fatal_error` and intended `exit_code`. Consumers must also
observe stdout EOF, subsequent errors, and actual exit. See the
[Serial Worker contract](serial-worker-contract.md) for the complete surface;
the one-shot `error` event above retains its schema-2 contract.

## Exit codes

`0` means success or dry-run success. `2` means usage, Sequence input, or
validation failure.
`3` means connection, enumeration, timeout, or serial I/O failure. Clap parser
errors may use stderr and exit `2` before machine handling starts.
