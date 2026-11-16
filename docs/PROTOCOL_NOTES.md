# Protocol Notes — Web configurator の解析結果

2026-11-14 に Claude の内蔵ブラウザで 2 つの Web 版を開き、JS バンドルを読んで得た事実のまとめ。
**バックエンド実装・新デバイス追加の一次資料**。バンドルは配信のたびにファイル名 (ハッシュ) が変わるので、
ここに書いた名前が無ければ同じ手順 (docs/ADD_DEVICE.md §2) で探し直すこと。

---

## 1. www.mouse.fit (G-Wolves 系マウス)

### 1.1 配信物

| URL | 中身 |
|---|---|
| `/static/js/index-D8T-BrOM.js` (1.8MB) | **本体**。i18n 辞書 (ja/en/zh)・プロトコルクラス `cm`・EEPROM アドレス表 `Ge`・WebHID 呼び出し |
| `/Config/env-models.json` | **機種表**。VID/PID・各機能の有無・値の範囲。caps の作り方はほぼこれで決まる |
| `/static/js/index-BeTfQRF9.js` 他 | 画面 (Vue ルートチャンク) |

`sendFeatureReport` / `receiveFeatureReport` / `sendReport` / `oninputreport` は本体にだけ出てくる。

### 1.2 env-models.json (機種ごとの caps の元)

先頭要素はアプリ情報 (`AppTitle: "G-Wolves"`, `AppVer 1.0.0.51`)。2 要素目以降が機種。キー一覧:

```
ModelEN ModelCN XVI PollingRateWired PollingRateWireless UIFolder GoldenDongleNames GoldenDonglePIDs
DefaultDPIColors DefaultSleepTime VIDWired PIDWired VIDWireless PIDWireless VIDWireless4K8K PIDWireless4K8K
LightEnablePID LightDeviceID LightEffects CoreVersion IsNewProtocol IsSensorModelCmd DPIMax LOD
HyperModeEnable CombineKeyEnable DPIXYEnable SleepTimeEnable SleepTimeGrade ButtonDebounceTimeMax
WheelDebounceTimeMax SensorSwitchEnable SPDTStateEnable LongDistanceModeEnable SensorPosEnable
SensorAngleEnable LiftOffDebounce CompetitiveEnable ButtonDebounceAllDiy ButtonDebounceLevel
DPIIndicatorEnable KeyBindingSeq ReceiverKeyBindingSeq ReceiverKeyBindingDeviceID DPIMaxStageNum BtnMaxNum
PollingInfoLevel DeviceLightingOnOff WiredDeviceID MacroEnable _1KDongle _4KDongle _8KDongle _8KDongleIDVD
_1KDonglePollingRate _4KDonglePollingRate _8KDonglePollingRate commonDelay MID ButtonType
```

対象機種の抜粋 (VID はすべて `33E4`):

| ModelEN | PIDWired | PIDWireless | 4K8K | UIFolder | DPIMax | LOD | Stage | Btn |
|---|---|---|---|---|---|---|---|---|
| **HTS Plus Pro** | 5219 | 3854 | 3854 | JM_052_3955 | 40000 | 0.7;0.9;1.2;1.4;1.6 | 7 | 5 |
| **HTXU (=HTX Ultra)** | 5608 | 5617 | 5617 | GW_056 | 30000 | 0.7;1;2 | 7 | 5 |
| HTXU (3950) | 5618 | 3854 | 3854 | JM_056_3950 | 32000 | 0.7;1;2 | 7 | 5 |
| HTXU (3955) | 5619 | 3854 | 3854 | JM_056_3955 | 40000 | 0.7;0.9;1.2;1.4;1.6 | 7 | 5 |
| HTS Plus | 5408/5418/5419 | … | | | | | 7 | 6 |

他に HTM Plus / HSK Pro 2.0 / Fenrir 系 / VUK / HT-S2 / HTR / Lycan / WARG など 36 機種。
`PollingRateWired = 125;250;500;1000;2000;4000;8000`、無線は使用ドングルで変わる
(`_1KDonglePollingRate` / `_4KDonglePollingRate` / `_8KDonglePollingRate`)。

**ATK F1 Ultimate はこの表に無い。** mouse.fit の「別のドライバリンク (LinkJump)」先か、ATK 独自の
configurator の可能性が高い。実機の VID/PID を取ってから判断する (docs/ADD_DEVICE.md §7)。

### 1.3 コマンド・アドレス (本体 JS の定数そのまま)


マクロ送信時は 65B バッファの `[12..]` を `^ 0x55` で難読化している (`f[g]^=85`)。マクロ名は GBK 変換表を内蔵。

プロトコルクラス `cm` のメソッド (= Web 版が持つ全操作。**機能網羅のチェックリスト**):

```
getFirmwareVersion* getDongleFirmwareVersion getRFAddress getEID getDonglePR getBatPer getOldBattery
getLongDistanceMode/set  setProfileID/getProfileID/getProfileList  setPollingRate/getPollingRate getPollRate/setPollRate
getLiftOffDebounce(New)/set  getDebounceTime/set getDebounce/set  getSensorIndex/set  getTrackingMode/set
getCompitiveMode/set  getSPDTState(New)/set  getScrollScanConfig/set  getWheelDebounce/set  getSleepTime/set
getLedState/set  getAngleTune(New)/set  getSensorModel  getPollingInfoLevel/set  getLOD/set
getSensorVirtualPos/set  startCalibr  getMSButtonTrigger/FastTrigger/TactileFeedback + set*
getLiftOff/set  getButtonCombine/set  getDPIIndicator/set  getDPIXYOnOff/set  getAngleSnap(GW)/set
getMotionSync(GW)/set  getHyperMode/set  getRippleControl/set  getDeviceLightingEnable/set
resetProfile resetDevice defaultDevice*  getDPIStageInfo/set getActiveDPI/set GetDPIMax setActiveDPIValue
setDPIStageNum GetDPIStageColors/Set  SetLightEffect/Get GetLightness/Set
AllocateMacroDataSize SetMacroData GetMacroDataSize GetMacroData GetMacroByButtonID Set_MS_Macro* DeleteMacro
Set_MS_KeyFunction/Get SetButton/GetButton SetButtonByDeviceID  Set_MS_ShortcutKey/Get
enterBL getBLFWVer erase program verify exitBL (ファーム更新)  enterPairMode getPairResult
Get_Device_Eeprom_Buffer Set_Device_Eeprom_Value Set_Device_Eeprom_Array Set_MS_DPIValue(_XY)
setHyperModePerformanceTime getDPIMaxStageNum GetUSBPID SetUSB1KIDVDPID
```

### 1.4 実機で検証済み — report-8/EEPROM 経路 (JS の `Die` クラス)

**重要: 観測した全インターフェースで feature report は使えない** (2026-11-15 実測)。
8K ドングル (33e4:3854) も **有線 HTS Plus Pro (33e4:5219)** も、ff02/ff04/ff05/ff06 の
どの usagePage でも `HidD_SetFeature`/`GetFeature` が失敗する (ff04 は report 自体が
存在する兆候 0x57 だが全サイズで拒否)。JS の `setReport`/`getReport` 系 (64B feature
report) は Windows の WebHID でも同じ失敗をするはず (コード上 try/catch で握り潰される)。
→ **現状の設定経路は report-8/EEPROM のみ** (ff02/02 で確実に応答)。
feature-report 新プロトコル (`cm` クラス) のドライバは `mousefit.rs` に残してあるが、
devhub-hid は全 mouse.fit 機種を `mousefit8.rs` に振っている。

- output report ID `8`、16B コマンド `[op, 0, arg…, cksum]`、応答は input report ID `8` の 17B
  `[id][op echo][status][arg…][data@recv[5..]][cksum]` — **データは `r[6]` (=recv[5]) から**。
- チェックサム: `t[15] = 85 - sum(t[0..15]) - reportID` ⇔ 全 17B (id 含む) の和 ≡ 0x55。
- op: `4`=battery, `18`=FW(本体) — 実機 `[18,0,0,0,2,5,48]` → recv[5]=5/recv[6]=48 → **"5.30"**
  (recv[4]=2 は長さ欄。データは常に recv[5]=r[6] から — 長らく r[5] を参照して "2.05" と誤表示していた),
  `29`=FW(ドングル側), `8`=EEPROM 読, `7`=EEPROM 書,
  `14`=getProfileID (recv[5]+1), `15`=setProfileID (`[15,0,0,0,1,id-1]`),
  `22/23`=長距離モード set/get, `5`=enterPairMode (`[5,0,0,0,2,0,0,11]`),
  `6`=pairResult (recv[5]), `1`=EID/donglePR (nonce を [5..8] に入れる; recv[10]=EID, recv[11]=PR),
  `9`=ClearSetting, `46`=StartCalibr。
- EEPROM 読み: `[8,0,addrHi,addrLo,len]` (len ≤ 10)。書き: `[7,0,addrHi,addrLo,len,data…]` 10B チャンク。
  スカラ値は `(v, 0x55-v)` の 2B ペア (`Set_Device_Eeprom_Value(addr,v,2)`)。
- バッテリー応答: `recv[5]`=percent (実測 85)、`recv[6]`=state、`recv[7..8]`=voltage BE (実測 4031mV)。
- **ReportRate@0 のコード表** (OpenMouse @openmouse/protocol gwolves が実機キャプチャで検証、本実機でも
  書き戻し確認済み): ≤1K は周期 ms (8=125, 4=250, 2=500, 1=1000)、>1K は bit4/5/6 one-hot
  (**16=2K, 32=4K, 64=8K**)。⚠️ 公式 JS の表 (32/64/128) は 1bit ずれていて 128 は FW が無効値として
  黙って拒否する — 書き込み応答は返るが値が変わらない。無効値・休眠中も同じく黙殺されるので、
  無線経由では書き込み前にマウスを起こしておくこと。
