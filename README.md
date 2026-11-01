# Serial Tool

Serial Tool is a generic Serial / COM execution tool. It is written in Rust,
targets Windows first, and keeps its Core portable where practical.

The root workspace contains `serial-tool-core` and `serial-tool-cli`. Core owns serial
settings, port discovery, raw byte transport, deterministic simulation,
session RX buffering, and a shared persistent runtime with one I/O owner and
continuous RX. Desktop, the interactive terminal, and CLI Auto TX use this runtime.
The `serial-tool` CLI provides engineering commands and machine-readable output
through Core, including a persistent Serial Worker.

Core also provides a linear Serial Step Runner with `SendText`, `SendBytes`,
`Wait`, `Read`, `ReadUntil`, and `Repeat`. JSON Sequence files persist line
settings and steps. Execution keeps step results and a logical TX/RX transcript
in memory. There is no workflow engine.

This project is not a device-specific controller, workflow orchestrator, or
test-record database.

## Desktop

The Windows-first Desktop app uses Tauri 2 with React, TypeScript, and Vite.
It uses Core's persistent runtime through a thin backend adapter. Its Terminal
provides a persistent Live or Simulation connection, continuous RX, exact Text
or Hex TX, periodic Auto TX from the Send payload or a raw file, Send File
(Raw), Events (Hex / Text / Both) or Continuous RX (Text / Hex) display, and a
Show TX control. Terminal history can be exported as a TX/RX hexadecimal text log, RX raw binary, or
flattened TX+RX raw binary; the last Sequence result can be saved as JSON.
The Sequence editor loads, saves, validates, and runs the six
Core step types on the current connection. System, Light, and Dark themes are
available.

Windows Desktop requires the system Microsoft Edge WebView2 Runtime. Serial Tool
checks it before creating a Tauri window and offers the official Microsoft
download page in a native dialog if it is unavailable. It does not download or
install the runtime automatically. See [Desktop documentation](docs/desktop/README.md)
for startup behavior.

```sh
cd apps/desktop
npm ci
npm run tauri -- dev
```

For frontend checks, run `npm test`, `npm run typecheck`, and `npm run build` from
`apps/desktop`. See [Desktop documentation](docs/desktop/README.md) for usage,
architecture, and backend checks.

## Build and test

Install a Rust toolchain that supports edition 2024, then run:

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

The automated tests use simulation and require no serial hardware. The Common
Worker, CLI JSON/JSONL, and orchestrator contracts are in [`docs/contracts`](docs/contracts/).

## CLI usage

Run `cargo run -p serial-tool-cli -- <command>` from the workspace, or use the
`serial-tool` binary after building. For example:

```sh
serial-tool manifest --json
serial-tool list-ports
serial-tool list-ports --json
serial-tool send --port COM4 --baud 115200 --text "STATUS?"
serial-tool send --port COM4 --baud 115200 --hex "00 55 AA FF 0D 0A"
serial-tool send --port COM4 --baud 115200 --file payload.bin
serial-tool send --port COM4 --baud 115200 --file payload.bin --dry-run --json
serial-tool receive --port COM4 --baud 115200 --max-bytes 256
serial-tool query --port COM4 --baud 115200 --hex "00 55 AA FF 0D 0A" --until-hex "0D 0A" --max-bytes 1024
serial-tool query --port COM4 --baud 115200 --text "STATUS?" --until-text "OK" --max-bytes 1024 --dry-run --json
```

`send` requires exactly one of `--text`, `--hex`, or `--file`. File input reads
the entire nonempty file as raw bytes, without decoding, hex parsing, or line
ending conversion. Missing or unreadable files are input errors (exit 2).
Dry-run reads and validates the file without opening a port. File input uses
the same TX machine fields as text and hex input.
`query` requires exactly one of `--text` or `--hex` and exactly one of
`--until-text` or `--until-hex`; it does not accept `--file`. Text uses UTF-8 bytes
exactly as supplied and does not append CR or LF. Use hex for explicit control
bytes. Raw bytes are authoritative; machine output represents them as lowercase
hex without separators.

