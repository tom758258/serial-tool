[繁體中文](README.zh-TW.md)

# Serial Tool

Serial Tool is a Windows-first, Rust-based tool for raw UART/COM and TCP **client** communication. It provides a shared Core, an engineering CLI/Worker, and a Tauri 2 Desktop application. Core remains portable where practical.

Serial Tool is a communication and execution tool, **not** a device-specific controller, workflow orchestrator, protocol decoder, or test-record database.

## Features

- Connect to COM ports or raw TCP endpoints; transmit and receive exact bytes.
- Send UTF-8 text, hexadecimal bytes, or unchanged raw files; monitor continuous RX.
- Use interactive Terminal and periodic Auto TX on supported Live or Simulation connections.
- Build, validate, load, save, and run linear Sequence v1 steps: Send Text, Send Bytes, Wait, Read, Read Until, and Repeat.
- View Terminal traffic in event or continuous RX modes and export available raw bytes or a hex transcript; export the latest Sequence result as JSON.
- Use deterministic `loopback-v1` simulation without hardware. The optional `periodic-rx-v1` profile is **only** for Streaming Simulation Workers.
- Integrate a machine-readable local Worker through Common Worker Protocol v3; CLI/Manifest machine output retains its separate schema.
- Switch the Desktop interface between English and Traditional Chinese, and select System, Light, or Dark appearance.

The TCP connection is a **raw byte-stream client**: no TCP server, automatic reconnection, application framing, or device protocol is provided.

## Components

- **Core** (`crates/serial-tool-core`): transport, settings, simulation, persistent session runtime, RX ownership, and Sequence execution.
- **CLI / Worker** (`crates/serial-tool-cli`): command-line operations, interactive terminal, and local HTTP/JSONL Worker adapter over Core.
- **Desktop** (`apps/desktop`): React/TypeScript/Vite interface with a Tauri 2 Rust adapter. It uses Core directly, not the CLI or Worker.

See the [monorepo layout](docs/architecture/monorepo-layout.md) for the two separate Rust workspaces and ownership boundaries.

## Quick start

Install a Rust toolchain supporting edition 2024. From the repository root:

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo run -p serial-tool-cli -- list-ports
cargo run -p serial-tool-cli -- send --port COM4 --baud 115200 --hex "AA 55" --dry-run
```

Dry-run validates a supported operation without opening a COM port, TCP socket, or DNS connection. For a real command, select the correct device and remove `--dry-run`.

### Desktop

Install Node.js/npm and Tauri 2 platform prerequisites. Windows requires the **system** Microsoft Edge WebView2 Runtime; the application does not install it for you.

```sh
cd apps/desktop
npm ci
npm run tauri -- dev
```

The Desktop supports COM/TCP Live connections and hardware-free loopback simulation. Read the [Desktop User Guide](docs/desktop/USER_GUIDE.md) before operating a device.

## Build and validation

The root Cargo workspace contains Core and CLI; `apps/desktop/src-tauri` is a separate Cargo workspace with its own lockfile. For focused development commands and validation, follow [Core](docs/core/README.md), [CLI](docs/cli/README.md), [Desktop](docs/desktop/README.md), and [AGENTS.md](AGENTS.md). Default automated tests do not require physical hardware.

## Documentation

| Audience | English | 繁體中文 |
| --- | --- | --- |
| Project overview | This README | [README.zh-TW](README.zh-TW.md) |
| Core developers | [Core README](docs/core/README.md) | [Core 說明](docs/core/README.zh-TW.md) |
| CLI / Worker developers | [CLI README](docs/cli/README.md) | [CLI 說明](docs/cli/README.zh-TW.md) |
| CLI operators | [CLI User Guide](docs/cli/USER_GUIDE.md) | [CLI 使用指南](docs/cli/USER_GUIDE.zh-TW.md) |
| Desktop developers | [Desktop README](docs/desktop/README.md) | [Desktop 說明](docs/desktop/README.zh-TW.md) |
| Desktop operators | [Desktop User Guide](docs/desktop/USER_GUIDE.md) | [Desktop 使用指南](docs/desktop/USER_GUIDE.zh-TW.md) |

Engineering references: [Repository architecture](docs/architecture/monorepo-layout.md), [Serial CLI JSON/JSONL contract](docs/contracts/serial-cli-jsonl-contract.md), [Serial Worker contract](docs/contracts/serial-worker-contract.md), [Serial Sequence contract](docs/contracts/serial-sequence-contract.md), and [Orchestrator integration contract](docs/contracts/serial-orchestrator-workflows.md). The [contracts directory](docs/contracts/) also holds the shared Common contracts.

The source of truth for machine fields, Worker lifecycle, and Sequence JSON is the applicable contract, not a translated guide. CLI `--help` documents command arguments; a bundled offline `user-guide` command and Desktop Help window are **not part of this version**.
