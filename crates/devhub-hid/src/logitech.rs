//! Logitech HID++ 2.0 汎用ドライバ (G PRO Wireless / G304 / G703 / G PRO X SUPERLIGHT 等)。
//!
//! - デュアルチャネル (`0xFF00:0x02` Long 20B + 同一 IF の `0xFF00:0x01` Short 7B):
//!   Windows 上では `0xFF00:0x02` 宛てコマンドの HID++ 1.0 エラー (`0x8F` — スリープ時の
//!   `ERR_RESOURCE_ERROR = 0x09` 等) が Short チャネル (`0x10`) に返るため両方を監視する。
//! - 動的 Feature 検出 (`IRoot` `0x0000` → `IFeatureSet` `0x0001`):
//!   - `0x0003` DeviceFwInfo / `0x0005` DeviceName
//!   - `0x1004` UnifiedBattery / `0x1000` BatteryStatus / `0x1001` BatteryVoltage
//!   - `0x2201` AdjustableDPI (`getSensorDpiList` から機種固有 min/max/step を動的取得)
//!   - `0x8060` ReportRate / `0x8061` ExtendedAdjustableReportRate
//!   - `0x8070` ColorLedEffects / `0x8071` RgbEffects
//!   - `0x8100` OnboardProfiles (256B セクタ RMW + CRC-16/CCITT-FALSE + 初回バックアップ)

use devhub_core::backend::*;
use devhub_core::model::*;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SW_ID: u8 = 0x0A;

// HID++ 2.0 Feature IDs
const FEAT_FW_INFO: u16 = 0x0003;
const FEAT_DEVICE_NAME: u16 = 0x0005;
const FEAT_BATTERY_STATUS: u16 = 0x1000;
const FEAT_BATTERY_VOLTAGE: u16 = 0x1001;
const FEAT_UNIFIED_BATTERY: u16 = 0x1004;
const FEAT_ADJUSTABLE_DPI: u16 = 0x2201;
const FEAT_REPORT_RATE: u16 = 0x8060;
const FEAT_EXT_REPORT_RATE: u16 = 0x8061;
const FEAT_COLOR_LED_EFFECTS: u16 = 0x8070;
const FEAT_RGB_EFFECTS: u16 = 0x8071;
const FEAT_ONBOARD_PROFILES: u16 = 0x8100;

type Watcher = Option<Arc<dyn Fn(HubEvent) + Send + Sync>>;
type DetectedMap = Arc<Mutex<HashMap<String, (String, String)>>>;

/// CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF, no xorout)。
pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidppErr {
    Io(String),
    Disconnected,
    Timeout,
    Hidpp10(u8),
    Hidpp20(u8),
    Protocol(String),
}

impl From<HidppErr> for HubError {
    fn from(e: HidppErr) -> Self {
        match e {
            HidppErr::Disconnected => HubError::Disconnected,
            HidppErr::Io(s) => HubError::Io(s),
            HidppErr::Timeout => HubError::Protocol("logitech: response timeout".into()),
            HidppErr::Hidpp10(c) => HubError::Protocol(format!("logitech: HID++ 1.0 error {c:#04x}")),
            HidppErr::Hidpp20(c) => HubError::Protocol(format!("logitech: HID++ 2.0 error {c:#04x}")),
            HidppErr::Protocol(s) => HubError::Protocol(s),
        }
    }
}

fn is_disconnected_err(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("not connected")
        || l.contains("no such device")
        || l.contains("0x0000048f")
        || l.contains("1167")
        || l.contains("device has been removed")
}

trait HidppOps: Send + Sync {
    fn call_20(&self, dev_idx: u8, feat_idx: u8, func: u8, args: &[u8]) -> Result<[u8; 20], HidppErr>;
    fn get_reg_long(&self, dev_idx: u8, sub_id: u8, addr: u8, p0: u8) -> Result<Vec<u8>, HidppErr>;
}

struct DualChannel {
    long: Mutex<hidapi::HidDevice>,
    short: Option<Mutex<hidapi::HidDevice>>,
}

impl DualChannel {
    fn open(long: hidapi::HidDevice, vid: u16, pid: u16) -> Self {
        long.set_blocking_mode(false).ok();
        let short = hidapi::HidApi::new().ok().and_then(|api| {
            api.device_list()
                .find(|d| {
                    d.vendor_id() == vid
                        && d.product_id() == pid
                        && d.usage_page() == 0xFF00
                        && d.usage() == 0x01
                })
                .and_then(|d| api.open_path(d.path()).ok())
                .inspect(|s| {
                    s.set_blocking_mode(false).ok();
                })
                .map(Mutex::new)
        });
        Self {
            long: Mutex::new(long),
            short,
        }
    }

    fn drain(&self) {
        let mut buf = [0u8; 64];
        if let Some(s) = &self.short {
            let guard = s.lock().unwrap_or_else(|p| p.into_inner());
            while matches!(guard.read_timeout(&mut buf, 2), Ok(n) if n > 0) {}
        }
        let guard = self.long.lock().unwrap_or_else(|p| p.into_inner());
        while matches!(guard.read_timeout(&mut buf, 2), Ok(n) if n > 0) {}
    }
}

