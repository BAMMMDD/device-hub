# GPUI Device HUB 監査・作業引き継ぎ（2026-11-15）

DirectDrive の最新レビュー、修正、機能案は [DirectDrive 再監査](DIRECTDRIVE_AUDIT.md) に分離した。以下の D/G は以前の検証記録。

次のエージェントは **D「ライブ機能を停止・再接続できるようにする」の残り (OBS 側の再探索) と F の残り**から着手。A・B・C・E は実装済み (下記)。P1 は設定を壊すか復旧を妨げる問題、P2 は個別機能の誤動作、P3 は運用と保守の改善。今回の実行確認は Windows のデモと既存フィクスチャ。実機・OBS ホスト・ベンダー API は今回の検証範囲外。

## 今回修正した箇所

| 箇所 | 修正と確認方法 |
| --- | --- |
| [`store.rs` / `coalesce_jobs`](../crates/device-hub/src/store.rs) | 後続の単一 EQ バンド変更が先行する全バンド設定を消していた。間引きを連続する同一対象に限定し、全バンド → 単一バンドの両方を保持。読込・別変更・DirectDrive 停止をまたいでジョブを消さない。間引いた書き込みも `pending` の減算対象。回帰テスト3本。 |
| [`app.rs` / `stop_device_preview`](../crates/device-hub/src/app.rs) | ホーム・設定・別デバイスへ移動する際に、移動前のデバイスにライブ測定停止と DirectDrive 停止を依頼。従来の定期処理では移動後の選択先しか見ていなかった。 |
| [`app.rs` / `after_msg`](../crates/device-hub/src/app.rs) | 初回起動でデバイスを選んだ後、ホームに戻っても定期列挙でデバイス画面に戻された。復元候補を起動時の設定から一度だけ取得し、最初の列挙成功または手動操作で消費する。スレッドローカルの `AUTO` を撤去。 |
| [`devhub-obs/src/live.rs` / `loop_body`](../crates/devhub-obs/src/live.rs) | 描画側と共有する mutex を HID 応答待ちの間保持していた。ワーカーでフレームを作り、結果を公開するときだけロックする。 |

## 機能ごとの確認範囲

| 機能 | 今回確認したこと | 追加の受け入れ確認 |
| --- | --- | --- |
| 起動・ナビゲーション | デモホームで5台を表示し、ATK F1 Ultimate は「検出済み・ドライバ未実装」と表示。修正後の初回利用でホームに戻ると定期列挙後もそのまま。再起動では直前に開いた HTS Plus Pro を復元。 | 実機なし・HID 初期化失敗・一時的な列挙失敗からの復帰を、空の設定と既存設定で確認。 |
| マウス設定 | デモの HTS Plus Pro で電池表示と DPI ステージの変更（400→1200）を確認。既存テストで DPI ステージとボタン等の変換を検証。 | HTS Plus Pro の有線・無線、無線の休眠→復帰、別ドングルのレート候補、HTX Ultra の読み出しと復元可能な1項目の往復。 |
| キーボード | SU66 Max のキー割り当て画面と P1→P2 切替をデモで確認。Undo は A の手順で不具合を再現。既存フィクスチャはキーマップのコード変換・性能・高度キー・照明等をカバー。 | 実機では各プロファイルのキー割り当て、ライブ測定、マクロ、校正、照明プレビュー終了後のファーム点灯を個別に確認。 |
| TP55 / PEQ | デモで「低音ブースト」を適用。EQ カーブとプリアンプが変わり、「元に戻す」で両方が戻る（`2 項目`）。既存テストで APO テキストと PEQ フレームを検証。 | 実機でプリセット直後に1バンドを編集し、全バンドを読み戻す。音量と作業前の設定を確保してから確認。 |
| OBS・更新通知 | OBS はワークスペースのテスト実行時にコンパイル。更新側は指紋変更と VID/PID 書式の既存テストを実行。 | 実 OBS の追加・シーン切替・削除・抜き差し。更新通知は E の固定レスポンスで先に検証。 |

`cargo test --locked --workspace` は39件成功（既存36件、追加3件）。`cargo clippy --locked --workspace --all-targets` は成功し、既存の HID 検証用 example 3件に警告。変更した Rust ファイルは `rustfmt --check` を通過。通常の debug 出力先は既存プロセスが掴んでいたため、リンク済みの `target/debug/deps/device_hub.exe` を一時ディレクトリへコピーしてデモを実行した。デモ用の `%APPDATA%` も分離した。

