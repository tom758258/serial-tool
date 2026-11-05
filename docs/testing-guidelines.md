# Testing Guidelines

Serial Tool tests protect public contracts, core communication behavior, safety boundaries, and supported cross-component integration. Keep effort proportional to the changed behavior. The baseline and change discipline remain in [AGENTS.md](../AGENTS.md).

## What to test

- Core UART/COM and raw TCP client parsing, validation, simulation, I/O ownership, errors, cancellation, and Sequence semantics when those behaviors change.
- CLI flags, exit behavior, manifest/JSON/JSONL fields, and Worker HTTP/JSONL lifecycle according to the [contracts](contracts/).
- Core-to-CLI/Worker and Core-to-Tauri boundaries when integration changes, at the narrowest useful layer.
- Desktop user workflows and localization when user-visible behavior changes; stable interfaces rather than incidental implementation.
- Safety: dry-run must not open transport resources; simulation must not masquerade as live, and hardware access must remain explicit.
- Packaging/release entry points and the real two-EXE distribution when build or release logic changes.

## What not to freeze

- Natural-language README/guide wording, paragraph counts, incidental headings, and noncontractual text order.
- CSS measurements, visual styles, markup snapshots, internal function or variable names, and component implementation details.
- Repeated tests for the same contract at every layer, or a permanent regression test for every minor fix.
- Byte-for-byte copies of contracts maintained by other repositories; review Serial's published tracked contracts for actual semantics.

## Documentation and frontend checks

Check that important links exist, commands and paths are current, and published schema names and versions remain correct. Test runtime behavior in runtime tests, not by matching source-text fragments. For translation, protect semantic key coverage rather than exact UI prose. When Offline Help sources or templates change, run the existing Help generator tests and synchronization check.

## Validation responsibility

- Run the smallest tests covering the changed boundary first. Run applicable AGENTS.md Rust/Tauri/frontend baseline when practical, without a new platform matrix.
- Existing `.github/workflows/ci.yml` covers Rust and Desktop on Linux and Windows. `scripts/build-all.ps1` produces two Windows EXEs with basic CLI smoke; the manual Windows Build workflow does not constitute formal release acceptance.
- Reserve `scripts/release-acceptance.ps1` for Windows release candidates or relevant packaging/distribution changes. It requires a clean committed HEAD, reuses the build script, validates packaged outputs without hardware, and emits a report. It does not create a tag or Release.
- Real COM/remote TCP validation is separate, explicitly authorized, and only needed when device/transport behavior warrants it. Never infer hardware coverage from simulation, CI, or release acceptance.
- Report tested commit/platform and distinguish PASS, FAIL, SKIPPED, BLOCKED, and NOT RUN. Environment-blocked and unexecuted checks are not PASS.

## Review standard

Each assertion should protect a meaningful regression. Prefer existing focused tests and maintainable checks. Do not trigger a full Windows release build for README-only changes or add permanent CI jobs without demonstrated need.