impl HidppOps for DualChannel {
    fn call_20(&self, dev_idx: u8, feat_idx: u8, func: u8, args: &[u8]) -> Result<[u8; 20], HidppErr> {
        let fn_sw = ((func & 0x0F) << 4) | SW_ID;
        let mut req = [0u8; 20];
        req[0] = 0x11;
        req[1] = dev_idx;
        req[2] = feat_idx;
        req[3] = fn_sw;
        let n = args.len().min(16);
        req[4..4 + n].copy_from_slice(&args[..n]);

        for attempt in 0..2 {
            self.drain();
            {
                let long = self.long.lock().unwrap_or_else(|p| p.into_inner());
                long.write(&req).map_err(|e| {
                    let msg = e.to_string();
                    if is_disconnected_err(&msg) {
                        HidppErr::Disconnected
                    } else {
                        HidppErr::Io(format!("hidpp write: {msg}"))
                    }
                })?;
            }
            let deadline = Instant::now() + Duration::from_millis(750);
            let mut buf = [0u8; 64];
            loop {
                if Instant::now() >= deadline {
                    break;
                }
                if let Some(s) = &self.short {
                    let sg = s.lock().unwrap_or_else(|p| p.into_inner());
                    if let Ok(len) = sg.read_timeout(&mut buf, 5) {
                        if len >= 6
                            && buf[0] == 0x10
                            && buf[1] == dev_idx
                            && buf[2] == 0x8F
                            && buf[3] == feat_idx
                            && buf[4] == fn_sw
                        {
                            return Err(HidppErr::Hidpp10(buf[5]));
                        }
                    }
                }
                let lg = self.long.lock().unwrap_or_else(|p| p.into_inner());
                match lg.read_timeout(&mut buf, 25) {
                    Ok(len) if len >= 20 && buf[0] == 0x11 && buf[1] == dev_idx => {
                        if buf[2] == 0xFF && buf[3] == feat_idx && buf[4] == fn_sw {
                            return Err(HidppErr::Hidpp20(buf[5]));
                        }
                        if buf[2] == 0x8F && buf[3] == feat_idx && buf[4] == fn_sw {
                            return Err(HidppErr::Hidpp10(buf[5]));
                        }
                        if buf[2] == feat_idx && buf[3] == fn_sw {
                            let mut out = [0u8; 20];
                            out.copy_from_slice(&buf[..20]);
                            return Ok(out);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let msg = e.to_string();
                        if is_disconnected_err(&msg) {
                            return Err(HidppErr::Disconnected);
                        }
                        return Err(HidppErr::Io(format!("hidpp read: {msg}")));
                    }
                }
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        Err(HidppErr::Timeout)
    }

    fn get_reg_long(&self, dev_idx: u8, sub_id: u8, addr: u8, p0: u8) -> Result<Vec<u8>, HidppErr> {
        let req = [0x10u8, dev_idx, sub_id, addr, p0, 0x00, 0x00];
        self.drain();
        if let Some(s) = &self.short {
            s.lock()
                .unwrap_or_else(|p| p.into_inner())
                .write(&req)
                .map_err(|e| HidppErr::Io(format!("short write: {e}")))?;
        } else {
            let mut lreq = [0u8; 20];
            lreq[0] = 0x11;
            lreq[1..7].copy_from_slice(&req[1..7]);
            self.long
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .write(&lreq)
                .map_err(|e| HidppErr::Io(format!("long write: {e}")))?;
        }
        let deadline = Instant::now() + Duration::from_millis(400);
        let mut buf = [0u8; 64];
        loop {
            if Instant::now() >= deadline {
                return Err(HidppErr::Timeout);
            }
            if let Some(s) = &self.short {
                let sg = s.lock().unwrap_or_else(|p| p.into_inner());
                if let Ok(n) = sg.read_timeout(&mut buf, 15) {
                    if n >= 6 && buf[1] == dev_idx && buf[2] == 0x8F && buf[3] == sub_id {
                        return Err(HidppErr::Hidpp10(buf[5]));
                    }
                    if n >= 4 && buf[1] == dev_idx && buf[2] == sub_id && buf[3] == addr {
                        return Ok(buf[..n].to_vec());
                    }
                }
            }
            let lg = self.long.lock().unwrap_or_else(|p| p.into_inner());
            if let Ok(n) = lg.read_timeout(&mut buf, 15) {
                if n >= 6 && buf[1] == dev_idx && buf[2] == 0x8F && buf[3] == sub_id {
                    return Err(HidppErr::Hidpp10(buf[5]));
                }
                if n >= 4 && buf[1] == dev_idx && buf[2] == sub_id && buf[3] == addr {
                    return Ok(buf[..n].to_vec());
                }
            }
        }
    }
}

/// `0x2201` `getSensorDpiList` パース結果: `(dpi_min, dpi_max, dpi_step)`。
pub fn parse_dpi_list(bytes: &[u8]) -> (u32, u32, u32) {
    let mut vals = Vec::new();
    for pair in bytes.chunks_exact(2) {
        let v = u16::from_be_bytes([pair[0], pair[1]]);
        if v == 0 {
            break;
        }
        vals.push(v);
    }
    if vals.is_empty() {
        return (100, 12000, 50);
    }
    // 連続レンジ: [min, 0xE000 | step, max]
    if let Some(pos) = vals.iter().position(|&v| (v & 0xE000) == 0xE000 && v != 0xE000) {
        let min = vals.get(pos.saturating_sub(1)).copied().unwrap_or(100) as u32;
        let step = (vals[pos] & 0x1FFF).max(1) as u32;
        let max = vals.get(pos + 1).copied().unwrap_or(12000) as u32;
        return (min.max(50), max.max(min), step);
    }
    let min = *vals.iter().min().unwrap_or(&100) as u32;
    let max = *vals.iter().max().unwrap_or(&12000) as u32;
    let mut step = 50u32;
    if vals.len() >= 2 {
        let mut sorted = vals.clone();
        sorted.sort_unstable();
        let min_diff = sorted
            .windows(2)
            .filter_map(|w| (w[1] > w[0]).then_some((w[1] - w[0]) as u32))
            .min();
        if let Some(d) = min_diff {
            step = d.max(1);
        }
    }
    (min.max(50), max.max(min), step)
}

/// `0x8060` / `0x8061` のビットマスクから対応ポーリングレート (Hz) 一覧を組み立てる。
pub fn decode_report_rate_mask(mask: u16, extended: bool) -> Vec<u32> {
    let mut out = Vec::new();
    if extended {
        // 0x8061 bit0=8K? Or standard 125..8000 mapping:
        let map: &[(u16, u32)] = &[
            (1 << 0, 1000),
            (1 << 1, 500),
            (1 << 2, 250),
            (1 << 3, 125),
            (1 << 4, 2000),
            (1 << 5, 4000),
            (1 << 6, 8000),
        ];
        for (bit, hz) in map {
            if (mask & bit) != 0 {
                out.push(*hz);
            }
        }
    } else {
        // 0x8060 bit N = (N + 1) ms: bit0=1ms(1000Hz), bit1=2ms(500Hz), bit3=4ms(250Hz), bit7=8ms(125Hz)
        let map: &[(u8, u32)] = &[
            (1 << 0, 1000),
            (1 << 1, 500),
            (1 << 3, 250),
            (1 << 7, 125),
        ];
        for (bit, hz) in map {
            if ((mask as u8) & bit) != 0 {
                out.push(*hz);
            }
        }
    }
    out.sort_unstable();
    if out.is_empty() {
        vec![125, 250, 500, 1000]
    } else {
        out
    }
}

/// `0x1001` BatteryVoltage (mV) からの残量 (%) 推定テーブル。
pub fn mv_to_percent(mv: u16) -> u8 {
    // リポ / 単三アルカライン近似カーブ (libratbag / Solaar 準拠)
    const TABLE: &[(u16, u8)] = &[
        (4186, 100),
        (4067, 90),
        (3989, 80),
        (3922, 70),
        (3859, 60),
        (3807, 50),
        (3763, 40),
        (3725, 30),
        (3686, 20),
        (3624, 10),
        (3500, 5),
    ];
    if mv <= 1650 {
        // 単3乾電池 (G304/G305 等 ~1.5V セル)
        return match mv {
            1450.. => 100,
            1350..=1449 => 80,
            1250..=1349 => 60,
            1180..=1249 => 40,
            1100..=1179 => 20,
            1000..=1099 => 10,
            _ => 5,
        };
    }
    for &(v, p) in TABLE {
        if mv >= v {
            return p;
        }
    }
    0
}

#[derive(Clone, Debug)]
pub struct OnboardDesc {
    pub memory_model: u8,
    pub profile_format: u8,
    pub macro_format: u8,
    pub profile_count: u8,
    pub profile_count_oob: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
}

#[derive(Clone, Debug)]
pub struct ParsedProfile {
    pub sector: u16,
    pub enabled: bool,
    pub raw: Vec<u8>,
    pub report_rate_hz: u32,
    pub default_dpi_idx: usize,
    pub switched_dpi_idx: usize,
    pub stages: Vec<DpiStage>,
    pub buttons: Vec<ButtonAction>,
    pub name: String,
    pub lighting: Vec<LightingState>,
}

/// 4 バイトボタンレコード (`union hidpp20_button_binding`) を `ButtonAction` へデコード。
pub fn decode_button(b: &[u8]) -> ButtonAction {
    if b.len() < 4 {
        return ButtonAction::Disabled;
    }
    match b[0] {
        0xFF => ButtonAction::Disabled,
        0x80 => match b[1] {
            0x01 => {
                let mask = u16::from_be_bytes([b[2], b[3]]);
                let bit = if mask == 0 { 0 } else { mask.trailing_zeros() + 1 };
                match bit {
                    1 => ButtonAction::Mouse(MouseFn::Left),
                    2 => ButtonAction::Mouse(MouseFn::Right),
                    3 => ButtonAction::Mouse(MouseFn::Middle),
                    4 => ButtonAction::Mouse(MouseFn::Back),
                    5 => ButtonAction::Mouse(MouseFn::Forward),
                    _ => ButtonAction::Unsupported {
                        ty: b[0],
                        params: b[1..4].to_vec(),
                    },
                }
            }
            0x02 => ButtonAction::Key(KeyStroke {
                mods: b[2],
                usage: b[3] as u16,
            }),
            0x03 => {
                let code = u16::from_be_bytes([b[2], b[3]]);
                let media = match code {
                    0x00CD => Some(MediaFn::PlayPause),
                    0x00B7 => Some(MediaFn::Stop),
                    0x00B5 => Some(MediaFn::Next),
                    0x00B6 => Some(MediaFn::Prev),
                    0x00E9 => Some(MediaFn::VolumeUp),
                    0x00EA => Some(MediaFn::VolumeDown),
                    0x00E2 => Some(MediaFn::Mute),
                    0x0192 => Some(MediaFn::Calculator),
                    0x0194 => Some(MediaFn::MyComputer),
                    0x0196 => Some(MediaFn::Browser),
                    0x018A => Some(MediaFn::Mail),
                    0x006F => Some(MediaFn::BrightnessUp),
                    0x0070 => Some(MediaFn::BrightnessDown),
                    0x0227 => Some(MediaFn::Refresh),
                    _ => None,
                };
                match media {
                    Some(m) => ButtonAction::Media(m),
                    None => ButtonAction::Unsupported {
                        ty: b[0],
                        params: b[1..4].to_vec(),
                    },
                }
            }
            _ => ButtonAction::Unsupported {
                ty: b[0],
                params: b[1..4].to_vec(),
            },
        },
        0x90 => match b[1] {
            0x01 => ButtonAction::Mouse(MouseFn::ScrollLeft),
            0x02 => ButtonAction::Mouse(MouseFn::ScrollRight),
            0x03 => ButtonAction::Dpi(DpiFn::Up),
            0x04 => ButtonAction::Dpi(DpiFn::Down),
            0x05 => ButtonAction::Dpi(DpiFn::Cycle),
            0x07 => ButtonAction::Dpi(DpiFn::Shift(400)),
            0x0A => ButtonAction::ProfileCycle,
            0x10 => ButtonAction::Mouse(MouseFn::ScrollDown),
            0x11 => ButtonAction::Mouse(MouseFn::ScrollUp),
            _ => ButtonAction::Unsupported {
                ty: b[0],
                params: b[1..4].to_vec(),
            },
        },
        _ => ButtonAction::Unsupported {
            ty: b[0],
            params: b[1..4].to_vec(),
        },
    }
}

/// `ButtonAction` を 4 バイト `union hidpp20_button_binding` へエンコード。
pub fn encode_button(a: &ButtonAction) -> [u8; 4] {
    match a {
        ButtonAction::Disabled => [0xFF, 0xFF, 0xFF, 0xFF],
        ButtonAction::Mouse(m) => match m {
            MouseFn::Left => [0x80, 0x01, 0x00, 0x01],
            MouseFn::Right => [0x80, 0x01, 0x00, 0x02],
            MouseFn::Middle => [0x80, 0x01, 0x00, 0x04],
            MouseFn::Back => [0x80, 0x01, 0x00, 0x08],
            MouseFn::Forward => [0x80, 0x01, 0x00, 0x10],
            MouseFn::ScrollLeft => [0x90, 0x01, 0x00, 0x00],
            MouseFn::ScrollRight => [0x90, 0x02, 0x00, 0x00],
            MouseFn::ScrollDown => [0x90, 0x10, 0x00, 0x00],
            MouseFn::ScrollUp => [0x90, 0x11, 0x00, 0x00],
            MouseFn::DoubleClick => [0x80, 0x01, 0x00, 0x01],
        },
        ButtonAction::Key(k) => [0x80, 0x02, k.mods, (k.usage & 0xFF) as u8],
        ButtonAction::Media(m) => {
            let code: u16 = match m {
                MediaFn::PlayPause => 0x00CD,
                MediaFn::Stop => 0x00B7,
                MediaFn::Next => 0x00B5,
                MediaFn::Prev => 0x00B6,
                MediaFn::VolumeUp => 0x00E9,
                MediaFn::VolumeDown => 0x00EA,
                MediaFn::Mute => 0x00E2,
                MediaFn::Calculator => 0x0192,
                MediaFn::MyComputer => 0x0194,
                MediaFn::Browser => 0x0196,
                MediaFn::Mail => 0x018A,
                MediaFn::BrightnessUp => 0x006F,
                MediaFn::BrightnessDown => 0x0070,
                MediaFn::Refresh => 0x0227,
            };
            let b = code.to_be_bytes();
            [0x80, 0x03, b[0], b[1]]
        }
        ButtonAction::Dpi(d) => match d {
            DpiFn::Up => [0x90, 0x03, 0x00, 0x00],
            DpiFn::Down => [0x90, 0x04, 0x00, 0x00],
            DpiFn::Cycle => [0x90, 0x05, 0x00, 0x00],
            DpiFn::Shift(_) => [0x90, 0x07, 0x00, 0x00],
        },
        ButtonAction::ProfileCycle => [0x90, 0x0A, 0x00, 0x00],
        ButtonAction::Unsupported { ty, params } => {
            let mut out = [*ty, 0, 0, 0];
            let n = params.len().min(3);
            out[1..1 + n].copy_from_slice(&params[..n]);
            out
        }
        _ => [0xFF, 0xFF, 0xFF, 0xFF],
    }
}

/// 11B `hidpp20_internal_led` (`data[208..219]`) をデコードする。
pub fn decode_led(b: &[u8]) -> LightingState {
    if b.len() < 11 {
        return LightingState {
            on: false,
            effect: 0,
            color: Rgb::hex(0x00b8fc),
            brightness: 100,
            speed: 50,
            direction: 0,
            per_key: BTreeMap::new(),
            zone_on: vec![],
            palette: vec![],
            palette_sel: 0,
            color_correction: None,
        };
    }
    let mode = b[0];
    match mode {
        0x00 => LightingState {
            on: false,
            effect: 0,
            color: Rgb::hex(0x00b8fc),
            brightness: 100,
            speed: 50,
            direction: 0,
            per_key: BTreeMap::new(),
            zone_on: vec![],
            palette: vec![],
            palette_sel: 0,
            color_correction: None,
        },
        0x01 => LightingState {
            on: true,
            effect: 0x01,
            color: Rgb::new(b[1], b[2], b[3]),
            brightness: 100,
            speed: 50,
            direction: 0,
            per_key: BTreeMap::new(),
            zone_on: vec![],
            palette: vec![],
            palette_sel: 0,
            color_correction: None,
        },
        0x03 => {
            let period = u16::from_be_bytes([b[6], b[7]]).clamp(1000, 20000) as u32;
            let bri = if b[8] == 0 { 100 } else { b[8].min(100) };
            let speed = (((20000 - period) * 100) / 19000).clamp(1, 100) as u8;
            LightingState {
                on: true,
                effect: 0x03,
                color: Rgb::hex(0x00b8fc),
                brightness: bri,
                speed,
                direction: 0,
                per_key: BTreeMap::new(),
                zone_on: vec![],
                palette: vec![],
                palette_sel: 0,
                color_correction: None,
            }
        }
        0x0A => {
            let period = u16::from_be_bytes([b[4], b[5]]).clamp(1000, 20000) as u32;
            let bri = if b[7] == 0 { 100 } else { b[7].min(100) };
            let speed = (((20000 - period) * 100) / 19000).clamp(1, 100) as u8;
            LightingState {
                on: true,
                effect: 0x0A,
                color: Rgb::new(b[1], b[2], b[3]),
                brightness: bri,
                speed,
                direction: 0,
                per_key: BTreeMap::new(),
                zone_on: vec![],
                palette: vec![],
                palette_sel: 0,
                color_correction: None,
            }
        }
        _ => LightingState {
            on: true,
            effect: 0x01,
            color: Rgb::new(b[1], b[2], b[3]),
            brightness: 100,
            speed: 50,
            direction: 0,
            per_key: BTreeMap::new(),
            zone_on: vec![],
            palette: vec![],
            palette_sel: 0,
            color_correction: None,
        },
    }
}

/// `LightingState` を 11B `hidpp20_internal_led` にエンコードする。
pub fn encode_led(ls: &LightingState) -> [u8; 11] {
    let mut out = [0u8; 11];
    if !ls.on || ls.effect == 0x00 {
        out[0] = 0x00;
        return out;
    }
    let speed = ls.speed.clamp(1, 100) as u32;
    let period = (20000 - (speed * 19000 / 100)).clamp(1000, 20000) as u16;
    let bri = if ls.brightness >= 100 { 0 } else { ls.brightness.max(1) };
    match ls.effect {
        0x03 => {
            out[0] = 0x03;
            out[6..8].copy_from_slice(&period.to_be_bytes());
            out[8] = bri;
        }
        0x0A => {
            out[0] = 0x0A;
            out[1] = ls.color.r;
            out[2] = ls.color.g;
            out[3] = ls.color.b;
            out[4..6].copy_from_slice(&period.to_be_bytes());
            out[6] = 0x01; // sine waveform
            out[7] = bri;
        }
        _ => {
            out[0] = 0x01;
            out[1] = ls.color.r;
            out[2] = ls.color.g;
            out[3] = ls.color.b;
        }
    }
    out
}

/// 255B/256B `hidpp20_internal_profile` セクタをパースする。
pub fn parse_profile_sector(
    sector: u16,
    enabled: bool,
    raw: &[u8],
    button_count: usize,
    led_count: usize,
    check_crc: bool,
) -> HubResult<ParsedProfile> {
    if raw.len() < 254 {
        return Err(HubError::Protocol(format!(
            "logitech: profile sector {} too short ({}B)",
            sector,
            raw.len()
        )));
    }
    if check_crc {
        let stored_crc = u16::from_be_bytes([raw[raw.len() - 2], raw[raw.len() - 1]]);
        let calc_crc = crc16_ccitt_false(&raw[..raw.len() - 2]);
        if stored_crc != calc_crc {
            return Err(HubError::Protocol(format!(
                "logitech: sector 0x{sector:04X} CRC mismatch (stored={stored_crc:#06x} calc={calc_crc:#06x})"
            )));
        }
    }

    let rr_div = raw[0].max(1) as u32;
    let report_rate_hz = (1000 / rr_div).max(125);
    let default_dpi_idx = raw[1] as usize;
    let switched_dpi_idx = raw[2] as usize;

    let mut stages = Vec::new();
    for i in 0..5 {
        let v = u16::from_le_bytes([raw[3 + i * 2], raw[3 + i * 2 + 1]]) as u32;
        if v == 0 {
            continue;
        }
        stages.push(DpiStage {
            x: v,
            y: None,
            color: Rgb::hex(0x00b8fc),
            enabled: true,
        });
    }
    if stages.is_empty() {
        stages.push(DpiStage {
            x: 800,
            y: None,
            color: Rgb::hex(0x00b8fc),
            enabled: true,
        });
    }

    let btn_n = button_count.clamp(1, 16);
    let mut buttons = Vec::with_capacity(btn_n);
    for i in 0..btn_n {
        let off = 32 + i * 4;
        buttons.push(decode_button(&raw[off..off + 4]));
    }

    let name_slice = &raw[160..208];
    let name = if name_slice.iter().all(|&b| b == 0xFF || b == 0x00) {
        String::new()
    } else {
        let end = name_slice.iter().position(|&b| b == 0x00 || b == 0xFF).unwrap_or(48);
        String::from_utf8_lossy(&name_slice[..end]).trim().to_string()
    };

    let mut lighting = Vec::with_capacity(led_count.min(2));
    for z in 0..led_count.min(2) {
        let off = 208 + z * 11;
        lighting.push(decode_led(&raw[off..off + 11]));
    }

    Ok(ParsedProfile {
        sector,
        enabled,
        raw: raw.to_vec(),
        report_rate_hz,
        default_dpi_idx: default_dpi_idx.min(stages.len().saturating_sub(1)),
        switched_dpi_idx: switched_dpi_idx.min(stages.len().saturating_sub(1)),
        stages,
        buttons,
        name,
        lighting,
    })
}

/// 変更内容を RMW して `sector_size` バイト列を生成し、末尾 2 バイトに CRC-16/CCITT-FALSE を書き込む。
pub fn serialize_profile_sector(p: &ParsedProfile, sector_size: usize, led_count: usize) -> Vec<u8> {
    let target_len = sector_size.max(254);
    let mut buf = p.raw.clone();
    if buf.len() != target_len {
        buf.resize(target_len, 0xFF);
    }
    let rr = (1000 / p.report_rate_hz.max(125)).clamp(1, 8) as u8;
    buf[0] = rr;
    buf[1] = p.default_dpi_idx.min(p.stages.len().saturating_sub(1)) as u8;
    buf[2] = p.switched_dpi_idx.min(p.stages.len().saturating_sub(1)) as u8;

    for i in 0..5 {
        let val: u16 = p.stages.get(i).map(|s| s.x.clamp(50, 45000) as u16).unwrap_or(0);
        buf[3 + i * 2..3 + i * 2 + 2].copy_from_slice(&val.to_le_bytes());
    }

    for (i, act) in p.buttons.iter().take(16).enumerate() {
        let off = 32 + i * 4;
        buf[off..off + 4].copy_from_slice(&encode_button(act));
    }

    let name_bytes = p.name.as_bytes();
    if name_bytes.is_empty() {
        buf[160..208].fill(0xFF);
    } else {
        buf[160..208].fill(0x00);
        let n = name_bytes.len().min(47);
        buf[160..160 + n].copy_from_slice(&name_bytes[..n]);
    }

    let n_leds = led_count.min(2).min(p.lighting.len());
    for z in 0..n_leds {
        let led = encode_led(&p.lighting[z]);
        let off = 208 + z * 11;
        let alt_off = 230 + z * 11;
        buf[off..off + 11].copy_from_slice(&led);
        if n_leds >= 2 || buf[alt_off] != 0xFF {
            buf[alt_off..alt_off + 11].copy_from_slice(&led);
        }
    }

    let crc = crc16_ccitt_false(&buf[..target_len - 2]);
    buf[target_len - 2..target_len].copy_from_slice(&crc.to_be_bytes());
    buf
}

/// ユーザーディレクトリセクタ `0x0000` を組み立てる。
pub fn build_directory_sector(sector_size: usize, enabled_flags: &[bool]) -> Vec<u8> {
    let sz = sector_size.max(64);
    let mut buf = vec![0xFFu8; sz];
    let mut off = 0usize;
    for (i, &en) in enabled_flags.iter().enumerate() {
        if off + 8 > sz - 2 {
            break;
        }
        buf[off] = 0x00;
        buf[off + 1] = (i + 1) as u8;
        buf[off + 2] = if en { 0x01 } else { 0x00 };
        buf[off + 3] = 0x00;
        off += 4;
    }
    if off + 4 <= sz - 2 {
        buf[off] = 0xFF;
        buf[off + 1] = 0xFF;
        buf[off + 2] = 0x00;
        buf[off + 3] = 0x00;
    }
    let crc = crc16_ccitt_false(&buf[..sz - 2]);
    buf[sz - 2..sz].copy_from_slice(&crc.to_be_bytes());
    buf
}

/// ユーザーディレクトリセクタ `0x0000` をパースして `(sector_addr, enabled)` 列を返す。
pub fn parse_directory_sector(raw: &[u8], max_profiles: usize) -> HubResult<Vec<(u16, bool)>> {
    if raw.len() < 8 {
        return Err(HubError::Protocol("logitech: short dir sector".into()));
    }
    let stored = u16::from_be_bytes([raw[raw.len() - 2], raw[raw.len() - 1]]);
    let calc = crc16_ccitt_false(&raw[..raw.len() - 2]);
    if stored != calc {
        return Err(HubError::Protocol("logitech: dir sector CRC mismatch".into()));
    }
    let mut out = Vec::new();
    for i in 0..max_profiles {
        let off = i * 4;
        if off + 4 > raw.len() - 2 {
            break;
        }
        let addr = u16::from_be_bytes([raw[off], raw[off + 1]]);
        if addr == 0xFFFF {
            break;
        }
        let enabled = raw[off + 2] != 0;
        out.push((addr, enabled));
    }
    Ok(out)
}

#[derive(Default, Clone)]
struct FeatTable {
    dev_idx: u8,
    fw_info: Option<u8>,
    dev_name: Option<u8>,
    batt_status: Option<u8>,
    batt_voltage: Option<u8>,
    batt_unified: Option<u8>,
    adj_dpi: Option<u8>,
    report_rate: Option<u8>,
    ext_report_rate: Option<u8>,
    color_led: Option<u8>,
    rgb_led: Option<u8>,
    onboard: Option<u8>,
    desc: Option<OnboardDesc>,
    dpi_min: u32,
    dpi_max: u32,
    dpi_step: u32,
    rates: Vec<u32>,
    has_lighting: bool,
    led_count: usize,
}

pub struct LogitechMouse {
    ch: Arc<dyn HidppOps>,
    info: Mutex<DeviceInfo>,
    events: Watcher,
    detected: DetectedMap,
    feats: Mutex<Option<FeatTable>>,
    caps: Mutex<Capabilities>,
    state_cache: Mutex<Option<(Vec<ParsedProfile>, usize, bool)>>,
}

fn button_names_for(count: usize, is_g305_or_gpw: bool) -> Vec<ButtonSlot> {
    let base = if is_g305_or_gpw {
        &[
            "左クリック",
            "右クリック",
            "中クリック",
            "サイド (戻る)",
            "サイド (進む)",
            "DPI サイクル",
            "右サイド 1",
            "右サイド 2",
        ][..]
    } else {
        &[
            "左クリック",
            "右クリック",
            "中クリック",
            "サイド (戻る)",
            "サイド (進む)",
            "DPI / 特殊 1",
            "特殊 2",
            "特殊 3",
        ][..]
    };
    (0..count)
        .map(|i| ButtonSlot {
            name: base
                .get(i)
                .map(|s| (*s).to_string())
                .unwrap_or_else(|| format!("ボタン {}", i + 1)),
        })
        .collect()
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn read_receiver_meta(ch: &dyn HidppOps, info: &mut DeviceInfo) {
    if let Ok(r) = ch.get_reg_long(0xFF, 0x81, 0xF1, 0x01) {
        if r.len() >= 7 {
            let major = r[5];
            let minor = r[6];
            let build = ch
                .get_reg_long(0xFF, 0x81, 0xF1, 0x02)
                .ok()
                .filter(|b| b.len() >= 7)
                .map(|b| u16::from_be_bytes([b[5], b[6]]))
                .unwrap_or(0);
            info.dongle_firmware = Some(format!("{major:02X}.{minor:02X}.B{build:04X}"));
        }
    }
    if let Ok(r) = ch.get_reg_long(0xFF, 0x83, 0xB5, 0x20) {
        if r.len() >= 9 {
            let wpid = u16::from_be_bytes([r[7], r[8]]);
            if wpid != 0 {
                info.model = format!("logitech-{wpid:04x}");
            }
        }
    }
    if let Ok(r) = ch.get_reg_long(0xFF, 0x83, 0xB5, 0x40) {
        if r.len() >= 7 {
            let len = (r[5] as usize).min(r.len() - 6);
            let name = String::from_utf8_lossy(&r[6..6 + len])
                .trim_end_matches('\0')
                .trim()
                .to_string();
            if !name.is_empty() {
                info.name = name;
            }
        }
    }
}

fn probe_features(
    ch: &dyn HidppOps,
    transport: Transport,
    info: &mut DeviceInfo,
) -> Result<FeatTable, HidppErr> {
    let candidates: &[u8] = if transport == Transport::Wired {
        &[0xFF, 0x01, 0x02]
    } else {
        &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]
    };

    let mut active_idx = None;
    let mut last_err = HidppErr::Timeout;
    for &idx in candidates {
        match ch.call_20(idx, 0x00, 1, &[0x00, 0x00, 0xAA]) {
            Ok(_) => {
                active_idx = Some(idx);
                break;
            }
            Err(HidppErr::Disconnected) => return Err(HidppErr::Disconnected),
            Err(e) => last_err = e,
        }
    }
    let Some(dev_idx) = active_idx else {
        return Err(last_err);
    };

    let r_fs = ch.call_20(dev_idx, 0x00, 0, &[0x00, 0x01, 0x00])?;
    let fs_idx = r_fs[4];
    let r_cnt = ch.call_20(dev_idx, fs_idx, 0, &[])?;
    let count = r_cnt[4];

    let mut table = FeatTable {
        dev_idx,
        dpi_min: 100,
        dpi_max: 12000,
        dpi_step: 50,
        rates: vec![125, 250, 500, 1000],
        ..Default::default()
    };

    for i in 0..=count {
        if let Ok(r) = ch.call_20(dev_idx, fs_idx, 1, &[i]) {
            let fid = u16::from_be_bytes([r[4], r[5]]);
            match fid {
                FEAT_FW_INFO => table.fw_info = Some(i),
                FEAT_DEVICE_NAME => table.dev_name = Some(i),
                FEAT_BATTERY_STATUS => table.batt_status = Some(i),
                FEAT_BATTERY_VOLTAGE => table.batt_voltage = Some(i),
                FEAT_UNIFIED_BATTERY => table.batt_unified = Some(i),
                FEAT_ADJUSTABLE_DPI => table.adj_dpi = Some(i),
                FEAT_REPORT_RATE => table.report_rate = Some(i),
                FEAT_EXT_REPORT_RATE => table.ext_report_rate = Some(i),
                FEAT_COLOR_LED_EFFECTS => table.color_led = Some(i),
                FEAT_RGB_EFFECTS => table.rgb_led = Some(i),
                FEAT_ONBOARD_PROFILES => table.onboard = Some(i),
                _ => {}
            }
        }
    }

    if let Some(idx) = table.dev_name {
        if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
            let len = r[4] as usize;
            let mut name_bytes = Vec::new();
            while name_bytes.len() < len {
                let off = name_bytes.len() as u8;
                if let Ok(rn) = ch.call_20(dev_idx, idx, 1, &[off]) {
                    let take = (len - name_bytes.len()).min(16);
                    name_bytes.extend_from_slice(&rn[4..4 + take]);
                } else {
                    break;
                }
            }
            let n = String::from_utf8_lossy(&name_bytes)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            if !n.is_empty() {
                info.name = n;
            }
        }
    }

    if let Some(idx) = table.fw_info {
        if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
            let n = r[4];
            for e in 0..n {
                if let Ok(rf) = ch.call_20(dev_idx, idx, 1, &[e]) {
                    if rf[4] == 0 {
                        // Main application FW
                        let major = rf[8];
                        let minor = rf[9];
                        let build = u16::from_be_bytes([rf[10], rf[11]]);
                        info.firmware = Some(format!("{major:02X}.{minor:02X}.B{build:04X}"));
                        break;
                    }
                }
            }
        }
    }

    if let Some(idx) = table.adj_dpi {
        if let Ok(r) = ch.call_20(dev_idx, idx, 1, &[0]) {
            let (dmin, dmax, dstep) = parse_dpi_list(&r[5..]);
            table.dpi_min = dmin;
            table.dpi_max = dmax;
            table.dpi_step = dstep;
        }
    }

    if let Some(idx) = table.ext_report_rate {
        if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
            table.rates = decode_report_rate_mask(u16::from_be_bytes([r[4], r[5]]), true);
        }
    } else if let Some(idx) = table.report_rate {
        if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
            table.rates = decode_report_rate_mask(r[4] as u16, false);
        }
    }

    if table.rgb_led.is_some() {
        table.led_count = 2;
        table.has_lighting = true;
    } else if let Some(idx) = table.color_led {
        if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
            table.led_count = (r[4] as usize).min(2);
            table.has_lighting = table.led_count > 0;
        }
    }

    if let Some(idx) = table.onboard {
        if let Ok(d) = ch.call_20(dev_idx, idx, 0, &[]) {
            table.desc = Some(OnboardDesc {
                memory_model: d[4],
                profile_format: d[5],
                macro_format: d[6],
                profile_count: d[7],
                profile_count_oob: d[8],
                button_count: d[9],
                sector_count: d[10],
                sector_size: u16::from_be_bytes([d[11], d[12]]).max(64),
            });
        }
    }

    Ok(table)
}

