[繁體中文](README.zh-TW.md) | [Desktop User Guide](USER_GUIDE.md) | [Project](../../README.md)

# Serial Tool Desktop

The Windows-first Desktop uses Tauri 2, React, TypeScript and Vite. This README documents setup, implementation boundaries, and maintainer-facing behavior. For everyday operations, use the [Desktop User Guide](USER_GUIDE.md).

## Setup

Install Rust, Node.js, npm, and the platform prerequisites for Tauri 2. On
Windows, Desktop uses the system Microsoft Edge WebView2 Runtime. Serial Tool
checks for a usable runtime before creating a Tauri window. If unavailable, a
native Windows warning offers Yes to open the [official Microsoft WebView2
page](https://developer.microsoft.com/microsoft-edge/webview2/) or No to close
Serial Tool. Neither choice creates an application WebView. Serial Tool does not
bundle, download, or automatically install WebView2. Install the runtime, then
restart Serial Tool. If the check passes but Desktop initialization still
fails, a native startup-error dialog shows the Tauri error details.

From `apps/desktop`, run `npm ci` and `npm run tauri -- dev`. To check the
frontend, run `npm test`, `npm run typecheck`, and `npm run build`. To compile without an
installer, run `npm run tauri -- build --no-bundle`.

The Tauri crate at `apps/desktop/src-tauri` has its own `Cargo.lock`. Check it
with:

```sh
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --check
cargo clippy --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## Frontend, locale, and contracts

- `apps/desktop/src/` owns React UI, Terminal/Sequence presentation, and `en`/`zh-TW` translation.
- `apps/desktop/src-tauri/` is a **separate** Cargo workspace and a thin adapter over `serial-tool-core`. It does not run device I/O through CLI or Worker.
- Language choices are persisted under `serial-tool.locale`; System/Light/Dark appearance uses the independent `serial-tool.theme` preference. Switching locale does not recreate the session, clear the transcript, rewrite Sequence JSON, or change exported bytes.
- Core/CLI/Worker raw diagnostics and machine payloads are not localized. See [operator usage](USER_GUIDE.md), [repository layout](../architecture/monorepo-layout.md), and [contracts](../contracts/).

## Ownership

Tauri commands and the Desktop backend are thin adapters for Core's shared
persistent runtime, including conversion to the existing UI event and result
DTOs. One Core owner thread holds the connected `SerialSession` for UART, TCP or Simulation.
It handles manual sends, periodic scheduling, continuous RX, Sequence
runs, and disconnect commands. The monitor checks the session's buffered byte
count and the transport's pending RX count before reading, with a short wait
between empty polls. Commands are checked before the next monitor read.
`StepRunner` runs synchronously on the same owner, so the monitor cannot read
while a Sequence runs, and monitoring resumes afterward. Core rejects manual
Send, Auto TX, and Sequence as Busy while another send, Auto TX, or Sequence
is accepted or running; conflicting requests are not queued for later execution.
The configured connection I/O timeout remains unchanged. Desktop does not use the CLI, Worker, HTTP, or persistent run data.
