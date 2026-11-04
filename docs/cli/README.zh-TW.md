[English](README.md) | [CLI 使用指南](USER_GUIDE.zh-TW.md) | [專案介紹](../../README.zh-TW.md)

# Serial Tool CLI／Worker

本文件是 `crates/serial-tool-cli` 的工程與指令詳細參考。一般操作流程請看 [CLI 使用指南](USER_GUIDE.zh-TW.md)；機器可讀欄位及生命週期則以 [contracts](../contracts/) 為準，不以一般說明或翻譯取代正式 Schema。

`serial-tool` 是 CLI 與 Worker 共用的執行檔入口。CLI 參數、終端機顯示、Worker HTTP／JSONL 與輸出格式屬 Adapter；裝置 I/O 與序列執行由 `serial-tool-core` 負責。

## 開發

在 Repository 根目錄：

```sh
cargo build -p serial-tool-cli --locked
cargo test -p serial-tool-cli --locked
cargo run -p serial-tool-cli -- --help
```

指令完整 Flags／預設值請使用 `serial-tool <command> --help`。本版本**尚未**提供內建離線 `user-guide` 指令。

## 文件與整合

- [CLI 使用指南](USER_GUIDE.zh-TW.md)：一般操作與問題排除。
- [Core README](../core/README.zh-TW.md)：共用 Runtime／傳輸所有權。
- [Serial CLI JSON／JSONL Contract](../contracts/serial-cli-jsonl-contract.md)：機器輸出及 Exit Code。
- [Serial Worker Contract](../contracts/serial-worker-contract.md)：Worker 指令、HTTP、Streaming 及生命週期。
- [Serial Sequence Contract](../contracts/serial-sequence-contract.md)：Sequence v1 的 JSON 格式及驗證。
- [Serial Orchestrator Workflow Contract](../contracts/serial-orchestrator-workflows.md)：與 Orchestrator 的整合邊界。
- [Common Worker Protocol](../contracts/common-worker-protocol.md)：共用 Worker 合約。
- [Monorepo Layout](../architecture/monorepo-layout.md)、[AGENTS.md](../../AGENTS.md)：所有權與測試規範。

## 指令與進階操作參考

## CLI 基本指令

在 Root Workspace 使用 `cargo run -p serial-tool-cli -- <command>`，或直接執行建置好的 `serial-tool`：

```sh
serial-tool manifest --json
serial-tool list-ports
serial-tool list-ports --json
serial-tool send --port COM4 --baud 115200 --text "STATUS?"
serial-tool send --port COM4 --baud 115200 --hex "00 55 AA FF 0D 0A"
serial-tool send --port COM4 --baud 115200 --file payload.bin
serial-tool send --port COM4 --baud 115200 --file payload.bin --dry-run --json
serial-tool receive --port COM4 --baud 115200 --max-bytes 256
serial-tool query --port COM4 --baud 115200 --hex "00 55 AA FF 0D 0A" --until-hex "0D 0A" --max-bytes 1024
serial-tool query --port COM4 --baud 115200 --text "STATUS?" --until-text "OK" --max-bytes 1024 --dry-run --json
serial-tool send --transport tcp --host 127.0.0.1 --tcp-port 5025 --hex "AA 55"
serial-tool query --transport tcp --host 127.0.0.1 --tcp-port 5025 --text "STATUS?" --until-text "OK"
```

`send` 必須恰好指定 `--text`、`--hex` 或 `--file` 之一，原始檔不進行文字解碼或換行／Hex 轉換；空檔、遺失或無法讀取檔案會拒絕，檔案輸入錯誤 Exit Code 為 2。Dry-run 可讀取並驗證檔案，但不開埠、不建立 TCP Socket 或進行 DNS。

`query` 必須從 `--text`／`--hex` 選一項，再從 `--until-text`／`--until-hex` 選一項；不接受 `--file`。Text 原樣轉成 UTF-8，不自動加 CR／LF；需控制位元組時應用 Hex。機器可讀的原始位元組一律使用無分隔符號的小寫 Hex。

`--transport` 預設 `serial`。UART 單次指令須有 `--port`、`--baud`；預設 8 Data Bits、無 Parity、1 Stop Bit、無 Flow Control、1000 ms Timeout。TCP 需 `--transport tcp`、`--host`、`--tcp-port`（1～65535），不能混用 UART 專用參數；TCP 預設 Connect Timeout 3000 ms、共用 Read／Write I/O Timeout 1000 ms，可由 `--connect-timeout-ms`／`--io-timeout-ms` 設定。TCP Live 支援 IPv4、IPv6、Hostname；Dry-run 不解析 DNS。