fn build_caps(ft: Option<&FeatTable>, model: &str) -> Capabilities {
    let Some(ft) = ft else {
        // スリープ中初期 caps（覚醒時に再構築）
        return Capabilities {
            settings: vec![],
            actions: vec![],
            profiles: None,
            mouse: Some(MouseCaps {
                dpi_min: 100,
                dpi_max: 12000,
                dpi_step: 50,
                min_stages: 1,
                max_stages: 5,
                xy_split: false,
                stage_colors: false,
                stage_toggle: false,
                buttons: button_names_for(6, true),
                button_kinds: vec![
                    ButtonActionKind::Disabled,
                    ButtonActionKind::Mouse,
                    ButtonActionKind::Dpi,
                    ButtonActionKind::Key,
                    ButtonActionKind::Media,
                    ButtonActionKind::ProfileCycle,
                ],
                require_left_click: true,
                macros: None,
                live_buttons: vec![],
                adaptive_dpi: None,
            }),
            keyboard: None,
            lighting: vec![],
            battery: true,
            peq: None,
            firmware_feed: None,
        };
    };

    let rate_opts: Vec<ChoiceOpt> = ft
        .rates
        .iter()
        .map(|&hz| ChoiceOpt::new(hz as i64, format!("{hz} Hz")))
        .collect();
    let settings = if ft.desc.is_some() || ft.report_rate.is_some() || ft.ext_report_rate.is_some()
    {
        vec![SettingSpec::choice(
            "polling_rate",
            section::PERFORMANCE,
            "ポーリングレート",
            rate_opts,
        )]
    } else {
        vec![]
    };

    let (profiles, buttons, max_stages) = if let Some(desc) = &ft.desc {
        let p = (desc.profile_count > 1).then_some(ProfileCaps {
            count: desc.profile_count,
            renamable: true,
        });
        let btn_n = (desc.button_count as usize).clamp(1, 16);
        let is_gpw = model.contains("gpro")
            || model.contains("4079")
            || model.contains("4074")
            || model.contains("4086");
        (p, button_names_for(btn_n, is_gpw), 5usize)
    } else {
        (None, vec![], 1usize)
    };

    let make_light_area = |id: u8, name: &str| LightArea {
        id,
        name: name.into(),
        effects: vec![
            LightEffect::new(0x01, "単色").color().still(),
            LightEffect::new(0x03, "カラーサイクル"),
            LightEffect::new(0x0A, "呼吸").color(),
        ],
        brightness_max: 100,
        speed_max: 100,
        directions: vec![],
        switchable: true,
        zones: vec![],
        per_key: false,
        direct_drive: false,
        live_readback: false,
    };
    let lighting = if ft.led_count >= 2 {
        vec![make_light_area(0, "ロゴ"), make_light_area(1, "ホイール")]
    } else if ft.led_count == 1 {
        vec![make_light_area(0, "ロゴ / LED")]
    } else {
        vec![]
    };

    let button_kinds = if buttons.is_empty() {
        vec![]
    } else {
        vec![
            ButtonActionKind::Disabled,
            ButtonActionKind::Mouse,
            ButtonActionKind::Dpi,
            ButtonActionKind::Key,
            ButtonActionKind::Media,
            ButtonActionKind::ProfileCycle,
        ]
    };

    let require_left_click = !buttons.is_empty();
    Capabilities {
        settings,
        actions: vec![],
        profiles,
        mouse: Some(MouseCaps {
            dpi_min: ft.dpi_min,
            dpi_max: ft.dpi_max,
            dpi_step: ft.dpi_step,
            min_stages: 1,
            max_stages,
            xy_split: false,
            stage_colors: false,
            stage_toggle: false,
            buttons,
            button_kinds,
            require_left_click,
            macros: None,
            live_buttons: vec![],
            adaptive_dpi: None,
        }),
        keyboard: None,
        lighting,
        battery: ft.batt_unified.is_some()
            || ft.batt_status.is_some()
            || ft.batt_voltage.is_some(),
        peq: None,
        firmware_feed: None,
    }
}

