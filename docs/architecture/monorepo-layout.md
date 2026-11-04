# Monorepo Layout

[Project README](../../README.md) | [Core](../core/README.md) | [CLI](../cli/README.md) | [Desktop](../desktop/README.md)

Serial Tool is a Rust Core + CLI/Worker + Tauri 2 Desktop repository. This document records the **current** ownership and publication structure; it does not propose new components.

```text
.
|-- AGENTS.md
|-- Cargo.toml / Cargo.lock         # Root Rust workspace
|-- crates/
|   |-- serial-tool-core/
|   |   |-- src/                   # Transport, settings, runtime, Sequence
|   |   `-- tests/
|   `-- serial-tool-cli/
|       |-- src/                   # CLI commands, terminal, auto-tx, Worker
|       `-- tests/
|-- apps/
|   `-- desktop/
|       |-- src/                   # React/TypeScript UI and en/zh-TW i18n
|       |-- tests/
|       |-- package.json / package-lock.json
|       `-- src-tauri/             # Tauri adapter; separate Cargo workspace
|           |-- Cargo.toml / Cargo.lock
|           `-- src/
|-- docs/
|   |-- architecture/              # Repository layout and ownership
|   |-- core/                      # Core engineering README
|   |-- cli/                       # CLI/Worker README and operator guide
|   |-- desktop/                   # Desktop README and operator guide
|   `-- contracts/                 # Common and Serial machine contracts
|-- scripts/                       # Windows build entry point
`-- .github/workflows/            # Rust/Desktop CI and Windows build
```

## Workspaces and dependencies

The root `Cargo.toml` has exactly two members: `crates/serial-tool-core` and `crates/serial-tool-cli`. It excludes `apps/desktop/src-tauri`. The Tauri crate has its **own** Cargo workspace and lockfile; Desktop frontend dependencies are managed by npm in `apps/desktop`.

Core owns UART/COM and TCP-client transport settings and I/O, deterministic simulation, shared persistent session management and the sole I/O owner, and Sequence/Step execution. CLI/Worker depends on Core. Desktop's Tauri adapter also depends on Core **directly**, not on CLI or Worker. UI presentation, terminal interaction, CLI parsing and Worker HTTP/JSONL do not belong in Core.

One session has one Core I/O owner: continuous RX monitoring and the Sequence Step Runner must not race to read a connection. A Worker is an integration adapter, not an embedded workflow orchestrator. Desktop has neither a Worker HTTP server nor a persistent run-history database.

## Documentation ownership

- Root `README.md` and `README.zh-TW.md`: project purpose, quick start and navigation.
- `docs/core/README*.md`: Core behavior and engineering boundaries.
- `docs/cli/README*.md`: CLI/Worker technical reference; `docs/cli/USER_GUIDE*.md`: operator workflow.
- `docs/desktop/README*.md`: Tauri/Desktop development; `docs/desktop/USER_GUIDE*.md`: operator workflow.
- `docs/contracts/`: canonical Common and Serial machine-facing contracts. Keep JSON keys, protocol versions and public semantics defined here, not independently in translated guides.
- `AGENTS.md`: repository development and validation policy. Actual code and contracts take precedence over explanatory documentation.

English is the primary maintenance language for public guides; Traditional Chinese documents remain semantically aligned. The architecture and contracts can remain English-only. Serial Tool has no separate WebUI or supported-device model registry, so neither `docs/webui/` nor `supported-models.md` is appropriate.

## Documentation publication boundary

Only Git-tracked repository documentation is a public source of truth. Private/local project plans, temporary validation results, logs, generated runtime data, and untracked scratch material do **not** become public specifications and must not be copied into GitHub. Do not add run-specific evidence or machine-local identifying paths to published documentation.

Desktop offline HTML Help is generated from bilingual Desktop USER_GUIDE files by scripts/generate_help.py, with tracked assets under apps/desktop/public/help/. The CLI offline user-guide command remains future work.