## 残タスク

### G · ライティング全モード実機検証 — **完了 (2026-11-16)**

**方法**: OBS Virtual Camera でキーボードを常時撮影し、プローブ (`litparam`/`litreact`/`litm16`/`litcolor`/`litone`) でモード・パレット・輝度・方向・速度・ゾーンを順に書き換え、フレームの輝度/色相/重心/セル単位の変化を定量化 + 代表フレームを目視確認。

**確定した対応 (wire mode → 実機挙動)**: `docs/PROTOCOL_NOTES.md` の「Lighting (group 5)」節の表が正本。要点:
- mode 0=固定色 — 選択パレット色を全面固定。**sel=0 はファームの虹マルチカラーで保存RGBは無視される** (公式の「more」スロット)。sel1-7 はスロット色ソリッド
- mode 1..15 = SDK enum Y1[2..16] に対応 (wire は enum より1小さい): 1=虹の横ながれ … 14=光の雨， 15=跳ねる光
- mode 16=カスタム (isCustom セルのみ点灯、他は消灯 — 塗り/クリアで実機確認)
- mode 17-19=キー押下反応: 17=押下周辺ブロック， 18=押下行， 19=押下周囲広範囲 (未押下は全面消灯 — ユーザー打鍵で確認)
- mode 20-23 はファームが readback 19 にクランプ (範囲外は存在しない)
- 全動的モードが選択パレット色を使用 (sel=1=赤で全モード赤)。輝度は全モード有効。方向は掃引反転と思われるが確定的な実機確認は未了

**コード反映**: `xsyd.rs`/`demo.rs` の effect 一覧を検証結果に更新 (旧ラベルは全体に+1ずれ＋16-19が全く別物だった)。パレットUIの slot0 を虹スウォッチ表示に変更。DD 周辺の F1-F5 (ピン追跡/`dd_pre` 退避/解放/isCustom除外/import時sel整合) も同時修正 — テストは `mock_dd_pre_empty` で読み出しを再生して全パス。

**プレビュー (追記 11-15, ユーザー報告「実機と違う」への対応)**: 数式近似をやめ、**公式 HUB と同じ仕組みに置き換えた**。現行バンドル `lighting-CVjtByxR.js` + ストア (tooltip チャンク) を解析 → 公式は `getKeyCustomLighting` (= `[5,3]` 全セル読み) を定期ポーリングして**実機の現在の点灯色をそのまま全キーに表示**していた。実機プローブ `litlive` で `[5,3]` が非カスタムセルにも flag=0xF0 でライブ描画色を返すことを確認 (旧コードは `===255` だけ拾っていた)。→ `DeviceDriver::live_lighting` を契約に追加 (xsyd=実機 readback、他=Unsupported)、`store.rs` のワーカーがライティングタブ表示中 ~6Hz で `Msg::LiveLight` を送り、`lighting.rs` は `session.live_rgb` があればそれを描画し、無い間だけ旧 `preview_rgb` 近似へフォールバック。ライブ有効時は「実機でプレビュー」ボタンを出さない (意味が無いため)。これでパターン・動き・色は定義上実機と一致する。

**バグ修正 (追記 11-16, ユーザー報告「全然違う」への対応)**: `live_lighting` がセルの点灯判定に flag バイトを使っていたため、書き込みの無いセッションでは全セル flag=0 で**常に空マップ→近似フォールバックが出続けていた**。実機検証で flag は点灯判定ではなくマーカー (0xFF=カスタム/0xF0=ファーム書き込み/0=無印) だと確定 — BGR は常に現在フレームの色を返す。判定を `flag!=0 || BGR!=0` に修正し litpoll で 66 キーのライブ色が取れることを確認。

### H · 公式 HUB/SDK/ファーム網羅解析 — **完了 (2026-11-16)**

**方法**: 4 並行解析 (mouse.fit 全チャンク / xsyd-sdk.js ≡ keyboard-BQAULKwa.js ≡ dshk-index-XpTpGr6X.js のプロトコル層 / 内部実装監査 / hub.xsyd.top 全アプリチャンク) + SU66 Max 実機でのグループ・サブ網羅プローブ (grpsweep/grpdeep/kbdstat/bootapp)。

