# Agent Instructions

These instructions define long-term, repository-specific boundaries for agents working on `serial-tool`.
Keep changes small, preserve clear component ownership, and do not build future layers before a concrete requirement exists.

## 1. Project Context

- Read the affected code and relevant documentation before changing behavior.
- Keep the project focused on serial/device communication and execution. Do not turn it into a test-record management system, workflow engine, or orchestration system.
- The product structure is Core + CLI + Desktop. Keep implementation proportional to concrete requirements.
- The project is Windows-first, but Core should use Rust abstractions where practical instead of unnecessarily hard-coding Windows-specific behavior.

## 2. Repository And Architecture Boundaries

The intended initial repository structure is:

```text
serial-tool/
├─ crates/
│  ├─ serial-tool-core/
│  └─ serial-tool-cli/
├─ examples/
├─ tests/
├─ Cargo.toml
├─ Cargo.lock
├─ README.md
└─ AGENTS.md
```

The Desktop application lives at:

```text
apps/desktop/                 # Tauri 2 + React/TypeScript/Vite
└─ src-tauri/                 # Separate Cargo workspace
```

Architecture rules:

- `serial-tool-core` owns serial communication behavior, protocol-independent execution logic, configuration models, validation, simulation behavior, and reusable domain logic.
- `serial-tool-cli` depends on Core and provides an engineering/diagnostic command-line interface.
- Core must not depend on CLI-specific parsing, terminal presentation, Tauri, frontend frameworks, TypeScript, WebView APIs, or other UI-specific code.
- Desktop depends directly on Core. Core must not depend on Desktop, Tauri, or frontend code.
- Tauri commands remain thin; one connected `SerialSession` has one I/O owner thread.
- Continuous RX and `StepRunner` must never race to read the same connection.
- Desktop is not Worker and has no database or persistent run history.
- Keep UI/application-layer commands thin. Reusable behavior belongs in Core.
- Do not split the project into additional crates unless a real ownership or dependency boundary requires it.

## 3. Serial Communication Boundary

- Keep serial-port communication logic in Core.
- Do not embed device-specific business logic into generic serial transport code.
- Separate transport concerns from higher-level command/response interpretation when such a distinction is required by an actual use case.
- Preserve explicit control over serial settings such as port, baud rate, data bits, parity, stop bits, flow control, read/write timeout, and line termination when supported by the implementation.
- Do not silently guess communication parameters when correctness depends on them.
- Handle port-open, read, write, timeout, disconnect, malformed input, and cancellation paths explicitly.
- Avoid unnecessary background threads, async runtimes, channels, or buffering layers unless the implementation actually needs them.
- TCP/IP support may be added when there is a concrete requirement, but do not force serial and TCP/IP into a premature generic transport framework.

## 4. Orchestrator / Worker Boundary

`serial-tool` is an executable tool that may be invoked by `orchestrator-tool`.

- Keep orchestration logic in `orchestrator-tool`; do not reimplement scheduling, workflow execution, template ownership, or orchestration policy in this repository.
- The worker surface must remain deterministic, machine-readable, and suitable for non-interactive invocation by the orchestrator.
- Interactive CLI behavior and worker behavior may share Core logic, but worker execution must not depend on prompts, terminal interaction, or GUI state.
- Keep tool identity, capabilities, request handling, result handling, and error reporting compatible with the shared common contracts used by the orchestrator ecosystem.
- Do not invent a Serial-only replacement for shared common contracts when the existing shared contract can represent the requirement.
- Do not change shared contract semantics, schema compatibility rules, or cross-tool behavior without explicit approval.
- When a contract change is required for Serial support, keep the change minimal and compatible with the other tool projects unless an intentional breaking revision has been approved.
- Contract compatibility and implementation revision are separate concerns; do not bump a schema version merely to record an internal implementation change.

## 5. Simulation And Dry-Run