- DPI: `maxDpiStage@2` (個数), `CurrentDPI@4` (**0 始まり**、JS の get は +1 で返す)。
  3955 系は 6B レコード @`Sensor3955DPI=6912` `[xLo,xHi,yLo,yHi,flags,crc]`、値=(raw+1)*step、
  flags: bit2-3=x 上位 2bit、bit6-7=y 上位 2bit、bit0/4=×2。
  非 3955 は 4B @`DPIValue=12` `[x,y,flags,crc]` step=50。
  色は機種によらず 4B `[r,g,b,crc]` @`DPIColor=44`。
- ボタン (KeyFunction @96, 4B/slot): `[type, paramHi, paramLo, crc]`、param は
  `SetButton` の ctx から `(ctx[0]<<8)+ctx[1]` (Ya エンコードと同じ並び)。
  type5 (ShortcutKey) の実体は @`ShortcutKey=256`+slot*32 (`[count][press×k][release 逆順][crc]`、
  crc = `85 - sum(それ以前)`、書き込みは `count*3+2` B のコンパクト列)。
  マクロは `param=(macroIdx<<8)+runMode`、runMode: 回数 n / 254=押してる間 / 253=トグル。
- マクロ本体 @`Macro=768` + slot*384B: name@+0 `[len, GBK名 ≤30B]`、ctx@+31 `[count][5B×≤70][crc]`。
  イベント 5B: `[flags|type, valLo, valHi, delayHi, delayLo]` — flags 上位 2bit (2=down,1=up)、
  type 下位 4bit (1=マウス,2=keydown,3=keyup,9=mod down,10=mod up)。delay は BE。
