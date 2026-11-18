# 次の作業指示書 v4 (2026-11-14)

> 対象: 実装担当のエージェント (**今回はフロントエンドも含めて全部**)。
> 前提資料: `AGENTS.md` → `docs/FRONTEND.md` → `docs/BACKEND_BRIEF.md` → `docs/PROTOCOL_NOTES.md`。
> 守ること (`AGENTS.md`): UI は caps だけで出し分ける / 契約 (`model.rs`) は追加のみ / `paint_backdrop` は
> `glass.rs` 経由 / Web 版のタブを開いたまま実機を触らない。

---

## 0. いまの完成度 (コードと docs を読んだ評価)

| 領域 | 完成度 | 根拠 |
|---|---|---|
| フロントエンド (全画面・元に戻す・プリセット・検索・テスト・キャリブレーション) | 9 割 | 画面はそろっている。今回の要望 (§1〜§5) と §6 の改善が残り |
| SU66 Max (xsyd) | 8〜9 割 | P0/P1 はほぼ実機検証済み (キーごと性能・RT・ポーリング・構成 4・高度キー・マクロ・デコライト・per-key 色・キャリブ・ストロークテスト)。高度キー/マクロ書き込みは実データでの検証待ち。互換モード・ライト連動・ファーム更新が未 |
| HTS Plus Pro (mouse.fit) | 7 割 | 有線・無線とも DPI/ボタン/マクロ/主要設定は実機検証済み。高速モード・デバウンス DIY・誤操作防止・組み合わせキー・センサー詳細・本体/レシーバーライト・プロファイル・リセット・ボタン校正・ファーム更新が未 |
| HTX Ultra | 未検証 | 機種表から登録しただけ |
| ATK F1 Ultimate | 0 | VID/PID 未確定 (mouse.fit の機種表に無い) |
| 本体側の変更の追従 (本体ボタンで DPI を変えた等) | 0 | Web 版はやっている (§6.1)。ここが無いと「上位互換」にならない |

**毎日使う機能は Web 版の 8〜9 割に到達。Web 版の全機能という意味では 7〜8 割。**
`docs/BACKEND_BRIEF.md` §2.2 (マウス) の「現状」列が古いので、作業の最初に実態へ更新すること。

## 作業の順番

1. §1 クリック感の削除 (小)
2. §3 サイドバーのバッテリー (中)
3. §2 磁気ボタンの押下深さのリアルタイム表示 (大・実機調査あり)
4. §5 フォント (中)
5. §4 ファームウェア更新の通知 (中)
6. §6 改善 (本体側の変更の追従を最優先)

各節の「完了条件」をすべて満たしたら、その節を done にして `AGENTS.md` の現状を更新する。

---

## 1. HTS Plus Pro の「クリック感」を消す

ユーザー要望: **「左ボタン クリック感」「右ボタン クリック感」は要らない。**

- `crates/devhub-hid/src/mousefit8.rs` の caps から `ms_tactile_left` / `ms_tactile_right` の `SettingSpec` を外す
  (803〜815 行付近)。読み取り (1011 行付近) と apply (1271 行付近) の対応も消してよい。
- 同じ内容が `crates/devhub-core/src/demo.rs` (デモの `mag_vibration` / `mag_sound`) にもあるので、デモからも消す。
- 他の磁気ボタン設定 (トリガー位置・クイックトリガー) は残す。

**完了条件**: 実機・デモとも「性能」タブの磁気スイッチのカードにクリック感 (振動・クリック音) の行が出ない。

## 2. 磁気ボタンの押下深さをリアルタイムで見る

ユーザー要望: **マウスのボタンをどのくらい押しているか (押下深さ) をリアルタイムで見たい。**

### 2.1 実機から取れるデータ (mouse.fit の JS から判明)

