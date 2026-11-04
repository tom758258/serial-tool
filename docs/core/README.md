[繁體中文](README.zh-TW.md) | [Project](../../README.md) | [Monorepo Layout](../architecture/monorepo-layout.md)

# Serial Tool Core

`serial-tool-core` is the shared Rust communication and execution library used by the CLI/Worker and by the Tauri Desktop backend. It owns device I/O and transport-independent execution behavior; adapters own human interaction, UI, and machine-facing presentation.

## Purpose and ownership

The current Core crate is under `crates/serial-tool-core`:

| Module | Responsibility |
| --- | --- |
| `config.rs` / `discovery.rs` | UART settings and COM port discovery; connection configuration |
| `transport/serial.rs` | Raw UART/COM I/O |
| `transport/tcp.rs` | Raw TCP **client** connection and I/O |
| `transport/simulation.rs` | Deterministic no-hardware transport behavior |
| `session.rs` | Transport-backed session and raw TX/RX primitives |
| `runtime.rs` | Persistent session, periodic TX, continuous RX and exclusive I/O ownership |
| `step.rs` / `sequence.rs` | Linear Step Runner, Sequence parsing, validation and execution |
| `error.rs` / `lib.rs` | Shared errors and public crate surface |

Core supports a shared `Transport` boundary without performing CLI parsing, Worker HTTP/JSONL, React rendering, or Tauri window operations. The Desktop and CLI are parallel adapters over Core; neither is a separate transport engine.

## Runtime and communication boundaries

- One connected session has **one** Core I/O owner. Continuous RX monitoring and `StepRunner` never compete for the same read.
- UART line settings include baud rate, data bits, parity, stop bits, flow control and timeout. TCP settings include host, port, connect timeout and one shared read/write I/O timeout.
- TCP supports IPv4, IPv6 and hostnames as a raw byte-stream **client**. It does not provide server mode, application framing, device-specific parsing, TLS, or auto-reconnect.
- A read timeout without new bytes is not necessarily a broken connection. TCP EOF and fatal I/O errors follow connection cleanup behavior; a failed write must not imply that no bytes reached the remote side.
- Raw bytes remain the source of truth; text/hex presentation belongs to adapters. No automatic line-ending insertion is part of raw TX.

## Simulation

`loopback-v1` echoes transmitted raw bytes to RX without opening physical COM resources or TCP sockets. `periodic-rx-v1` autonomously produces an incrementing four-byte big-endian counter at approximately 100 ms intervals, with no TX echo; **only Streaming Simulation Workers** admit this profile. It is not a general Desktop, interactive CLI, or ordinary Worker mode.

## Sequence v1

Core provides `send_text`, `send_bytes`, `wait`, `read`, `read_until` and nested `repeat` steps. The canonical JSON shape is `sequence_version: 1` with `transport: {type, settings}`; the old root `serial` member is not supported. UART and TCP share execution, but their settings must match the selected connection. Sequence files do not store COM port, TCP host/port, execution mode, or TCP connect timeout. For exact fields, errors and execution rules see the [Serial Sequence Contract](../contracts/serial-sequence-contract.md).

## Development and validation

From the repository root, the crate is part of the root Cargo workspace:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

Use focused tests under `crates/serial-tool-core/tests/` when changing Core. Default automated tests use simulation or local sockets as appropriate and require no physical serial device. Follow [AGENTS.md](../../AGENTS.md) for scope and evidence requirements. This README explains ownership; public CLI/Worker machine schemas remain in [contracts](../contracts/).
