# Serial Tool Core

`serial-tool-core` 是 CLI／Worker 與 Tauri Desktop Backend 共用的 Rust 通訊與執行函式庫。Core 擁有裝置 I/O 及與傳輸協定無關的執行邏輯；互動介面、CLI 輸入及機器可讀輸出則由各 Adapter 負責。

## 用途與責任

目前 Core 位於 `crates/serial-tool-core`：

| 模組 | 責任 |
| --- | --- |
| `config.rs`／`discovery.rs` | UART 設定、COM 埠探索及連線設定 |
| `transport/serial.rs` | 原始 UART／COM I/O |
| `transport/tcp.rs` | 原始 TCP **Client** 連線與 I/O |
| `transport/simulation.rs` | 可重現且免硬體的模擬傳輸 |
| `session.rs` | 傳輸 Session 與原始 TX／RX |
| `runtime.rs` | 持續連線、週期 TX、連續 RX 與獨占 I/O 所有權 |
| `step.rs`／`sequence.rs` | Step Runner、序列解析、驗證及執行 |
| `error.rs`／`lib.rs` | 共用錯誤與公開 Crate API |

Core 提供共用 `Transport` 邊界，但不負責 CLI 參數解析、Worker HTTP／JSONL、React 畫面或 Tauri 視窗。Desktop 與 CLI 是使用同一 Core 的平行 Adapter，不各自建立一套傳輸引擎。

## Runtime 與通訊邊界

- 一條已連線 Session 只有**一個** Core I/O Owner；連續 RX 監控不可與 `StepRunner` 同時競爭讀取。
- UART 設定包含鮑率、資料位元、同位檢查、停止位元、流量控制及逾時；TCP 包含 Host、Port、Connect Timeout 及共用的讀寫 I/O Timeout。
- TCP 僅是支援 IPv4、IPv6 與 Hostname 的原始位元組串流 **Client**，沒有 Server、應用層封包切分、設備協定解析、TLS 或自動重連。
- 單次 Read 逾時但沒有新資料，不必然表示連線中斷。TCP EOF 與不可恢復 I/O 錯誤須走連線清理；Write 失敗也不能假設對方完全未收到位元組。
- 原始位元組是資料依據；Text／Hex 顯示屬於 Adapter 責任，原始 TX 不會自動加換行。

## Simulation

`loopback-v1` 會把送出的原始位元組迴送至 RX，不開啟實體 COM 埠或 TCP Socket。`periodic-rx-v1` 約每 100 ms 自主產生一筆遞增的四位元組 Big-endian 計數，不迴送 TX；**只供 Streaming Simulation Worker 使用**，不適用一般 Desktop、互動式 CLI 或普通 Worker。

## Sequence v1

Core 支援 `send_text`、`send_bytes`、`wait`、`read`、`read_until` 與巢狀 `repeat`。正式 JSON 格式為 `sequence_version: 1` 加上 `transport: {type, settings}`；不支援舊的根欄位 `serial`。UART／TCP 共用執行邏輯，但設定必須與目前連線一致。Sequence 不保存 COM 埠、TCP Host／Port、執行模式或 TCP Connect Timeout。欄位、錯誤及執行規則以 [Serial Sequence Contract](../contracts/serial-sequence-contract.md) 為準。

## 開發與驗證

Core 屬於 Root Cargo Workspace；從根目錄執行：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

修改 Core 時，先使用 `crates/serial-tool-core/tests/` 的相關聚焦測試。預設自動化測試依需求使用模擬或本機 Socket，不必連接實體串列設備。測試範圍與證據遵守 [AGENTS.md](../../AGENTS.md)。本文件解釋元件責任；CLI／Worker 的機器可讀格式仍以 [contracts](../contracts/) 為準。