**実機で確定した新規事実**:
- **group 8 FirmwareUpgrade**: `[8,1]` AppToBoot → 実機がブートローダへ遷移 (応答シグネチャ: `[1,1]`/`[1,2]` のみ、`[1,3]` UNSUP、group 2-13 全滅)。`[8,4]` BootToApp で ~4.5s 後に完全復帰 — **抜き差し不要で復旧可能**であることを検証。`[8,2]` Flash/`[8,3]` ValiDate はパケット形式未検証のため送信せず (ファーム書き込みは今後も安全設計が無い限り実装しない)
- **`[2,15,0]` ECHO** (enum 抜け番だがハンドラ残存、値は全0)、**`[2,17,0]` ECHO** — arg=0 は安全。arg>0 は detach 観測済みで **GFSOpenWebDriver (互換/WebDriver モード遷移) が最有力** — 引き続き非ゼロ引数の送信禁止
- **`[10,*]` 奇数 sub (3,5,7,9,11) が ECHO** — OEM 拡張枠 (shell color / keyboard color 系の可能性)、データは全0
- グループ 0,11,12,14,16-20 は本機 UNSUP。group 9 は haptic 系らしいがハード無しで ECHO のみ

**バンドル解析で確定した新規事実**:
- **マクロキーコード空間 = 0xF500-0xF50F (62720-62735)** — `code_to_action`/`action_to_code` 双方向に実装済み (`KeyAction::Macro` が Unsupported ではなくなった)
- **特殊キーコード全カテゴリ確定** (PROTOCOL_NOTES.md のキーコード節に一覧): レイヤー 61696-61699、システム 61952-61961、メインライト 62208-62216、装飾エリア 62224-/62240-/62256-、ThreeMode 62466-、マウス 16384+256n・移動 17920/18176/18432/18688、ホイール 18945/19201
- **DeviceInfo 残バイト**: `r[12..16]`=PCB id、`r[17..29]`=シリアル ASCII、`r[29..41]`=製造タイムスタンプ (要 caps/モデル露出の検討)
- **`[3,5]` GetKeyLayoutStyle** は `{row:5,col:7,ratio:4}`×~15 で物理レイアウトを自己記述 (ratio 12=ノブ, 13-15=画面) — SU66 では定数既存・将来の別レイアウト機種で必要
- **公式 import は WIN/MAC 両テーブルを書く** (`setSystemType` 切替で両 OS 分) — 我々は現行 OS 側のみ (既知差分)
- **公式のエクスポートは AES-256-CBC+EVP_BytesToKey 暗号化** — 復号 import は実装済、公式互換の暗号化 export は未実装
- **公式 preview = `[5,3]` 実機 readback ポーリング** (§G と同じ結論を SDK 側からも確認)
- mouse.fit 側の残項目: ClearSetting(op9)、磁気ボタン校正(op46)、タクタイルフィードバック(EEPROM 243/249)、SensorVirtualPos(6984/6986)、HyperMode/スクロールスキャン系の一部

**今回実装した修正** (内部監査指摘への対応):
- `read_lighting` の per-chunk 部分成功握り潰しを解消 → 読み出し失敗は全体をエラーに (不完全な設定を完全として後続書き込みで実機カスタム色を消す事故を防止)
- `live_axis` が全エラーを 0 埋めしていたのを Disconnected 等の伝播に修正 (切断後も 0 のままゾンビ計測が続き `TravelStopped` に届かない問題)
- `live_lighting` を capabilities に追加 (UI が caps だけを見る原則の例外を解消)
- `KeyAction::Macro` ↔ 0xF500+i の双方向 (16 スロット、UI のマクログループ `kc.macros` と接続)
- TP55 デモの eq_def 宣言不整合、HTX Ultra の SPDT 宣言/handler 不整合 (宣言を落下)、`mousefit8.rs` の `ee_read` 失敗時 `4..0` スライス panic

**残る実用差分 (優先度順)**:
1. WIN/MAC 両キーマップ書き込み (import/reset_keymap が現行 OS のみ — MAC モードのユーザーに実害)
2. `[3,5]` レイアウト自己記述の caps 化 (別レイアウト機種対応の前提)
3. Decorate エリア (area1) の per-cell カスタム書き込み (import で custom 未解析、area0 のみ対応)
4. 公式形式エクスポート (暗号化 JSON を書き出せると公式 import と往復互換)
5. `[5,6]` Caps Lock インジケータ色 (write-only、要検討のまま)
6. `[2,9]` 既定軸スロット、`[3,3]` 単体キー読み、SpecialLightingQuery の露出、軸名 HTTP API (`api.sparklinkplayjoy.com` getAxisList → 現在「軸 N」表示)
7. group 16 Handle/互換モード (SU66 は gamepad ext=0 で低優先) と `[2,17]`=GFSOpenWebDriver の最終同定 (再列挙先モードの確認が必要)
8. OBS 側: 機器ごとの再探索・片側エラー時の正常側表示・実 OBS ホストでの ABI/テクスチャ検証 (§D)

