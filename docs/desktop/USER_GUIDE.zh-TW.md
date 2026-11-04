# Serial Tool Desktop 使用指南

本指南說明目前已實作的 Windows Desktop 操作，適用於需要透過 COM 埠或原始 TCP 收發設備資料的使用者。

## 啟動與畫面

Windows 必須安裝系統 Microsoft Edge WebView2 Runtime；如果未偵測到，Serial Tool 會提供開啟 Microsoft 官方頁面的原生警告，**不會**自行安裝，且不建立應用視窗。使用已建置的 `Serial Tool.exe`，或在 `apps/desktop` 使用 `npm run tauri -- dev` 啟動開發版。

主要功能區域是**連線設定（Connection Setup）**、**終端機（Terminal）**及**序列（Sequence）**。先選傳輸類型、實機／模擬，再連線。Desktop 模擬僅使用固定 `loopback-v1`；`periodic-rx-v1` **只供 Streaming Simulation Worker**，不是 Desktop 選項。

## 語言與外觀

右上角語言按鈕顯示目前選用的 English／繁體中文，可立即切換，不重載畫面；手動選擇保存在本機 `serial-tool.locale`。首次開啟時參考 WebView 語言，其餘以英文備援。語言與外觀彼此獨立，外觀可選系統、淺色或深色。

語言切換不會更動連線、Terminal 位元組、Sequence 資料或任何匯出格式；Core／CLI／Worker 原始診斷及機器可讀輸出不翻譯。

## 離線說明

按右上角「**說明**」可於獨立視窗開啟內建離線使用指南，並沿用當下的介面語言（English／繁體中文）及外觀（系統／淺色／深色）。Help 視窗可以自行切換語言，不影響主視窗。若主視窗的語言或外觀後來變更，關閉再重新開啟 Help 即會套用。指南本身不需要網路；指向 GitHub 開發文件的連結是額外線上參考。

## 連線設定：COM 埠／TCP/IP

在連線設定分頁選擇 COM 埠或 TCP/IP。實機 COM 埠要選實體 Port；「重新整理 COM 埠」會列出 Port 與 USB Metadata，**不會**先開啟它們。依設備設定鮑率、資料位元、同位檢查、停止位元、流量控制與逾時。

TCP/IP 實機輸入 Hostname／IPv4／IPv6、設備 Port（1～65535）、Connect Timeout（預設 3000 ms）、共用讀寫 I/O Timeout（預設 1000 ms）。Host 和 Port 是不同欄位，不要填完整 HTTP URL。TCP 是 Raw Byte Stream **Client**，沒有 HTTP、封包 Frame、自動重連；Connect Timeout 不保證涵蓋 DNS 與全部候選位址嘗試的總時間。

連線設定的齒輪控制 COM 與 TCP **共用的**實機／模擬模式。模擬為確定性的 `loopback-v1`，不開實體 COM、DNS 或 TCP Socket。COM Port／TCP Host／Port 在模擬時停用，但原值會保留；TCP Timeout 仍須有效。

切換 COM／TCP 分頁會各自保留此次執行期間的輸入值，**關閉程式後不保存連線設定**。COM 數字欄位仍採原生數字語意但隱藏 Spin Buttons；TCP Port／Timeout 輸入時保留未完成文字，按「連線」再驗證。

Connecting、Connected、Running、Disconnecting 時鎖定傳輸、設定及模式；先中斷連線才能切換。齒輪選項可再按一次、點外側或按 Escape 收合（Escape 恢復焦點）；收合不會清除模式。Header 顯示 COM／TCP、模式及連線狀態；連線後兩種傳輸都自動持續接收 RX。

## 終端機：傳送及顯示

文字傳送會原樣輸出 UTF-8 位元組，**不自動加入 CR／LF**；Hex 可輸入大小寫十六進位配對與 ASCII 空白。Terminal 最多保留 **5000 筆** TX／RX Event 在記憶體，沒有永久 History。

顯示模式有事件（Hex、跳脫控制字元的 Lossy UTF-8 文字、兩者），以及連續 RX（文字／Hex）。切換顯示模式只重新呈現已保存的 Raw Bytes，不會傳送、重連，或更改 Send Text／Hex Format 與 Auto TX Payload。事件模式保持 TX／RX 各自成列，TX 列使用原來的 Hex。

「清除畫面」只清掉畫面 History，不影響裝置 Buffer 或連線。連續 RX 文字只顯示 RX，不加方向標籤或事件分行；LF 產生換行、CR 略過、其他控制字元跳脫，且可處理橫跨 RX Event 的 UTF-8 字元。這**不是** ANSI／VT100 終端機模擬器。

連續 RX Hex 把所有接收位元組以兩位大寫 Hex、每個 Byte 間恰好一個空白接續顯示；空 Event 不插入分隔。CR／LF／NUL 仍以 Hex 顯示。兩種連續 RX 都不含 TX Events、事件標籤或時間戳記。

「顯示 TX」預設開啟，關閉時只隱藏事件模式的 TX 列，不停止 TX 或清除 History。連續 RX 模式會暫時停用此控制但保留偏好；回到事件模式時仍適用。Sequence Result 的 RX 顯示只有 Hex／文字／兩者。

## 自動傳送（Auto TX）

來源預設「輸入內容」，使用與 Manual Send 共用的 Text／Hex Payload；切換來源不清除 Send 欄位。按「啟動自動傳送」後**立即**傳第一筆，再以正整數毫秒間隔重複，直到 Stop Auto TX 或 Disconnect。間隔從上一筆 Send 完成後起算，延遲週期不補送；持續 RX 不停止。