pub fn open(
    dev: hidapi::HidDevice,
    mut info: DeviceInfo,
    events: Watcher,
    detected: DetectedMap,
) -> HubResult<LogitechMouse> {
    let ch: Arc<dyn HidppOps> = Arc::new(DualChannel::open(dev, info.vid, info.pid));
    if info.transport == Transport::Wireless24G {
        read_receiver_meta(&*ch, &mut info);
    }

    let feats = match probe_features(&*ch, info.transport, &mut info) {
        Ok(ft) => {
            info.asleep = false;
            detected
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(info.id.0.clone(), (info.model.clone(), info.name.clone()));
            Some(ft)
        }
        Err(HidppErr::Disconnected) => return Err(HubError::Disconnected),
        Err(_) => {
            // レシーバーが見えておりマウス本体がスリープ/OFF の場合は asleep=true で開く。
            info.asleep = true;
            None
        }
    };

    let caps = build_caps(feats.as_ref(), &info.model);
    let drv = LogitechMouse {
        ch,
        info: Mutex::new(info),
        events,
        detected,
        feats: Mutex::new(feats),
        caps: Mutex::new(caps),
        state_cache: Mutex::new(None),
    };

    if drv.feats.lock().unwrap_or_else(|p| p.into_inner()).is_some() {
        let _ = drv.battery();
    }

    Ok(drv)
}