### A · P1：オンボード構成をまたいで Undo やプリセットが書き込まないようにする — **実装済み (2026-11-15)**

**実装**：`Job::Profile(u8)` が切替+全読み直しをひとまとめにする専用ジョブ。`Hub::apply` が `Change::Profile` をそこへ振り分け、`Session.switching_profile` の間は構成内の編集を UI (`Shell::apply` でトースト) と `Hub::apply` の両方で止める。切替か再読の失敗時は `state=None`・`switch_failed` で保持値を破棄して再読する。`diff_changes` は `Profile` を出さず (`ProfileName` は出す — 名前の変更が undo できるようになった)。別構成への undo/redo/プリセット適用は `Shell::pending_restore` に積み、切替+再読が成功してから新しい構成の状態との差分だけを書く。失敗したら履歴へ戻して書き込み0件。プリセット/インポートは `state_compatible` で別機種・EQ バンド数不一致を弾き、未対応項目数を警告し、「現在の構成に反映」と明記した上で `profile.active` を現在値へ正規化する。デモバックエンドは構成ごとの退避状態を持ち、未訪問の構成は工場状態を返す — P1/P2 で値が違う実機と同じ振る舞い。

**検証**：`store.rs` テスト5本 — 構成ごとの分離・切替中の編集遮断+再読 (実ワーカー経由)・`ProfileName` 差分・`state_compatible`・`Job::Profile` の間引き不可。ワークスペースの test/clippy 全パス。実機でのプロファイル切替+undo は未確認。

**残課題**：成功トーストは投入件数ベース (失敗は `Applied` 経路で通知+再読される)。切替成功時に「構成 N に切り替えました」のトーストは出さない。

### B · P1：UI の表示を報告順や再接続で巻き戻さない — **実装済み (2026-11-15)**

**実装**：`Session` に `submit_seq`/`read_seq`/`pending_changes` を追加。`Msg::State` 着信時、読み込み発行より後に投入した未確定の変更を確定状態へ重ねる (古い読み出しで巻き戻さない)。`Msg::Scanned` に世代 `gen` を持たせ、古い列挙は破棄 (バックエンド切替対策)。`Disconnected` ではバックエンドの invalidate に加えてワーカー保持の `driver` を `None` にする — 再オープンで新しいハンドルを引き直す。

**検証**：`state_replays_writes_issued_after_the_read_started` — 読み込み開始後の変更が古い State で消えないこと。実機切断→再接続の往復は未確認。

### C · P1：部分的な HID 読み出しを完全な設定として扱わない — **実装済み (2026-11-15)**

**実装**：`xsyd::read` の必須項目を全て `?` 伝播に変更 — キーごと性能 (既定値化を廃止)、高度キー (途中 break で部分一覧を返すのを廃止)、マクロ本文 (`unwrap_or_default` 廃止)、ライティング各エリア、プロファイル (`unwrap_or_default` 廃止)。「存在しない高度キー=削除」と差分誤解される経路を塞いだ。`mousefit8` の休眠時最小応答は `info.asleep` で区別済みのため据え置き。

**検証**：既存テスト全パス。失敗注入による部分読み出しテストは未実施 (要 fake 応答)。

### D · P2：ライブ機能 — **主要部実装・実機確認済み (2026-11-16)**

**実装**：ワーカーがライブ測定失敗で止めたとき `Msg::TravelStopped` を送り、`travel_on` と深度バッファを UI 側で戻してトースト通知する。終了時は `Shell::Drop` → `Hub::dd_stop_all()`、さらに `XsydDriver::Drop` が追跡セルの読み戻しと復元を best-effort で試す。`direct_drive` は pin/色を追跡し、変化セルのみ DD・外れセルは `[5,4]` 解放・空/未変化チャンクは不送。Live に「キーボードのエフェクトを背景に使う」ハイブリッドを追加。

