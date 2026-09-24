# Serial Sequence Contract

Serial Sequence files use JSON with exact integer `sequence_version: 1`. This
persistent file version is independent of the Common CLI and Worker runtime
`schema_version: 2`. No version fallback or negotiation is supported.

## File shape

```json
{
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
```

All shown top-level and serial fields are required. Unknown fields in the root,
serial settings, or any step are rejected. `steps` must contain at least one
step. `baud_rate` must be positive; `timeout_ms` is a nonnegative `u64`.
`data_bits` is 5, 6, 7, or 8; `parity` is `none`, `odd`, or `even`;
`stop_bits` is 1 or 2; `flow_control` is `none`, `software`, or `hardware`.
There are no hidden serial defaults in a Sequence file.

The file describes line settings and ordered steps. It does not store a COM
port, execution mode, or simulation profile. Those are selected when running
the file, so one Sequence can run against different ports or simulation.

## Steps

Each step requires an `id` and `type`. IDs use lowercase ASCII letters,
digits, and nonempty hyphen-separated segments. IDs are unique across the
entire tree, including nested repeat steps.

| Type | Required fields | Meaning |
| --- | --- | --- |
| `send_text` | `text` string | Send exact UTF-8 bytes; no line ending is appended. |
| `send_bytes` | `hex` string | Send raw bytes. |
| `wait` | `duration_ms` unsigned `u64` | Wait the specified milliseconds; zero is valid. |
| `read` | `max_bytes` integer | Perform one bounded read. |
| `read_until` | `delimiter_hex` string, `max_bytes` integer | Read through a raw byte delimiter. |
| `repeat` | `count` integer, `steps` array | Execute child steps in order a fixed positive number of times. |

`max_bytes` must be in `1..=1048576`, the Core `MAX_READ_BYTES` limit.
Send payloads and read delimiters must be nonempty. A delimiter must fit within
`max_bytes`. Repeat count must be positive. These rules are checked by the same
Core step validator used by `StepRunner`.

Hex input accepts uppercase or lowercase digits and ASCII whitespace. Saved
JSON always uses lowercase hex with no separators and two digits per byte.
Wait values are stored as integer milliseconds; saving a programmatically
created wait with submillisecond precision or a duration outside `u64`
milliseconds fails rather than rounding. Field order and pretty formatting
are deterministic, with one final newline.

Core load reads JSON, converts the wire fields, and validates the complete
Sequence before returning it. Core save validates before writing. File I/O,
JSON/schema, version, serial config, step ID, hex, step validation, and
serialization errors remain distinguishable. Execution binds the runtime
resource, creates a `SerialSession`, and calls the existing `StepRunner`; the
Sequence format does not define a workflow engine.

## CLI execution

`serial-tool sequence validate --file sequence.json` checks the file without
port discovery, device I/O, step execution, sleep, or Worker startup.
`serial-tool sequence run --file sequence.json --mode live --port COM4` fully
loads and validates the file before opening the port.
`serial-tool sequence run --file sequence.json --mode simulate
--simulation-profile-id loopback-v1` uses Core's deterministic loopback
transport without opening hardware. The mode and its matching runtime resource
are required explicitly. See the [Serial CLI machine contract](serial-cli-jsonl-contract.md)
for result objects and exit codes.