[mouse.fit](https://www.mouse.fit/) の通知処理から確認した report-8 の形式。
通知のオフセットは report ID を含むバッファの先頭から数える。

| 条件 | 内容 |
| --- | --- |
| オフセット 1 が 10 | 本体からの自発通知 |
| オフセット 6 が 1 / 2 / 64 | DPI / ポーリング / バッテリーの変化 |
| オフセット 6 が 0、4、8、16、32 | オフセット 14 が左、15 が右。下位 7 ビットが押下深さ（0〜100%） |

公式 UI の作動点は 5% 刻みで、読み出したトリガー値に 1 を足した段階を表示する。

feature-report 系にも同種の通知がある (`ka` ハンドラ、report id 4: `04 01/04` DPI、`04 03` バッテリー、
`04 02` プロファイル、`04 06 00/01` 切断/接続、`04 07` LOD、`04 08` ポーリング、`04 0f` MotionSync、`04 18` センサー)。

**まず実機で確かめること** (`crates/devhub-hid/examples/` に `btnprobe.rs` を作る):
1. 何も送らずに input report を読み続け、左右クリックで op 10 の `r[14]/r[15]` が変わるか。
2. 変わらなければ、`StartCalibr(46)` の後、または mouse.fit の磁気ボタン画面を開いたときに送っているコマンドの後で変わるか
   (WebHID フック `docs/ADD_DEVICE.md §2.3` で mouse.fit の通信を記録すると確実)。
3. 有線 (5219) とドングル (3854) の両方で。結果を `docs/PROTOCOL_NOTES.md` に書く。

### 2.2 契約の追加 (`crates/devhub-core`)

```rust
// model.rs
/// 押下深さを返せるボタン 1 個分 (磁気スイッチなど)。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonDepth {
    /// 0.0 (離している) 〜 1.0 (底まで)。
    pub depth: f32,
    /// 作動点 0..1 (分かれば)。UI はこの位置に線を引く。
    pub trigger: Option<f32>,
    /// デバイスが「押下」と判定しているか (分かれば)。
    pub pressed: Option<bool>,
}

// MouseCaps に追加
/// 押下深さを返せるボタンの slot 番号 (`buttons` の index)。空なら UI は出さない。
pub live_buttons: Vec<usize>,

// backend.rs の DeviceDriver に追加 (既定は Unsupported)
/// `MouseCaps::live_buttons` と同じ並びで、最新の押下深さ。UI がテスト画面で 60Hz で呼ぶ。
/// 通知 (push) で届くデバイスは、受信スレッドが保存した最新値を返すだけにする (ここで HID を叩かない)。
fn live_buttons(&self) -> HubResult<Vec<ButtonDepth>> { Err(HubError::Unsupported("live_buttons")) }
```

`MouseCaps` を struct リテラルで作っているドライバ・デモは `live_buttons: vec![]` を足す。
デモ (`demo.rs` の磁気ボタン付き HTX Ultra) は sin 波で深さを返す。

### 2.3 バックエンド (`mousefit8.rs`)

- **受信スレッド**: コマンドの応答待ちと通知が同じ input report に混ざる。1 本の受信スレッドで全 input report を読み、
  「応答 (op がコマンドと一致)」はチャネルでコマンド側へ、「通知 (op 10 / report 4)」は通知処理へ振り分ける。
  今の `cmd()` が自分で read している作りを、この形に直す。
- 通知で受けた L/R の深さを `Mutex<[ButtonDepth; 2]>` に保存し、`live_buttons()` はそれを返す。
  `trigger` は `ms_trigger_left/right` の値 (5% 刻みの段階 → 0..1) から入れる。
- 通知が届かない機種・状態では `live_buttons` を caps に宣言しない。

### 2.4 フロントエンド

- `store.rs`: いまのストロークテスト (`Job::Travel`) と同じ仕組みで、マウスでは `live_buttons()` を 60Hz (16ms) で読む。
  テストタブを開いている間だけ ON、離れたら OFF (いまの `set_travel` と同じ扱い)。
- `pages/test.rs` のマウスのテストに **「ボタンの押下深さ」カード** を追加:
  - 左右 2 本の縦バー (0〜100%)。作動点に線、作動中はバーをアクセント色、未作動は灰色。
  - 現在値 (%)・その操作での最大値・作動した回数。
  - 直近 5 秒の深さの折れ線 (左右 2 本)。`canvas` + `PathBuilder` で描く。
  - クリック回数の既存カードと並べる (同じ「テスト」タブ)。
- 「性能」タブの磁気スイッチのカードにも、トリガー位置スライダーの横に小さなライブバー (左右) を出す
  (動かしながら作動点を決められる。Web 版には無い使い方)。

**完了条件**: HTS Plus Pro (有線・無線) で左右ボタンをゆっくり押すとバーが追従し、作動点を越えると色が変わる。
離すと 0 に戻る。テストタブから離れると読み取りが止まる (CPU が下がる)。

## 3. バッテリーをサイドバーで見る

ユーザー要望: **バッテリーのデータがあるデバイスは、左側の一覧で残量を見られるように。**

### 3.1 今の問題 (コードを読んだ結果)

1. `HubBackend::scan` は `battery: None` を返し、`store.rs` の `Msg::Scanned` が 2 秒ごとに `self.devices = list` で
   一覧を丸ごと置き換えるので、取れたバッテリー値が次の scan で消える。
2. `Hub::poll_battery` は「開いていて ready かつ `info.battery.is_some()`」のセッションしか問い合わせない。
   開いていないデバイスや、`info()` が battery を埋めないドライバでは一度も読まれない。

### 3.2 直し方

- **契約**: `Capabilities` に `pub battery: bool` を追加 (バッテリーを持つ機種で true)。
  ドライバの struct リテラルは `..Capabilities::empty()` を使うか、項目を足す。
- **store.rs**:
  - `Msg::Scanned` で、前回の一覧とセッションにあった `battery` / `asleep` を同じ ID に引き継ぐ (None で上書きしない)。
  - 起動時と挿抜時に、`supported` な全デバイスのセッションを**裏で開く** (画面は開かない)。caps に
    `battery: true` があるものを 60 秒ごとに `battery()` で問い合わせる (今の 30 秒間隔の仕組みを流用)。
  - ドライバが `HubEvent::Battery` を送ってきたら即反映 (mouse.fit は op 10 の `b == 64` / `04 03` で通知が来る — §2.3 の受信スレッドで拾う)。
- **バックエンド**: `mousefit8.rs` は `battery: true` を caps に入れ、`battery()` が寝ているときは `Err` ではなく
  最後に取れた値を返し、`DeviceInfo::asleep` を立てる。
- **UI (サイドバー `app.rs::side_device`)**:
  - デバイス名の下の行に、電池アイコン (残量で 4 段階) + `85%`。充電中は稲妻アイコンと「充電中」。
  - 20% 以下は橙、10% 以下は赤。スリープ中は「スリープ中 · 最後 85%」と薄く。
  - 値をまだ一度も取れていないデバイスには何も出さない (`—` も出さない)。
  - ホームのデバイスカードも同じ表示にそろえる。

**完了条件**: アプリを起動しただけで (デバイス画面を開かなくても) HTS Plus Pro の残量がサイドバーに出て、
2 秒ごとの scan で消えない。充電ケーブルを挿すと 60 秒以内 (通知があれば即時) に充電中表示になる。

## 4. 公式ファームウェアの更新通知

ユーザー要望: **公式のファームウェア更新が出たら通知してほしい。** (自動で書き込むのではなく、知らせるだけ)

### 4.1 公式の配布元 (調査済み)

| 系統 | どこを見るか | 新しいかどうかの判定 |
|---|---|---|
| mouse.fit (G-Wolves) | `https://www.mouse.fit/Config/<UIFolder>/FWLINK.txt` — 中身は zip の URL 1 行。2026-11-14 時点: HTS Plus Pro (`JM_052_3955`) → `https://www.mouse.fit/firmware/10-052-HTS_PLUS_Pro_8K/3955/HTS_PLUS_Pro_8K_3955JM.zip`、HTX Ultra (`GW_056`) → `https://www.mouse.fit/firmware/HTX_Ultra_54XG.zip`、(`JM_056_3955`) → `.../02-056-htxultra/3955/htxultra-3955JM.zip`、(`JM_056_3950`) → `.../02-056-htxultra/3950/htxultra-3950JM.zip` | zip のファイル名にバージョンが無い。**FWLINK.txt の URL が変わった**、または zip の **ETag / Last-Modified / Content-Length** が変わったら「新しいファームウェアが公開された」とみなす。加えて `env-models.json` の `DeviceFWFile_00` / `Receiver4K8KFWFile_00` (今は空) が入ったら、そこからバージョン (`DeviceNewFWVer`) を出して本体 FW と比べる |
| xsyd / Sparklink (SU66 Max) | `GET https://api.sparklinkplayjoy.com/api/v1/getKeyboardFirmware` (SDK の `getFirmwarePack`)。引数は dshk の firmware 画面のチャンクで呼び出し元を探して確定する (`getFirmwarePack(` で grep。VID/PID/boardId 等のはず) | 応答のバージョンと `getDeviceInfo` の `appVersion` (`DeviceInfo::firmware`) を比べ、新しければ通知 |

UIFolder は `env-models.json` の機種行 (registry の model から引けるように、mousefit8.rs の機種表に持たせる)。

### 4.2 実装

- **契約**: `Capabilities` に `pub firmware_feed: Option<FirmwareFeed>` を追加。
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub enum FirmwareFeed {
      /// mouse.fit の FWLINK.txt (UIFolder 名)。
      MouseFit { ui_folder: String },
      /// Sparklink API (問い合わせに必要な値)。
      Sparklink { vid: u16, pid: u16, board_id: Option<u32> },
  }
  ```
- **新クレート `crates/devhub-update`** (GUI 依存なし・テストしやすく): `fn check(feed, current_fw, last_seen) -> Result<UpdateStatus>`。
  HTTP は `ureq` (参照実装 と同じ `ureq = { version = "2", features = ["json", "native-tls"] }`)。
  タイムアウト 5 秒。オフラインは黙って失敗。結果:
  ```rust
  pub struct UpdateStatus { pub new_available: bool, pub version: Option<String>,
                            pub url: Option<String>, pub fingerprint: String, pub checked_at: u64 }
  ```
- **状態の保存**: `%APPDATA%\DeviceHub\firmware.json` に「機種ごとの最後に見た fingerprint・通知済みか」。
  **初回は基準として記録するだけで通知しない** (起動しただけで「更新あり」と出さない)。
- **頻度**: 起動時に、前回から 24 時間以上たっていれば裏スレッドで確認。アプリ設定に
  「ファームウェアの更新を確認する」(既定 ON) と「今すぐ確認」ボタン。
- **通知の出し方**:
  - トースト「HTS Plus Pro の新しいファームウェアが公開されています」。
  - サイドバーのデバイス行に小さな点 (バッジ)。
  - そのデバイスの「一般」タブ上部にカード: バージョン (分かれば)・確認日時・「公式ページを開く」
    (既定ブラウザで URL を開く。`open::that` 等) と「この通知を消す」。
  - Windows の通知センターにも出すオプション (`notify-rust`、参照実装 で使用実績あり)。既定 OFF。
- ファームウェアの書き込み自体は `docs/BACKEND_BRIEF.md` の P3 のまま (ここではやらない)。

**完了条件**: firmware.json の fingerprint を手で書き換えて「今すぐ確認」→ 通知・バッジ・カードが出る。
オフラインでもエラー表示が出ずに普段どおり動く。24 時間以内の再起動では問い合わせない。

## 5. フォントを変える

ユーザー要望: **今のフォント (Yu Gothic UI) がダサいので変えたい。**

### 5.1 方針

- フォントを**同梱**し、アプリ設定で選べるようにする (プレビュー付き)。既定も変える。
- 参照実装: 参照実装の `src/main.rs` の `BUNDLED_FONT_Z` / `bundled_font` / `register_ui_font`
  (zlib 圧縮した TTF を exe に入れ、選んだ書体だけ展開して `cx.text_system().add_fonts` で登録)、
  `src/ui.rs` の `FONTS` 一覧、圧縮ツール `tools/compress-fonts.py`。同じ仕組みをそのまま移植してよい
  (`flate2` を依存に足す)。フォントファイルとライセンスも `assets/fonts-z/`・`licenses/` から流用できる。
- 日本語の字形が無い欧文フォント (Geist 等) は、和文フォントへのフォールバックを必ず設定する
  (参照実装 の `ui_font()` / `native_font_family()` の扱いを確認)。

### 5.2 候補と既定

| 表示名 | 中身 | 用途 |
|---|---|---|
| **モダン (既定)** | 英数字 **Geist** + 日本語 **Noto Sans JP** (または IBM Plex Sans JP) | Liquid Glass に合う細身のサンセリフ。Apple 系の見た目に一番近い |
| 丸ゴシック | Geist + Zen Maru Gothic | 柔らかい |
| M+ 丸ゴシック | M PLUS Rounded 1c | 丸く軽い |
| UD ゴシック | BIZ UDPGothic (Windows 標準) | 読みやすさ重視 |
| 游ゴシック UI | Yu Gothic UI (Windows 標準、今の既定) | 従来どおり |
| メイリオ UI | Meiryo UI (Windows 標準) | |

- Noto Sans JP / IBM Plex Sans JP は OFL。可変フォントか Regular/Medium/Bold の 3 ウェイトに絞り、圧縮して同梱
  (exe の肥大を抑える。参照実装 は全書体で約 11MiB を常駐)。
- 大きな数字 (DPI・バッテリー%・テストの数値) は Geist の等幅数字 (tabular) を使う (`ui::big_number`)。
- `theme.rs` の `FONT` / `FONT_DISPLAY` を定数から「設定から決まる値」に変え、`prefs.rs` に `font: String` を追加。
- アプリ設定の「見た目」カードに「フォント」行: ドロップダウン + 「あいう ABC 1600 DPI」のプレビュー。変更は即時反映。

**完了条件**: 既定がモダン (Geist + Noto Sans JP) になり、日本語・英数字とも豆腐 (□) にならない。
設定で切り替えると全画面に即反映され、再起動後も保持される。

---

## 6. 改善案 (Web 版より便利にする)

優先順。各項目の担当範囲はバックエンド (B) / フロント (F)。

1. **本体側の変更に追従する (B+F・最優先)**: 本体の DPI ボタン・プロファイル切替・LOD/ポーリング変更・充電開始・
   スリープ/復帰を通知 (op 10、report 4: §2.1 の一覧) から拾い、`HubEvent::StateChanged` / `Battery` を送る。
   UI は該当部分を読み直し、DPI 画面の「使用中」ステージが即座に動く。今は本体ボタンで変えても画面が古いまま。
2. **ゲーム連動プリセット (F)**: 前面のプロセス (例 `VALORANT-Win64-Shipping.exe`) を 2 秒ごとに見て、
   紐づけたプリセットを自動適用 (一般タブで「このプリセットを使うアプリ」を登録)。ゲームを閉じたら元に戻す。
   Web 版には無い。`windows` crate の `GetForegroundWindow` + `QueryFullProcessImageNameW`。
3. **起動の軽さ (F)**: タスクトレイに入れて常駐する設定 (既定 OFF) と、「設定したら閉じる」運用のための
   起動時間短縮 (初回の列挙と描画を 1 秒以内)。
4. **HTX Ultra・ATK F1 の実機対応 (B)**: `docs/BACKEND_BRIEF.md` §2.2/§2.3。
5. **マウスの残り機能 (B)**: 高速モード、デバウンス DIY 4 値、誤操作防止、組み合わせキー、センサー詳細、
   本体/レシーバーのライト、オンボードプロファイル、リセット、磁気ボタン校正。
6. **docs の整備 (B)**: `BACKEND_BRIEF.md` §2.2 の現状列を実態に更新、検証日を記入。

## 7. 全体の完了条件

- §1〜§5 の完了条件をすべて満たす (実機は HTS Plus Pro と SU66 Max、デモでも全画面が動く)。
- `cargo clippy --workspace --all-targets` 警告 0、`cargo test` が通る。
- `docs/FRONTEND.md` (新しい画面・部品)、`docs/PROTOCOL_NOTES.md` (op 10 / report 4 の通知・ファーム配布元)、
  `AGENTS.md` (現状) を更新。