**実機検証 (OBS Virtual Camera)**：30fps DD で対象 7 セルだけ赤、非対象はファームのエフェクト継続。ストリーム停止後もピンは残る (ファーム側 DD タイムアウト無し)。`[5,4]` 解放/`DdStop` で全面ファーム復帰。**残り**: OBS 側の機器ごとの再探索・片側エラー時の正常側表示、実 OBS ホストでのテクスチャ/ABI、ユーザー打鍵での 8K 入力ヒストグラム。

**追加 (2026-11-16)**: 既存4モード (押し込み深度/打鍵ヒートマップ/キー強調/波紋) に続き「意味のある DD 用途」として3モードを追加 — `Status` (CapsLock で Caps 点灯 + ワイヤレス機の電池 40% 以下で数字行をゲージ化・15% 以下で速い点滅)、`Audio` (WASAPI ループバック→Goertzel 15バンド 60Hz-16kHz→列高スペクトル)、`AppFocus` (GetForegroundWindow→exe 名で `app_map` のプリセットを自動選択)。`Engine::frame` は `FxEnv` (caps_lock/battery/audio/app_focus/fg_app) を受け取り、`audio.rs` (windows 0.62 WASAPI) を新設。ハイブリッドでの無効セル判定は従来どおり `off_color` 一致、いずれも押下フラッシュは共通パスで重畳。テスト 3 本追加 (status_caps_and_battery / audio_column_height / appfocus_uses_resolved_keys)。

### E · P2：更新「あり／なし／不明」を正しく分ける — **実装済み (2026-11-15)**

**実装**：`version_newer` (a.b.c 数値列比較、空白埋めで桁違い吸収) — 比較不能な版は「新しい」と言わない。指紋変化による通知は**バージョンが取れない系統のみ**に限定 — Sparklink で同じ配布物のヘッダ指紋更新でカードが出る/消える問題を解消。**残り**: 「不明」を型で表現する拡張 (UpdateStatus への三値化) は未実施。固定レスポンスのテストは未実施。

### F · P3：文書整合 — **一部実施 (2026-11-15)**

**実施**：`AGENTS.md` の example 一覧を実在物 (`enum`/`kbdiff`/`lite2e`/`tp55e2e`) に更新。`BACKEND_BRIEF.md` §2.2 の「5219 有線経路未検証」を「report-8 検証済み / 64B feature-report ドライバ経路 未検証」に分離。**残り**: store.rs の責務分割 (ワーカーと純粋状態差分の分離)、prefs/fwstate/presets の保存処理統一、プローブ examples の整理/ドキュメント化。

### D 残りの詳細 (旧監査メモ)

[`devhub-obs::live`](../crates/devhub-obs/src/live.rs) では両方のターゲットが `None` の場合しか再探索しない。一度見つけたターゲットはエラー後も残る。片側エラーでも [`render::draw`](../crates/devhub-obs/src/render.rs) がステータスだけ返すため、応答がある側まで隠れる。OBS は機器ごとの有無・一時エラーを分け、一定間隔で差分を再探索し、正常側を表示し続ける。DirectDrive の終了時/セッション破棄時の明示停止と停止失敗の反映も要確認。OBS のテクスチャ生成・破棄のスレッドと適用 ABI は対応 OBS のソースと実 OBS で別途確認。

### E 残りの詳細 (旧監査メモ)

`store::handle` は差分がない、配布なし、dismiss 済みのいずれも手動確認で「最新」と表示する。固定レスポンスで旧版・同版・新版・現在版不明・配布なし・応答異常・dismiss を検証するテストが残る。

### F 残りの詳細 (旧監査メモ)

- [`store.rs`](../crates/device-hub/src/store.rs) の責務分割: ワーカーと純粋な状態差分を分け、その後 `app.rs` からルート遷移と履歴の管理を抜き出す。大規模移動は避ける。
- [`prefs.rs`](../crates/device-hub/src/prefs.rs)、[`fwstate.rs`](../crates/device-hub/src/fwstate.rs)、[`presets.rs`](../crates/device-hub/src/presets.rs) の設定ディレクトリ解決と保存処理をまとめる。プリセット保存は正規化後の同名を無条件上書きするため、衝突を UI へ返す。
- プローブ系 examples (`probe*.rs`, `kbdump`, `kbraw` 等) を整理するか `examples/README` 相当で用途を明記する。

## エージェントへ渡す条件

