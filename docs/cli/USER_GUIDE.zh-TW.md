# Serial Tool CLI 使用指南

本指南提供已安裝 `serial-tool` CLI 的日常操作方式。進階選項請見 CLI README，各指令接受的參數以 `serial-tool <command> --help` 為準。可使用 `serial-tool user-guide` 開啟英文離線指南，或使用 `serial-tool user-guide --lang zh-TW` 開啟繁體中文指南。

## 1. 啟動與確認 CLI

在已建置執行檔的目錄中操作；開發時也可從 Repository 根目錄使用 `cargo run -p serial-tool-cli -- <command>`：

```sh
serial-tool --help
serial-tool --version
serial-tool list-ports
serial-tool manifest --json
```

`list-ports` 只列舉本機 COM 埠，不開啟設備。`manifest --json` 用於機器整合，不是實體設備探測。

## 2. 選擇 COM 或 TCP

UART 是預設傳輸；實機操作必須選擇正確 COM 埠與鮑率：

```sh
serial-tool send --port COM4 --baud 115200 --text "STATUS?"
```

TCP 僅是原始位元組串流 **Client**；指定 Host／IP 與設備 Port，不要輸入 HTTP URL：

```sh
serial-tool send --transport tcp --host 127.0.0.1 --tcp-port 5025 --hex "AA 55"
```

UART 預設 8 資料位元、無同位檢查、1 停止位元、無流量控制及 1000 ms 逾時；TCP 預設 3000 ms 連線逾時、1000 ms 共用 I/O 逾時。不可混用 UART／TCP 專用參數。TCP 不負責封包切分或自動重連。

## 3. 傳送、接收、查詢與 Dry-run

```sh
serial-tool send --port COM4 --baud 115200 --hex "00 55 AA FF 0D 0A"
serial-tool send --port COM4 --baud 115200 --file payload.bin
serial-tool receive --port COM4 --baud 115200 --max-bytes 256
serial-tool query --port COM4 --baud 115200 --text "STATUS?" --until-text "OK" --max-bytes 1024
serial-tool send --port COM4 --baud 115200 --hex "AA 55" --dry-run --json
```

`send` 的 `--text`、`--hex`、`--file` 必須擇一；`query` 需指定一種 Text／Hex Payload 與一種 Text／Hex Delimiter，不支援檔案。`--text` 原樣傳送 UTF-8，**不自動加 CR／LF**；使用 Hex 可指定非文字位元組。Raw File 不改變檔案內容。Dry-run 僅驗證，不開啟 COM 或 TCP。

`receive`、`query`、`sequence run` 的 `--rx-display hex|text|both` 只影響人類可閱讀的呈現；JSON／JSONL 仍保持原始位元組機器欄位。輸出與 Exit Code 詳 CLI Machine Contract。

## 4. 互動式終端機

```sh
serial-tool terminal --mode live --port COM4 --baud 115200
serial-tool terminal --transport tcp --mode live --host 127.0.0.1 --tcp-port 5025
serial-tool terminal --mode simulate --baud 115200 --simulation-profile-id loopback-v1
```

Terminal 保留單一連線、持續接收 RX，也可由鍵盤輸入 TX。按 Enter 送出一行；透過 `--tx-format text|hex`、`--line-ending none|lf|crlf`、`--rx-display` 和 `--tx-display` 控制資料與畫面。Ctrl+C 或 EOF 可離開。

若只希望把 RX 文字／Hex 呈現重新導向至檔案：

```sh
serial-tool terminal --mode live --port COM4 --baud 115200 --rx-display stream --tx-display off > rx.txt
serial-tool terminal --mode live --port COM4 --baud 115200 --rx-display hex-stream --tx-display off > rx.hex
```

`stream` 輸出連續 RX 文字，`hex-stream` 輸出以空白分隔的大寫 Hex；生命週期與錯誤輸出到 stderr。這些是**顯示用串流**，不是 Raw Binary Capture 或機器 JSONL 協定。

## 5. 週期性自動傳送

