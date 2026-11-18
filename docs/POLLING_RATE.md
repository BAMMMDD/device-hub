# 「ポーリング実測」の検証と改善案 (2026-11-16)

症状: HTS Plus Pro を 8K に設定しているのに「ポーリング実測」が ~2700Hz。

## Windows でゲームを前面にしたまま測る (2026-11-18)

Device HUB のテスト画面は Windows がアプリへ配信したイベントを数える。背面のマウス入力は OS がまとめて配信するため、受信件数から元の転送件数を復元できない。USB 有線マウスと USB ドングルでは、USBPcap でマウス移動用の interrupt IN エンドポイントの転送完了を数えると、ウィンドウのフォーカスに依存せず比較できる。ドングルで測れるのはドングルから PC への転送。Bluetooth はこの手順の対象外。[Microsoft の背景説明](https://blogs.windows.com/windowsdeveloper/2023/05/26/delivering-delightful-performance-for-more-than-one-billion-users-worldwide/)・[USBPcap の手順](https://desowin.org/usbpcap/tour.html)

### アプリから測る (USBPcap のみ必要 — Python/TShark 不要)

計測器は `crates/devhub-polling` (USBPcap ドライバを直接開く Rust 実装)。OBS の「Device HUB USB Polling Rate」ソースと同じ値が出る。実機検証済 (2026-11-18, G-Wolves Receiver RS 8K @ USBPcap2/addr2/EP0x81 — 連続移動で ~7,800Hz・ピーク 7,923Hz)。管理者権限が必要。

1. [USBPcap](https://desowin.org/usbpcap/) を導入して再起動する (初回のみ)。ハブの読み書きオープンには管理者権限が要る環境がある — 「権限不足」と出たら Device HUB を管理者として起動し直す。
2. デバイスの「テスト」タブ →「ポーリング実測」→ 計測方式を **USB 転送 (USBPcap)** にする。
3. 「対象デバイス」の一覧から製品名で選ぶ — ハブ番号と USB アドレスは自動で入る (列挙は権限不要・`IOCTL_USBPCAP_GET_HUB_SYMLINK` と構成マネージャで対応付ける。抜き差し・ポート変更後は「更新」で再スキャン)。IN EP は「EP 自動検出」で対象を動かしながら 3 秒数えて最も多いものを採用できる。分からない場合や手動指定したい場合は下の詳細フィールド (ハブ番号・USB アドレス・IN EP 番号 0x81→1・Report ID -1=全件) で直接入れられる。診断用に `cargo run --release -p devhub-polling --example usbenum` (列挙、権限不要) / `usbscan <hub> [秒]` (ハブ全体スキャン、要管理者) / `usbfind <hub> <addr> [秒]` (EP 検出、要管理者) の各プローブも残してある。アドレスは抜き差しで変わる。
4. 別アプリを前面にしたまま数秒間動かす。「期間内の最高」は受信ごとに (t−1秒, t] の件数を更新するので、表示更新の間の山も取りこぼさない。「停止」で終わると対象・計測元・最高値が残る。同じハブを同時に掴めるのは 1 プロセス (OBS の USB ソースや TShark と同時計測は不可 — 「ハブ使用中」と出る)。

### TShark スクリプトで測る (比較用の別経路)

1. Windows で Wireshark の TShark と USBPcap を用意する。Wireshark のキャプチャ設定で、対象マウスが接続された USBPcap インターフェースと USB デバイスアドレスを調べる。短く試し撮りし、移動に連動する HID マウスの interrupt IN を選ぶ。パケット詳細からバス番号とエンドポイントアドレスを読む。再接続後はデバイスアドレスを再確認する。
2. 下記の値を実機のものに置き換えて実行し、ゲームに戻って数秒間マウスを動かす。インターフェース名は `tshark -D` の USBPcap の名前。キャプチャ権限が必要な環境では管理者 PowerShell を使う。

   ```powershell
   .\scripts\measure-usb-rate.ps1 -Interface '\\.\USBPcap1' -Bus 1 -Device 5 -Endpoint 0x81 -Seconds 10
   ```

3. 終了時の表で、動かし続けた **丸 1 秒の行の Frames** を読む。例えば 7,950 件ならその秒の実測は 7,950 転送/秒。先頭・末尾の端数区間は除く。ゼロが続く場合は、選んだアドレスとエンドポイントのパケットが試し撮りにあるか確認する。フィルタは成功・データあり・受信完了を条件にする。同じエンドポイントに複数の Report ID が混在する機種では合計値になる。アイドル時の USB ポーリング試行や無線区間の周期は観測できない。

Wireshark で対象デバイスに限定して保存した USBPcap キャプチャも `-CaptureFile .\mouse.pcapng` を `-Interface` の代わりに指定して解析できる。前面と背面を比較するときは DPI・設定レート・動かし方を揃える。[Wireshark の USB フィールド](https://www.wireshark.org/docs/dfref/u/usb.html)

アプリ内の配信間隔は µs 単位で記録し、履歴表示は直近 60 バケットに制限する。まとめ読みした入力にはバッチ単位の時刻が付くため、中央値は参考値。連続区間の判定は 125 Hz (8 ms 間隔) を含めるため 12 ms まで許容する。以下は 11 月 16 日時点の調査記録。

## 1. 今の計測の仕組み
- `devhub-obs/src/rawinput.rs`: メッセージ専用ウィンドウで Raw Input (マウス/キーボード) を `RIDEV_INPUTSINK` 受信し、
  `WM_INPUT` 1 通ごとに `GetRawInputData` → hDevice ごとに時刻を記録。
- 表示値 = `rawinput::hz()` = **直近 1.0 秒に届いた WM_INPUT の個数**。`store.rs` の `Job::PollRate` が ~400ms ごとに取得。
- 参考: `rawinput::gap_stats()` (間隔の中央値/p95) と `min_gap_ms()` も計算済みだが、大きな数字には使っていない。

## 2. 結論: 「届いた入力レポート数/秒」としては正しいが、「ポーリングレート」としては過小に出る

### 原因 A (最大の要因): マウスは「動きがあるときだけ」レポートを送る
8K ポーリングでも、125µs の間にセンサーのカウント (移動量) が 1 以上溜まらなければレポートは送られない。
よって **届くレポート数 ≦ カウント/秒 = 移動速度 × DPI**。

| DPI | 8000 レポート/秒に必要な速度 |
| --- | --- |
| 400 | 50.8 cm/s |
| 800 | 25.4 cm/s |
| 1600 | 12.7 cm/s |
| 3200 | 6.4 cm/s |

例: 800 DPI で 10 cm/s 動かすと 約 3150 カウント/秒 → 上限 約 3150Hz。**2700Hz は「DPI と速度で頭打ち」の典型値**で、
8K が効いていないとは限らない。MouseTester 等も同じ制約がある (高 DPI で速く円を描いて計測するのが定石)。

### 原因 B: 1 秒の固定窓に止まっている時間・減速中が混ざる
円を描く途中の減速や折り返しも分母に入るので、ピークより必ず低く出る。

### 原因 C: WM_INPUT を 1 通ずつ処理している
8000 通/秒の dispatch + 毎回の Vec 確保 + Mutex で、高負荷時に取りこぼしや dispatch 合体の余地がある。
`GetRawInputBuffer` でまとめて読むのが Microsoft 推奨の高頻度向け手順。
また時刻は「到達時刻」ではなく「処理時刻」なので、間隔統計はジッタを含む。

### 原因 D (前面/背面): Windows 11 は背面プロセスへの raw input を制限する
→ 既に UI で「背面のため OS が入力を制限中」と出し分け済み。前面で計測すること。

### 別件: OBS オーバーレイの「8K Hz」表示も別物
`mousefit8::live_report_hz()` は **ベンダー IF の op10 / report4 通知の数** を数えている (`rate_tick`)。
マウス移動のレポートではないので「ポーリングレート」として表示するのは誤り。OBS 側は rawinput か表記変更を。

## 3. 改善案 (優先順)

### P1 表示を「ポーリングレート」として意味のある値に (フロント + rawinput.rs)
1. **連続して動かした区間だけで測る**: 間隔が 2ms を超えたら「止まった」とみなして区間を切り、
   区間内の (レポート数 / 区間長) を出す。100ms 以上続いた区間の最大値を「ピーク」、直近区間を「現在」。
2. **中央値間隔からの推定**: `1000 / gap_stats().median` (0.05ms 未満は除外済み) を「推定ポーリング周期」として併記。
   8K が効いていれば中央値は ~0.125ms (= 8000Hz) 付近に出る — 速度/DPI 不足でも、来ているレポートの間隔は 0.125ms の倍数になる。
3. **頭打ち診断**: RAWMOUSE の `lLastX/lLastY` から「1 レポートあたりのカウント」を集計。
   ほとんどのレポートが ±1 カウントなら「DPI か速度が足りずレポートが間引かれています — DPI を 3200 以上にして速く大きく円を」と表示。
   ついでに `カウント/秒` を出せば「この速度・DPI での理論上限 ≒ ○○Hz」が示せる。
4. 大きな数字は「ピーク」、小さく「平均 (1 秒)」「推定周期 0.125ms」。グラフは 250ms バケットのままで良い。
5. 計測ボタン「5 秒間計測」: 押してから 5 秒の最大区間レートと中央値間隔を結果として固定表示 (MouseTester 的な使い方)。

### P2 計測の取りこぼし対策 (rawinput.rs)
- `WM_INPUT` 受信時に `GetRawInputBuffer` で溜まっている分をまとめて読む (1 回の syscall で複数 RAWINPUT)。
  バッファは使い回し、ロックは 1 バッチ 1 回にする (今は 1 イベントごとに Vec 確保 + Mutex)。
- イベント時刻は Vec<Instant> ではなく u64 (QPC tick) のリングバッファ (固定 16K 件) にして、UI 側の取得は
  集計済みの数値だけ返す (今は `devs_for` が 1 万件近い Instant/gap をロック中にコピーしている)。
- 可能なら `RAWINPUTHEADER` と一緒に `GetMessageTime` ではなく QPC を即時取得 (今と同じ) — 到達時刻ではない点は注記を維持。

### P3 OBS の Hz 表示
- `live_report_hz` を「通知レート」と明記するか、OBS プラグインでも rawinput (背面制限あり — OBS が前面でないと不正確) を使う。
  正確さが担保できないなら OBS 表示から Hz を外す方が安全。

## 4. ユーザー向けの計測手順 (UI のヘルプに出す文言案)
1. Device HUB を前面にする (背面だと OS が入力を間引く)
2. DPI を一時的に 3200 以上にする (計測後に戻す)
3. マウスパッド上で速く大きな円を 3〜5 秒描く
4. 「ピーク」と「推定周期」を見る (8K なら ~8000Hz / ~0.125ms)

## 5. 実装状況 (2026-11-16)

- **P1 (表示)** — 実装済。テストタブの「ポーリング実測」カード: 大きな数字はピーク
  (動いている区間の最大レート)、サブ行に平均(1秒)と現在区間、推定周期 (間隔中央値) 行、
  「移動量 X/秒 → 上限 ≒ X Hz」行 + ほぼ1カウントのとき DPI/速度不足ヒント。
  「5 秒間計測」ボタン: 計測中はカーソルを窓内に ClipCursor 固定、終了で
  ピーク・間隔中央値・移動量を結果として固定表示。
- **P2 (取りこぼし対策)** — 実装済。`WM_INPUT` で `GetRawInputData` 読み後
  `GetRawInputBuffer` でキューをドレイン (Microsoft の高周波向け公式手順、
  バッファは使い回し・ロックは 1 バッチ 1 回)。イベントは u64 ms 時刻のリング、
  `stats()` が集計済みスナップショットを 1 ロックで返す。
- **P3 (OBS)** — 対応済 (Hz 表示はオーバーレイから撤去済み)。
- rawinput.rs API 変更: `hz/hist/min_gap_ms/gap_stats` → `stats() -> PollStats`
  (hz, hist, min_gap_ms, gap_stats, seg_hz, peak_hz, counts_per_sec, low_frac, segs)。
  `Session.poll: Option<PollStats>` が UI の窓口。検証は `cargo run -p devhub-obs --example rate`。
