//! Razer 90B Feature Report ドライバ (DeathAdder V4 Pro 等)。
//!
//! OpenRazer の 90B USB Feature Report プロトコル準拠 + 実機 `razerprobe` 検証済み:
//! - Windows hidapi では `if=0 up=0001 u=02` (または応答する別コレクション) に対し、
//!   先頭に Report ID `0x00` を付けた 91 バイトで `send_feature_report` / `get_feature_report` する。
//! - `tx_id = 0x1F`、CRC = bytes `[2..88]` の XOR を `report[88]` に格納。
//! - DeathAdder V4 Pro (`1532:00BF` / `0x00BE`):
//!   - FW (`0x00/0x81`), Serial (`0x00/0x82`), Polling Rate 2 (`0x00/0xC0`/`0x40`, 2回書込み),
//!   - DPI XY (`0x04/0x85`/`0x05`), DPI Stages (`0x04/0x86`/`0x06`, 1-based),
//!   - Battery (`0x07/0x80`), Charging (`0x07/0x84`), Idle Time (`0x07/0x83`/`0x03`),
//!   - Low Battery Threshold (`0x07/0x81`/`0x01`), Dongle Multi Indicator LEDs (`0x07/0x95`/`0x15`).

use devhub_core::backend::*;
use devhub_core::model::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TX_ID: u8 = 0x1F;
const VARSTORE: u8 = 0x01;

const STATUS_NEW: u8 = 0x00;
const STATUS_BUSY: u8 = 0x01;
const STATUS_SUCCESS: u8 = 0x02;
const STATUS_TIMEOUT: u8 = 0x04;
const STATUS_NOT_SUPPORTED: u8 = 0x05;

type Watcher = Option<Arc<dyn Fn(HubEvent) + Send + Sync>>;

/// Razer 90B レポートの XOR チェックサム (`report[2..88]`)。
pub fn crc90(report: &[u8; 90]) -> u8 {
    let mut crc = 0u8;
    for &b in &report[2..88] {
        crc ^= b;
    }
    crc
}

/// 90B Razer コマンドレポートを組み立てる。
pub fn build_report(
    tx_id: u8,
    data_size: u8,
    cmd_class: u8,
    cmd_id: u8,
    args: &[u8],
) -> [u8; 90] {
    let mut r = [0u8; 90];
    r[0] = STATUS_NEW;
    r[1] = tx_id;
    r[5] = data_size;
    r[6] = cmd_class;
    r[7] = cmd_id;
    let n = args.len().min(80);
    r[8..8 + n].copy_from_slice(&args[..n]);
    r[88] = crc90(&r);
    r
}

/// 応答バッファ（91B または 90B）から 90B レポートを取り出す。
pub fn unpack_response(raw: &[u8]) -> HubResult<[u8; 90]> {
    let mut out = [0u8; 90];
    if raw.len() >= 91 {
        out.copy_from_slice(&raw[1..91]);
        Ok(out)
    } else if raw.len() == 90 {
        out.copy_from_slice(&raw[0..90]);
        Ok(out)
    } else {
        Err(HubError::Protocol(format!(
            "razer: unexpected feature report length {}",
            raw.len()
        )))
    }
}

/// ポーリングレート (Hz) → Razer `0x00/0x40` コード。
pub fn encode_polling_rate(hz: u32) -> u8 {
    match hz {
        8000 => 0x01,
        4000 => 0x02,
        2000 => 0x04,
        1000 => 0x08,
        500 => 0x10,
        250 => 0x20,
        125 => 0x40,
        _ => 0x08,
    }
}

/// Razer `0x00/0xC0` コード → ポーリングレート (Hz)。
pub fn decode_polling_rate(code: u8) -> Option<u32> {
    match code {
        0x01 => Some(8000),
        0x02 => Some(4000),
        0x04 => Some(2000),
        0x08 => Some(1000),
        0x10 => Some(500),
        0x20 => Some(250),
        0x40 => Some(125),
        _ => None,
    }
}

