[English](README.md)

# Serial Tool

Serial Tool 是以 Windows 為優先、使用 Rust 開發的通用 UART／COM 與 TCP **Client** 原始位元組通訊工具。專案包含共用 Core、工程用途 CLI／Worker，以及 Tauri 2 Desktop；Core 在可行範圍內保持跨平台。

Serial Tool 的定位是通訊與執行工具，**不是**特定儀器控制器、工作流程調度器、設備協定解碼器或測試紀錄資料庫。

## 主要功能

- 連接 COM 埠或 Raw TCP 端點，準確傳送與接收原始位元組。
- 傳送 UTF-8 文字、Hex 位元組或未經轉換的原始檔案，並持續接收 RX。
- 在支援的實機／模擬連線上使用互動式終端機及週期性自動傳送（Auto TX）。
- 建立、驗證、載入、儲存及執行 Sequence v1：傳送文字、傳送位元組、等待、讀取、讀取直到、重複。
- 使用事件或連續 RX 模式檢視傳輸資料；匯出 Hex 終端機紀錄、原始位元組或上次序列結果 JSON。
- 以 `loopback-v1` 進行免硬體模擬；`periodic-rx-v1` **僅**支援 Streaming Simulation Worker。
- 透過 Common Worker Protocol v3 提供機器可讀的本機 Worker；CLI／Manifest 的機器輸出使用另一套既有 Schema。
- Desktop 提供 English／繁體中文，以及系統／淺色／深色外觀切換。

TCP 僅是**原始位元組串流 Client**，不包含 TCP Server、自動重連、封包切分或特定設備協定。

## 元件分工

- **Core**（`crates/serial-tool-core`）：傳輸、設定、模擬、持續連線 Runtime、RX 所有權及序列執行。
- **CLI／Worker**（`crates/serial-tool-cli`）：CLI 操作、互動式終端機及透過 Core 執行的本機 HTTP／JSONL Worker Adapter。
- **Desktop**（`apps/desktop`）：React／TypeScript／Vite 介面及 Tauri 2 Rust Adapter；直接使用 Core，不透過 CLI／Worker。

兩個獨立 Rust Workspace 與各層責任詳見 [Monorepo Layout](docs/architecture/monorepo-layout.md)。

## 專案結構

```text
crates/
  serial-tool-core/        # UART/TCP Core、模擬及 Sequence
  serial-tool-cli/         # CLI 與 Worker HTTP/JSONL
apps/desktop/
  src/                     # React/TypeScript 前端
  src-tauri/               # 獨立 Rust/Tauri Workspace
docs/
  architecture/ core/ cli/ desktop/ contracts/ help/
scripts/                   # Windows Build、Release Acceptance 與 Help 產生器
```

各層責任、Lockfile 與完整結構詳見 [Monorepo Layout](docs/architecture/monorepo-layout.md)。

## 主要相依技術

| 元件 | 用途 |
| --- | --- |
| Rust、serialport | Core 與 UART／COM 通訊 |
| Clap、Serde／serde_json | CLI 參數及機器可讀資料 |
| Tauri 2 | 直接使用 Core 的 Windows Desktop 外殼 |
| React、TypeScript、Vite | Desktop 介面及前端建置 |
| Microsoft Edge WebView2 Runtime | Windows 系統提供的 Runtime；Serial Tool 不隨附 |

確切相依版本以 Cargo 與 npm 的 Manifest／Lockfile 為準。

## 快速開始

