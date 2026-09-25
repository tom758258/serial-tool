# Serial CLI JSON / JSONL Contract

This document defines the current `serial-tool` command-line machine surface.
All objects follow the [Common CLI JSON / JSONL contract](common-cli-jsonl-contract.md)
with exact integer `schema_version: 2`. Consumers should ignore unknown optional
fields.

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

## Events

- `tool_manifest`: static tool identity with `tool_id: "serial"`,
  `tool_version`, and `worker_protocol` containing
  `compatibility_policy: "v2-only"` and `schema_versions: [2]`. This command
  performs no port discovery, HTTP bind, or device I/O. The Worker surface is
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

## Exit codes

`0` means success or dry-run success. `2` means usage, Sequence input, or
validation failure.
`3` means connection, enumeration, timeout, or serial I/O failure. Clap parser
errors may use stderr and exit `2` before machine handling starts.