`send`, `receive`, and `query` require `--port` and `--baud`. Defaults are 8
data bits, no parity, 1 stop bit, no flow control, and a 1000 ms timeout.
`receive` performs one bounded read. Dry-run validates the plan without opening
or checking the port. Use `--format text|json|jsonl` (default `text`), or
`--json` as an alias for `--format json`. See the
[Serial CLI machine contract](docs/contracts/serial-cli-jsonl-contract.md) for
event fields and exit codes.

`receive`, `query`, and `sequence run` accept `--rx-display hex|text|both`
(default `hex`). RX remains raw bytes internally; `--rx-display` changes only
human-readable text output. `hex` uses spaced uppercase bytes, `text` uses
lossy UTF-8 with control characters escaped, and `both` shows both. JSON/JSONL
machine output remains canonical lowercase hex regardless of `--rx-display`.

## Interactive CLI terminal

The human-interactive terminal keeps one connection open for continuous RX
and line-oriented manual TX:

```sh
serial-tool terminal --mode live --port COM4 --baud 115200
serial-tool terminal --mode simulate --baud 115200 --simulation-profile-id loopback-v1
```

It accepts the same serial line settings as the one-shot commands, plus
`--rx-display hex|text|both|stream|hex-stream` (default `hex`), `--tx-display hex|off`
(default `hex`), `--tx-format text|hex` (default `text`), and
`--line-ending none|lf|crlf` (default `none`). Enter submits a line;
its stdin CR/LF is removed before applying the requested text line ending.
Hex sends exact bytes using the existing parser and ignores text line endings.
Invalid input is reported without ending the terminal. EOF or Ctrl+C gracefully
disconnects; EOF allows a short 50 ms window for final monitor events, without
waiting for a device response. Terminal has no JSON/JSONL output contract.
Worker remains the machine persistent interface and uses its existing runtime.

Text is an escaped log presentation. Stream presents RX text without prefixes
or event-boundary line breaks: LF creates a newline, CR is omitted, and other
control characters remain escaped. Output is flushed even without LF. CLI
UTF-8 decoding remains lossy per RX event, so characters split across events
may display replacement characters. Stream uses stdout for RX and stderr for
lifecycle messages, errors, and TX hex logs. `--tx-display off` only hides TX
presentation; bytes are still sent. For an RX-only capture, use:

```sh
serial-tool terminal --mode live --port COM4 --baud 115200 --rx-display stream --tx-display off > rx.txt
```

Hex Stream presents every RX byte as two uppercase hex digits, separated by
exactly one space across events, with no prefix, trailing space, or newline.
For example, events `AA 55` and `0D 0A` produce `AA 55 0D 0A`. It flushes each
RX event and routes lifecycle messages and TX logs to stderr, like Stream:

```sh
serial-tool terminal --mode live --port COM4 --baud 115200 --rx-display hex-stream --tx-display off > rx.hex
```

## CLI Auto TX

`auto-tx` keeps one Live or Simulation connection open, sends a fixed payload
periodically, and continues receiving RX until Ctrl+C:

```sh
serial-tool auto-tx --mode live --port COM4 --baud 115200 --text "STATUS?" --interval-ms 1000
serial-tool auto-tx --mode live --port COM4 --baud 115200 --hex "AA 55" --interval-ms 500
serial-tool auto-tx --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --file payload.bin --interval-ms 1000
```

Live requires `--port` and forbids `--simulation-profile-id`; Simulation
requires `--simulation-profile-id loopback-v1` and forbids `--port`. The command
accepts Terminal's serial settings, `--rx-display`, and `--tx-display` options.
Exactly one of `--text`, `--hex`, or `--file` is required; payloads must be
nonempty. Text and hex use the same exact-byte rules as `send`. File input is
read once before connecting; changes to the file do not affect the running
payload. `--interval-ms` is a required positive integer in milliseconds.

Core's persistent runtime sends the first payload immediately and measures each
subsequent interval from completion of the previous send and flush. It does
not catch up missed intervals. Ctrl+C stops periodic TX and disconnects;
connection or serial I/O failures exit 3 and release the connection. There is
no stdin interaction, dry-run, or JSON/JSONL contract for `auto-tx`.

