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

## Project structure

```text
crates/
  serial-tool-core/        # UART/TCP Core, Simulation and Sequence
  serial-tool-cli/         # CLI and Worker HTTP/JSONL
apps/desktop/
  src/                     # React/TypeScript frontend
  src-tauri/               # Separate Rust/Tauri workspace
docs/
  architecture/ core/ cli/ desktop/ contracts/ help/
scripts/                   # Windows build, release acceptance, Help generator
```

See [Monorepo Layout](docs/architecture/monorepo-layout.md) for ownership, lockfiles, and the full structure.

## Key dependencies

| Component | Purpose |
| --- | --- |
| Rust, serialport | Core and UART/COM communication |
| Clap, Serde/serde_json | CLI arguments and machine-readable data |
| Tauri 2 | Windows Desktop shell backed directly by Core |
| React, TypeScript, Vite | Desktop interface and frontend build |
| Microsoft Edge WebView2 Runtime | System-provided Windows runtime; not bundled by Serial Tool |

Exact dependency versions are defined by the Cargo and npm manifests/lockfiles.

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

## Build

The root Cargo workspace contains Core and CLI; `apps/desktop/src-tauri` is a separate workspace with its own lockfile. To build the Windows CLI and Desktop EXEs without an installer, run `.\scripts\build-all.ps1` in PowerShell from the repository root. See the [Core](docs/core/README.md), [CLI](docs/cli/README.md), and [Desktop](docs/desktop/README.md) READMEs for component commands.

## Testing and release acceptance

Routine development uses the existing Linux/Windows Rust and Desktop CI. Tests run without physical hardware by default; see [AGENTS.md](AGENTS.md) and [Testing Guidelines](docs/testing-guidelines.md) for focused checks.

For a **Windows release candidate only**, run `.\scripts\release-acceptance.ps1` from a clean, committed HEAD. The script reuses `build-all.ps1`, verifies versions and both built EXEs with hardware-free CLI smoke checks, and stages license notices, SHA-256 checksums, and a PASS/FAIL report under ignored `release/` and `reports/` directories. This does not test real COM devices, external TCP equipment, or GUI interaction and does not create a tag or GitHub Release.

## Documentation

- [Core README](docs/core/README.md)
- [CLI README](docs/cli/README.md)
- [CLI User Guide](docs/cli/USER_GUIDE.md)
- [Desktop README](docs/desktop/README.md)
- [Desktop User Guide](docs/desktop/USER_GUIDE.md)
- [Offline Help maintenance](docs/help/README.md)

Engineering references: [Repository architecture](docs/architecture/monorepo-layout.md), [Serial CLI JSON/JSONL contract](docs/contracts/serial-cli-jsonl-contract.md), [Serial Worker contract](docs/contracts/serial-worker-contract.md), [Serial Sequence contract](docs/contracts/serial-sequence-contract.md), and [Orchestrator integration contract](docs/contracts/serial-orchestrator-workflows.md). The [contracts directory](docs/contracts/) also holds the shared Common contracts.

The source of truth for machine fields, Worker lifecycle, and Sequence JSON is the applicable contract, not a translated guide. CLI `--help` documents command arguments; `serial-tool user-guide [--lang en|zh-TW]` opens the bundled bilingual CLI guide in the default browser without internet access. Desktop provides its own bilingual offline Help window.

## Contributing

Keep changes focused and review [AGENTS.md](AGENTS.md), [Testing Guidelines](docs/testing-guidelines.md), and the applicable [machine contracts](docs/contracts/) before opening a pull request. Do not claim simulation, CI, or packaging checks as live-device verification.

## License and disclaimer

Copyright (c) 2026 tom758258. Serial Tool is licensed under the [MIT License](LICENSE). Third-party notices and the separately licensed `serialport` library are described in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt); its MPL-2.0 license text is included at [licenses/MPL-2.0.txt](licenses/MPL-2.0.txt).

Serial Tool is an independent, unofficial project. It is not affiliated with, endorsed by, or sponsored by hardware, operating system, or framework vendors. Third-party names, products, and trademarks belong to their respective owners. Users must comply with applicable third-party software, driver, hardware, and documentation licenses. No warranty is provided under the MIT License.