impl LogitechMouse {
    fn ensure_awake(&self) -> HubResult<FeatTable> {
        if let Some(ft) = self.feats.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            // Ping して生きているか確認
            if self.ch.call_20(ft.dev_idx, 0x00, 1, &[0, 0, 0xAA]).is_ok() {
                return Ok(ft);
            }
        }
        let mut info = self.info.lock().unwrap_or_else(|p| p.into_inner());
        let was_asleep = info.asleep;
        match probe_features(&*self.ch, info.transport, &mut info) {
            Ok(ft) => {
                info.asleep = false;
                self.detected
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(info.id.0.clone(), (info.model.clone(), info.name.clone()));
                *self.feats.lock().unwrap_or_else(|p| p.into_inner()) = Some(ft.clone());
                *self.caps.lock().unwrap_or_else(|p| p.into_inner()) =
                    build_caps(Some(&ft), &info.model);
                let snap = info.clone();
                drop(info);
                if was_asleep {
                    if let Some(cb) = &self.events {
                        cb(HubEvent::Info(snap.id.clone(), Box::new(snap)));
                    }
                }
                Ok(ft)
            }
            Err(HidppErr::Disconnected) => Err(HubError::Disconnected),
            Err(_) => {
                info.asleep = true;
                let snap = info.clone();
                drop(info);
                if !was_asleep {
                    if let Some(cb) = &self.events {
                        cb(HubEvent::Info(snap.id.clone(), Box::new(snap)));
                    }
                }
                Err(HubError::Protocol("マウスがスリープ中です".into()))
            }
        }
    }

    fn read_sector(&self, ft: &FeatTable, ob_idx: u8, sector: u16, size: u16) -> HubResult<Vec<u8>> {
        let mut data = Vec::with_capacity(size as usize);
        let mut off = 0u16;
        while off < size {
            let read_off = if size - off < 16 { size - 16 } else { off };
            let sb = sector.to_be_bytes();
            let ob = read_off.to_be_bytes();
            let r = self
                .ch
                .call_20(ft.dev_idx, ob_idx, 5, &[sb[0], sb[1], ob[0], ob[1]])
                .map_err(HubError::from)?;
            let skip = (off - read_off) as usize;
            let take = (size - off).min(16) as usize;
            data.extend_from_slice(&r[4 + skip..4 + skip + take]);
            off += take as u16;
        }
        // 初回バックアップ保存（未存在時のみ）
        let id_str = self.info.lock().unwrap_or_else(|p| p.into_inner()).id.0.clone();
        if let Some(base) = std::env::var_os("APPDATA").map(PathBuf::from) {
            let dir = base
                .join("DeviceHub")
                .join("backups")
                .join("logitech")
                .join(sanitize_id(&id_str));
            let file = dir.join(format!("sector_{sector:04x}.bin"));
            if !file.exists() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(file, &data);
            }
        }
        Ok(data)
    }

    fn write_sector(&self, ft: &FeatTable, ob_idx: u8, sector: u16, data: &[u8]) -> HubResult<()> {
        let size = data.len() as u16;
        let sb = sector.to_be_bytes();
        let cb = size.to_be_bytes();
        self.ch
            .call_20(ft.dev_idx, ob_idx, 6, &[sb[0], sb[1], 0x00, 0x00, cb[0], cb[1]])
            .map_err(HubError::from)?;
        for chunk in data.chunks(16) {
            let mut block = [0xFFu8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            self.ch
                .call_20(ft.dev_idx, ob_idx, 7, &block)
                .map_err(HubError::from)?;
        }
        self.ch
            .call_20(ft.dev_idx, ob_idx, 8, &[])
            .map_err(HubError::from)?;
        Ok(())
    }

    fn ensure_onboard_mode(&self, ft: &FeatTable, ob_idx: u8) -> HubResult<()> {
        if let Ok(r) = self.ch.call_20(ft.dev_idx, ob_idx, 2, &[]) {
            if r[4] != 1 {
                self.ch
                    .call_20(ft.dev_idx, ob_idx, 1, &[0x01])
                    .map_err(HubError::from)?;
            }
        }
        Ok(())
    }

    fn load_profiles(&self, ft: &FeatTable, ob_idx: u8, desc: &OnboardDesc) -> HubResult<(Vec<ParsedProfile>, usize, bool)> {
        if !(1..=5).contains(&desc.profile_format) {
            return Err(HubError::Unsupported("logitech: unsupported profile format"));
        }
        let pcnt = (desc.profile_count as usize).max(1);
        let btn_n = (desc.button_count as usize).clamp(1, 16);

        let mut any_rom = false;
        let entries = match self.read_sector(ft, ob_idx, 0x0000, desc.sector_size) {
            Ok(dir_raw) => match parse_directory_sector(&dir_raw, pcnt) {
                Ok(e) if !e.is_empty() => e,
                _ => {
                    any_rom = true;
                    vec![]
                }
            },
            Err(_) => {
                any_rom = true;
                vec![]
            }
        };

        let mut profiles = Vec::with_capacity(pcnt);
        let oob = desc.profile_count_oob as usize;
        for i in 0..pcnt {
            let target_sec = (i + 1) as u16;
            if !entries.is_empty() {
                if let Some(&(sector, enabled)) = entries.get(i) {
                    if let Ok(raw) = self.read_sector(ft, ob_idx, sector, desc.sector_size) {
                        if let Ok(p) = parse_profile_sector(
                            sector,
                            enabled,
                            &raw,
                            btn_n,
                            ft.led_count,
                            true,
                        ) {
                            profiles.push(p);
                            continue;
                        }
                    }
                }
            }
            any_rom = true;
            let rom_sec = 0x0100 + if i < oob { (i + 1) as u16 } else { 1 };
            let raw = self.read_sector(ft, ob_idx, rom_sec, desc.sector_size)?;
            profiles.push(parse_profile_sector(
                target_sec,
                i == 0,
                &raw,
                btn_n,
                ft.led_count,
                false,
            )?);
        }

        let active_idx = self
            .ch
            .call_20(ft.dev_idx, ob_idx, 4, &[])
            .ok()
            .map(|r| {
                let sec = u16::from_be_bytes([r[4], r[5]]);
                let low = (sec & 0xFF) as usize;
                low.saturating_sub(1).min(profiles.len().saturating_sub(1))
            })
            .unwrap_or(0);

        Ok((profiles, active_idx, any_rom))
    }

    fn commit_profile(&self, ft: &FeatTable, ob_idx: u8, desc: &OnboardDesc, mut profiles: Vec<ParsedProfile>, active: usize, was_rom: bool) -> HubResult<()> {
        self.ensure_onboard_mode(ft, ob_idx)?;
        let sec_sz = desc.sector_size as usize;
        let was_enabled = profiles[active].enabled;
        profiles[active].sector = (active + 1) as u16;
        profiles[active].enabled = true;

        if was_rom || !was_enabled {
            let mut flags = Vec::with_capacity(profiles.len());
            if was_rom {
                for (i, p) in profiles.iter_mut().enumerate() {
                    p.sector = (i + 1) as u16;
                    flags.push(p.enabled);
                    let data = serialize_profile_sector(p, sec_sz, ft.led_count);
                    self.write_sector(ft, ob_idx, p.sector, &data)?;
                }
            } else {
                for p in &profiles {
                    flags.push(p.enabled);
                }
                let data = serialize_profile_sector(&profiles[active], sec_sz, ft.led_count);
                self.write_sector(ft, ob_idx, profiles[active].sector, &data)?;
            }
            let dir = build_directory_sector(sec_sz, &flags);
            self.write_sector(ft, ob_idx, 0x0000, &dir)?;
            self.ch
                .call_20(ft.dev_idx, ob_idx, 3, &[0x00, (active + 1) as u8])
                .map_err(HubError::from)?;
        } else {
            let data = serialize_profile_sector(&profiles[active], sec_sz, ft.led_count);
            self.write_sector(ft, ob_idx, profiles[active].sector, &data)?;
        }
        *self.state_cache.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((profiles, active, false));
        Ok(())
    }
}