先安裝支援 Rust edition 2024 的工具鏈，在 Repository 根目錄執行：

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo run -p serial-tool-cli -- list-ports
cargo run -p serial-tool-cli -- send --port COM4 --baud 115200 --hex "AA 55" --dry-run
```

Dry-run 只驗證適用的操作，不開啟 COM 埠、TCP Socket 或進行 DNS 連線。實際操作前請確認目標設備，並移除 `--dry-run`。

### Desktop

需安裝 Node.js／npm 及 Tauri 2 平台相依環境。Windows 必須有系統提供的 Microsoft Edge WebView2 Runtime；程式不會自動安裝。

```sh
cd apps/desktop
npm ci
npm run tauri -- dev
```

Desktop 支援 COM／TCP 實機連線及免硬體迴送模擬。操作設備前請閱讀 [Desktop 使用指南](docs/desktop/USER_GUIDE.zh-TW.md)。

## 建置

Root Cargo Workspace 包含 Core 與 CLI；`apps/desktop/src-tauri` 是另一個獨立 Workspace，並有自己的 Lockfile。在 Repository 根目錄使用 PowerShell 執行 `.\scripts\build-all.ps1`，可產生不含安裝程式的 Windows CLI 與 Desktop EXE。各元件的開發命令請參考 [Core](docs/core/README.zh-TW.md)、[CLI](docs/cli/README.zh-TW.md) 與 [Desktop](docs/desktop/README.zh-TW.md)。

## 測試與正式發布驗收

一般開發沿用既有 Linux／Windows Rust 與 Desktop CI；預設不需要實體硬體。聚焦驗證及測試選擇請參閱 [AGENTS.md](AGENTS.md) 與 [Testing Guidelines](docs/testing-guidelines.md)。

**僅在 Windows 正式發布候選版本**，於既有 GitHub 手動 **Build Windows CLI and Desktop** Action 選擇 **release-acceptance**，或於乾淨且已提交的 HEAD 本機執行 `.\scripts\release-acceptance.ps1`。Action 預設 **build**，維持原本單純建置流程。腳本重用 `build-all.ps1`，比對版本、驗證雙 EXE 與無硬體 CLI Smoke，並在被 Git 忽略的 `release/`、`reports/` 目錄產生授權聲明、SHA-256 與 PASS／FAIL 報告。這不代表已驗證實體 COM、外部 TCP 設備或 GUI 操作，也不會建立 Tag／GitHub Release。

## 文件導覽

- [Core 說明](docs/core/README.zh-TW.md)
- [CLI 說明](docs/cli/README.zh-TW.md)
- [CLI 使用指南](docs/cli/USER_GUIDE.zh-TW.md)
- [Desktop 說明](docs/desktop/README.zh-TW.md)
- [Desktop 使用指南](docs/desktop/USER_GUIDE.zh-TW.md)
- [離線說明維護文件](docs/help/README.md)
- [版本更新紀錄](CHANGELOG.md)

工程文件：[Repository 架構](docs/architecture/monorepo-layout.md)、[CLI JSON／JSONL 合約](docs/contracts/serial-cli-jsonl-contract.md)、[Worker 合約](docs/contracts/serial-worker-contract.md)、[序列合約](docs/contracts/serial-sequence-contract.md) 及 [Orchestrator 整合合約](docs/contracts/serial-orchestrator-workflows.md)。共用 Common 合約亦放在 [contracts](docs/contracts/)。

機器可讀欄位、Worker 生命週期及 Sequence JSON 以正式合約為準，而非翻譯後的指南。CLI `--help` 用於指令參數說明；`serial-tool user-guide [--lang en|zh-TW]` 可透過預設瀏覽器開啟內建雙語 CLI 離線指南；Desktop 仍提供獨立的雙語離線說明視窗。

## 參與開發

提交 Pull Request 前請先閱讀 [AGENTS.md](AGENTS.md)、[Testing Guidelines](docs/testing-guidelines.md) 及適用的[機器合約](docs/contracts/)。請保持變更範圍精簡，不將模擬、CI 或打包測試宣稱為實機驗證。

## 授權與免責聲明

Copyright (c) 2026 tom758258。Serial Tool 採用 [MIT License](LICENSE)。第三方授權資訊及另行採用 MPL-2.0 的 `serialport` 元件，請參閱 [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt)；MPL-2.0 全文收錄於 [licenses/MPL-2.0.txt](licenses/MPL-2.0.txt)。

Serial Tool 為獨立、非官方專案，與硬體、作業系統及框架廠商沒有隸屬、背書或贊助關係。第三方名稱、產品及商標均屬各自權利人所有。使用者須遵守適用的第三方軟體、驅動程式、硬體與文件授權條款。MIT License 不提供保固。
