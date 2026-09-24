# serial_tool

`serial_tool` is a generic Serial / COM execution tool. It is written in Rust,
targets Windows first, and keeps its Core portable where practical.

The workspace contains `serial-tool-core` and `serial-tool-cli`. Core owns serial
settings, port discovery, raw byte transport, deterministic simulation, and
session RX buffering. The `serial-tool` CLI provides engineering commands and
machine-readable output through Core, including a persistent Serial Worker.

Core also provides a linear Serial Step Runner with `SendText`, `SendBytes`,
`Wait`, `Read`, `ReadUntil`, and `Repeat`. It keeps step results and a logical
TX/RX transcript in memory. There is no persisted Sequence format or workflow
engine.

This project is not a device-specific controller, workflow orchestrator, or
test-record database.

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
serial-tool receive --port COM4 --baud 115200 --max-bytes 256
serial-tool query --port COM4 --baud 115200 --hex "00 55 AA FF 0D 0A" --until-hex "0D 0A" --max-bytes 1024
serial-tool query --port COM4 --baud 115200 --text "STATUS?" --until-text "OK" --max-bytes 1024 --dry-run --json
```

`send` and `query` require exactly one of `--text` or `--hex`; `query` also
requires exactly one of `--until-text` or `--until-hex`. Text uses UTF-8 bytes
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

## Worker

Start a local Worker for an explicit serial port or deterministic simulation:

```sh
serial-tool worker --mode live --port COM4 --baud 115200 --control-port 0
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --control-port 0
```

Worker stdout is JSONL. Its HTTP control plane binds only to `127.0.0.1`, and
the `ready` event supplies the selected endpoint URLs. A live Worker holds one
COM port through a persistent session. Simulation profile `loopback-v1` makes
each written byte available for later reads without opening hardware. `/stop`
closes the session without sending device-specific bytes. See the
[Serial Worker contract](docs/contracts/serial-worker-contract.md) for requests,
events, and lifecycle details.