impl DeviceDriver for LogitechMouse {
    fn info(&self) -> DeviceInfo {
        self.info.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn caps(&self) -> Capabilities {
        self.caps.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn read(&self) -> HubResult<DeviceState> {
        let ft = self.ensure_awake()?;
        if let (Some(ob_idx), Some(desc)) = (ft.onboard, ft.desc.clone()) {
            let (profiles, active_idx, from_rom) = self.load_profiles(&ft, ob_idx, &desc)?;
            *self.state_cache.lock().unwrap_or_else(|p| p.into_inner()) =
                Some((profiles.clone(), active_idx, from_rom));

            let p = &profiles[active_idx];
            let active_stage = self
                .ch
                .call_20(ft.dev_idx, ob_idx, 11, &[])
                .ok()
                .map(|r| (r[4] as usize).min(p.stages.len().saturating_sub(1)))
                .unwrap_or(p.default_dpi_idx);

            let mut settings = BTreeMap::new();
            settings.insert(
                "polling_rate".into(),
                SettingValue::Choice(p.report_rate_hz as i64),
            );
            let mut lighting = BTreeMap::new();
            for (idx, ls) in p.lighting.iter().enumerate() {
                lighting.insert(idx as u8, ls.clone());
            }

            return Ok(DeviceState {
                profile: ProfileState {
                    active: active_idx as u8,
                    names: profiles.iter().map(|pr| pr.name.clone()).collect(),
                },
                settings,
                mouse: Some(MouseState {
                    stages: p.stages.clone(),
                    active_stage,
                    buttons: p.buttons.clone(),
                    macros: vec![],
                    adaptive: None,
                }),
                keyboard: None,
                lighting,
                peq: None,
            });
        }

        // Fallback (0x8100 非搭載の簡易 Logitech デバイス)
        let mut stages = vec![DpiStage {
            x: 800,
            y: None,
            color: Rgb::hex(0x00b8fc),
            enabled: true,
        }];
        if let Some(idx) = ft.adj_dpi {
            if let Ok(r) = self.ch.call_20(ft.dev_idx, idx, 2, &[0]) {
                let dpi = u16::from_be_bytes([r[5], r[6]]) as u32;
                if dpi >= 50 {
                    stages[0].x = dpi;
                }
            }
        }
        let mut settings = BTreeMap::new();
        if let Some(idx) = ft.report_rate {
            if let Ok(r) = self.ch.call_20(ft.dev_idx, idx, 1, &[]) {
                let ms = r[4].max(1) as u32;
                settings.insert("polling_rate".into(), SettingValue::Choice((1000 / ms) as i64));
            }
        }
        Ok(DeviceState {
            profile: ProfileState {
                active: 0,
                names: vec![String::new()],
            },
            settings,
            mouse: Some(MouseState {
                stages,
                active_stage: 0,
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
        let ft = self.ensure_awake()?;
        if let (Some(ob_idx), Some(desc)) = (ft.onboard, ft.desc.clone()) {
            let cached = self
                .state_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            let (mut profiles, active_idx, was_rom) = match cached {
                Some(c) => c,
                None => self.load_profiles(&ft, ob_idx, &desc)?,
            };

            match change {
                Change::ActiveStage(idx) => {
                    let s_len = profiles[active_idx].stages.len();
                    let target = (*idx).min(s_len.saturating_sub(1));
                    self.ensure_onboard_mode(&ft, ob_idx)?;
                    if self
                        .ch
                        .call_20(ft.dev_idx, ob_idx, 12, &[target as u8])
                        .is_ok()
                    {
                        profiles[active_idx].default_dpi_idx = target;
                        *self.state_cache.lock().unwrap_or_else(|p| p.into_inner()) =
                            Some((profiles, active_idx, was_rom));
                        return Ok(());
                    }
                    profiles[active_idx].default_dpi_idx = target;
                    return self.commit_profile(&ft, ob_idx, &desc, profiles, active_idx, was_rom);
                }
                Change::Profile(idx) => {
                    let p = (*idx as usize).min(profiles.len().saturating_sub(1));
                    self.ensure_onboard_mode(&ft, ob_idx)?;
                    if was_rom || !profiles[p].enabled {
                        return self.commit_profile(&ft, ob_idx, &desc, profiles, p, was_rom);
                    }
                    self.ch
                        .call_20(ft.dev_idx, ob_idx, 3, &[0x00, (p + 1) as u8])
                        .map_err(HubError::from)?;
                    *self.state_cache.lock().unwrap_or_else(|p| p.into_inner()) =
                        Some((profiles, p, was_rom));
                    return Ok(());
                }
                Change::ProfileName { index, name } => {
                    let p = (*index as usize).min(profiles.len().saturating_sub(1));
                    profiles[p].name = name.clone();
                    return self.commit_profile(&ft, ob_idx, &desc, profiles, p, was_rom);
                }
                Change::DpiStages { stages, active } => {
                    if stages.is_empty() {
                        return Err(HubError::Invalid("stages empty".into()));
                    }
                    profiles[active_idx].stages = stages.iter().take(5).cloned().collect();
                    let act = (*active).min(profiles[active_idx].stages.len() - 1);
                    profiles[active_idx].default_dpi_idx = act;
                    self.commit_profile(&ft, ob_idx, &desc, profiles, active_idx, was_rom)?;
                    let _ = self.ch.call_20(ft.dev_idx, ob_idx, 12, &[act as u8]);
                    return Ok(());
                }
                Change::Button { slot, action } => {
                    let p = &mut profiles[active_idx];
                    if *slot >= p.buttons.len() {
                        return Err(HubError::Invalid("button slot out of range".into()));
                    }
                    p.buttons[*slot] = action.clone();
                    return self.commit_profile(&ft, ob_idx, &desc, profiles, active_idx, was_rom);
                }
                Change::Setting { id, value } if id == "polling_rate" => {
                    profiles[active_idx].report_rate_hz = value.as_choice().max(125) as u32;
                    return self.commit_profile(&ft, ob_idx, &desc, profiles, active_idx, was_rom);
                }
                Change::Lighting { area, state } if ft.has_lighting => {
                    let zi = *area as usize;
                    if zi < profiles[active_idx].lighting.len() {
                        profiles[active_idx].lighting[zi] = state.clone();
                        return self.commit_profile(&ft, ob_idx, &desc, profiles, active_idx, was_rom);
                    }
                    return Err(HubError::Unsupported("lighting area"));
                }
                _ => return Err(HubError::Unsupported("change")),
            }
        }

        // Fallback (0x8100 非搭載機)
        match change {
            Change::DpiStages { stages, active } => {
                let idx = ft.adj_dpi.ok_or(HubError::Unsupported("dpi"))?;
                let st = stages.get(*active).or_else(|| stages.first()).ok_or(HubError::Invalid("empty".into()))?;
                let dpi = (st.x.clamp(ft.dpi_min, ft.dpi_max) as u16).to_be_bytes();
                self.ch
                    .call_20(ft.dev_idx, idx, 3, &[0x00, dpi[0], dpi[1]])
                    .map_err(HubError::from)?;
                Ok(())
            }
            Change::Setting { id, value } if id == "polling_rate" => {
                let idx = ft.report_rate.ok_or(HubError::Unsupported("polling_rate"))?;
                let ms = (1000 / value.as_choice().max(125) as u32).clamp(1, 8) as u8;
                self.ch
                    .call_20(ft.dev_idx, idx, 2, &[ms])
                    .map_err(HubError::from)?;
                Ok(())
            }
            _ => Err(HubError::Unsupported("change")),
        }
    }

    fn battery(&self) -> HubResult<Battery> {
        let ft = self.ensure_awake()?;
        let res = if let Some(idx) = ft.batt_unified {
            let r = self
                .ch
                .call_20(ft.dev_idx, idx, 1, &[])
                .map_err(HubError::from)?;
            Battery {
                percent: Some(r[4].min(100)),
                charging: matches!(r[6], 1 | 2 | 3),
            }
        } else if let Some(idx) = ft.batt_status {
            let r = self
                .ch
                .call_20(ft.dev_idx, idx, 0, &[])
                .map_err(HubError::from)?;
            Battery {
                percent: Some(r[4].min(100)),
                charging: matches!(r[6], 1 | 2 | 3),
            }
        } else if let Some(idx) = ft.batt_voltage {
            let r = self
                .ch
                .call_20(ft.dev_idx, idx, 0, &[])
                .map_err(HubError::from)?;
            let mv = u16::from_be_bytes([r[4], r[5]]);
            let flags = r[6];
            Battery {
                percent: Some(mv_to_percent(mv)),
                charging: (flags & 0x80) != 0 && (flags & 0x07) <= 3,
            }
        } else {
            return Err(HubError::Unsupported("battery"));
        };
        self.info.lock().unwrap_or_else(|p| p.into_inner()).battery = Some(res);
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_ccitt_false_standard_vector() {
        assert_eq!(crc16_ccitt_false(b"123456789"), 0x29B1);
    }

    #[test]
    fn parse_dpi_list_range_and_discrete() {
        // 連続レンジ: min=100, step=50 (0xE032), max=25600
        let range = [0x00, 0x64, 0xE0, 0x32, 0x64, 0x00, 0x00, 0x00];
        assert_eq!(parse_dpi_list(&range), (100, 25600, 50));

        // 離散リスト: 400, 800, 1600, 3200
        let disc = [0x01, 0x90, 0x03, 0x20, 0x06, 0x40, 0x0C, 0x80, 0x00, 0x00];
        assert_eq!(parse_dpi_list(&disc), (400, 3200, 400));
    }

    #[test]
    fn profile_sector_roundtrip_preserves_unknown_bytes_and_crc() {
        let mut raw = vec![0xFFu8; 256];
        raw[0] = 1; // 1000 Hz
        raw[1] = 1; // default stage 1
        raw[2] = 0; // switched stage 0
        raw[3..5].copy_from_slice(&400u16.to_le_bytes());
        raw[5..7].copy_from_slice(&800u16.to_le_bytes());
        raw[7..9].copy_from_slice(&1600u16.to_le_bytes());
        raw[9..11].copy_from_slice(&0u16.to_le_bytes());
        raw[11..13].copy_from_slice(&0u16.to_le_bytes());
        // 未知領域のセンチネル値（保持されること）
        raw[20] = 0x5A;
        raw[100] = 0xA5;

        raw[32..36].copy_from_slice(&[0x80, 0x01, 0x00, 0x01]); // Left
        raw[36..40].copy_from_slice(&[0x80, 0x01, 0x00, 0x02]); // Right
        raw[40..44].copy_from_slice(&[0x80, 0x02, 0x01, 0x04]); // Ctrl+A
        raw[44..48].copy_from_slice(&[0x90, 0x05, 0x00, 0x00]); // DPI Cycle

        let led = encode_led(&LightingState {
            on: true,
            effect: 0x01,
            color: Rgb::new(0x12, 0x34, 0x56),
            brightness: 100,
            speed: 50,
            direction: 0,
            per_key: BTreeMap::new(),
            zone_on: vec![],
            palette: vec![],
            palette_sel: 0,
            color_correction: None,
        });
        raw[208..219].copy_from_slice(&led);
        let crc = crc16_ccitt_false(&raw[..254]);
        raw[254..256].copy_from_slice(&crc.to_be_bytes());

        let mut parsed = parse_profile_sector(1, true, &raw, 6, 1, true).unwrap();
        assert_eq!(parsed.report_rate_hz, 1000);
        assert_eq!(parsed.stages.len(), 3);
        assert_eq!(parsed.stages[1].x, 800);
        assert_eq!(parsed.buttons[0], ButtonAction::Mouse(MouseFn::Left));
        assert_eq!(
            parsed.buttons[2],
            ButtonAction::Key(KeyStroke { mods: 0x01, usage: 0x04 })
        );
        assert_eq!(parsed.buttons[3], ButtonAction::Dpi(DpiFn::Cycle));
        assert_eq!(parsed.lighting[0].color, Rgb::new(0x12, 0x34, 0x56));

        parsed.report_rate_hz = 500;
        parsed.stages[1].x = 1200;
        let out = serialize_profile_sector(&parsed, 256, 1);
        assert_eq!(out.len(), 256);
        assert_eq!(out[20], 0x5A);
        assert_eq!(out[100], 0xA5);
        assert_eq!(out[0], 2);
        let reparsed = parse_profile_sector(1, true, &out, 6, 1, true).unwrap();
        assert_eq!(reparsed.report_rate_hz, 500);
        assert_eq!(reparsed.stages[1].x, 1200);
    }

    #[test]
    fn profile_sector_255_bytes_dual_led_and_rom_no_crc() {
        let mut raw = vec![0xFFu8; 255];
        raw[0] = 1; // 1000 Hz
        raw[1] = 1; // default DPI idx 1
        raw[2] = 0;
        raw[3..5].copy_from_slice(&400u16.to_le_bytes());
        raw[5..7].copy_from_slice(&800u16.to_le_bytes());
        raw[208..219].copy_from_slice(&[0x03, 0, 0, 0, 0, 0, 0x1F, 0x40, 0x64, 0, 0]);
        raw[219..230].copy_from_slice(&[0x03, 0, 0, 0, 0, 0, 0x1F, 0x40, 0x64, 0, 0]);
        raw[230..241].copy_from_slice(&[0x03, 0, 0, 0, 0, 0, 0x1F, 0x40, 0x32, 0, 0]);
        raw[241..252].copy_from_slice(&[0x03, 0, 0, 0, 0, 0, 0x1F, 0x40, 0x32, 0, 0]);
        // ROM セクタ (末尾 0xFF 0xFF で CRC 未設定) を check_crc=false で読めること
        let mut parsed = parse_profile_sector(1, true, &raw, 6, 2, false).unwrap();
        assert_eq!(parsed.lighting.len(), 2);
        assert_eq!(parsed.lighting[0].effect, 0x03);
        assert_eq!(parsed.lighting[1].brightness, 100);

        parsed.report_rate_hz = 250;
        let out = serialize_profile_sector(&parsed, 255, 2);
        assert_eq!(out.len(), 255);
        assert_eq!(out[0], 4);
        let stored = u16::from_be_bytes([out[253], out[254]]);
        assert_eq!(stored, crc16_ccitt_false(&out[..253]));
        let reparsed = parse_profile_sector(1, true, &out, 6, 2, true).unwrap();
        assert_eq!(reparsed.report_rate_hz, 250);
    }

    #[test]
    fn report_rate_mask_0x8060_real_hardware() {
        assert_eq!(decode_report_rate_mask(0x8B, false), vec![125, 250, 500, 1000]);
    }

    #[test]
    fn directory_sector_roundtrip() {
        for sz in [255usize, 256] {
            let dir = build_directory_sector(sz, &[true, false, true]);
            assert_eq!(dir.len(), sz);
            let parsed = parse_directory_sector(&dir, 5).unwrap();
            assert_eq!(parsed, vec![(1, true), (2, false), (3, true)]);
        }
    }
}