Auto TX 期間來源、Payload、格式、選定檔案與間隔被鎖定；Manual Send、Send File、Run Sequence 不可操作，但 Disconnect、顯示模式及 Clear View 仍可用。連線錯誤自動停止 Auto TX；其他 TX 請求由 Core 回報 Busy。

選 Input 須提供非空 Text／Hex，選 File (Raw) 則須有檔案，兩者不合併。File (Raw) 可選任意副檔名，選擇時只保存路徑；啟動時 Rust Backend **一次讀取整份檔案**的 Raw Bytes，重複發送固定 Snapshot，不做 UTF-8、換行或 Hex Text 轉換。

空檔、遺失或無法讀取的檔案會被拒絕，Auto TX 不啟動；執行中修改檔案不影響目前 Payload，停止後重新啟動才會重讀。Stop 不會中斷已開始的單次 Send。沒有分塊進度、Playlist、每週期重讀、檔案監視或 Mid-file Cancellation。

## 傳送檔案（Send File Raw）

Send File 由 Rust Backend 傳送選定檔案的**原始位元組**，拒絕空檔，不做 UTF-8、換行或十六進位文字解析。例如檔案內是文字 `AA 55`，實際傳送為 `41 41 20 35 35`，而非 `AA 55`。

傳送檔案期間其他 TX 和 Sequence 不可同時執行。此版本把整份檔案讀入記憶體，沒有 Progress／Cancellation／Chunk Delay／XMODEM／YMODEM。

## 終端機匯出

匯出是當下 History Snapshot，提供三種格式：

| 格式 | 匯出內容 |
| --- | --- |
| 終端機紀錄（Hex） | 人類可讀的大寫 Hex，每筆一行，例如 `TX 00 FF`、`RX 00 FF` |
| RX 原始資料（.bin） | 只串接 RX 位元組 |
| TX + RX 原始資料（.bin） | 依 History 順序串接所有 TX／RX 位元組 |

Raw 匯出不保留方向標籤、分隔符號、換行、事件邊界、時間戳記或其他 Metadata。預設檔名帶本機時間 `YYYYMMDD_HHmmss`。顯示模式或「顯示 TX」只影響畫面，不影響匯出；收到至少一個 RX Byte 才能使用 RX Raw。

Clear View 清除後無法匯出先前紀錄；超過 5000 筆而被移除的舊 Event 亦不能匯出。這不是持續 Capture Writer。TX + RX Raw 只是攤平的歷史位元組，**不是**可回放方向／時間的 Session Replay；若將其送回設備，原 RX Byte 也會作為 TX 發送。

## Sequence Editor 與結果

Editor 支援 **Send Text、Send Bytes、Wait、Read、Read Until、巢狀 Repeat** 六種步驟，Properties 顯示選定步驟說明。提供 New、Load、Save、Validate、Add Step、Add Child、Delete、Move Up、Move Down。編輯中的 Draft 可以暫時不合法，但 Validate／Save／Run 都透過 Core 的 Sequence v1 Parser 和驗證。

Sequence JSON 使用 `transport: {type, settings}`，舊的根欄位 `serial` 不再支援。UART 保存原六項線路設定；TCP **只保存 `io_timeout_ms`**，不保存 Host、Port 或 Connect Timeout。實際設備資源與模式另外在 Connection Setup 選擇。

New 依目前 COM／TCP 分頁建立空白 Draft；Load 保留檔案自己的 Transport，即使與目前分頁不同。Save、Validate 不需建立連線。「從連線設定複製」只有 Editor Transport 與 Connection Setup 分頁一致時能用，只複製 UART 六項設定或 TCP I/O Timeout。切換分頁或複製設定不會修改已建立的連線，亦不會自動轉換載入的 Sequence。

Editor 提示未連線、設定相符或具體 Transport／Settings 不相符。Run 要求同一傳輸、完全匹配的 UART 六項或 TCP I/O Timeout；不符合則停用 Run，保留連線和 Draft。Core 執行前也會重驗。

最近一筆結果只保存在記憶體，包含完成步驟、Raw-byte Transcript，以及失敗時可取得的 Partial Bytes。狀態與控制位於高度 250px 的 Step Results／Transcript 共用捲動區上方。新的 Run 從頂端顯示；切換 RX 顯示不重設 Scroll Position。

「儲存結果」匯出**完整 JSON**及原始 Byte Arrays，不受 Hex／文字／兩者顯示選擇影響。失敗結果也保留失敗步驟、Error、已完成步驟、Transcript 和可取得的 Partial Bytes；沒有結果則無法儲存。沒有持久 Run History 或自動結果目錄。

## 常見問題與限制

- **不能切換 COM／TCP：** 先中斷連線，連線中或執行中會鎖定設定。
- **文字亂碼：** 用 Hex 檢查原始位元組；文字是顯示方式，不是設備解碼器。
- **Text 傳送後沒有回覆：** 確認設備是否要求 CR／LF；Text 不會自動加入。
- **TCP 連線失敗：** 檢查 Host/IP 與 Port，不要填 HTTP URL；不會自動重連。
- **Sequence Run 不可使用：** 先 Validate，並核對目前連線 Transport／Settings。
- **匯出無資料：** 匯出僅使用目前 History，Clear View 與 5000 筆上限都會影響資料。
- **Auto TX 時無法 Send File／Run：** 先 Stop Auto TX。

本版本沒有永久 Run History、持續寫檔 Capture 或 Protocol Framing。