```sh
serial-tool auto-tx --mode live --port COM4 --baud 115200 --hex "AA 55" --interval-ms 500
serial-tool auto-tx --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --file payload.bin --interval-ms 1000
```

指定一種非空的 Text／Hex／File Payload 與正整數毫秒間隔。第一筆立即送出，下一次間隔從前一筆傳送結束後起算。檔案在連線前讀取一次，過程中持續接收 RX，按 Ctrl+C 結束。`auto-tx` 不提供 Dry-run 或 JSON／JSONL 模式。

## 6. Sequence 檔案

Sequence 是包含 `sequence_version: 1`、`transport.type/settings` 與有序步驟的 JSON；步驟為 `send_text`、`send_bytes`、`wait`、`read`、`read_until`、`repeat`。例如建立 `sequence.json`：

```json
{
  "sequence_version": 1,
  "transport": {
    "type": "serial",
    "settings": { "baud_rate": 115200, "data_bits": 8, "parity": "none", "stop_bits": 1, "flow_control": "none", "timeout_ms": 1000 }
  },
  "steps": [
    { "id": "send-1", "type": "send_bytes", "hex": "aa550d0a" },
    { "id": "read-1", "type": "read_until", "delimiter_hex": "0d0a", "max_bytes": 64 }
  ]
}
```

```sh
serial-tool sequence validate --file sequence.json
serial-tool sequence run --file sequence.json --mode live --port COM4
serial-tool sequence run --file sequence.json --mode simulate --simulation-profile-id loopback-v1 --json
```

TCP Sequence 改用 `"transport":{"type":"tcp","settings":{"io_timeout_ms":1000}}`，執行時指定 `--transport tcp --host HOST --tcp-port PORT`。Sequence 不儲存設備資源或執行模式；Validate 不會開啟連線。正式格式詳 Sequence Contract。

## 7. Worker 與 Streaming

若要讓其他程式透過持續運作的程序操作：

```sh
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id loopback-v1 --control-port 0
serial-tool worker --transport tcp --mode simulate --simulation-profile-id loopback-v1 --control-port 0
```

Worker 提供僅本機可用的 HTTP 控制面及 JSONL Event。普通 Worker 支援 `send`、`send-file`、`receive`、`query`、`run-sequence`，同時只有一個 Active Job。若要讓模擬設備主動產生 RX，需明確開啟 Streaming：

```sh
serial-tool worker --mode simulate --baud 115200 --simulation-profile-id periodic-rx-v1 --streaming --control-port 0
```

`periodic-rx-v1` **只供 Streaming Simulation Worker** 使用，不迴送 TX。Streaming Worker 只接受 `send`、`send-file`；RX Event 是原始位元組區段，不保證對應封包。HTTP 接受不代表 Job 完成；Worker Stop 接受也不代表程序已結束。詳 Worker Contract。

## 8. 常見問題及限制

- **收不到資料：** 確認 COM 埠／TCP 目標、鮑率與線路設定，以及設備實際回應格式；單次讀取逾時不必然表示斷線。
- **文字顯示異常：** 切換 Hex；原始設備位元組才是依據，無效 UTF-8 在顯示時可能被替代。
- **行尾錯誤：** `send --text` 不會自動加入 CR／LF；可用明確 Hex 或互動式 Terminal 的 Line Ending。
- **傳輸參數不符：** TCP 不接受 UART 的 `--port`／`--baud`，UART 不接受 TCP 的 `--host`／`--tcp-port`。
- **沒有硬體：** Terminal、Auto TX、Sequence Run、Worker 可使用適用的 `loopback-v1`；單次操作則利用 Dry-run 驗證，並非模擬收發。
- **Sequence 無法執行：** 確認 JSON 的 Transport／Settings 與執行環境一致，且不使用舊的根欄位 `serial`。
- **Streaming 命令遭拒：** `receive`、`query`、`run-sequence` 只供非 Streaming Worker。

Serial Tool 不會自動重送失敗資料、解碼設備協定或建立 Capture 資料庫。詳細指令與限制見 CLI README。
