# Serial Tool Desktop

## Setup

Install Rust, Node.js, npm, and the platform prerequisites for Tauri 2. On
Windows, Microsoft Edge WebView2 Runtime is required. The app checks for it
before creating its window and shows a native warning when it is missing.

From `apps/desktop`, run `npm ci` and `npm run tauri -- dev`. To check the
frontend, run `npm run typecheck` and `npm run build`. To compile without an
installer, run `npm run tauri -- build --no-bundle`.

The Tauri crate at `apps/desktop/src-tauri` has its own `Cargo.lock`. Check it
with:

```sh
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --check
cargo clippy --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## Terminal

Refresh Ports lists available ports and USB metadata without opening them.
Select Live and an explicit port, or choose Simulation for deterministic
loopback. Set baud, data bits, parity, stop bits, flow control, and timeout,
then Connect. Settings are locked until Disconnect. The app automatically
receives available bytes while connected.

Send Text transmits the exact UTF-8 bytes entered without adding CR or LF.
Send Hex accepts ASCII whitespace and case-insensitive pairs of hex digits.
The terminal keeps up to 5000 TX/RX entries in memory. RX display can be Hex,
lossy UTF-8 Text with escaped controls, or Both; switching only rerenders
stored bytes. Clear View only clears the on-screen history. It does not clear
serial buffers or affect the device.

## Sequence

The editor supports Send Text, Send Bytes, Wait, Read, Read Until, and nested
Repeat. Select a step to edit its properties. New, Load, Save, Validate,
Add Step, Add Child, Delete, Move Up, and Move Down are available. Drafts may
be temporarily invalid while editing. Validate, Save, and Run all use Core's
Sequence v1 parser and validation. A Sequence file stores line settings and
steps, never a port or mode. Use Current Connection Settings copies line
settings into the draft without reconnecting.

Run requires a connected session whose line settings exactly match the
Sequence. It uses the existing Core `StepRunner` on that session. The last
result, including completed steps, raw-byte transcript, and failure partial
bytes when available, is held only in application memory.

## Ownership

Tauri commands adapt UI requests to Core. One Rust owner thread holds the
connected `SerialSession`. It handles manual sends, continuous RX, Sequence
runs, and disconnect commands. The monitor checks the session's buffered byte
count and the transport's pending RX count before reading, with a short wait
between empty polls. Commands are checked before the next monitor read.
`StepRunner` runs synchronously on the same owner, so the monitor cannot read
while a Sequence runs. The connection's configured serial timeout remains
unchanged. Desktop does not use the CLI, Worker, HTTP, or persistent run data.