- 磁気ボタン (ButtonType=1): `LeftTrigger@239 / RightTrigger@245` (生値 0-19、
  **公式 UI は +1 の 1-20 表示**、書込は -1),
  `LeftFastTrigger@241 / RightFastTrigger@247` (**bit7=有効, 下位7bit=感度** —
  公式 set は `(en<<7)+(val&127)`、get は `byte>>7`/`byte&15`、UI 感度 1-10。
  **無効化しても値は残る** (実機: 0x04 = 無効+感度4)。公式 UI は RT を
  `nButtonType==1 && isWired` でしか表示しない — **プロトコル上はドングル経由でも
  同じ EEPROM 書き込みが通る UI 側のポリシー** (実機でドングル経由の往復も確認済)。
  `nButtonType==1 && isWired` でしか表示しない — **プロトコル上はドングル経由でも
  同じ EEPROM 書き込み経路 (cmd7 透過中継、他アドレスは無線往復確認済み) で、
  ハード制約ではなく UI 側のポリシー**。
  `LeftTactileFeedback@243 / RightTactileFeedback@249` (同じく bit7=有効, 下位=値)。
- スリープ @173: JS `setSleepTime` は `r>=60 ? r/60*6 : r/10` — 秒を 10 等分、すなわち **wire = min×6**。
- センサー角度: `AngleTuneState@191=1` を先に書き、続けて `AngleTune@189`
  (>0 はそのまま、≤0 は `255-|v|+1`)。戻し読みは `>=128` なら `v-256`。
- その他のスカラ (すべて (v,85-v) ペア): DebounceTime@169, DebounceReleaseTime@229,
  WheelDebounceTime@227, MotionSync@171, Angle@175 (angle snap), Ripple@177,
  SensorFPS20K@225 (競技/トラッキングモード), PerformanceState@181/Performance@183 (HyperMode)。
- **マウス休眠中はコマンドに応答が一切返らない** (33E4:3854 実機 2026-11-15 再確認):
  「ドングル ACK + 全 0」ではなく**完全タイムアウト**が実態。さらに op10 ストリームは
  活動中しか流れず (放置 ~10 秒で停止)、`04 xx` 通知は全 vendor IF をリッスンしても観測されない
  (電源 OFF/ON でも無言)。→ **休眠検知 = コマンドタイムアウト + 全パケット ≥5 秒沈黙**、
  覚醒検知 = パケット再開またはコマンド成功 (実装: `mousefit8.rs` の `last_pkt`/`set_asleep`)。
  寝ている間の `read()` は探査 1 発で早期リターンし空状態を返す (全設定分タイムアウトを防ぐ)。
- 実機値 (HTS Plus Pro 無線、2026-11): fw 2.05 / battery 85% 4031mV / polling code 1 (1000Hz) /
  LOD index 1 (0.7mm) / competitive on / ripple on / trigger L=7 R=8 / sleep wire=6 (1分) /
  DPI stage 1600 #ff0000 / サイドボタンにキー "2" "1" の実割当。書き込み全経路 ACK + 再読一致。
- 実機値 (HTX Ultra 3950 有線 33e4:5618、2026-11): fw 2.05 (op18 `[6]=2,[7]=5` で一致 — 別
  オフセットで読むと 5.33 に見えるが誤り) / DPI stage 400/1600/1600/3200 / 5ボタン
  (サイド=キー "2" "1" 実割当) / ポーリング 125–8K 実測 ~5949Hz / LOD@10 は非 index
  型 {0.7mm→3, 1.0mm→1, 2.0mm→2} (JS `setLOD` マップ通り) / SPDT@`KeyOperation=8`
  bit0=左 bit1=右 (`getSPDTStateNew`/`setSPDTStateNew` と同じ)。SPDT・ポーリングの
  UI 書き込み ACK + 再読一致、LOD は probe で往復確認。用途: SPDT は両接点でクリック
  判定して遅延を減らすモード (チャタリング防止ではない)。
- **3854 ドングルの内側機種の特定 (2026-11-16 実機確定)**: op1 を `[1,0,0,0,8,nonce4B]`
  で送ると応答 `r[11]` に EID が返る (公式 `getEID` の `receivedData[10]` = report id を
  除いた配列の添字 = 我々の `r[11]`)。EID は env-models.json の **MID フィールド**で、
  公式 HUB も `a(MID)` で機種を引いている (3854 は HTS Plus Pro / HTXU-3950/3955 /
  HTS Plus / HTX Mini 等 8 機種共用)。実測: HTXU-3950 ペアで `r[11]=5` (MID=5)、
  `r[12]`=donglePR。**休眠中は 0** (公式は 0→2 にフォールバック、我々は未確定として
  覚醒後に再試行)。MID 表: 2=HTS Plus(054_3950) 4=Lycan 5=HTXU-3950 6=Lycan-3955
  7=HTXU-3955 8=HTS Plus-3955 9=FenrirPro 10=HTXMini 11=HTS Plus Pro 12=WARG
  13=HTXMiniAsym 15=VUKMini 17=FenrirAsym 21=FenrirMaxAsym。
  Die の `GetDevicePID` は stub (常時 0x8888) — PID 自体は取れず MID 経由のみ。
- **自発通知 op10 (2026-11-15 実機確認)**: 何も送らなくても input report `[8][10]…` が届く
  (ドングル経由でも確認)。`r[6]` が種別: **0/4/8/16/32=磁気ボタン深さ** (`r[14]&127`=左 %、
  `r[15]&127`=右 %、実測 0→90%+ で追従)、**1=DPI 変更**, **2=ポーリング変更**, **64=バッテリー変化**
  → 本体側の変更は `HubEvent::StateChanged`/`Battery` で UI が読み直す。
  作動点判定 (JS): `depth/5 >= getMSButtonTrigger()+1` → 押下 (5% 刻み)。
- feature-report 通知 (report id 4): `04 01/04`=DPI、`04 02`=プロファイル、`04 03`=バッテリー、
  `04 06 00/01`=切断/接続、`04 07`=LOD、`04 08`=ポーリング、`04 0f`=MotionSync、`04 18`=センサー。

### 1.5 UI 文言 (i18n ja) から読める機能一覧

性能: ポーリングレート (有線/無線)、高速モード (HyperMode)、競技モード、長距離モード、組み合わせキー機能。
センサー: リフトオフ距離、モーション同期、角度スナップ、リップル制御、センサー角度、センサー変更、センサー仮想中心点、20K FPS。
クリック: クリックデバウンス (押す前/押した後/離す前/離した後の 4 値 DIY・低/中/高プリセット)、ホイールデバウンス、
左/右ボタン SPDT、ボタン誤操作防止 (LiftOffDebounce)、スクロール速度。
磁気マイクロスイッチ機種: トリガーポイント、クイックトリガー、振動強度、クリック音、左右校正。
電源: スリープ時間、DPI インジケーター、LED ステータス、バッテリー表示。ライト効果: 単色/ブリージング/スペクトル/波 + 明るさ/速度。
ボタン: 左/右/中/戻る/進む/スクロール/Xクリック/ダブルクリック/DPI 循環・+・−/DPI ロック/連射/
キーボード (キーコード)/Windows ショートカット/メディア (再生・停止・曲送り・音量・ミュート・電卓・マイPC・ホーム・メール・明るさ・更新)/
マクロ (回数指定・離すまで・再度押すまで)/ポーリング切替/上下ロール/無効。「少なくとも一つのボタンを左ボタンに」制約。
マクロ: 最大 3 個 (機種による)、記録 (遅延を記録/固定/なし)、挿入、マクロ名。
その他: 設定リセット、デフォルトに戻す、ファーム更新 (DFU)、ペアリング、レシーバーボタンバインド。

---

## 2. hub.xsyd.top/keyboard (星闪 / Sparklink, SU66 Max)

### 2.1 配信物

| ファイル | 中身 |
|---|---|
| `/keyboard/js/main-*.js` | アプリ本体・**WebHID filters** (`Ur=[{vendorId:7334,productId:…,usagePage:65456,usage:1},…]`) |
| `/keyboard/js/esm-*.js` (260KB) | **プロトコル SDK**。`sendReport(0, data)` + `inputreport` で 1 要求 1 応答 |
| `/keyboard/js/tooltip-*.js` | i18n 辞書 (ja 641 項目) |
| `KeyPerformance-*`, `advanced-key-*`, `lighting-*`, `custom-key-*`, `config-*`, `firmware-*`, `screen-*`, `voice-*`, `handle-*` | 画面 |

VID `7334 = 0x1CA6`、usagePage `65456 = 0xFFB0`、usage 1。SU66 Max は PID `12297 = 0x3009`。
filters には同じ VID で 100 以上の PID が並ぶ (同 SDK の他社キーボードも同じドライバで扱える)。

### 2.2 SDK の列挙値 (esm-*.js 先頭)

**プロトコルグループ (dt, 完全)**: Device=1 Global=2 LayoutAndKey=3 Performance=4
Lighting=5 HigherKey=6 Macro=7 **FirmwareUpgrade=8** CustomCommand=10
Displayer=12 ThreeMode=13 Voice=14 Touch=15 Handle=16 ThreeD=17

```
GFS コマンド: ResetFactory=1 SaveParam=2 ConfigSwitch=3 SystemSetting=4 ReportRateSetting=5
  CalibrationSetting=6 AxisLibraryQuery=7 EffectAreaQuery=8 ModifyDefaultAxis=9 DoubleLightingQuery=10
  SpecialLightingQuery=11 RtPrecisionQuery=12 SleepTimeQuery=13 MacroSpaceInfoQuery=14
  ShakeOptimizationSwitch=16 USBModeSetting=18
リセット範囲: All=0 Calibration=1 Performance=2 Lighting=3 Layout=4 HigherKey=5 Macro=6 Axis=7 HighPollingRateReset=15
OS: WIN=0 MAC=1
ポーリング: R8KHz=0 R4KHz=1 R2KHz=2 R1KHz=3 R500Hz=4 R250Hz=5 R125Hz=6
構成: Config1..4 = 0..3
ライト領域: Keyboard=0 Decorate1..5=1..5
動的エフェクト: ripple=1 EbbAndFlow=2 RippleGently=3 RotatingStorm=4 LuckyRainbow=5 ShiningRainbow=6
  Shining=7 MovingWindow=8 WaveTransformation=9 Shift=10 SineCurve=11 CloudFlow=12 Blooming=13
  Rainbow=14 Rainfall=15 Jump=16
ライトの種類: Static=0 Dynamic=1 Custom=2   デコライト: Close=0 OpenDown=1 OpenUp=2 Open=3
USB モード: USB2_0=0 USB3_0_8K=1 USB3_0_16K=2 USB3_0_32K=3   (GetStatus=1 GetStoredMode=2 SetMode=3)
```

### 2.3 SDK のメソッド (= 全機能。チェックリスト)

```
getKeyLayout/setKeyLayout getKeyCode/setKeyCode getDefaultKeyLayout
getPerformance(row,col) / setPerformance{mode,normalPress,normalRelease,rtFirstTouch,rtPress,rtRelease,
   pressDeadStroke,releaseDeadStroke,axis,row,col,calibrate,axisV2Id,axisRangeMax,axisCoefficient}  ← 値は mm×1000 (orden oficial: rangeMax@23,24 / coef@25,26)
getADC getRoute getCalibrate getKeyStatus  (ストロークテスト・キャリブレーション)
getLightingBase/Palette/ColorCorrection (+set)  getLightingCustom saveLightingCustom setLightingDirectDrive
  setLightingCaps getDecorate1Custom getLightingArea getDoubleLighting getSpecialLighting getLightingSleepTime/set
getHigherKeyData / setHigherKey{NONE,DKS,MPT,MT,TGL,END,SOCD,RS}  (SOCD/RS は 2 パケット。
  XSYD SDK の switch はこの 8 種のみ — MDS/TURBO は walkplay 由来の誤記)
setMacroData getMacroData setMacroMode getMacroMode getMacroSpaceInfo
getConfigList getConfigName/setConfigName  (構成 1〜4) setImportData (構成コードの取り込み)
getRtPrecision getAxisList getCurrentAxis setDefaultAxis/getDefaultAxis
setShakeOptimizationSwitch/get  getUSBModeStatus getUSBModeStored setUSBMode  highPollingRateReset
getKeyboardColor/set getDeviceShellColor/set getKeyboardMode getSystemTypeList setSystemType
getBlackoutStatus (open/close)  getBasicInfo{mode,charge,battery} setSleepTime{shallow,deep} getSleepTime
getDeviceInfo{type,subType,boardId,appVersion,pcbVersion,runModeVersion,sn,timestamp} getDeviceFeature getDeviceProtocol
getAppToBoot getBootToApp getFlash upgrade (ファーム更新)
displayer: getDisplayerInfo setDisplayerBoot setDisplayerFlash setDisplayerGIF (画面付き機種)
voice / haptic / animation / three-mode / handle (ゲームパッドマッピング) — 機種限定
```

### 2.4 キーコード空間 (i18n の keyTip_<code> から)

| 範囲 | 意味 |
|---|---|
| `0x0000` 空 / `0x0001` 透過 | |
| `0x0004..0x00E7` | HID Keyboard usage そのまま |
| `0x2000 | consumer` | メディア (例 `8373=0x20B5` 次の曲, `8418=0x20E2` ミュート, `8303/8304` 明るさ±) |
| `0x4000..0x4Bxx` | マウス (`16640` 左, `16896` 右, `17152` 中, `17408` 進む, `17664` 戻る, `18945/19201` ホイール, `17920..18688` カーソル移動) |
| `0x5xxx`, `0xCxxx`, `0xDxxx` | ゲームパッド (ハンドルモード) |
| `0xF100..0xF103` | レイヤー切替 (メイン / Fn1..Fn3) |
| `0xF200..0xF209` | システム (初期化, Win/Mac 配列, 磁軸キャリブレーション, 構成 1〜4, Win ロック) |
| `0xF300..0xF318` | ライティング操作 (モード/色/明るさ/速度/ON-OFF/方向、デコライト) |

### 2.5 実機で検証済み (legacy/xsyd.rs より)

- 64B フレーム (hidapi では先頭に report-id 0 を足して 65B)。`[group][sub][args…]` → `[group][sub][data…]`、`sub==0xFF` は未対応。
- group: 1=Device 2=Global 3=Layout/Key 4=Performance 5=Lighting 6=Higher key 7=Macro 10=Custom 12=Displayer 13=Three-mode。
- `1,2` device info (ASCII `49142`, build `2026101216:41:03`)、`4,1` performance、`5,1` lighting が応答。
- **レイヤーは 2 枚**。キーの位置は `(row<<4)|col` (row 1..5, col 0..14)、キーコード列上は `row*30+col`。

### 2.7 確定フレーム (dshk.xsyd.top SDK + SU66 Max 実機検証 2026-11-15)

u16 はすべて **リトルエンディアン**。mm 値は **×1000 の u16**。

**Device (group 1)**: `1,1` Protocol → `r[2],r[3]` (実機 1.3)。`1,2` info → `r[2]`=type
(0=磁気有線,1=磁気3mode,2=磁気+画面,3=メカ有線,4=メカ3mode,5=メカ+画面,6=光学有線,7=光学3mode,
8=光学+画面,9=誘導有線,10=誘導3mode,11=誘導+画面,12=磁気フル画面,13=磁気振動,14=磁気音声,
15=磁気振動音声,16=磁気ゲームパッド), `r[3]`=subType, `r[4..8]`=boardId BE, `r[8..12]`=appVersion。
`1,3` DeviceFeature → **機能ビットマスク**: `r[2]`=軸 {mechanical:1,magnetic:2,optical:4,
inductive:8,magnetic3D:16}, `r[3]`=接続 {usb:1,2.4G:2,ble:4}, `r[4]`=基本 {rgb:1,knob:2},
`r[5]`=拡張 {smallScreen:1,fullScreen:2,haptic:4,voicePlay:8,voiceRec:16,gamepad:32,dotMatrix:64}。
SU66 実機 = 磁気/USB専用/RGBあり/拡張なし。**`r[2]` をレイヤー数と読む旧実装は誤り
(SU66 では magnetic=2 と層数=2 が偶然一致していただけ)**。

**Layout/Key (group 3)**:
- `[3,1,layer,row]` → `r[4..64]` = 30×u16 (キーストリーム行)
- `[3,2,layer,row,…30×u16]` 行書き / `[3,4,layer,row,col,codeLE]` キー単体書き (**実機で検証済み**)
- `[3,3,layer,row,col]` → `r[5,6]`=keycode

**Performance (group 4)** — **実機読み書き検証済み**:
- `[4,1,row,col]` → `r[4]`=mode (**0=通常/1=RT 連続/2=IntervalRT 非連続** — 公式 enum
  `vn={Normal:0,RT:1,IntervalRT:2}`。公式 UI の「連続 RT」トグルは 1↔2 を切替),
  `r[5,6]`=normalPress, `r[7,8]`=normalRelease,
  `r[9,10]`=rtFirstTouch, `r[11,12]`=rtPress, `r[13,14]`=rtRelease, `r[15,16]`=pressDeadStroke(上側DZ),
  `r[17,18]`=releaseDeadStroke(下側DZ), `r[19]`=axis, `r[20]`=calibrate, `r[21,22]`=axisV2Id,
  `r[23,24]`=axisRangeMax (全ストローク mm×1000), `r[25,26]`=axisCoefficient
  (**公式デコード確定: rangeMax が先・係数が後。旧実装は係数 25.7 を全ストロークと誤読**)
- `[4,2,row,col,…]` 同レイアウトで書き込み → 後に `[2,2,2]` (SaveParam:Performance)
- `[4,3,kind,row]` → 30×u16 (col 順、row=1-5)。kind (AxisData 内サブ、SDK enum 確定):
  0=ADC (raw センサー値 ~2200-2500), 1=Route (mm×1000、**live_travel で使用**。無押下は 0 —
  キャリブレーション後の正しいベースライン), 2=Calibrate (キーごと status=2 で校正済み), 3=KeyStatus
  (無押下は全 0、押下中は非 0 — OBS オーバーレイの押下表示に使用)。
  **注意: kind が先、row が後**。`[4,3,row,kind]` は別クエリとして解釈され誤データを返す (実機で確認)。
- **軸データは自律ストリーム (2026-11-15 実機確認)**: live 軸クエリを受けた状態のデバイスは
  `[4,3,kind,row]` フレームを**勝手に連続配信**する (全 20 組を ~16ms で巡回、実測 ~1270 pkt/s)。
  「クエリへの応答」は区別されないので、受信側は**エコー r[2]=kind/r[3]=row が一致する
  フレームまで読み続ける**必要がある。最初の group-4 パケットを応答とみなすと別行/別 kind
  のデータを拾い、値がずれる (ADC 値が KeyStatus 欄に混入→押下表示が点滅した実害あり)。
- **ストリームの武装/解除 (2026-11-16 実機プローブ `dupread`/`disarm`/`streamlife`/`disarmcmd`)**:
  - 1 回の `[4,3]` で ~28fps の自律配信が始まり **150 秒以上自己終了しない**
    (武装したハンドルを閉じても継続 — デバイス側の状態)。
  - **任意の非軸コマンドで即時解除**される (`[4,1]`/`[2,3]`/`[4,4]`/`[0,0]` 全て有効。
    `[4,3]` だけ再武装。`[1,2]` device-info は減速するが停止しない)。
  - **入力レポートは全オープンハンドルへ扇出**される (別クライアントの応答が見える)。
    → 公式 HUB (sendAndWait で最初のフレームを応答とみなす、エコー検証なし) と
    OBS プラグインを併用すると、武装中のストリームが公式の全読み出しを破壊する。
    `live_axis` は外部トラフィック (他人のコマンド応答) を検出して受動化することで
    併用可能にした (`xsyd.rs` の `drain_axis`/`note_foreign`/`cmd_self`)。

**Global (group 2)** — 第1引数は多くが op (0=list/開始, 1=read/終了, 2=write):
- `[2,1,scope]` 工場リセット / `[2,2,scope]` フラッシュ保存 (scope: 0=all,1=calib,2=perf,3=light,4=layout,5=higher,6=macro,7=axis)
- `[2,3,0]` 構成数 / `[2,3,1]` 現在構成 / `[2,3,2,i]` 切替 / `[2,3,3,i]` 名取得(32B UTF-8) / `[2,3,4,i,…]` 名設定
- `[2,4,1]` OS 取得 / `[2,4,2,0|1]` Win/Mac 設定
- `[2,5,0]` ポーリング選択肢 / `[2,5,1]` 現在 / `[2,5,2,code]` 設定 (code: 0=8K,1=4K,2=2K,3=1K,4=500,5=250,6=125)
- `[2,6,0|1]` キャリブレーション開始/終了
- `[2,7,0]` 軸一覧 (u16 id) / `[2,7,1]` 現在軸 / `[2,9,0|1]` 既定軸 set/get
- `[2,8,0]` ライト領域 (`r[3]`=数, 各 {index,count,rows,cols}) / `[2,10,0]` 二重ライト → `r[3]` /
  `[2,11,0]` 特殊ライト → `r[3]`。**SU66 実機は両方 `r[3]=1` (存在確認済み probe8)** —
  arg=1 を送ると `r[2]=0xff` で未対応。クエリには必ず arg=0 を使う
- `[2,12,0]` RT 精度 → `r[3]`/1000 = 最小刻み mm
- `[2,13,0,u16]` / `[2,13,1]` ライトスリープ時間 (分)
- `[2,14,0]` マクロ領域 → `r[3]`=スロット数, `r[4,5]`=最大イベント数
- `[2,16,0,v]` / `[2,16,1]` 防チャタリング最適化
- `[2,18,*]` USB モード — **SU66 実機は 0xFF 応答 (未対応)**。有線専用機には無い

**Lighting (group 5)** — 実機読み書き検証済み (公式 `lightingBaseData`/`setLightingBase` と照合、
E2E `lite2e` で write→readback→restore 確認 11-16):
- `[5,1,area,0]` → `r[4]`=open, `r[5]`=mode, `r[6]`=輝度 0-100, `r[7]`=速度 0-100,
  `r[8]`=方向 (**0=Forward/1=Backward のみ**), `r[9]`=パレット選択色 index
- **mode バイトは `lightingEffectModes` 配列のインデックス** (公式 `applyBase{mode:i}` で
  `i` をそのまま送る)。SU66 は **20 モード (UI 上 L1〜L20、mode 20-23 は書き込み時に
  ファームが 19 へクランプ — 実機 readback 確認)**。
  **実機カメラ検証で確定した対応 (2026-11)**: SDK enum `Y1` は 1..16
  (ripple,EbbAndFlow,RippleGently,RotatingStorm,LuckyRainbow,ShiningRainbow,Shining,
  MovingWindow,WaveTransformation,Shift,SineCurve,CloudFlow,Blooming,Rainbow,Rainfall,
  Jump) だが、実機の wire 値はこの並びより **1 つ前**に対応する:
  - **0=固定色** — 選択パレット色を全面固定表示。**sel=0 はファームの
    マルチカラー (横方向虹グラデ)** で slot0 の保存 RGB は無視される
    (公式 UI でも index0 は「more」虹アイコン)。sel1-7 はスロット色をソリッド表示
  - **1..15 = 動的エフェクト** (Y1[2..16] に対応): 1=虹の横ながれ (左右スクロール虹),
    2=ゆっくり変色 (均一色+ゆる色相回転), 3=すばやく変色 (高速色相回転),
    4=虹の市松 (虹チェッカー), 5=虹のめぐり (色相循環グラデ),
    6=虹の明滅 (色相循環+明滅), 7=ほのかな窓 (移動する光帯),
    8=ゆるやか変形 (均一色がゆっくりモーフ), 9=色帯シフト (色ゾーンが移動),
    10=虹のうねり (マルチバンド虹波), 11=斜めの光帯 (斜め光帯掃引),
    12=コンフェッティ (虹パッチ点在), 13=虹のモザイク (細かい虹チェッカー),
    14=光の雨 (色ドット落下), 15=跳ねる光 (点灯ブロックが跳んで移動)
  - **16=カスタム** — isCustom=255 で塗ったセルだけが点灯、他は消灯
    (公式「Decorate」ペイント領域の表示モード; J1.Custom=2 と同義)
  - **17-19=キー押下反応** (L17-L20 の残り): 17=押下キー周辺 ~4×4 ブロック点灯,
    18=押下キーの行全体点灯, 19=押下キー周囲の広範囲点灯。未押下時は全面消灯
  - 全モードが選択パレット色を使用 (sel=1=赤 で全モード赤になったのを確認)。
    明るさは固定色含め全モードで有効。方向バイトは公式 UI 未公開だが
    波系モードで掃引方向を反転すると思われる (実機で確定的な反転は未確認)
  - カテゴリ enum `J1={Static:0,Dynamic:1,Custom:2}` は UI 内部のみで wire には出ない
- **open バイトは 4 値 enum `jo`** (`{Close:0, OpenDown:1, OpenUp:2, Open:3}`):
  DoubleLighting 機の Keyboard エリアでは上下どちらを点けるかを表す。**DoubleLighting は
  固定ではなく `[2,10,0]` (GFSDoubleLightingQuery) で問い合わせる** — `r[3]==1` で真。
  公式は点灯時に `open=3` (Open=両ゾーン) を送る。**1 を書くと下ゾーンしか点かない**ので
  真偽値そのまま送るのは誤り。SingleLighting 機では非 0=Open の真偽扱い (公式パーサ:
  `lamp==="DoubleLighting" && area===0 ? jo[r[4]] : r[4]===0?"Close":"Open"`)。
  モデル側は `LightArea.zones=[上,下]` + `LightingState.zone_on` で UI の
  上/下ライトスイッチに対応し、書き込み時に jo へ合成する (両方=3, 上のみ=2, 下のみ=1)
- `[5,2,area,0,open,mode,輝度,速度,方向,sel]` 書き (応答レイアウトと同じ並び) → 後に `[2,2,3]`。
  現行値と同じなら送らない (E2E では write→80ms 待ち→readback で検証 — save の flash
  書き込みが反映されるまで本体は古い値を返すことがある)
- 固定色/色つきエフェクトの色は**パレット**経由: `[5,1,area,1]` → `r[4..]`=`{B,G,R,H}`×8 色、
  `[5,2,area,1,…32B]` 書き (選択スロットを read-modify-write)。モデル側は
  `LightingState.palette` (全 8 スロット) + `palette_sel` + `color`(=選択スロット色) で表す —
  スロット選択は sel を書くだけ、色編集は選択スロットを書き換える (公式の
  `selectStaticColor`/`setStaticColor` と同じ分離)
- カスタム表示は **mode=16** — isCustom=255 セルだけが塗った色で点灯し、他セルは消灯
  (実機カメラ確認: 行単位で塗ったセルだけ点灯、クリアで全面消灯)。
  isCustom=255 は他モードでもベースエフェクトの上に重なるオーバーレイでもある
  (実機で観測: effect=0・固定色黒 + カスタム 10 セル)。公式パーサは `isCustom===255`
  のみ有効と判定する
- カスタム色 (サブ番号は SDK enum 確定): `ReadBack=3` / `Custom=4` / `DirectDrive=5` / `Caps=6`
- `[5,3,area,offset]` → `r[4..]` = 15 セル×4B `{B,G,R,flag}`。area0 は 9 パケット=135 セル
  - **BGR は常に「いま描画している色」** (エフェクトの現在フレーム)。flag は**マーカー**:
    `0xFF`=ユーザー/DD カスタム、`0xF0`=ファームが書き込み/確定したセル、`0`=未マーク。
    **点灯判定に flag を使ってはいけない** — 書き込み無しのセッションでは全セル flag=0
    でも色は普通に流れてくる (litpoll で実機確認 11-16。「0=消灯セル」は誤りだった)。
    点灯中の判定は `flag!=0 || BGR!=0` (消灯セルは BGR も全 0)。
    公式 HUB のライティングプレビューはこの read を全セルそのまま表示する仕組み —
    `getKeyCustomLighting` (= `[5,3]` 全チャンク読み) を定期ポーリング
    (JS 側シミュレーションではない)。当アプリも同様に `live_lighting` で実装。
    `isCustom===255` の判定だけは引き続き flag を使う (カスタムセルは flag=255 で保持)。
- `[5,4,area,offset,…60B]` カスタム書き込み (永続)。`[5,5,area,offset,…60B]` DirectDrive
  (応答なし・ライブ表示のみ)。
  **モデル側の `per_key` は絶対状態** — 未指定セルは isCustom=0 で上書きする
  (read-modify-write で残すと消せない)。色バイトは既存値を保持したままフラグだけ落とす。
  `write_per_key` は変化した chunk のみ送信する (E2E `lite2e` で write→clear→restore 検証 11-16)
- `[5,6,area]` caps → 実機は全 0 応答
- **彩度 (ColorCorrection, Za=2)**: `[5,1,area,2]` → `r[4,5,6]` = R,G,B、
  `[5,2,area,2,R,G,B]` 書き。公式 UI は「彩度」の R/G/B 各 **0-100 スライダー×3**。
  SU66 実機値: area0=(38,35,40), area1=(14,13,15)。モデル側は `LightingState.color_correction`
  (E2E `lite2e` cc 往復で検証 11-16)
- **ライティング連動 (link)**: 公式 UI の link/link-unlink アイコンはデバイス設定ではなく
  UI ローカル — ON のとき上下スイッチを同時に動かす。我々も同様に実装

**カスタム色のセル→キー対応 (実機目視で確定 11-15, `example litest` で検証)**
- LED グリッド = **6 行×21 列** (SDK 既定 `rows:6, cols:21` と一致)。物理行 1-5 がグリッド行 1-5、
  グリッド行 0 (cell 0-20) と 126-134 は未使用
- 行 1-4: `cell = 行*21 + floor(見た目 x)` で全観測と一致 (Esc=21, 9=30, W=44, '=74, Enter=75,
  LShift=84, V=89, B=90)
- 行 5 は右側が圧縮されているため実測テーブル (`xsyd.rs::led_cells`):
  LCtrl=105, LWin=106, LAlt=107, **スペースバー下=109-113 (5 LED が 1 キャップ下)**,
  RAlt=114, Fn=115, ←=117, ↓=118, →=119。**デッドセル: 108, 116, 120-125**

**Higher key (group 6)**: `[6,1,row,col,0]` 読み → `r[4]`=mode (0=NONE,1=DKS,2=MPT,3=MT,4=TGL,5=END,6=SOCD,7=RS)。
`[6,2,row,col,mode,…]` 書き。オフセットは応答 r の位置 (書き込みパケットは r[2..] 相当なので -2)。
- DKS: 4×kc u16@5..12 + 4×trp u8@13..16 + db u16@17 + db2 u16@19 (db=最小行程, db2=最大行程)。
  **trp は 8bit マスク**: b0=押始Tap b1=押始Hold b2=底打ちTap b3|b4=底打ちHold b5=戻りTap b6=戻りHold b7=全リリースTap
- MPT: 3×kc@5..10 + 3×db u16@11..16 (mm×1000) / MT: kc@5..8 + time u16@9..10
- TGL: kc@5..6 + **time u16@7..8** (トグル出力の押下時間 ms)
- END: **kcs@5..8 = [押した時のキー, 離した時のキー]** + delay u16@9..10。`point` のような位置フィールドは wire に存在しない
- SOCD: **2 パケット** `[6,2,r1,c1,6,r2,c2,kcA,kcB,delay@11..12,x0]` + `[6,2,r2,c2,6,r1,c1,kcB,kcA,delay,x1]` (ペア・キーコード反転)。
  socdMode: 0=後上書き(x0=0,x1=0) 1=A優先(1,2) 2=B優先(2,1) 3=ニュートラル(3,3)。**delay u16@11..12 は反転判定の遅延**
- RS: SOCD と同構造 (末尾モードバイトなし)、同じく delay u16@11..12
- モデル側 `Advanced::{Tgl,Socd,Rs,End}::delay_ms` がこれらの time/delay に対応 (2026-11-15 実機往復で検証)

**Macro (group 7)**: `[7,1,id]` → `r[3]`=valid, `r[4,5]`=actNum, `r[6,7]`=repNum, `r[8]`=mode。
`[7,2,id,valid,actNumLE,repNumLE,mode]` 書き。
`[7,3,id,offset]` → `r[4..]` = 4B アクション×15/パケット。`[7,4,id,offset,…]` 書き。
アクション u32 LE: **bit31=status (1=keydown/0=keyup), bits16-30=delay_ms (≤32767), bits0-15=keycode**。
mode: 0..3=クリックで実行(回数), 4=押下中リピート(即停止), 5=押下中リピート(完了後停止)。

**FirmwareUpgrade (group 8)** — 公式 OTA (オンライン/ローカル .bin の両方あり):
- `[8,1]` AppToBoot: アプリモード → ブートローダへ遷移
- `[8,2,addr u32 LE,len u16,data...]` Flash: ファームデータをアドレス単位で書き込み
- `[8,3]` ValiDate: 書き込み検証パケット
- `[8,4]` BootToApp: ブートローダ → アプリ復帰 (成功時デバイス再起動)
- **実機検証済み (grpsweep/bootapp, 2026-11-16)**: `[8,1]` で実機がブートローダへ遷移。
  ブート中の応答シグネチャ: `[1,1]`/`[1,2]` のみ ECHO、`[1,3]` は UNSUP、
  group 2-13 の機能コマンドは全てタイムアウト、`[8,0]`/`[8,10]` は UNSUP。
  `[8,4]` でアプリへ復帰し ~4.5s で re-enum・全コマンド回復 (抜き差し不要)。
- 配布 API は `api.sparklinkplayjoy.com/api/v1/getKeyboardFirmware` (board_id +
  appVersion → 最新バージョン URL)。**我々は通知のみで書き込みは実装しない**

**CustomCommand (group 10)** — その他機能用パススルー:
- Blackout: ステータス `[10,1,0]` → `r[3]` (255=有効/0=無効)、
  開く `[10,1,1,255]` / 閉じる `[10,1,1,0]` (di enum {Mid:1, Open:255, Close:0})。
  SU66 実機は応答あり (r[3]=0) — 全 LED を一時消灯するモード。
  アクション `blackout` で実装済み (状態を読んでトグル)。実機往復確認済み

**設定のエクスポート/インポート**: 公式は `exportConfiguration` →
`encryptData(設定JSON)` を `{data:...}` としてダウンロード、import は
`setImportData` が各 set* コマンドで書き戻す。我々は `export_config` /
`import_config` アクションで自前 JSON (`{app,model,name,state:DeviceState}`、
serde そのまま) — **公式の暗号化形式とは非互換** (復号鍵未解析)

**ThreeMode (group 13)** — トライモード機のみ (SU66 有線機は 0xFF 未対応を実測):
- `[13,1]` BasicInfo → `r[2]`=mode (USB=0/2.4G=1/BT1=2..), `r[3]`=charge, `r[4]`=battery
- `[13,2,shallow u32,deep u32]` SetSleepTime / `[13,3]` GetSleepTime → `r[2..10]` 同構造

**SpecialLightingQuery `[2,11,0]`** → `r[3]` を `[1,3,5]` にマップ (特殊照明機能の値、
SU66 実機は 1→3)。情報のみ、UI では未使用

**キャリブレーション enum Xo**: `{Open:0, Close:1}` — `GFSCalibration("Open")` → `[2,6,0]`、
`("Close")` → `[2,6,1]` (我々の calibrate/calibrate_finish と一致)

**スコープ (GFSRestore/SaveParam)**: All=0 Calibration=1 Performance=2 Lighting=3
Layout=4 HigherKey=5 Macro=6 Axis=7 (HighPollingRateReset=15 も enum に存在)
**高度キー・マクロの write は実機検証済み** (TGL write/read/clear・マクロ write/read/delete、11-15 hktest)。

**キーコード**: 基本は u16 LE。組み合わせキー = `0x1000 | mods<<8 | usage` (custom-key 画面で確定、
mods は HID 修飾ビット bit0-3=左 Ctrl/Shift/Alt/Win)。
**特殊キーコード空間 (tooltip バンドルの J_ カタログから完全確定 2026-11-16)**:
- 0=未割当, 1=透過, 61696-61699=レイヤー切替 (メイン/Fn1-3)
- 61952=初期化(長押し), 61953/61954=Win/Mac レイアウト切替(長押し), 61955=磁軸キャリブレーション,
  61957-61960=構成1-4切替, 61961=Winキーロック
- 62208-62216=メインライト制御, 62224-62232/62240-62248/62256-62264=装飾エリア1-3制御
- 62466-62469,62471=ThreeMode 切替
- **62720-62735 (0xF500-0xF50F)=マクロスロット 0-15 → 実装済み**
- 16384+256n=マウス操作, media/system/xboxHandle/classHandle カテゴリは J_ 参照
- マウス移動系: 17920=↑移動 18176=→移動 18432=↓移動 18688=←移動 18945/19201=ホイール上下

**Three-mode (group 13)**: `[13,1]` → `{mode,charge,battery}` (SU66 有線では未対応返し)。
`[13,2,…]` スリープ (shallow/deep 各 u32 LE)、`[13,3]` 取得 — 無線機のみ。

### 2.6 UI から読める機能一覧

キー割り当て (基本/マウス/メディア/ライティング/コントロール/システム切替/組み合わせ/マクロ、メイン+Fn レイヤー)、
パフォーマンス (トリガー方式: 通常/RT/初回トリガー、トリガートラベル・押下/リリース、RT 押下/リリース感度、
上下デッドゾーン、連続 RT、軸 (スイッチ) 選択、RT 精度)、キャリブレーション、ストロークテスト、
高度キー (DKS・MPT・MT・TGL・END・SOCD (後上書き/A 優先/B 優先/ニュートラル)・RS)、
マクロ (クリック実行で回数・押している間リピート (即停止/完了後停止)・クリックで開始停止、最大 32768ms)、
ライティング (メイン+デコレーション、エフェクト 16 種 + 固定色 + カスタム、明るさ・速度・方向・色、自動消灯、連動)、
構成 1〜4 (切替・名前・インポート/エクスポート)、ポーリングレート、USB モード、防チャタリング最適化、互換モード、
Win/Mac、スリープ (無線機)、ファーム更新、工場出荷リセット、画面 GIF・音声・触覚 (対応機のみ)。

## 3. WalkPlay / peq.szwalkplay.com — TP55 (USB DAC)

`peq.szwalkplay.com` のバンドル解析 + TP55 実機プローブ (3302:4383, consumer control IF
up=0x000C u=01) で確定。クラス分類は CB 系 (KT02H20/KT0231H は別の [sub,0,0,0,'R'|'W'] 10B フレーム、
KT1213 は report 87)。

### トランスポート

- **report 0x4B** (IN/OUT とも 64B)。wakeup report 0x54 も存在するが通常モードでは不要。
- 読み取り: `[0x80, sub, params...]` → 応答 `[0x4B, 0x80, sub, …]` (公式ディスパッチャは
  `e[1]`=sub で分岐、`e[3..]` に値)。
- 書き込み: `[0x01, sub, params...]` (応答無し)。保存 `[1,1,0]` (refereshToFlash)。
- **⚠ `[1,255]` = ブートローダ移行** (0666:0888 "USB20" 救砖モード。抜き差しでしか戻らない)。
  誤って 2 回発動済み — 書き込み系の適当打ちは厳禁。

### サブコマンド表 (TP55 実機で応答確認済み)

| cmd | 意味 | read 値位置 | write |
|---|---|---|---|
| `[80,0c]` | fw バージョン | ASCII | — |
| `[80,09,0,0,band]` | EQ バンド読み取り (**バンドセレクタは 5 バイト目** `[80,9,x]` は常に band0) | `[4B,80,09]` + param エコー `[0,0,band]` + 書き込みフレーム v[5..35] (生 [8..27]=係数, [28..29]=freq, [30..31]=q, [32..33]=gain, [34]=type, [36]=tag) | — |
| `[80,02]` | マイクオフセット | `e[3..4]` int16 **LE**、UI 値 = round(raw×15/32767)、範囲 -15..15 | `[1,2,2,lo,hi]` (raw = v×32767/15) |
| `[80,03,2,0,0]` | **EQ ボリューム** (Volume スライダー。`readDacOffset`/`setDacOffset`) | `e[4]` raw、UI 値 = raw-16、範囲 -16..+6 | `[1,3,2,0,raw]` (raw = v+16) |
| `[80,11]` | DAC フィルター | `e[3]`-1 → 0..4 | `[1,17,1,idx+1]` |
| `[80,16,2,ch,0]` | チャンネルバランス | `e[5]` (0 or 256-v)、ch0/ch1 独立 | **2 フレーム**: `[1,22,4,att側,0,-v,0]` + `[1,22,4,反対側,0,0,0]` (書かない側の減衰は残存するため必ず 0 クリア) |
| `[80,19]` | 出力ゲイン | `e[3]` 0=Low/1=High | `[1,25,1,v]` |
| `[80,1d]` | DAC ワークモード | `e[3]` 0=Class-H/1=Class-AB | `[1,29,1,v]` |
| `[80,0e]` | 4B (シリアル/機能?) | — | — |
| `[80,18]`/`[80,1e]`/`[80,1a]` | 意味未確定 | — | — |
| `[80,0d,0,addr32]` | **コードメモリ 32B 読み** (ファームダンプ用。設定はここに無い) | | |

TP55 非対応 (タイムアウト): ENC `[80,1b]`、フィルターバンク `[80,25,1,n]`、`[80,22,24,26,d3,da]`。

### TP55 の公式 UI (実機スクリーンショットで確認)

- **EQ Effect**: 10 バンド PEQ (freq/Q/タイプ/gain) + Volume (-16..+6dB)。
  プリセット DEF/CUSTOM/ONLINE/SHARE。TP55 はバンドル内の **CB1300D (s2) クラス**
  (reportId 75, `getBandCount()=10`)。
  - **バンドフレーム 36B** (読み書き共通レイアウト):
    `[1,9,24, 0,band, 0,0, <5×i32 LE 係数>, freq u16LE, q×256 u16LE, gain×256 i16LE, type, offset, tag]`
    - 係数は `[b0,b1,b2,-a1,-a2]` の biquad。**96kHz で計算** (実機応答とバイト一致で確認)。
      PK/LS/LP/HP は **Q30**、HS (公式 X1 クラス) のみ **Q27** + ±(2^27-1) クランプ +
      ゲイン×0.85 headroom + Q 0.5..5 クランプ。分母係数は符号反転で格納。
    - `type`: **LS=1, PK=2, HS=3, LP=4, HP=5** (getFilterIndexs)
    - `offset` (v[34]): 公式はスカラー field、常に 0 を観測
    - `tag` (v[35]): 読み取り応答の e[35] をそのままエコー (実機では 0x65、DEF プリセットの
      ハードコード値は 1)
  - **読み取り**: `[80,9,0,0,band]` (実機確認: `[80,9,x]` はどんな x でも band0 を返す。
    5 バイト目がバンドセレクタ)。応答は書き込みフレームとほぼ同じだが、param エコー
    `[0,0,band]` が 3 バイト余分に入るためメタ位置は **+1** (freq は生 [28..29])。
  - **適用**: 全バンドフレーム送信 → `[1,10,4,0,0,255,255]` (refereshToMemery、即時反映)
  - **保存**: さらに ~1 秒後 `[1,1,0]` (refereshToFlash)。公式 saveEqInfo と同じ順序。
  - ⚠ **実機メモ**: EQ 適用シーケンスの後に `output_gain` (cmd 25) がフラッシュ値側に
    戻る現象を観測 (live レジスタの未保存値が捨てられる挙動)。公式も同じシーケンスを
    使うためハードウェア仕様と判断。ドライバ側で特別な対処はしない。
  - **DEF プリセット** (バンドルにハードコード): 8 バンド
    `freq 50/200/500/1000/2500/5000/10000/15000, PK, Q 0.75, 0dB`。バンド 8/9 には触らない。
  - 係数実装は `devhub-core/src/peq.rs` (UI グラフと wire 生成で共有)。
- **MIC**: マイクオフセット 1 本 (範囲 -15..15)、リッスン、AI ENC (TP55 は「nonsupport」)。
  L/R のバーはブラウザマイクの VU メーターで設定項目ではない (ゲイン L/R は存在しない)。
- **DAC Filter**: DAC ワークモード (Class-H/Class-AB)、出力ゲイン (Low/High)、
  チャンネルバランス、デジタルフィルター 5 種 (FAST-LL/FAST-PC/SLOW-LL/SLOW-PC/NON-OS)。
- Firmware Update ボタンあり (group とは別系の救砖/更新フロー)。

### TP55 実機 E2E (書き込み→読み戻し往復)

全 6 設定で往復確認済み: dac_filter / dac_work_mode / output_gain / channel_balance (±30/0) /
eq_volume (-3/+2) / mic_offset (±5)。書き込み直後の読み取りが稀に 1 回ドロップする実機癖あり
→ ドライバの `cmd` はタイムアウト時 40ms 後に 1 回だけ再送する。

実装: `devhub-hid/src/walkplay.rs`。fixture テスト 6 件 (スカラー読み書き、EQ 読み取りデコード、
36B 書き込み wire、PeqAll シーケンス、係数の実機バイト一致)。
チャンネルバランスの極性は公式ディスパッチャ準拠: **ch0 減衰 = 正、ch1 減衰 = 負**
(正 = 左が減衰され音が右へ寄る)。

### 2.8 スキャンで見つかった未文書化コマンド (probe3/probe4、SU66 実機 2026-11-16)

全グループ×サブコマンド走査。応答規則: `[grp,0xff,…]` = 未対応、タイムアウト = write-only/無応答、
`[grp,sub,…]` エコー = 実在。sub 0 は全グループでエコー (generic ping)。

**実在・公式 UI 未使用 / 我々未実装:**
- `[3,3,layer,row,col]` GetKeyCode → キー単体読み (実機 Esc=0x29 確認)
- `[3,5,page]` GetKeyLayoutStyle → **ページ分割の物理レイアウト記述** (probe7 で確定):
  r[3] から **u16LE セル**を最大 ~15 個。各 u16 = `{row:5bit,col:7bit,ratio:4bit}`、
  row/col は ÷4 のクォーター単位、ratio は生値 (12=ノブ, 13-15=画面)。
  page 1-5 に実データ (row0 は空)。公式はこれでキーマップ canvas を描く (我々は layout をハードコード)
- `[3,6,(system<<4)|fn,row]` GetKeyLayoutDefault → **工場出荷キーマップ** (4 fn × 2 OS = 8 セット、
  row ごと 30×u16)。実機: fn0/win row1 = Esc,1..9 (0x29,0x1e-0x26)。「工場レイアウト復元」に使える
  → **`reset_keymap` / `KeyMapReset` は factory_stream でこれを読んで書き戻すように変更済み**
  (従来は Fn 層を全透過で上書きし、工場 Fn1 の数字行→F1-F12 を潰していた)
  エコー形式は `[3,6,sel,row]` → `r[2]=sel, r[3]=row` (probe8 確認、factory_stream が照合)。
  fn1 工場値 row2 には 0xf3xx 系メディアコードあり (F7,F0,F1,F17,F10)。sys0/sys1 で同一行が同値のことあり
- `[5,5,area,chunk,…15×(B,G,R,flag)]` **DirectDrive** — 応答なしの per-LED 直駆動
  (SDK sendDataNoResponse = report 書くだけ)。公式 SDK は公開するが UI で未使用。
  実機/OBSカメラ確認 (2026-11-16):
  - `flag=255` = isCustom を立ててセルをピン留め。**ストリーム停止後も最後の色が残る** —
    ファーム側 DD タイムアウト/keepalive は無い。明示解除が必須。
  - `flag=0` = **このセルは触らない** (解放でも消灯でもない)。DD に含めない/flag=0 のセルは
    ファームのアニメーションがそのまま駆動する → **ハイブリッド可能** (赤 DD 7セル +
    背景のファーム水色エフェクトを同時表示)。
  - セル解放は `[5,4]` カスタム書き込みで isCustom=0 にする。`direct_drive` は pin/色を追跡し、
    外れたセルは readback で他セル温存のうえ `[5,4]` 解放、未変化セル/flag=0 のみチャンクは再送しない。
  - `direct_drive_stop` = `[5,4]` 全解除 + `[5,2]` 再送 → 全面ファーム復帰を実機確認。
  **実装済み**: ライティングタブ「実機でプレビュー」+ Live ハイブリッド → 実機 LED が同期。
- `[5,6,B,G,R]` Caps = setLightingCaps — 書き込み受理 (エコー)。読み取り `[5,6,arg]` は
  r[3]=arg エコーのみで値は返らない (write-only)。CapsLock インジケーター色と推定 (未目視確認)
- `[2,15,*]` — enum 上は抜け番 (Wa: …MacroSpaceInfoQuery=14, ShakeOptimizationSwitch=16) だが
  `[2,15,0]` は ECHO(全0) で応答 — ハンドラ自体は残っている (2026-11-16 実測)
- `[9,1]`/`[9,2]` (Touch/haptic = dt.Pi): GetHapticParams/GetHapticListAndPosition が応答するが
  DeviceFeature ext=0 → ハード無し。残り sub は 0xff
- `[10,3]`〜`[10,11]` (Custom): ハンドラあり・引数エコーのみ・データ全 0 — OEM 拡張枠、意味なし
  (2026-11-16 再掃引: sub 0,3,5,7,9,11 が ECHO、13,15 は UNSUP — 奇数 sub のみ実装)
- `[2,17,0]` は ECHO(全0) で detach せず (下の「危険」注記の detach は別条件 —
  引き続き arg>0 は送らないこと)
- `[5,1]` エリア列挙: SU66 は area 0(キー)/1(装飾) のみ、area2 以降は UNSUP (実測確定)
- group 0/11/12/14-20 は SU66 では UNSUP またはタイムアウト (grp12 Displayer は
  axisType=2 でも ext=0 のためハンドラ無し — SDK 側の機能表どおり)
- Custom リソース enum (`mr`): {?=2:ImageResources, UpgradeFlash=3, GIF=4, bootAnimation=5}
  — 画面付き機種向けのアセットアップコマンド群
- **grp14 Voice (スピーカー搭載機のみ、SU66 非対象)** — SDK から解読:
  `[14,10,ctl,index,…60B]` = playVoiceRealTime リアルタイム音声ストリーム。
  ctl enum: Stop=0 Start=1 TransmitDirect=2(生PCM) TransmitADPCM=3 RestartDirect=4 RestartADPCM=5。
  他: 1=GetVoiceParams 3=StartVoiceUpdate 4=SendVoiceFile 5=StopVoiceUpdate 6=ReadVoiceInfo
  7-9=ReadVoiceFile 系 11=PlayVoice 12=VoiceSpaceManagement 13=WriteKeyVoice 14=ReadKeyVoice
  15=ReadKeyVoiceByPosition — キー音声割り当て機能。エラー enum: 249=TidySpace
  250=InsufficientSpace 251=CommandRejected 252=ChecksumError 253=ParameterError
  254=IndexNotFound 255=PeripheralFault
- grp12 Displayer の enum 名は `Jr` を共有するが、実際に SDK が使うのは Voice ストリーム側
  (「Displayer」は画面系の別関数群 getDisplayerInfo 等 — grp12 は画面付き機種のブート/GIF/
  flash アセット管理)。SU66 の axisType=2 (WithScreen) でも DeviceFeature ext=0 で画面機能なし

**レイヤー数は 4 固定 (2026-11-16 probe5 確定):**
- SDK は `fn0..fn3` を**固定で 4 層**読む。実機: layer0-3 全て実データ、layer4 は全 0。
  layer1 = 工場 Fn 層 (数字行→F1-F12, Esc→`)、layer2/3 = 空のカスタム層 (全透過 0001)
- 公式 UI の層名: index0=メインレイヤー, 1-3=Fn層。切替キーコード 0xF100-0xF103 (61696-61699)
- **以前は DeviceFeature r[2] をレイヤー数と誤解釈** (r[2]=axisType=2 と層数=2 が偶然一致)。
  → `xs.layers = 4` に修正、層名は メイン/Fn1/Fn2/Fn3
- axisType enum 完取: 0=MagneticAxisWired, 1=MagTriple, 2=**MagneticAxisWiredWithScreen (SU66)**,
  3-5=Mechanical系, 6-8=Optical系, 9-11=Inductive系, 12=MagWiredFullScreen,
  13=Vibration, 14=Voice, 15=VibrationVoice, 16=Gamepad
- DeviceFeature `[1,3]` 完全デコード: r[2]=axisType, r[3]=connectionMode,
  r[4]={bit0:rgbLighting, bit1:knob}, r[5]={bit0:smallScreen, bit1:fullScreen, bit2:haptic, …, numpad, dotMatrix}

**危険 — 絶対に送らない:**
- `[2,17,*]` — **ファームが USB から detach する** (0x48F)。2 回再現、replug/再接続で復旧。
  DFU/OTA ブート入口かデバッグコマンドの疑い
- 同じく `[3,3,0,1,0]` を 3 引数で連打したケースでも detach 観測 — 未知コマンド連打は
  reopen-per-probe でやること

**公式 UI 側にあるが我々が未実装の機能 (価値順):**
1. ~~工場デフォルトキーマップ参照 (`[3,6]`)~~ → **実装済み** (factory_stream で reset に使用)
2. ~~DirectDrive ソフトウェア RGB (`[5,5]`)~~ → **実装済み** (実機プレビュー)
3. keyLayoutStyle (`[3,5]`) — 物理レイアウト自己記述 (ノブ/画面検出、将来機種対応用)
4. Caps 色 (`[5,6]`) — CapsLock インジケーター色 (write-only、UI 化するかは要検討)

## 4. ATK HUB (hub.atk.pro / ATK HUB 3.2.x) — ATK F1 V3 ULTIMATE+ (373b:12a7)

解析元: `C:\Program Files\ATK HUB` (Electron) + `https://desktop.vxe.com/v3hub` + `https://hub.atk.pro`
(hub-v3/production/3.2.33)。デスクトップアプリの UI はリモートの Web アプリで、HID I/O は
preload が露出する `window.nodeHid` = WebHID 相当 (`navigator.hid`) が直接行う。
`resources/service/atk-hub-service.exe` は汎用 HID 中継エージェント (Rust+hidapi、パイプ)、
`atk-mouse-system-api*.node` は OS のマウス設定用でデバイスプロトコルは含まない。
ファームウェア ISP だけ別 (sn_isp_lib_bg.wasm、SN32F24xB 向け)。

### 4.1 トランスポート (実機確認 2026-11-15)

- **コレクション: usagePage 0xff02 / usage 2 (MI_01 Col05), report ID 0x08**
- 書き込みは 16B パケット + reportId=8 の output report (パディング長は不問)。
- ff03/ff04/ff05 は出力レポートを持たない (WriteFile 不可)。ff06 は別レポート体系。
- 要求:  `[0]=cmd [1]=0 [2..3]=addr u16BE [4]=len [5..14]=data [15]=checksum`
- 応答:  `[0]=0x08(reportId) [1]=cmd echo [2]=status [3..4]=addr [5]=len [6..]=data [16]=checksum`
- checksum = `0x55 - ((8 + sum(bytes[0..14])) & 0xff)` (JS `Si` クラス、定数 8 は reportId)
- status: Success=0 / Fail=1 / EEPROMError=2 / NotSupport=255

### 4.2 コマンド enum (D)

```
DownLoadData=1 DownLoadDriverStatus=2 GetWirelessMouseOnline=3 GetBatteryLevel=4
SetWirelessDonglePair=5 GetWirelessDonglePairResult=6 SetEEPROM=7 GetEEPROM=8
RestoreFactory=9 ReportMouseStatus=10 EnterUSBUpgradeMode=13
GetCurrentConfig=14 SetCurrentConfig=15 GetMouseCIDMID=16 GetMouseVersion=18
DongleExitPair=19 Set4KRGBMode=20 Get4KRGBMode=21 SetFarDistanceMode=22
GetFarDistanceMode=23 SetDongleLightMode=24 GetDongleLightMode=25 GetDongleVersion=29
ReportMouseUpgradeError=90 ReportMouseUpgradeStatus=91
```

### 4.3 EEPROM アドレスマップ (JS `y.Address` + 実機値)

全レコードは `value, crc` ペア (crc = 0x55 - value)。SetEEPROM で書き、GetEEPROM で読む。

| addr | 内容 | 実機値 |
|---|---|---|
| 0 | reportRate ブロック (10B): rate,maxDpi,currentDpi,bhop,buttonOpMode + 各crc | `[64,21,4,81,2,83,0,85,0,85]` = 8KHz・4段・現在stage2・bhop/btnop off |
| 10 | silentHeight (LOD) | `1` |
| 12,20,28,36 | dpi1/3/5/7 旧式レコード `{xDpi,yDpi,dpiEx,crc}` | `[7,7,0,71]` `[31,31,0,23]` |
| 44,52,60,68 | dpi1/3/5/7 色 RGB+crc | `[255,0,0,86]` |
| 76-95 | DPI RGB ライト (effects,brightness,speed,enable / articleLamp R,G,B,effects,longBright,breathSpeed,energySave) | — |
| 96+4k | key0..15 ボタン割当 (4B) | `[fn,param,0,crc]` fn:Mouse=1 Dpi=2 FirepowerKey=4 ShortcutKey=5 Macro=6 ReportRate=7 ConfigFile=9 DpiLock=10 Wheel=11 TurnAround=12; param bit: Left=1 Right=2 Middle=4 Side1=8 Side2=16 |
| 160 | decorationLight ブロック | — |
| 169 | stabilizationTime (デバウンス?) | 0 |
| 171 | motionSync | 0 |
| 173 | closeLedTime (LED消灯, 分) | 6 |
| 175 | linearCorrection (角度スナップ) | 0 |
| 177 | rippleControl (リップル補正) | 0 |
| 179 | moveCloseLights (移動中消灯) | 0 |
| 181/183/185 | sensorEnable / sensorTime / sensorMode (スリープ系) | 0,1,6 |
| 187 | rfTxTime | 2 |
| 189 | angle | — |
| 227 | rollingDelay (ホイールデバウンス?) | 30 |
| 256+32k | keyShortcuts0..15 (32B/個、**slot=物理ボタン index**) | 0xff=空 |
| 768+384k | macro0..15 (384B/個) | — |
| 6912+6k | **paw3955Dpi1..8** (PAW3955 系は旧 dpi レコードではなくこちら) | 6B/段: `xdpi u16LE, ydpi u16LE, ex, crc=0x55-sum(5B)`、**値は dpi-1** |
| 6960 | dynamicSensitivity (enable + ポイント列) | — |
| 6964 | dynamicSensitivityPoints | — |
| 6984/6992 | sensorCenterPoint / v2 | — |
| 6988 | fastTriggerDebounce | — |

- ReportRate 値 (bitmask): Hz125=8 250=4 500=2 1000=1 2000=16 4000=32 8000=64
- キー fn enum: Mouse=1 Dpi=2 FirepowerKey=4 ShortcutKey=5 Macro=6 ReportRate=7 Chandeliers=8 ConfigFile=9 DpiLock=10 Wheel=11 TurnAround=12
- ボタン bit: Left=1 Right=2 Middle=4 Side1=8 Side2=16

### 4.4 実機応答 (2026-11-15, F1 V3 ULTIMATE+ 有線)

```
cmd16 CIDMID  -> [01 3e 16] CID=1 MID=62 (= デバイスDB "1,62" = F1 V3)
cmd18 version -> [01 21]     (FW 1.33?)
cmd4  battery -> [5f 01]     95% + 充電状態 1
@6912 dpi     -> [143,1 143,1 0 53]=400 [31,3]=800 [63,6]=1600 [127,12]=3200 x5段分
                 → 実アクティブは @0.maxDpi=4 段: 400/800/1600/3200
```

### 4.5 デバイス DB エントリ (useRemoteConfig)

`{vendorId:14139(0x373b), productId:4775(0x12a7), usagePage:65284(0xff04), usage:2,
name:"ATK F1 V3 ULTIMATE +", sensor:PAW3955MASTER, mouseCidMid:"1,62",
firmwareMark:"f1", upgradeTogether:true, BottomButton:false, DpiRgb:false, DpiToggle:true}`

注意: DB の usagePage=0xff04 は列挙フィルタ用で、実際のコマンド経路は ff02/u2。
同世代機種 (mouseCidMid "1,x") は全てこの 16B/ff02 方式と思われる。旧世代 "2,x"
(VXE R1 系) は usagePage 0xff05/usage1 フィルタで別プロトコルの可能性。

### 4.6 実機で検証済みの追加仕様 (2026-11-15 第2ラウンド)

- **ShortcutKey (keyfn type5)**: `keyShortcuts{物理ボタンindex}` @256+slot*32。
  実機ダンプ (HTS Plus Pro, 公式 Hub 書込後): `02 81 1f 00 41 1f 00 53 00…`。
  形式 `[count][press entry×k][release entry×k (press の逆順)][crc]` —
  entry = 3B `{keyState, v1, v2}`、keyState = `0x80|type`(押下) / `0x40|type`(解放)。
  **crc = `0x55 - sum(count + 全entry)` が index `1+3*count` に付く** —
  書き込みは `count*3+2` B だけ (残りは書かない。crc が無いと本体側で受理されない)。
  type: **0=修飾 (v1=mod フラグ 1/2/4/8/16/32/64/128)、1=Keyboard usage LE、2=メディア (consumer code)**。
  修飾の押下順は [Ctrl,Alt,Shift,Win]、解放は押下の逆順。
  実機: サイドボタン = Keyboard usage 31/30 (type1、修飾無し)。
- **closeLedTime @173**: wire 値は **10 秒単位** (6 = 60s = スリープ 1 分)。書き込み往復確認。
- **stabilizationTime @169**: クリックデバウンス ms。往復確認。
- **bhop @0 byte6**: ホイール誤操作防止 (公式 `time/10`)。往復確認。
- **moveCloseLights @179**: 移動中 LED 消灯。往復確認。
- **プロファイル cmd14/15**: `cmd14 → data[0]=active index`、`cmd15 data=[index]` で切替。
  index 0–3 まで実機で往復確認 (4 プロファイル)。
- **長距離モード cmd22/23**: 有線では cmd22 は ACK ([2]=1) するが cmd23 は常に全 0 →
  **ドングル側機能**で有線経路では反映されない。設定には出さない。
- **RestoreFactory cmd9**: ファクトリーリセット (ActionSpec・danger 確認付き)。
- 未実装: macro @768 領域のフォーマット (384B/slot)、dynamicSensitivity @6960+、
  sensorAngle @189、sensorEnable/Time/Mode @181/183/185、BT。

### 4.8 ドングル経由 (373b:1278 "Wireless mouse 8k dongle-L", 実機検証済み)

- **同じ 16B/report-8 プロトコルを透過中継** — ff02/u2 コレクションで全コマンド疎通。
- cmd16 → CID=1, MID=62 (内側の F1 V3 を確定)。registry 側は受信機名のまま出さず
  CID/MID で機種確定 (`maybe_detect`、休眠中は未確定 → 覚醒後に再試行)。
- **cmd29 = ドングル FW** (実機: 5,1 → 5.1) — マウス休眠中もドングル自身が応答。
- cmd3 GetWirelessMouseOnline — ドングル側応答 (常時返る)。
- **休眠検知**: 中継コマンド (cmd16/18/EEPROM) がタイムアウト → asleep。
  数十秒のアイドルで寝る。ドングル自身のコマンド (cmd3/29) は寝ていても応答。
- LOD @10: **1–11 → 0.7–1.7mm (0.1mm 刻み)** — 実機で write+公式表示対応を確認
  (@10=5 → 公式表示 "1.1mm")。0/12 以上も EEPROM には書けるが公式範囲外。
- ドングル経由でも全 EEPROM 読み書き・cmd14/15 プロファイル切替が動作 (往復確認)。

### 4.7 その他

- ファームウェア: `api-hub-cloud.atk.pro/firmware/{vid}/{pid}/{mark}` で取得。
- ワイヤレスドングル経由: `GetWirelessMouseOnline=3` / ペアリング cmd5/6/19、電池 cmd4、
  ドングル FW cmd29。応答は同じ 16B 構造。
- `is54H20` フラグを持つ機種あり (別 MCU バリアント?)。
- プローブ: `cargo run -p devhub-hid --example atkprobe` (読み取りのみ)。
