# オーディオ (USB DAC / PEQ) フロントエンド — TP55 対応

担当: フロントエンド。`crates/device-hub/src/pages/audio.rs`。
バックエンド (walkplay.rs / peq.rs / registry / demo) は別エージェント実装済みで、契約は変えていない。

## 使っている契約
- `Capabilities::peq: Option<PeqCaps>` → あれば EQ カードを出す (バンド数・範囲・使えるフィルター種別はすべて caps から)。
- `DeviceState::peq` / `Change::Peq { band, value }` / `Change::PeqAll { bands }`。
- プリアンプは汎用設定 `eq_volume` (Slider)。EQ カード内に出すので、peq がある機種では generic カードから除外。
- `eq_def` アクションも EQ カードのプリセット「DEF (公式)」に統合したため generic から除外。
- グラフは `devhub_core::peq::response_db` (ドライバと同じ biquad / 96kHz) なので実機の特性と一致する。

## 画面
1. 応答グラフ: 合成カーブ + 選択バンド単体のカーブ (塗り)。番号付きハンドルを
   ドラッグ = 周波数 + ゲイン同時変更 (差分ドラッグ。クリックだけでは動かない)、ホイール = Q。ホバーでカーソル位置の Hz / 合成 dB。
2. プリアンプ行: 合成カーブのピークから「余裕 / クリップの可能性」を表示、「自動」でクリップしない最大値に。
3. バンド一覧 (公式の縦列相当): 種別 / 周波数 / ゲイン / Q。クリックで選択。
4. 選択バンド編集: ◀▶ 切替、種別 (LS/PK/HS/LP/HP)、0dB に戻す、周波数 (対数スライダー) / Q / ゲインを数値入力 + スライダー。
5. ツールバー: プリセット (内蔵 8 種: フラット, DEF, 低音, V 字, ボーカル, FPS 足音, ハーマン風, 高域を抑える + 保存したもの)、保存、削除、
   **AutoEQ / Equalizer APO (ParametricEQ.txt) の貼り付け**、APO 形式でコピー。
   保存プリセットは `prefs.json` の `eq_presets` (デバイス共通)。内蔵プリセットのプリアンプはピークから自動計算。

## 追加した汎用部品
- `Shell::begin_drag_xy` / `CbXY` (2D ドラッグ。`Drag::xy`)。
- `Prefs::eq_presets` / `prefs::EqPreset`。
- サイドバー/ホーム/ヘッダーのアイコン: `DriverKind::Walkplay` は `headphones`。

## バックエンドへの要望 (任意)
- バンドの ON/OFF (バイパス) を wire で持てるなら `PeqBand::enabled` を追加してほしい (UI はバンド一覧にトグルを足すだけ)。
- 公式の「ONLINE / SHARE」(クラウド EQ) はログイン必須の walkplay-api のため未対応。やるなら devhub-update と同様の別 crate で。
- 別の WalkPlay 系 DAC を足すときは registry に VID/PID を足し、`PeqCaps` をその機種の値にするだけで UI は自動で合う。
