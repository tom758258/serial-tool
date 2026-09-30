# Serial Tool Desktop

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

In Connection Setup, use the Connection options gear to choose the execution
mode. Live uses an explicit physical port; Refresh ports lists available ports
and USB metadata without opening them. Simulation uses deterministic loopback
without opening a physical serial port. Port and Refresh ports are disabled in
Simulation, and the selected port is retained when switching modes. Set baud
rate, data bits, parity, stop bits, flow control, and timeout, then Connect.
Connection settings, including execution mode, are locked while connecting,
connected, running, or disconnecting. The gear remains available to view the
options; the panel closes on a second gear click, on a click outside it, or on
Escape, which also returns focus to the gear. Closing the panel only hides it
and never resets the selected mode. The header shows the current mode and
connection status. The app automatically receives available bytes while
connected.

Send Text transmits the exact UTF-8 bytes entered without adding CR or LF.
Send Hex accepts ASCII whitespace and case-insensitive pairs of hex digits.
Auto TX Source selects the Auto TX payload and defaults to Input. Input uses
the Text/Hex payload; switching sources never clears the Send payload, because
the same control also serves Manual Send.
Start Auto TX sends the selected payload immediately, then repeats with the
specified positive integer interval in milliseconds until Stop Auto TX or
Disconnect. Each interval starts after the preceding send completes; delayed
sends are not replayed in a burst. Continuous RX remains active. Payload source,
payload, format, Auto TX file, and interval are locked while Auto TX runs.
Manual Send, Send File, and Run Sequence are disabled until it stops;
Disconnect, RX display, and Clear View remain available. A connection error
stops Auto TX automatically. Start Auto TX requires a nonempty Text/Hex payload
for Input or a selected file for File (Raw); the two sources are never combined
and Core keeps other TX requests Busy while Auto TX runs.

Auto TX Source File (Raw) selects any file without restricting its extension and
records only its path. Start Auto TX then reads the whole file once in the Rust
backend and transmits that fixed byte snapshot, so the bytes are the same as
Send File (Raw): no UTF-8 conversion, newline conversion, or hex-text parsing.
Empty files, missing files, and unreadable files are rejected with an error and
do not start Auto TX or reserve the connection. Editing the file while Auto TX
runs has no effect; Stop and Start reads it again. Stop does not interrupt a
send that is already in progress. There is no chunking, progress display,
playlist, per-cycle re-read, file-change monitoring, or mid-file cancellation.

Send File (Raw) reads a selected file in the Rust backend and transmits its exact
bytes. Empty files are rejected. There is no UTF-8 conversion, newline conversion,
or hex-text parsing: a file containing `AA 55` sends `41 41 20 35 35`. File TX
excludes other TX and Sequence operations until it finishes. This first version
reads the complete file into memory and has no progress, cancellation, chunk
delay, XMODEM, or YMODEM support.

The terminal keeps up to 5000 TX/RX entries in memory. RX display can be Hex,
lossy UTF-8 Text with escaped controls, Both, or Stream; switching only rerenders
stored bytes. Clear View only clears the on-screen history. It does not clear
serial buffers or affect the device.

Stream is RX-only presentation without direction badges or event rows. RX
fragments concatenate without added line breaks; LF creates a newline and CR
is omitted. Other control characters remain escaped. The existing streaming
UTF-8 decoder is retained; this is not an ANSI/VT100 terminal emulator.
Show TX defaults to On. Turning it Off hides TX rows in Hex, Text, and Both
without affecting Send or stored history. The control is disabled in Stream;
its preference applies again when returning to a log view. Sequence Result
continues to offer only Hex, Text, and Both.

Export writes a snapshot of the current Terminal history in one of three formats.
Terminal Log (Hex) preserves the existing human-readable uppercase hex transcript,
one entry per line, for example `TX 00 FF` followed by `RX 00 FF`. RX Raw writes
only received bytes to a `.bin` file. TX + RX Raw writes every TX and RX entry's
bytes in history order to a `.bin` file. Raw exports contain no direction labels,
separators, line endings, event boundaries, timestamps, or other metadata. Their
default filenames include the local export time as `YYYYMMDD_HHmmss`.

RX display and Show TX only affect presentation and do not filter exports. RX Raw
is unavailable until at least one RX byte exists. Clear View clears the current
Terminal history, so cleared data is no longer available for any export. Entries
evicted by the 5000-entry history limit are likewise unavailable. Export is a
manual snapshot, not a continuous capture writer. TX + RX Raw is a flattened byte
history and is not a protocol/session replay format; bytes that originally came
from the device are transmitted as TX if that file is later sent with Send File
(Raw).

## Sequence

The editor supports Send Text, Send Bytes, Wait, Read, Read Until, and nested
Repeat. Select a step to edit its properties. New, Load, Save, Validate,
Add Step, Add Child, Delete, Move Up, and Move Down are available. Drafts may
be temporarily invalid while editing. Validate, Save, and Run all use Core's
Sequence v1 parser and validation. A Sequence file stores line settings and
steps, never a port or mode. Use Current Connection Settings copies line
settings into the draft without reconnecting. Communication Requirements names
these saved line settings; port and execution mode are still chosen at runtime.

Run requires a connected session whose line settings exactly match the
Sequence. It uses the existing Core `StepRunner` on that session. The last
result, including completed steps, raw-byte transcript, and failure partial
bytes when available, is held only in application memory. Save Result exports
that complete result as JSON with its original byte arrays, regardless of RX
display. Failed runs retain the failing step, error, completed results,
transcript, and partial bytes when available. The button is disabled until a
result exists. There is no saved run history or automatic result directory.

## Ownership

Tauri commands and the Desktop backend are thin adapters for Core's shared
persistent runtime, including conversion to the existing UI event and result
DTOs. One Core owner thread holds the connected `SerialSession`.
It handles manual sends, periodic scheduling, continuous RX, Sequence
runs, and disconnect commands. The monitor checks the session's buffered byte
count and the transport's pending RX count before reading, with a short wait
between empty polls. Commands are checked before the next monitor read.
`StepRunner` runs synchronously on the same owner, so the monitor cannot read
while a Sequence runs, and monitoring resumes afterward. Core rejects manual
Send, Auto TX, and Sequence as Busy while another send, Auto TX, or Sequence
is accepted or running; conflicting requests are not queued for later execution.
The configured serial timeout remains unchanged. Desktop does not use the CLI, Worker, HTTP, or persistent run data.
