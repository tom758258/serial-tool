# Serial Sequence Contract

Serial Sequence files use JSON with exact integer `sequence_version: 1`. This
persistent file version is independent of the Common CLI and Worker runtime
`schema_version: 2`. No version fallback or negotiation is supported.

## File shape

The v1 file replaces the old top-level `serial` field with a required
`transport` object. The old format is rejected; there is no legacy parser,
migration or fallback, and `sequence_version` stays the exact integer `1`.

UART:

```json
{
  "sequence_version": 1,
  "transport": {
    "type": "serial",
    "settings": {
      "baud_rate": 115200,
      "data_bits": 8,
      "parity": "none",
      "stop_bits": 1,
      "flow_control": "none",
      "timeout_ms": 1000
    }
  },
  "steps": [
    { "id": "send-frame", "type": "send_bytes", "hex": "0055aaff0d0a" },
    { "id": "read-frame", "type": "read_until", "delimiter_hex": "0d0a", "max_bytes": 64 }
  ]
}
```

TCP:

```json
{
  "sequence_version": 1,
  "transport": {
    "type": "tcp",
    "settings": { "io_timeout_ms": 1000 }
  },
  "steps": [
    { "id": "send-frame", "type": "send_bytes", "hex": "0055aaff0d0a" },
    { "id": "read-frame", "type": "read_until", "delimiter_hex": "0d0a", "max_bytes": 64 }
  ]
}
```

Root, transport and settings objects reject unknown fields; all displayed
settings are required and have no hidden defaults. `steps` must not be empty.
For UART, `baud_rate` is positive and `timeout_ms` is a nonnegative unsigned
`u64`. `data_bits` is 5, 6, 7 or 8; `parity` is `none`, `odd` or `even`;
`stop_bits` is 1 or 2; `flow_control` is `none`, `software` or `hardware`.
For TCP, `io_timeout_ms` is a positive unsigned `u64`; `host`, `port`,
`connect_timeout_ms` and UART-only settings are forbidden.

A Sequence stores neither UART COM port nor TCP Host/Port/Connect Timeout.
Execution selects the device resource, mode and simulation profile separately.
UART Sequence runs only with an UART connection whose six line settings match;
TCP Sequence runs only with a TCP connection whose I/O timeout matches.
Transport or settings mismatch rejects the run without altering the Sequence
or current connection. StepRunner and step/result/transcript formats are shared.

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
JSON/schema, version, transport config, step ID, hex, step validation, and
serialization errors remain distinguishable. Execution binds the runtime
resource, creates a `SerialSession`, and calls the existing `StepRunner`; the
Sequence format does not define a workflow engine.

## CLI execution

`serial-tool sequence validate --file sequence.json` parses and validates
either transport without opening a COM port, TCP socket, DNS lookup, executing
steps, sleeping, or starting a Worker.

The existing UART execution entry points continue to accept UART Sequence
files. `serial-tool sequence run --file sequence.json --mode live --port COM4`
selects the COM port at runtime; the loopback Simulation mode uses
`--mode simulate --simulation-profile-id loopback-v1`. The TCP Sequence parser
and Core Runtime support are included in this change; the TCP CLI/Worker
startup selectors and Desktop TCP connection controls are introduced in their
separately planned stages, not as part of this Sequence format change.

Ordinary Worker `run-sequence` admission requires the inline Sequence object
to match its startup transport and settings; otherwise it rejects the request
with HTTP 400. Streaming Worker still does not accept `run-sequence`.
See the [Serial CLI machine contract](serial-cli-jsonl-contract.md) for result
objects and exit codes.