- Worker-capable behavior must have a path that can be exercised without real hardware where practical.
- Keep simulation/dry-run behavior deterministic enough for automated validation.
- Simulation must not silently masquerade as real hardware execution. The result must make the execution mode identifiable when relevant.
- Dry-run must not open a real serial port or perform device I/O.
- Do not build an elaborate simulator unless required by concrete protocol/device behavior.
- Prefer the smallest simulation surface that validates request parsing, execution flow, result shape, errors, and orchestrator integration.

## 6. Data, Logging, And Persistence

- This project is an execution tool, not a test-record management system.
- Do not introduce SQL databases, run-history databases, persistent job stores, or record-management subsystems without explicit approval.
- Runtime logs, captures, reports, CSV/JSONL exports, and generated data are local/runtime artifacts unless a requirement explicitly says otherwise.
- Keep persistent configuration lightweight.
- Do not commit generated runtime data simply to support tests. Prefer small inline fixtures or narrowly scoped tracked fixtures only when they are genuinely required for deterministic testing.
- Avoid logging secrets, credentials, or unnecessarily large serial payloads.

## 7. Dependencies And Packaging

- Do not add a dependency until the implementation needs it.
- Prefer established Rust crates over custom platform bindings when they satisfy the requirement cleanly.
- Do not add an async runtime solely because asynchronous I/O might be useful later.
- Do not add Tauri, frontend dependencies, serialization formats, logging frameworks, plugin systems, database layers, or network stacks before a concrete requirement exists.
- Keep `Cargo.lock` tracked for reproducible application builds.
- Keep distribution and packaging changes separate from Core behavior unless the task explicitly requires both.

## 8. Testing And Validation

- Default tests and validation must not require real serial hardware.
- Prefer focused unit tests for parsing, validation, execution decisions, error mapping, and simulation behavior.
- Add integration tests when they validate an actual cross-component boundary such as CLI-to-Core or worker-to-contract behavior.
- Do not create large test matrices for trivial code or test implementation details that provide little regression value.
- Use real-hardware validation only when the behavior cannot be meaningfully verified otherwise.
- When only worker/common-contract integration changes and transport/device behavior is unchanged, do not require real-hardware validation unless the change could affect actual I/O behavior.

Repository baseline checks should normally include:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

Desktop baseline checks:

```text
cd apps/desktop && npm ci && npm run typecheck && npm run build
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --check
cargo clippy --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path apps/desktop/src-tauri/Cargo.toml
```

- Run the narrowest relevant checks first, then workspace-level checks when practical.
- If a check is not applicable yet because the corresponding component has not been created, do not add scaffolding only to make the check exist.
- Report failed, skipped, blocked, or unexecuted verification steps instead of implying they passed.

## 9. Scope Control

- Prefer the smallest implementation that establishes the current requirement.
- Do not implement future phases opportunistically.
- Do not refactor unrelated working code while implementing a focused task.
- Avoid abstractions with only one current implementation unless they materially improve correctness, testability, or ownership.
- Do not add extensibility points merely because another transport, protocol, device family, UI, or plugin may exist later.
- Ask for explicit approval before changing durable component ownership, shared contract semantics, worker behavior, public CLI semantics, persistence strategy, or distribution strategy.

## 10. Documentation Boundary

- Keep the root `README.md` as the project overview, setup guide, and primary entry point rather than a complete specification dump.
- Put durable architecture or contract details in focused documentation only when the implementation needs that level of detail.
- Keep operator-facing usage separate from maintainer/architecture notes when the documentation grows enough to justify that split.
- Do not add transient review notes, temporary validation results, run-specific evidence, or personal scratch notes to tracked documentation.
- Update documentation when user-visible behavior, CLI usage, worker behavior, supported communication settings, or required setup changes.

## 11. Change Discipline

Before completing a change:

1. Confirm the implementation stays within the requested scope.
2. Confirm Core/CLI/worker ownership remains clear.
3. Confirm no unnecessary dependency or future-layer scaffolding was added.
4. Confirm common-contract compatibility when worker-facing behavior changed.
5. Run the relevant validation commands.
6. State clearly what was changed and which validation was actually executed.

When there is tension between a clever design and a smaller design that fully satisfies the current requirement, prefer the smaller design.