`receive` 只執行一次有界讀取；其 Dry-run 不開啟或確認實體埠。`--format text|json|jsonl` 預設為 `text`，`--json` 等同 `--format json`。對 `receive`、`query`、`sequence run` 可使用 `--rx-display hex|text|both`（預設 Hex）；Text 使用 Lossy UTF-8 並跳脫控制字元。顯示模式不影響 JSON／JSONL 的小寫 Hex 機器輸出。

## 互動式 Terminal

持續連線並接收 RX，同時從標準輸入送出文字或 Hex：

```sh
serial-tool terminal --mode live --port COM4 --baud 115200
serial-tool terminal --mode simulate --baud 115200 --simulation-profile-id loopback-v1
serial-tool terminal --transport tcp --mode live --host 127.0.0.1 --tcp-port 5025
serial-tool terminal --transport tcp --mode simulate --simulation-profile-id loopback-v1
```

支援 `--rx-display hex|text|both|stream|hex-stream`（預設 Hex）、`--tx-display hex|off`（預設 Hex）、`--tx-format text|hex`（預設 Text）、`--line-ending none|lf|crlf`（預設 None）。Enter 送出一行；標準輸入的 CR／LF 先移除，再按選項加入文字行尾；Hex 不套用文字換行。錯誤輸入不會自動結束 Terminal；EOF／Ctrl+C 正常中斷連線。EOF 允許短暫約 50 ms 的最後監控時間，但不等待設備回覆。互動式 Terminal **沒有 JSON／JSONL 輸出合約**。

`stream` 只輸出連續 RX 文字（LF 換行、略去 CR、跳脫其他控制字元）；CLI 每筆 RX Event 個別做 Lossy UTF-8 解碼，跨事件的多位元組文字可能出現替代字元。`hex-stream` 將所有 RX 位元組以大寫、兩位 Hex 加單一空白連接；不加行尾換行。兩者把 RX 送到 stdout，生命週期、錯誤與 TX Hex 紀錄送到 stderr。`--tx-display off` 只隱藏 TX 呈現，不停止傳送。

```sh
serial-tool terminal --mode live --port COM4 --baud 115200 --rx-display stream --tx-display off > rx.txt
serial-tool terminal --mode live --port COM4 --baud 115200 --rx-display hex-stream --tx-display off > rx.hex
```

## 自動傳送（Auto TX）

`auto-tx` 保持單一連線並持續 RX，週期性傳送指定 payload，按 Ctrl+C 停止：

```sh
serial-tool auto-tx --mode live --port COM4 --baud 115200 --text "STATUS?" --interval-ms 1000
serial-tool auto-tx --mode live --port COM4 --baud 115200 --hex "AA 55" --interval-ms 500
serial-tool auto-tx --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --file payload.bin --interval-ms 1000
serial-tool auto-tx --transport tcp --mode live --host 127.0.0.1 --tcp-port 5025 --hex "AA 55" --interval-ms 500
```

Live 需指定設備資源，禁止 `--simulation-profile-id`；Simulation 需明確使用 `loopback-v1`，且不能混用設備資源選項。Payload 需從 `--text`、`--hex`、`--file` 擇一，內容不可為空；檔案在連線前讀取一次，執行期間檔案修改不影響當次內容。`--interval-ms` 必須是正整數；第一次立即傳送，後續間隔從上一筆 Send／Flush 完成後開始，不補發錯過的週期。連線／I/O 失敗會清理連線並使用 Exit Code 3；Auto TX 無互動式標準輸入、Dry-run 或 JSON／JSONL 合約。

## Worker RX Streaming（選用）

```sh
serial-tool worker --mode live --port COM4 --baud 115200 --streaming
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --streaming
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id periodic-rx-v1 --streaming
```

啟用 `--streaming` 時，Worker 在 `run_bound` 階段持續輸出 JSONL `rx` Event（`rx_hex`／`rx_bytes`），並透過非同步 HTTP `send`、`send-file` Job 傳送 TX。此模式**不提供** `receive`、`query`、`run-sequence`；普通 Worker 仍支援這些命令。`loopback-v1` 迴送 TX；`periodic-rx-v1` 約每 100 ms 自主產生遞增四位元組 Big-endian 計數，**不迴送 TX 且僅限 Streaming Simulation Worker**。RX Event 邊界不是設備 Frame，不提供協定解碼、Replay 或 Orchestrator 的 Capture Buffer。