/// `0x04/0x86` 応答ペイロード (`resp[8..]`) から `(active_0based, stages)` をデコードする。
pub fn decode_dpi_stages(args: &[u8]) -> HubResult<(usize, Vec<DpiStage>)> {
    if args.len() < 3 {
        return Err(HubError::Protocol("razer: short dpi stages".into()));
    }
    let active_1based = args[1] as usize;
    let count = (args[2] as usize).min(5);
    let mut stages = Vec::with_capacity(count);
    for i in 0..count {
        let off = 3 + i * 7;
        if off + 5 > args.len() {
            break;
        }
        let x = u16::from_be_bytes([args[off + 1], args[off + 2]]) as u32;
        let y = u16::from_be_bytes([args[off + 3], args[off + 4]]) as u32;
        stages.push(DpiStage {
            x: x.clamp(100, 45000),
            y: if y != x {
                Some(y.clamp(100, 45000))
            } else {
                None
            },
            color: Rgb::hex(0x44d62c),
            enabled: true,
        });
    }
    if stages.is_empty() {
        return Err(HubError::Protocol("razer: empty dpi stages".into()));
    }
    let active = active_1based
        .saturating_sub(1)
        .min(stages.len().saturating_sub(1));
    Ok((active, stages))
}

/// `0x04/0x06` 用引数（`args[0..38]`）をエンコードする（1-based stage id）。
pub fn encode_dpi_stages(stages: &[DpiStage], active_0based: usize) -> Vec<u8> {
    let count = stages.len().clamp(1, 5);
    let active_1based = (active_0based.min(count - 1) + 1) as u8;
    let mut args = vec![0u8; 3 + count * 7];
    args[0] = VARSTORE;
    args[1] = active_1based;
    args[2] = count as u8;
    for (i, st) in stages.iter().take(count).enumerate() {
        let off = 3 + i * 7;
        let x = st.x.clamp(100, 45000) as u16;
        let y = st.y.unwrap_or(st.x).clamp(100, 45000) as u16;
        args[off] = (i + 1) as u8;
        args[off + 1..off + 3].copy_from_slice(&x.to_be_bytes());
        args[off + 3..off + 5].copy_from_slice(&y.to_be_bytes());
    }
    args
}

/// バッテリー生値 (`0..=255`) → `0..=100` %。
pub fn raw_to_pct(raw: u8) -> u8 {
    ((raw as u32 * 100 + 127) / 255).min(100) as u8
}

/// 低バッテリー警告閾値 (`5..=25` %) ↔ デバイス値 (`0x0C..=0x4D`)。
/// 実機 `razerprobe` では 30% (`0x4D`) まで設定例があるため `0x0C..=0x7F` を許容し、UI は `5..=30%` (5% 刻み) にする。
pub fn pct_to_low_batt(pct: f32) -> u8 {
    let clamped = pct.round().clamp(5.0, 30.0) as u32;
    ((clamped * 255) / 100).clamp(0x0C, 0x4D) as u8
}

pub fn low_batt_to_pct(raw: u8) -> f32 {
    let pct = (raw as f32 * 100.0 / 255.0).round();
    ((pct / 5.0).round() * 5.0).clamp(5.0, 30.0)
}

pub fn idle_sec_to_min(sec: u16) -> f32 {
    ((sec as f32) / 60.0).round().clamp(1.0, 15.0)
}

pub fn idle_min_to_sec(min: f32) -> u16 {
    (min.round().clamp(1.0, 15.0) as u16) * 60
}

trait TransportOps: Send + Sync {
    fn xfer(&self, req: &[u8; 90]) -> HubResult<[u8; 90]>;
}

struct HidTransport {
    dev: Mutex<hidapi::HidDevice>,
}

impl TransportOps for HidTransport {
    fn xfer(&self, req: &[u8; 90]) -> HubResult<[u8; 90]> {
        let dev = self.dev.lock().unwrap_or_else(|p| p.into_inner());
        let mut last_err = HubError::Protocol("razer: no response".into());
        for _retry in 0..3 {
            let mut w = [0u8; 91];
            w[0] = 0x00;
            w[1..91].copy_from_slice(req);
            if let Err(e) = dev.send_feature_report(&w) {
                return Err(HubError::disconnected_if_lost(&e.to_string()));
            }
            for attempt in 0..5 {
                std::thread::sleep(Duration::from_millis(if attempt == 0 { 15 } else { 20 }));
                let mut r = [0u8; 91];
                r[0] = 0x00;
                let len = match dev.get_feature_report(&mut r) {
                    Ok(n) => n,
                    Err(e) => {
                        last_err = HubError::disconnected_if_lost(&e.to_string());
                        break;
                    }
                };
                let resp = unpack_response(&r[..len])?;
                if resp[2] != req[2] || resp[3] != req[3] || resp[6] != req[6] || resp[7] != req[7]
                {
                    last_err = HubError::Protocol("razer: mismatched response".into());
                    continue;
                }
                match resp[0] {
                    STATUS_SUCCESS => return Ok(resp),
                    STATUS_BUSY => {
                        last_err = HubError::Protocol("razer: busy".into());
                        continue;
                    }
                    STATUS_TIMEOUT => return Err(HubError::Protocol("razer: wireless timeout".into())),
                    STATUS_NOT_SUPPORTED => {
                        return Err(HubError::Unsupported("razer: command not supported"))
                    }
                    st => {
                        return Err(HubError::Protocol(format!("razer: command failed ({st:#04x})")))
                    }
                }
            }
        }
        Err(last_err)
    }
}

