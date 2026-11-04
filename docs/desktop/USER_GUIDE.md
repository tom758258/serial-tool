[繁體中文](USER_GUIDE.zh-TW.md) | [Desktop Technical README](README.md) | [Project](../../README.md)

# Serial Tool Desktop User Guide

This guide describes the existing Windows-first Desktop controls for operators of COM or raw TCP devices. For developer setup and technical ownership, see the [Desktop README](README.md).

## Getting started

On Windows, install the system Microsoft Edge WebView2 Runtime. If it is missing, the application offers to open the official download page and exits without creating a Tauri window; it does not install WebView2. Start the built `Serial Tool.exe` or use `npm run tauri -- dev` from `apps/desktop` for development.

The main areas are **Connection Setup**, **Terminal**, and **Sequence**. Choose COM or TCP and Live or Simulation before connecting. Desktop Simulation uses only `loopback-v1`; `periodic-rx-v1` is for Streaming Simulation Workers and is not a Desktop mode.

## Language

The top-right Language control switches between English and Traditional Chinese without reloading.
The displayed name is the current language. The selected language is remembered locally
(`serial-tool.locale`); the first launch uses the WebView language preference, with English
as fallback. Language and Appearance are independent. Appearance supports System,
Light, and Dark modes. Switching language never changes
device connectivity, terminal bytes, Sequence data, or exported file formats.
Core/CLI/Worker diagnostics and machine-readable data are not translated.

## Connection Setup: COM and TCP

In Connection Setup, choose COM Port or TCP/IP using the transport tabs.
COM Port Live uses an explicit physical port; Refresh ports lists available ports
and USB metadata without opening them. Set baud rate, data bits, parity, stop
bits, flow control and timeout. In TCP/IP Live, enter a hostname, IPv4 or IPv6
address, device Port (1–65535), Connect Timeout (default 3000ms) and shared
read/write I/O Timeout (default 1000ms). Host and Port are separate fields;
do not enter an HTTP URL. TCP is a raw byte-stream client: it does not provide
HTTP, frames, auto-reconnect, or a guarantee that DNS plus all address attempts
complete within a single Connect Timeout.

The Connection options gear controls one shared Live/Simulation mode for both
transports. Simulation uses deterministic `loopback-v1`, with no COM access,
DNS lookup, or TCP socket; COM Port selection and TCP Host/Port are disabled
but retain their values. TCP timeouts must remain valid in Simulation.
The COM and TCP form values are separately retained when switching tabs,
but neither is saved across app restarts. Numeric COM inputs retain native
number semantics with hidden spin buttons; TCP Port/Timeout inputs preserve
partial text while typing and validate on Connect.

Transport, connection settings and execution mode are locked while connecting,
connected, running or disconnecting. The gear remains available to view the
options; it closes on a second click, outside click or Escape (which restores
focus). Hiding options does not reset the mode. The header shows
COM Port or TCP/IP, mode, and the existing connection status. Incoming bytes
are automatically received for either transport while connected.

## Terminal transmission and Auto TX

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
Disconnect, Display Mode, and Clear View remain available. A connection error
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

## Send File (Raw)

Send File (Raw) reads a selected file in the Rust backend and transmits its exact
bytes. Empty files are rejected. There is no UTF-8 conversion, newline conversion,
or hex-text parsing: a file containing `AA 55` sends `41 41 20 35 35`. File TX
excludes other TX and Sequence operations until it finishes. This first version
reads the complete file into memory and has no progress, cancellation, chunk
delay, XMODEM, or YMODEM support.

## Terminal display modes

The terminal keeps up to 5000 TX/RX entries in memory. Display Mode offers
Events (Hex, lossy UTF-8 Text with escaped controls, or Both) and Continuous RX
(Text or Hex). Switching modes only rerenders stored raw bytes; it never sends
data, reconnects, changes the Send Text/Hex format, or changes the Auto TX
payload. Events keep separate TX/RX rows; TX rows use their existing Hex format.
Clear View only clears the on-screen history. It does not clear device buffers
or affect the connection.

Continuous RX Text retains the former Stream presentation: RX-only content
without direction badges or event rows. RX fragments concatenate without
event-boundary line breaks; LF creates a newline and CR is omitted. Other
control characters remain escaped, and the streaming UTF-8 decoder handles
characters split across RX events. This is not an ANSI/VT100 terminal emulator.
Continuous RX Hex joins received bytes across event boundaries as two-digit
uppercase hex pairs with exactly one space between bytes; empty RX events add
no separator. CR, LF, NUL, and non-text bytes are displayed literally as hex.
Neither Continuous RX mode includes TX events, event labels, or timestamps.