## Sequence v1

Sequence JSON 使用 `sequence_version: 1` 及 `transport.type/settings`，不接受舊的根欄位 `serial`。UART 包含六項線路設定；TCP 只有 `io_timeout_ms`。兩者都**不保存** COM Port、TCP Host／Port、Connect Timeout 或執行模式。

```json
{
  "sequence_version": 1,
  "transport": {
    "type": "serial",
    "settings": { "baud_rate": 115200, "data_bits": 8, "parity": "none", "stop_bits": 1, "flow_control": "none", "timeout_ms": 1000 }
  },
  "steps": [
    { "id": "send-frame", "type": "send_bytes", "hex": "0055aaff0d0a" },
    { "id": "read-frame", "type": "read_until", "delimiter_hex": "0d0a", "max_bytes": 64 }
  ]
}
```

TCP 則用 `"transport": {"type":"tcp","settings":{"io_timeout_ms":1000}}`。CLI TCP Sequence Run 的 I/O Timeout 以 Sequence JSON 為準，不另由 CLI 選項設定。驗證不開 COM Port、TCP Socket 或 DNS；執行時另提供實際設備資源與模式。

```sh
serial-tool sequence validate --file sequence.json
serial-tool sequence run --file sequence.json --mode live --port COM4
serial-tool sequence run --file sequence.json --mode simulate --simulation-profile-id loopback-v1 --json
serial-tool sequence run --file tcp-sequence.json --transport tcp --mode live --host 127.0.0.1 --tcp-port 5025
serial-tool sequence run --file tcp-sequence.json --transport tcp --mode simulate --simulation-profile-id loopback-v1
```

## Worker

```sh
serial-tool worker --mode live --port COM4 --baud 115200 --control-port 0
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --control-port 0
serial-tool worker --transport tcp --mode live --host 127.0.0.1 --tcp-port 5025 --control-port 0
serial-tool worker --transport tcp --mode simulate --simulation-profile-id loopback-v1 --control-port 0
serial-tool worker --transport tcp --mode simulate --simulation-profile-id periodic-rx-v1 --streaming --control-port 0
```

Worker 使用 Common v3.0、`v3-only`；HTTP／Runtime JSONL 的 Schema 為 3，Manifest／一般 CLI JSON 為 2。`manifest --json` 宣告 `multi_job`、`startup`、`common.async_jobs` v1，並可選用 `common.streaming` v1。普通 Worker 的五個非同步命令為 `send`、`send-file`、`receive`、`query`、`run-sequence`；同時只有一個 Active Job，不額外排隊、並行或提供通用 Cancel。Streaming Worker 僅接收前兩種 Job。

Worker 以 JSONL 寫入 stdout，HTTP 控制面只綁 `127.0.0.1`；`ready` 提供端點 URL。Live Worker 只保持一條 COM 或 TCP 連線，`/stop` 關閉 Session，不發送設備特定命令。`POST /command` 的 `send-file` 使用 Worker 主機上可存取的**絕對路徑**：

```json
{"schema_version":3,"command":"send-file","arguments":{"path":"C:\\TestData\\payload.bin"},"job_id":"send-file-001"}
```

Worker 每次以最多 4 KiB 分塊處理 Raw File，回報 `file_bytes` 與已確認的 `bytes_written`，不輸出整份 Payload Hex。`/stop` 合作式停止尚未發送的分塊，不撤銷已完成的傳送；Simulation 會在記憶體保留 Loopback Bytes，宜使用小檔案。

HTTP `202` 只表示 Job 已受理；完成需觀察 `job_finished` 的 `state`、`ok`、`result` 與 Error，`GET /status` 的 `last_job` 亦提供最後結果。`POST /stop` 回傳 `200`／`accepted` 並非 Worker 已結束，應確認 Summary、stdout EOF 與子程序 Exit。`run-sequence` 使用內嵌 Sequence，設定必須匹配 Worker 啟動值。完整格式請以 [Worker Contract](../contracts/serial-worker-contract.md) 與 [Orchestrator Workflow Contract](../contracts/serial-orchestrator-workflows.md) 為準。