trait HubErrorExt {
    fn disconnected_if_lost(msg: &str) -> HubError;
}

impl HubErrorExt for HubError {
    fn disconnected_if_lost(msg: &str) -> HubError {
        let l = msg.to_lowercase();
        if l.contains("not connected")
            || l.contains("no such device")
            || l.contains("0x0000048f")
            || l.contains("1167")
            || l.contains("device has been removed")
        {
            HubError::Disconnected
        } else {
            HubError::Io(format!("razer hid: {msg}"))
        }
    }
}

pub struct RazerMouse {
    io: Arc<dyn TransportOps>,
    info: Mutex<DeviceInfo>,
    events: Watcher,
    caps: Capabilities,
    has_dongle_leds: bool,
    last_stages: Mutex<(usize, Vec<DpiStage>)>,
}

fn test_device_fw(dev: &hidapi::HidDevice) -> HubResult<(u8, u8)> {
    let req = build_report(TX_ID, 0x02, 0x00, 0x81, &[]);
    let mut w = [0u8; 91];
    w[0] = 0x00;
    w[1..91].copy_from_slice(&req);
    dev.send_feature_report(&w)
        .map_err(|e| HubError::Io(format!("razer probe write: {e}")))?;
    for attempt in 0..5 {
        std::thread::sleep(Duration::from_millis(if attempt == 0 { 15 } else { 20 }));
        let mut r = [0u8; 91];
        r[0] = 0x00;
        let len = dev
            .get_feature_report(&mut r)
            .map_err(|e| HubError::Io(format!("razer probe read: {e}")))?;
        let resp = unpack_response(&r[..len])?;
        if resp[0] == STATUS_BUSY {
            continue;
        }
        if resp[0] == STATUS_SUCCESS && resp[6] == 0x00 && resp[7] == 0x81 {
            return Ok((resp[8], resp[9]));
        }
        return Err(HubError::Protocol(format!(
            "razer probe status={:#04x}",
            resp[0]
        )));
    }
    Err(HubError::Protocol("razer probe busy".into()))
}

fn resolve_working_device(
    dev: hidapi::HidDevice,
    vid: u16,
    pid: u16,
    serial: Option<&str>,
) -> HubResult<hidapi::HidDevice> {
    if test_device_fw(&dev).is_ok() {
        return Ok(dev);
    }
    let api = hidapi::HidApi::new().map_err(|e| HubError::Io(format!("hidapi: {e}")))?;
    for d in api.device_list() {
        if d.vendor_id() != vid || d.product_id() != pid {
            continue;
        }
        if let Some(ser) = serial {
            if d.serial_number().is_some_and(|s| !s.is_empty() && s != ser) {
                continue;
            }
        }
        if let Ok(cand) = api.open_path(d.path()) {
            if test_device_fw(&cand).is_ok() {
                return Ok(cand);
            }
        }
    }
    Ok(dev)
}