Show TX defaults to On. Turning it Off hides TX rows in Events modes without
affecting Send or stored history. The control is disabled in both Continuous RX
modes without resetting its preference; the preference applies again upon
returning to an Events mode. Sequence Result continues to offer only Hex, Text,
and Both.

## Terminal export

Export writes a snapshot of the current Terminal history in one of three formats.
Terminal Log (Hex) preserves the existing human-readable uppercase hex transcript,
one entry per line, for example `TX 00 FF` followed by `RX 00 FF`. RX Raw writes
only received bytes to a `.bin` file. TX + RX Raw writes every TX and RX entry's
bytes in history order to a `.bin` file. Raw exports contain no direction labels,
separators, line endings, event boundaries, timestamps, or other metadata. Their
default filenames include the local export time as `YYYYMMDD_HHmmss`.

Display Mode and Show TX only affect presentation and do not filter exports. RX Raw
is unavailable until at least one RX byte exists. Clear View clears the current
Terminal history, so cleared data is no longer available for any export. Entries
evicted by the 5000-entry history limit are likewise unavailable. Export is a
manual snapshot, not a continuous capture writer. TX + RX Raw is a flattened byte
history and is not a protocol/session replay format; bytes that originally came
from the device are transmitted as TX if that file is later sent with Send File
(Raw).

## Sequence

The editor supports Send Text, Send Bytes, Wait, Read, Read Until, and nested
Repeat. Properties shows a short explanation for the selected step.
New, Load, Save, Validate, Add Step, Add Child, Delete, Move Up, and
Move Down remain available. Drafts may be temporarily invalid while editing.
Validate, Save and Run use Core's unified Sequence v1 parser and validation.

Sequence files use `transport: {type, settings}`, not the legacy root `serial`.
UART Sequence settings contain the original six line parameters. TCP Sequence
settings contain only `io_timeout_ms`, never Host, Port or Connect Timeout.
The device resource and execution mode are selected in Connection Setup.

New creates an empty UART or TCP draft according to the selected transport tab.
Load keeps the JSON's own transport, including when it differs from the current
tab. Save and Validate work without a connection. Copy from Connection Setup
is available only when the editor transport matches the Connection Setup tab,
and copies only the applicable settings (six UART line parameters or TCP I/O
Timeout). Neither changing transport tabs nor copying changes the connection
or automatically converts a loaded Sequence.

The editor shows Not connected, Settings match, or an explicit transport or
settings mismatch. Run requires an active session of the same transport and
matching settings: all six UART settings or the single TCP I/O Timeout. A
mismatch disables Run and leaves the connection and draft intact. Core
rechecks the match before executing the shared StepRunner and producing the
existing result/transcript.
The last result, including completed steps, raw-byte transcript, and
failure partial bytes when available, is held only in application memory.
The status and controls remain visible above a 250px scrollable area shared
by Step Results and Transcript. Each new run starts at the top; switching
RX display does not reset the scroll position. Save Result exports
that complete result as JSON with its original byte arrays, regardless of RX
display. Failed runs retain the failing step, error, completed results,
transcript, and partial bytes when available. The button is disabled until a
result exists. There is no saved run history or automatic result directory.


## Troubleshooting and current limits

- **Cannot switch transport or edit settings:** Disconnect first; connection/mode controls are locked during connection and execution.
- **Unexpected text:** Inspect Hex. Raw bytes, not the display decoder, are authoritative.
- **No response after Text TX:** Confirm whether the device needs CR/LF; Text TX does not add either automatically.
- **TCP does not connect:** Check Host/IP and device Port, not an HTTP URL. TCP is a raw client without automatic reconnect.
- **Sequence Run is disabled:** Validate the JSON and match its transport and settings to the active session.
- **Export unavailable:** Exports use current in-memory Terminal history. Clear View and the 5000-event cap discard data from the export snapshot.
- **Send File or Sequence disabled during Auto TX:** Stop Auto TX first.

There is no persistent run-history database, continuous disk capture, protocol framing, or built-in offline Help window in this version. The [Sequence Contract](../contracts/serial-sequence-contract.md) and [Worker Contract](../contracts/serial-worker-contract.md) define machine-facing behavior.
