[English](README.md) | [Desktop 使用指南](USER_GUIDE.zh-TW.md) | [專案介紹](../../README.zh-TW.md)

# Serial Tool Desktop

Desktop 是以 Windows 為優先的 Tauri 2／React／TypeScript／Vite 應用程式。本文件提供開發設定、元件責任及技術限制；一般操作請參考 [Desktop 使用指南](USER_GUIDE.zh-TW.md)。

## 開發環境

需安裝 Rust、Node.js、npm 及 Tauri 2 的平台需求。Windows 使用**系統提供**的 Microsoft Edge WebView2 Runtime；Serial Tool 在建立 Tauri 視窗前檢查是否可用。若不存在，原生 Windows 對話框提供開啟 [Microsoft 官方 WebView2 網頁](https://developer.microsoft.com/microsoft-edge/webview2/) 的選項，無論選擇 Yes 或 No 都不會建立應用視窗。程式不會自行下載、安裝或打包 WebView2；請安裝後重新啟動。若檢查通過但視窗初始化失敗，原生對話框會呈現實際 Tauri 錯誤。

在 `apps/desktop` 內執行：

```sh
npm ci
npm run tauri -- dev
npm run typecheck
npm run build
npm run tauri -- build --no-bundle
```

Tauri 有自己的 Cargo Workspace 與 `Cargo.lock`。從 Repository 根目錄執行：

```sh
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --check
cargo clippy --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path apps/desktop/src-tauri/Cargo.toml
```

前端測試入口為 `npm test`。應依 [AGENTS.md](../../AGENTS.md) 先執行相關的聚焦驗證，再遵守適用 Baseline。

## Frontend、多語系與 Contracts

- `apps/desktop/src/` 管理 React UI、終端機／序列顯示及 `en`／`zh-TW` 翻譯。
- `apps/desktop/src-tauri/` 是**獨立** Rust Cargo Workspace，僅以薄 Adapter 使用 `serial-tool-core`，不透過 CLI 或 Worker 操作設備。
- 語言使用 `serial-tool.locale`、系統／淺色／深色外觀使用 `serial-tool.theme`，兩者分開保存。切換語言不會重新建立 Session、清除資料、改寫序列 JSON 或更動匯出位元組。
- Core／CLI／Worker 的原始診斷及 Machine Payload 不翻譯。操作詳 [使用指南](USER_GUIDE.zh-TW.md)；架構詳 [Monorepo Layout](../architecture/monorepo-layout.md)；正式資料格式詳 [contracts](../contracts/)。

## 所有權與共用 Runtime

Tauri Command 與 Desktop Backend 只做 Core 共用 Persistent Runtime 的輕量 Adapter，包括 UI Event、Result DTO 轉換。一條 UART、TCP 或 Simulation 連線由**單一** Core Owner Thread 持有 `SerialSession`，統一處理手動 TX、週期排程、連續 RX、Sequence 與 Disconnect。

Monitor 在讀取前會確認 Session Buffer 與 Transport 待接收 RX 的數量；無資料時短暫等待，並在下次監控讀取前處理命令。`StepRunner` 在相同 Owner 上同步執行，因此 Sequence 執行時 Monitor 不搶讀，結束後恢復連續 RX。

Core 在手動 Send、Auto TX 或 Sequence 已接受／執行期間，會把互斥的新請求回報為 Busy，而非排入等待佇列。Desktop 不修改既有連線 I/O Timeout，不經過 CLI／Worker／HTTP，也沒有永久 Run History。