## Worker RX Streaming (optional)

The persistent Worker also supports opt-in full-duplex RX streaming using the
Common v3 Optional Streaming Extension:

```sh
serial-tool worker --mode live --port COM4 --baud 115200 --streaming
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --streaming
```

Streaming starts with the Worker (`run_bound`) and reports raw incoming RX
as JSONL `rx` events (`rx_hex` / `rx_bytes`). TX is performed through
existing asynchronous `send` and `send-file` HTTP commands while RX
continues. `receive`, `query`, and `run-sequence` are available only
without `--streaming`, and non-streaming Worker operation is unchanged.
The simulated loopback returns transmitted bytes as RX; unsolicited RX
requires a source that independently sends data. No orchestration buffer,
replay, or device protocol decoding is added.
See [Serial Worker Contract](docs/contracts/serial-worker-contract.md).

## Sequences

A Sequence JSON file uses `sequence_version: 1` and records all serial line
settings and ordered steps:

```json
{
  "sequence_version": 1,
  "serial": { "baud_rate": 115200, "data_bits": 8, "parity": "none", "stop_bits": 1, "flow_control": "none", "timeout_ms": 1000 },
  "steps": [
    { "id": "send-frame", "type": "send_bytes", "hex": "0055aaff0d0a" },
    { "id": "read-frame", "type": "read_until", "delimiter_hex": "0d0a", "max_bytes": 64 }
  ]
}
```

Save this as `sequence.json`, then use:

```sh
serial-tool sequence validate --file sequence.json
serial-tool sequence run --file sequence.json --mode live --port COM4
serial-tool sequence run --file sequence.json --mode simulate --simulation-profile-id loopback-v1 --json
```

Validation performs no serial I/O. A Sequence file does not store the port or
execution mode; choose those at runtime. See the
[Serial Sequence contract](docs/contracts/serial-sequence-contract.md) for the
complete format.

## Worker

Start a local Worker for an explicit serial port or deterministic simulation:

```sh
serial-tool worker --mode live --port COM4 --baud 115200 --control-port 0
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --control-port 0
```

Worker implements Common v3.0 (`v3-only`): HTTP and runtime JSONL use schema 3,
while Manifest and ordinary CLI results remain schema 2. `manifest --json`
declares `multi_job` / `startup` execution and `common.async_jobs` version 1.
All five commands are `async_job`; there is one active slot, with no extra
queue, parallel jobs, cancellation, or streaming.

Worker stdout is JSONL. Its HTTP control plane binds only to `127.0.0.1`, and
the `ready` event supplies the selected endpoint URLs. A live Worker holds one
COM port through a persistent session. Simulation profile `loopback-v1` makes
each written byte available for later reads without opening hardware. `/stop`
closes the session without sending device-specific bytes. See the
[Serial Worker contract](docs/contracts/serial-worker-contract.md) for requests,
events, and lifecycle details.

Worker commands are `send`, `send-file`, `receive`, `query`, and `run-sequence`.
Submit raw files using `POST /command` with, for example:

```json
{"schema_version":3,"command":"send-file","arguments":{"path":"C:\\TestData\\payload.bin"},"job_id":"send-file-001"}
```

The path must be absolute and accessible on the Worker host. `send-file`
streams raw bytes in chunks of up to 4 KiB and reports `file_bytes` and
confirmed `bytes_written`, without payload hex. `/stop` cooperatively stops
remaining chunks after current I/O returns; an already completed transfer
remains successful. Simulation retains loopback bytes in memory, so prefer
small files there. See the Worker contract for partial results and errors.

HTTP `202` acknowledges admission. Completion uses `job_finished` with
`state: "succeeded"` or `"failed"`, `ok`, `result`, and a Common Error or null;
`GET /status` exposes the same terminal in `last_job`. `/stop` returns HTTP
`200`, `status: "accepted"`; shutdown still requires summary, stdout EOF,
and actual process exit.

`run-sequence` accepts an inline Sequence object whose line settings must
match the Worker startup settings. See the
[Serial Orchestrator workflow contract](docs/contracts/serial-orchestrator-workflows.md)
for the future orchestration flow.