1. `AGENTS.md` を読み、UI は caps で分岐する。スカラーは `SettingSpec`、操作は `ActionSpec`。コア契約を変える場合は追加のみとし、デモも更新する。GPU backdrop と `vendor/` はこの一連のタスクの範囲外。
2. まずデモまたは fake backend、次に必要な場合だけ実機。実機時は Web HUB と別の Device HUB / OBS の HID セッションを閉じ、設定を書き出してから読み戻しを確認する。初期化や DFU を通常のスモークテストに含めない。
3. A→B→C を順に進める。B の修正と C の UI 統合は `store.rs` を触るので同時に編集しない。OBS は D の一部として独立、更新の判定ロジックとテストは `devhub-update` で独立。実装ごとに再現手順、修正後の結果、実機で確認した範囲を返す。
4. 基本コマンド：`cargo test --locked --workspace`、`cargo clippy --locked --workspace --all-targets`。手動デモは PowerShell で `$env:DEVHUB_DEMO = '1'` を設定してから `cargo run --locked -p device-hub`。開いたままの `target\debug\device-hub.exe` がある場合、Windows のファイルロックで通常の出力先に上書きできないため、その個体を閉じるか別の出力先を使う。

## §I OBS プラグイン修正 (2026-11-15)

「ソースが出るが透明」問題を E2E で潰した記録。

- **配置**: OBS 30.2+ は `%APPDATA%\obs-studio\plugins` をスキャンしない。
  `C:\ProgramData\obs-studio\plugins\<module>\bin\64bit\<dll>` が正規。
- **`obs_module_load` false**: `gs_blend_push/pop` は存在しない。
  正しくは `gs_blend_state_push/pop`。
- **`OBS_SOURCE_CUSTOM_DRAW` = `1<<3`**。`1<<7` は `DO_NOT_DUPLICATE`
  (未設定だと既定 Draw テクニックの pass 内から video_render が呼ばれ、
  自前の technique_begin が入れ子になって描画が捨てられる)。
- **`GS_DYNAMIC` = `1<<1`**。`1` は `GS_BUILD_MIPMAPS` — 非動的テクスチャは
  `gs_texture_map` が失敗 → `set_image` が無言でスキップ → 透明テクスチャ。
- **`obs_source_draw` は pass 内でのみ有効**。正規パターン:
  `while (gs_effect_loop(effect, "Draw")) obs_source_draw(tex, 0,0,0,0,false);`
- 切り分け手順: `%TEMP%\devhub_obs.log` に create/tex/初回描画を記録 +
  `gs_clear` で RT 全体を塗れるか確認 (塗れた → ターゲット正常、
  テクスチャ/スプライト側の問題と確定できた)。

### キー押下表示の点滅 (同日)

- **原因**: SU66 の `[4,3,kind,row]` 軸データは**デバイス側の自律ストリーム**
  (全 20 組を ~16ms で巡回)。`xsyd.rs` の `cmd()` は「最初に届く group-4
  パケットを応答とみなす」ため別 kind/別行のフレームを拾い、ADC 値が
  `AxisSample.status` に混入 → レンダラが誤って押下表示 → オレンジ枠が点滅。
- **修正**: `axis_row` は `cmd` を通さず自前受信し、エコー (r[2]=kind, r[3]=row)
  が一致するフレームまで読み続ける。`route_mm`/`live_axis` 両方が同経路なので
  テストページの押下深さも直った。
- 検証プローブ: `example axraw` (生応答ダンプでストリーム実測 ~1270pkt/s)、
  `example axstat [秒×10]` (押下/離しの status/route 遷移)。

### ソース分離・プロパティ (同日)

- `devhub_live`(両方) / `devhub_live_kb` / `devhub_live_mouse` の 3 ソース登録。
  モード別キャンバス高さ (Both 380 / Kb 250 / Mouse 200)。
- ポーラーは `live::shared()` でプロセス内共有 (3 ソースでも HID 1 本)。
- プロパティ: タイトル/ラベル/アクセント色 + 「キー単位の表示切替」グループ
  (レイアウトから `hk{i}` チェックを生成、チェック=非表示) + 「ボタン単位…」
  (`hb{i}`、マウス接続時のみ実名。未接続は案内テキスト)。
- `update` は `hk{i}`/`hb{i}` を 0..160/0..24 までスキャン — 既定 false=表示で、
  未設定も false なので保存済み true だけ拾えば get_properties 未実行でも復元できる。
- レンダラ刷新: 角丸キーキャップ + 深さフィル (アクセント・不透明度は深さ比例)
  + 押下時グロー + マウスは水平フェーダー (トラック/トリガ線/%) に。