pub fn open(
    dev: hidapi::HidDevice,
    mut info: DeviceInfo,
    events: Watcher,
) -> HubResult<RazerMouse> {
    let serial_str = dev
        .get_serial_number_string()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let dev = resolve_working_device(dev, info.vid, info.pid, serial_str.as_deref())?;
    let io: Arc<dyn TransportOps> = Arc::new(HidTransport {
        dev: Mutex::new(dev),
    });

    // FW & Serial
    if let Ok(r) = io.xfer(&build_report(TX_ID, 0x02, 0x00, 0x81, &[])) {
        info.firmware = Some(format!("v{}.{}", r[8], r[9]));
    }
    if info.serial.is_none() {
        if let Ok(r) = io.xfer(&build_report(TX_ID, 0x16, 0x00, 0x82, &[])) {
            let s = String::from_utf8_lossy(&r[8..30])
                .trim_end_matches('\0')
                .to_string();
            if !s.is_empty() {
                info.serial = Some(s);
            }
        }
    }

    // Check if dongle multi-indicator LEDs command (0x07/0x95) is supported (wireless dongle).
    let has_dongle_leds = info.transport == Transport::Wireless24G
        && io
            .xfer(&build_report(TX_ID, 0x03, 0x07, 0x95, &[]))
            .is_ok();

    let caps = build_caps(has_dongle_leds);
    let drv = RazerMouse {
        io,
        info: Mutex::new(info),
        events,
        caps,
        has_dongle_leds,
        last_stages: Mutex::new((
            0,
            vec![DpiStage {
                x: 800,
                y: None,
                color: Rgb::hex(0x44d62c),
                enabled: true,
            }],
        )),
    };

    // 初回バッテリー取得
    if let Ok(b) = drv.battery() {
        drv.info.lock().unwrap_or_else(|p| p.into_inner()).battery = Some(b);
    }

    Ok(drv)
}

fn build_caps(has_dongle_leds: bool) -> Capabilities {
    let mut settings = vec![
        SettingSpec::choice(
            "polling_rate",
            section::PERFORMANCE,
            "ポーリングレート",
            vec![
                ChoiceOpt::new(125, "125 Hz"),
                ChoiceOpt::new(250, "250 Hz"),
                ChoiceOpt::new(500, "500 Hz"),
                ChoiceOpt::new(1000, "1000 Hz"),
                ChoiceOpt::new(2000, "2000 Hz"),
                ChoiceOpt::new(4000, "4000 Hz"),
                ChoiceOpt::new(8000, "8000 Hz"),
            ],
        ),
        SettingSpec::slider("sleep_time", section::POWER, "スリープ移行", 1., 15., 1., "分")
            .help("無操作で省電力スリープに入るまでの時間"),
        SettingSpec::slider(
            "low_battery_threshold",
            section::POWER,
            "低バッテリー通知閾値",
            5.,
            30.,
            5.,
            "%",
        )
        .help("この残量以下で低バッテリー警告表示に切り替わります"),
    ];
    if has_dongle_leds {
        let led_opts = vec![
            ChoiceOpt::new(0, "オフ"),
            ChoiceOpt::new(1, "バッテリー残量"),
            ChoiceOpt::new(2, "接続状態"),
            ChoiceOpt::new(3, "ポーリングレート"),
            ChoiceOpt::new(4, "DPI"),
        ];
        settings.push(SettingSpec::choice(
            "dongle_led_left",
            section::CONNECTION,
            "ドングル LED (左)",
            led_opts.clone(),
        ));
        settings.push(SettingSpec::choice(
            "dongle_led_center",
            section::CONNECTION,
            "ドングル LED (中央)",
            led_opts.clone(),
        ));
        settings.push(SettingSpec::choice(
            "dongle_led_right",
            section::CONNECTION,
            "ドングル LED (右)",
            led_opts,
        ));
    }

    Capabilities {
        settings,
        actions: vec![],
        profiles: None,
        mouse: Some(MouseCaps {
            dpi_min: 100,
            dpi_max: 45000,
            dpi_step: 50,
            min_stages: 1,
            max_stages: 5,
            xy_split: true,
            stage_colors: false,
            stage_toggle: false,
            buttons: vec![],
            button_kinds: vec![],
            require_left_click: false,
            macros: None,
            live_buttons: vec![],
            adaptive_dpi: None,
        }),
        keyboard: None,
        lighting: vec![],
        battery: true,
        peq: None,
        firmware_feed: None,
    }
}

impl RazerMouse {
    fn set_asleep(&self, asleep: bool) {
        let mut info = self.info.lock().unwrap_or_else(|p| p.into_inner());
        if info.asleep != asleep {
            info.asleep = asleep;
            let snapshot = info.clone();
            drop(info);
            if let Some(cb) = &self.events {
                cb(HubEvent::Info(snapshot.id.clone(), Box::new(snapshot)));
            }
        }
    }

    fn call(&self, size: u8, class: u8, cmd: u8, args: &[u8]) -> HubResult<[u8; 90]> {
        let req = build_report(TX_ID, size, class, cmd, args);
        match self.io.xfer(&req) {
            Ok(r) => {
                self.set_asleep(false);
                Ok(r)
            }
            Err(HubError::Protocol(m)) if m.contains("timeout") => {
                self.set_asleep(true);
                Err(HubError::Protocol(m))
            }
            Err(e) => Err(e),
        }
    }

