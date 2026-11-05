# Changelog

## 1.0.0

### Added

- First stable Windows-first release of the Rust Core, CLI/Worker, and
  Tauri 2 + React Desktop, distributed as two standalone Windows EXEs.
- Raw UART/COM and TCP **client** byte communication, Terminal, Auto TX,
  send-file, persistent session RX, and cooperative operation handling.
- Sequence v1 editor, validation, execution, and structured results;
  `loopback-v1` simulation and streaming Worker `periodic-rx-v1` simulation.
- Machine-readable CLI/Manifest schema 2 and Common Worker Protocol v3-only
  HTTP/JSONL, including asynchronous jobs and optional one-stream RX capture.
- English/Traditional Chinese Desktop UI, bilingual documentation, offline
  Desktop/CLI Help, and System/Light/Dark Desktop themes.
- MIT licensing, third-party notices, and optional full hardware-free
  `release-acceptance` in the existing manual Windows Build Action.
  Acceptance packages a versioned candidate, SHA-256 checksums, and separate
  JSON/Markdown evidence without automatically publishing a GitHub Release.

### Known Limitations

- TCP is a raw client transport; no server, application framing, automatic
  reconnect/replay, or device-specific protocol decoder is provided.
- The Windows Desktop needs the system WebView2 Runtime. Release acceptance
  does not establish real COM/TCP hardware compatibility or GUI behavior.
- Serial Tool is an execution/communication tool, not an orchestrator or
  persistent run-history database.
