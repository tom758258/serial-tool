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

## 建置與驗證

Root Cargo Workspace 包含 Core 與 CLI；`apps/desktop/src-tauri` 是另一個獨立 Cargo Workspace，並有自己的 Lockfile。開發與驗證方式請參考 [Core](docs/core/README.zh-TW.md)、[CLI](docs/cli/README.zh-TW.md)、[Desktop](docs/desktop/README.zh-TW.md) 與 [AGENTS.md](AGENTS.md)。預設自動化測試不需要實體硬體。

## 文件導覽

- [Core 說明](docs/core/README.zh-TW.md)
- [CLI 說明](docs/cli/README.zh-TW.md)
- [CLI 使用指南](docs/cli/USER_GUIDE.zh-TW.md)
- [Desktop 說明](docs/desktop/README.zh-TW.md)
- [Desktop 使用指南](docs/desktop/USER_GUIDE.zh-TW.md)
- [離線說明維護文件](docs/help/README.md)

工程文件：[Repository 架構](docs/architecture/monorepo-layout.md)、[CLI JSON／JSONL 合約](docs/contracts/serial-cli-jsonl-contract.md)、[Worker 合約](docs/contracts/serial-worker-contract.md)、[序列合約](docs/contracts/serial-sequence-contract.md) 及 [Orchestrator 整合合約](docs/contracts/serial-orchestrator-workflows.md)。共用 Common 合約亦放在 [contracts](docs/contracts/)。

機器可讀欄位、Worker 生命週期及 Sequence JSON 以正式合約為準，而非翻譯後的指南。CLI `--help` 用於指令參數說明；`serial-tool user-guide [--lang en|zh-TW]` 可透過預設瀏覽器開啟內建雙語 CLI 離線指南；Desktop 仍提供獨立的雙語離線說明視窗。