    fn read_dongle_leds(&self) -> HubResult<[u8; 3]> {
        let r = self.call(0x03, 0x07, 0x95, &[])?;
        Ok([r[8], r[9], r[10]])
    }
}

impl DeviceDriver for RazerMouse {
    fn info(&self) -> DeviceInfo {
        self.info.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn caps(&self) -> Capabilities {
        self.caps.clone()
    }

    fn read(&self) -> HubResult<DeviceState> {
        let mut settings = BTreeMap::new();

        if let Ok(r) = self.call(0x01, 0x00, 0xC0, &[]) {
            if let Some(hz) = decode_polling_rate(r[9]) {
                settings.insert("polling_rate".into(), SettingValue::Choice(hz as i64));
            }
        }
        if let Ok(r) = self.call(0x02, 0x07, 0x83, &[]) {
            let sec = u16::from_be_bytes([r[8], r[9]]);
            settings.insert("sleep_time".into(), SettingValue::Num(idle_sec_to_min(sec)));
        }
        if let Ok(r) = self.call(0x01, 0x07, 0x81, &[]) {
            settings.insert(
                "low_battery_threshold".into(),
                SettingValue::Num(low_batt_to_pct(r[8])),
            );
        }
        if self.has_dongle_leds {
            if let Ok([l, c, ri]) = self.read_dongle_leds() {
                settings.insert("dongle_led_left".into(), SettingValue::Choice(l as i64));
                settings.insert("dongle_led_center".into(), SettingValue::Choice(c as i64));
                settings.insert("dongle_led_right".into(), SettingValue::Choice(ri as i64));
            }
        }

        let (active_stage, stages) = match self.call(0x26, 0x04, 0x86, &[VARSTORE]) {
            Ok(r) => decode_dpi_stages(&r[8..46])?,
            Err(e) => {
                // スリープ中や直前のエラーなら最後の値を返すか失敗する
                if settings.is_empty() {
                    return Err(e);
                }
                self.last_stages
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone()
            }
        };
        *self.last_stages.lock().unwrap_or_else(|p| p.into_inner()) =
            (active_stage, stages.clone());

        Ok(DeviceState {
            profile: ProfileState {
                active: 0,
                names: vec!["オンボード".into()],
            },
            settings,
            mouse: Some(MouseState {
                stages,
                active_stage,
                buttons: vec![],
                macros: vec![],
                adaptive: None,
            }),
            keyboard: None,
            lighting: BTreeMap::new(),
            peq: None,
        })
    }

    fn apply(&self, change: &Change) -> HubResult<()> {
        match change {
            Change::DpiStages { stages, active } => {
                if stages.is_empty() {
                    return Err(HubError::Invalid("stages empty".into()));
                }
                let active_idx = (*active).min(stages.len() - 1);
                let args = encode_dpi_stages(stages, active_idx);
                self.call(0x26, 0x04, 0x06, &args)?;
                let st = stages[active_idx];
                let x = st.x.clamp(100, 45000) as u16;
                let y = st.y.unwrap_or(st.x).clamp(100, 45000) as u16;
                let mut xy = [0u8; 7];
                xy[0] = VARSTORE;
                xy[1..3].copy_from_slice(&x.to_be_bytes());
                xy[3..5].copy_from_slice(&y.to_be_bytes());
                self.call(0x07, 0x04, 0x05, &xy)?;
                *self.last_stages.lock().unwrap_or_else(|p| p.into_inner()) =
                    (active_idx, stages.clone());
                Ok(())
            }
            Change::ActiveStage(idx) => {
                let (_, stages) = self
                    .call(0x26, 0x04, 0x86, &[VARSTORE])
                    .and_then(|r| decode_dpi_stages(&r[8..46]))
                    .unwrap_or_else(|_| {
                        self.last_stages
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .clone()
                    });
                let active_idx = (*idx).min(stages.len().saturating_sub(1));
                let args = encode_dpi_stages(&stages, active_idx);
                self.call(0x26, 0x04, 0x06, &args)?;
                let st = stages[active_idx];
                let x = st.x.clamp(100, 45000) as u16;
                let y = st.y.unwrap_or(st.x).clamp(100, 45000) as u16;
                let mut xy = [0u8; 7];
                xy[0] = VARSTORE;
                xy[1..3].copy_from_slice(&x.to_be_bytes());
                xy[3..5].copy_from_slice(&y.to_be_bytes());
                self.call(0x07, 0x04, 0x05, &xy)?;
                *self.last_stages.lock().unwrap_or_else(|p| p.into_inner()) = (active_idx, stages);
                Ok(())
            }
            Change::Setting { id, value } => match id.as_str() {
                "polling_rate" => {
                    let hz = value.as_choice().max(125) as u32;
                    let code = encode_polling_rate(hz);
                    // OpenRazer PR #2657: 0x00 と 0x01 の 2 回送る。
                    self.call(0x02, 0x00, 0x40, &[0x00, code])?;
                    self.call(0x02, 0x00, 0x40, &[0x01, code])?;
                    Ok(())
                }
                "sleep_time" => {
                    let sec = idle_min_to_sec(value.as_num());
                    self.call(0x02, 0x07, 0x03, &sec.to_be_bytes())?;
                    Ok(())
                }
                "low_battery_threshold" => {
                    let raw = pct_to_low_batt(value.as_num());
                    self.call(0x01, 0x07, 0x01, &[raw])?;
                    Ok(())
                }
                "dongle_led_left" | "dongle_led_center" | "dongle_led_right" => {
                    let mut cur = self.read_dongle_leds()?;
                    let v = value.as_choice().clamp(0, 4) as u8;
                    match id.as_str() {
                        "dongle_led_left" => cur[0] = v,
                        "dongle_led_center" => cur[1] = v,
                        "dongle_led_right" => cur[2] = v,
                        _ => {}
                    }
                    self.call(0x03, 0x07, 0x15, &cur)?;
                    Ok(())
                }
                _ => Err(HubError::Unsupported("setting")),
            },
            _ => Err(HubError::Unsupported("change")),
        }
    }

    fn battery(&self) -> HubResult<Battery> {
        let bat = self.call(0x02, 0x07, 0x80, &[])?;
        let chg = self.call(0x02, 0x07, 0x84, &[]).ok();
        let pct = raw_to_pct(bat[9]);
        let charging = chg.is_some_and(|r| r[9] != 0);
        let res = Battery {
            percent: Some(pct),
            charging,
        };
        self.info.lock().unwrap_or_else(|p| p.into_inner()).battery = Some(res);
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc90_xor_matches_openrazer() {
        let mut r = build_report(0x1F, 0x02, 0x00, 0x81, &[]);
        assert_eq!(r[88], 0x02 ^ 0x81);
        r[88] = 0;
        assert_eq!(crc90(&r), 0x83);
    }

    #[test]
    fn polling_rate_roundtrip() {
        for hz in [125, 250, 500, 1000, 2000, 4000, 8000] {
            let code = encode_polling_rate(hz);
            assert_eq!(decode_polling_rate(code), Some(hz));
        }
    }

    #[test]
    fn dpi_stages_real_capture_roundtrip() {
        // 実機 razerprobe 生データ: 01 04 05  01 01 90 01 90 00 00  02 03 20 03 20 00 00 ...
        let raw = [
            0x01, 0x04, 0x05, 0x01, 0x01, 0x90, 0x01, 0x90, 0x00, 0x00, 0x02, 0x03, 0x20, 0x03,
            0x20, 0x00, 0x00, 0x03, 0x06, 0x40, 0x06, 0x40, 0x00, 0x00, 0x04, 0x03, 0xe8, 0x03,
            0xe8, 0x00, 0x00, 0x05, 0x0c, 0x80, 0x0c, 0x80, 0x00, 0x00,
        ];
        let (active, stages) = decode_dpi_stages(&raw).unwrap();
        assert_eq!(active, 3); // 1-based 4 -> 0-based 3
        assert_eq!(stages.len(), 5);
        assert_eq!(stages[0].x, 400);
        assert_eq!(stages[1].x, 800);
        assert_eq!(stages[2].x, 1600);
        assert_eq!(stages[3].x, 1000);
        assert_eq!(stages[4].x, 3200);

        let encoded = encode_dpi_stages(&stages, active);
        assert_eq!(encoded.as_slice(), &raw[..]);
    }

    #[test]
    fn battery_and_idle_conversions() {
        assert_eq!(raw_to_pct(205), 80);
        assert_eq!(raw_to_pct(255), 100);
        assert_eq!(raw_to_pct(0), 0);
        assert_eq!(idle_sec_to_min(300), 5.0);
        assert_eq!(idle_min_to_sec(5.0), 300);
        assert_eq!(low_batt_to_pct(0x4D), 30.0);
    }
}
