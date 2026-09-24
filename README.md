# serial_tool

`serial_tool` is a generic Serial / COM execution tool. It is written in Rust,
targets Windows first, and keeps its Core portable where practical.

The current workspace contains `serial-tool-core`. Core owns serial settings,
port discovery, raw byte transport, deterministic simulation, and session RX
buffering. A future CLI or Worker can use Core for application and machine
interfaces; a future Desktop application can use the same Core. Those
applications are not part of the current workspace.

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
