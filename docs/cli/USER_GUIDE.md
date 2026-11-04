[繁體中文](USER_GUIDE.zh-TW.md) | [CLI Reference](README.md) | [Project](../../README.md)

# Serial Tool CLI User Guide

This guide explains everyday use of the installed `serial-tool` CLI. The [CLI README](README.md) includes detailed options; `serial-tool <command> --help` lists currently accepted flags. A bundled `serial-tool user-guide` command is **not yet implemented**.

## 1. Start and inspect the CLI

From a directory containing the built binary (or run `cargo run -p serial-tool-cli -- <command>` from the repository):

```sh
serial-tool --help
serial-tool --version
serial-tool list-ports
serial-tool manifest --json
```

`list-ports` enumerates local COM ports without opening them. `manifest --json` describes machine integration; it is not a device probe.

## 2. Choose COM or TCP

UART is the default transport. Select a real port and baud rate for Live operations:

```sh
serial-tool send --port COM4 --baud 115200 --text "STATUS?"
```

TCP is a raw **client** transport. Use host/IP and a device port, not an HTTP URL:

```sh
serial-tool send --transport tcp --host 127.0.0.1 --tcp-port 5025 --hex "AA 55"
```

UART defaults to 8 data bits, no parity, 1 stop bit, no flow control, and 1000 ms timeout. TCP defaults to 3000 ms connect timeout and 1000 ms shared I/O timeout. Do not mix UART-only and TCP-only options. TCP does not frame messages or reconnect automatically.

## 3. Send, receive, query and dry-run

```sh
serial-tool send --port COM4 --baud 115200 --hex "00 55 AA FF 0D 0A"
serial-tool send --port COM4 --baud 115200 --file payload.bin
serial-tool receive --port COM4 --baud 115200 --max-bytes 256
serial-tool query --port COM4 --baud 115200 --text "STATUS?" --until-text "OK" --max-bytes 1024
serial-tool send --port COM4 --baud 115200 --hex "AA 55" --dry-run --json
```

`send` accepts exactly one of `--text`, `--hex`, `--file`; `query` accepts one Text/Hex payload and one Text/Hex delimiter, but not a file. Text sends exact UTF-8 without an automatic CR/LF. Hex supports explicit non-text bytes. Raw file transmission does not transform its contents. Dry-run validates without opening physical or TCP resources.

For `receive`, `query`, and `sequence run`, `--rx-display hex|text|both` affects only human-readable presentation; JSON/JSONL retains canonical raw-byte fields. See the [CLI machine contract](../contracts/serial-cli-jsonl-contract.md) for format and exit-code details.

## 4. Interactive Terminal

```sh
serial-tool terminal --mode live --port COM4 --baud 115200
serial-tool terminal --transport tcp --mode live --host 127.0.0.1 --tcp-port 5025
serial-tool terminal --mode simulate --baud 115200 --simulation-profile-id loopback-v1
```

Terminal keeps one connection and continuously receives while accepting typed TX. Enter submits an input line; `--tx-format text|hex`, `--line-ending none|lf|crlf`, `--rx-display` and `--tx-display` control interaction and display. Exit with Ctrl+C or EOF.

For an RX-only display redirected to a file:

```sh
serial-tool terminal --mode live --port COM4 --baud 115200 --rx-display stream --tx-display off > rx.txt
serial-tool terminal --mode live --port COM4 --baud 115200 --rx-display hex-stream --tx-display off > rx.hex
```

`stream` writes RX text; `hex-stream` joins RX as spaced uppercase hex. Lifecycle/errors use stderr. These are presentation streams, **not** a raw-binary capture protocol or machine JSONL interface.

## 5. Periodic Auto TX

```sh
serial-tool auto-tx --mode live --port COM4 --baud 115200 --hex "AA 55" --interval-ms 500
serial-tool auto-tx --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --file payload.bin --interval-ms 1000
```

Specify one nonempty Text/Hex/File payload and a positive interval in milliseconds. The first send is immediate; the next interval starts after the previous send completes. File content is read once before connection. Continuous RX runs until Ctrl+C. `auto-tx` has no dry-run or JSON/JSONL mode.

## 6. Sequence files

A Sequence is a JSON file with `sequence_version: 1`, `transport.type/settings`, and ordered steps (`send_text`, `send_bytes`, `wait`, `read`, `read_until`, `repeat`). For example, create `sequence.json`:

```json
{
  "sequence_version": 1,
  "transport": {
    "type": "serial",
    "settings": { "baud_rate": 115200, "data_bits": 8, "parity": "none", "stop_bits": 1, "flow_control": "none", "timeout_ms": 1000 }
  },
  "steps": [
    { "id": "send-1", "type": "send_bytes", "hex": "aa55" },
    { "id": "read-1", "type": "read_until", "delimiter_hex": "0d0a", "max_bytes": 64 }
  ]
}
```

```sh
serial-tool sequence validate --file sequence.json
serial-tool sequence run --file sequence.json --mode live --port COM4
serial-tool sequence run --file sequence.json --mode simulate --simulation-profile-id loopback-v1 --json
```

A TCP Sequence uses `"transport":{"type":"tcp","settings":{"io_timeout_ms":1000}}` and is run with `--transport tcp --host HOST --tcp-port PORT`. Neither format stores the device resource or execution mode. Validation does not open a connection. See the [Sequence Contract](../contracts/serial-sequence-contract.md).

## 7. Worker and streaming

For a persistent machine-controlled process:

```sh
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --control-port 0
serial-tool worker --transport tcp --mode simulate --simulation-profile-id loopback-v1 --control-port 0
```

The Worker exposes localhost-only HTTP control and JSONL events. Ordinary Workers accept `send`, `send-file`, `receive`, `query`, and `run-sequence` (one active job). To receive autonomous simulation bytes, explicitly enable streaming:

```sh
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id periodic-rx-v1 --streaming --control-port 0
```

`periodic-rx-v1` is **only for Streaming Simulation Workers** and does not echo TX. Streaming Workers accept only `send` and `send-file`; RX events carry raw bytes, not guaranteed packet boundaries. HTTP acceptance is not job completion; Worker Stop acceptance is not process exit. See the [Worker Contract](../contracts/serial-worker-contract.md).

## 8. Common problems and limits

- **Nothing receives:** verify COM port or TCP destination, baud/line settings, and the device's actual response format. An empty read timeout does not always mean disconnection.
- **Unexpected text:** use Hex; raw device bytes are authoritative, and display may replace invalid UTF-8.
- **Incorrect line ending:** `send --text` does not append CR/LF; use explicit Hex or interactive Terminal's line-ending option.
- **Transport argument error:** do not pass `--port`/`--baud` to TCP, or `--host`/`--tcp-port` to UART.
- **No hardware available:** use supported `loopback-v1` simulation in Terminal, Auto TX, Sequence Run, or Worker; one-shot dry-run validates instead of simulating I/O.
- **Cannot run a Sequence:** confirm its transport/settings match the runtime selection. The legacy root `serial` JSON layout is not accepted.
- **Worker streaming command rejected:** `receive`, `query`, and `run-sequence` are only available without `--streaming`.

Serial Tool does not automatically retry failed sends, decode device protocols, or provide a capture database. For technical settings and complete command cases, see the [CLI README](README.md).
