# legacy/ — 契約 v1 時代のドライバ (コンパイル対象外)

2026-11-14 に devhub-core の契約を v2 (Capabilities + DeviceState + Change) に作り直したため、
v1 の型に依存していたコードをここへ退避した。**捨てずに移植の材料として使う。**

| ファイル | 中身 | 価値 |
|---|---|---|
| `mousefit.rs` | mouse.fit 系 (HTS Plus Pro 33e4:5219) の report 8 / 16B コマンド / EEPROM 読み書き | 実機で読み書き検証済み。フレーミング・チェックサム (`0x55` 和)・補数ペア・DPI 4B レコードはそのまま使える |
| `xsyd.rs` | XSYD (SU66 Max 1ca6:3009) の 64B フレーム / group-sub コマンド / キーコード表 | 実機で読み取り検証済み (device info, performance, lighting)。レイヤー 2 枚 |
| `probe.rs`, `wtest.rs` | v1 API を使う実機テスト | 新 API に合わせて書き直してから `examples/` に戻す |

移植先: `crates/devhub-hid/src/mousefit/` と `crates/devhub-hid/src/xsyd/` (docs/BACKEND_BRIEF.md 参照)。
