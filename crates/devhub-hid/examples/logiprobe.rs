//! Read-only probe for Logitech HID++ 1.0/2.0 mice (G PRO Wireless / LIGHTSPEED).
//! Opens BOTH 0xFF00:0x01 (Short 7B, 0x10) and 0xFF00:0x02 (Long 20B, 0x11)
//! simultaneously so replies/errors on either TLC are never missed.

use hidapi::HidApi;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SW_ID: u8 = 0x0A;

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn crc16_ccitt_false(data: &[u8]) -> u16 {
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

#[derive(Debug)]
#[allow(dead_code)]
enum ProbeErr {
    Io(String),
    Timeout,
    Hidpp10(u8),
    Hidpp20(u8),
}

struct DualChannel {
    short: Option<hidapi::HidDevice>,
    long: hidapi::HidDevice,
}

impl DualChannel {
    fn drain(&self) {
        let mut buf = [0u8; 64];
        if let Some(s) = &self.short {
            while matches!(s.read_timeout(&mut buf, 5), Ok(n) if n > 0) {}
        }
        while matches!(self.long.read_timeout(&mut buf, 5), Ok(n) if n > 0) {}
    }

    /// Send a 7B HID++ 1.0 short request on `short` (or `long` if no `short`) and listen on both.
    fn send_short_and_listen(
        &self,
        dev_idx: u8,
        sub_id: u8,
        addr: u8,
        p0: u8,
        p1: u8,
        p2: u8,
    ) -> Result<Vec<u8>, ProbeErr> {
        let req = [0x10u8, dev_idx, sub_id, addr, p0, p1, p2];
        if let Some(s) = &self.short {
            s.write(&req)
                .map_err(|e| ProbeErr::Io(format!("short write: {e}")))?;
        } else {
            let mut lreq = [0u8; 20];
            lreq[0] = 0x11;
            lreq[1..7].copy_from_slice(&req[1..7]);
            self.long
                .write(&lreq)
                .map_err(|e| ProbeErr::Io(format!("long write: {e}")))?;
        }
        let deadline = Instant::now() + Duration::from_millis(400);
        let mut buf = [0u8; 64];
        loop {
            if Instant::now() >= deadline {
                return Err(ProbeErr::Timeout);
            }
            if let Some(s) = &self.short {
                if let Ok(n) = s.read_timeout(&mut buf, 30) {
                    if n > 0 {
                        if n >= 6 && buf[1] == dev_idx && buf[2] == 0x8F && buf[3] == sub_id {
                            return Err(ProbeErr::Hidpp10(buf[5]));
                        }
                        if n >= 4 && buf[1] == dev_idx && buf[2] == sub_id && buf[3] == addr {
                            return Ok(buf[..n].to_vec());
                        }
                        println!("  [short rx len={n}] {}", hex(&buf[..n]));
                    }
                }
            }
            if let Ok(n) = self.long.read_timeout(&mut buf, 30) {
                if n > 0 {
                    if n >= 6 && buf[1] == dev_idx && buf[2] == 0x8F && buf[3] == sub_id {
                        return Err(ProbeErr::Hidpp10(buf[5]));
                    }
                    if n >= 4 && buf[1] == dev_idx && buf[2] == sub_id && buf[3] == addr {
                        return Ok(buf[..n].to_vec());
                    }
                    println!("  [long rx len={n}] {}", hex(&buf[..n]));
                }
            }
        }
    }

    fn call_20(
        &self,
        dev_idx: u8,
        feat_idx: u8,
        func: u8,
        args: &[u8],
    ) -> Result<[u8; 20], ProbeErr> {
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
            self.long
                .write(&req)
                .map_err(|e| ProbeErr::Io(format!("write: {e}")))?;
            let deadline = Instant::now() + Duration::from_millis(800);
            loop {
                if Instant::now() >= deadline {
                    break;
                }
                let mut buf = [0u8; 64];
                // Check short channel too (receiver returns HID++ 1.0 0x8F errors as 0x10 short!)
                if let Some(s) = &self.short {
                    if let Ok(len) = s.read_timeout(&mut buf, 10) {
                        if len >= 6 && buf[0] == 0x10 && buf[1] == dev_idx {
                            if buf[2] == 0x8F && buf[3] == feat_idx && buf[4] == fn_sw {
                                return Err(ProbeErr::Hidpp10(buf[5]));
                            }
                            println!("  [short rx len={len}] {}", hex(&buf[..len]));
                        } else if len > 0 {
                            println!("  [short rx len={len}] {}", hex(&buf[..len]));
                        }
                    }
                }
                match self.long.read_timeout(&mut buf, 50) {
                    Ok(len) if len >= 20 && buf[0] == 0x11 && buf[1] == dev_idx => {
                        if buf[2] == 0xFF && buf[3] == feat_idx && buf[4] == fn_sw {
                            return Err(ProbeErr::Hidpp20(buf[5]));
                        }
                        if buf[2] == 0x8F && buf[3] == feat_idx && buf[4] == fn_sw {
                            return Err(ProbeErr::Hidpp10(buf[5]));
                        }
                        if buf[2] == feat_idx && buf[3] == fn_sw {
                            let mut out = [0u8; 20];
                            out.copy_from_slice(&buf[..20]);
                            return Ok(out);
                        }
                        println!("  [long skip] {}", hex(&buf[..len]));
                    }
                    Ok(len) if len > 0 => {
                        println!("  [long other len={len}] {}", hex(&buf[..len]));
                    }
                    Ok(_) => {}
                    Err(e) => return Err(ProbeErr::Io(format!("read: {e}"))),
                }
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        Err(ProbeErr::Timeout)
    }
}

fn feature_name(id: u16) -> &'static str {
    match id {
        0x0000 => "IRoot",
        0x0001 => "IFeatureSet",
        0x0003 => "DeviceFwInfo",
        0x0005 => "DeviceName",
        0x0007 => "DeviceFriendlyName",
        0x0020 => "ConfigChange",
        0x0021 => "UniqueIdentifier",
        0x00C2 => "DFUControl",
        0x00D0 => "DFU",
        0x1000 => "BatteryStatus",
        0x1001 => "BatteryVoltage",
        0x1004 => "UnifiedBattery",
        0x1800 => "GenericTest",
        0x1814 => "ChangeHost",
        0x1830 => "PowerModes",
        0x1861 => "BatteryLevelsCalibration",
        0x1890 => "RFTest",
        0x18B0 => "MonitorMode",
        0x1D4B => "WirelessDeviceStatus",
        0x1DF3 => "Equinox",
        0x1E00 => "EnableHiddenFeatures",
        0x1EB0 => "TDEAccessToNVM",
        0x2110 => "SmartShift",
        0x2121 => "HiResWheel",
        0x2201 => "AdjustableDpi",
        0x2205 => "PointerMotionScaling",
        0x2230 => "AngleSnapping",
        0x2240 => "SurfaceTuning",
        0x2250 => "AnalysisMode",
        0x8030 => "MacroRecord",
        0x8040 => "BrightnessControl",
        0x8060 => "ReportRate",
        0x8061 => "ExtendedAdjustableReportRate",
        0x8070 => "ColorLedEffects",
        0x8071 => "RgbEffects",
        0x8100 => "OnboardProfiles",
        0x8110 => "MouseButtonSpy",
        0x8111 => "LatencyMonitoring",
        0x8120 => "GamingAttachments",
        0x8123 => "ForceOnButton",
        _ => "Unknown",
    }
}

fn main() {
    let api = HidApi::new().expect("HidApi::new");
    println!("== Logitech 046d interfaces ==");
    let mut long_devs = Vec::new();
    let mut short_devs = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() == 0x046D {
            println!(
                "  {:04x}:{:04x} if={} up={:04x} u={:02x} name={:?} serial={:?}",
                d.vendor_id(),
                d.product_id(),
                d.interface_number(),
                d.usage_page(),
                d.usage(),
                d.product_string(),
                d.serial_number(),
            );
            if d.usage_page() == 0xFF00 && d.usage() == 0x02 {
                long_devs.push((
                    d.product_id(),
                    d.interface_number(),
                    d.path().to_owned(),
                    d.product_string().map(str::to_string),
                ));
            }
            if d.usage_page() == 0xFF00 && d.usage() == 0x01 {
                short_devs.push((d.product_id(), d.interface_number(), d.path().to_owned()));
            }
        }
    }

    for (pid, if_num, path, name) in long_devs {
        println!("\n== Probing 046d:{pid:04x} if={if_num} ({name:?}) ==");
        let long = match api.open_path(&path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  open long failed: {e}");
                continue;
            }
        };
        long.set_blocking_mode(false).ok();
        let short = short_devs
            .iter()
            .find(|(sp, si, _)| *sp == pid && *si == if_num)
            .and_then(|(_, _, spath)| api.open_path(spath).ok());
        if let Some(s) = &short {
            s.set_blocking_mode(false).ok();
            println!("  Opened paired short 0xFF00:0x01 channel too!");
        }
        let ch = DualChannel { short, long };
        ch.drain();

        // Probe receiver HID++ 1.0 registers on dev_idx=0xFF
        println!("-- Receiver (0xFF) HID++ 1.0 checks --");
        for (label, sub, addr, p0) in [
            ("GET_REG 0x00 (notif)", 0x81u8, 0x00u8, 0x00u8),
            ("GET_REG 0x02 (conn)", 0x81, 0x02, 0x00),
            ("GET_REG 0xF1 (fw1)", 0x81, 0xF1, 0x01),
            ("GET_LONG_REG 0xF1 (fw1)", 0x83, 0xF1, 0x01),
            ("GET_LONG_REG 0xF1 (fw2)", 0x83, 0xF1, 0x02),
            ("GET_LONG_REG 0xF1 (fw3)", 0x83, 0xF1, 0x03),
            ("GET_LONG_REG 0xF1 (fw4)", 0x83, 0xF1, 0x04),
            ("GET_LONG_REG 0xB5 (pair 03)", 0x83, 0xB5, 0x03),
            ("GET_LONG_REG 0xB5 (pair 20)", 0x83, 0xB5, 0x20),
            ("GET_LONG_REG 0xB5 (pair 30)", 0x83, 0xB5, 0x30),
            ("GET_LONG_REG 0xB5 (pair 40)", 0x83, 0xB5, 0x40),
            ("GET_REG 0xF1 (fw2)", 0x81, 0xF1, 0x02),
            ("GET_REG 0xF1 (fw3)", 0x81, 0xF1, 0x03),
            ("GET_REG 0xF1 (fw4)", 0x81, 0xF1, 0x04),
        ] {
            let res = ch.send_short_and_listen(0xFF, sub, addr, p0, 0, 0);
            println!("  {label}: {res:?} {}", match &res { Ok(v) => hex(v), Err(_) => String::new() });
        }

        // Probe HID++ 2.0 PING across indices
        println!("-- HID++ 2.0 PING --");
        let mut active_idx = None;
        for dev_idx in [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0xFF] {
            match ch.call_20(dev_idx, 0x00, 1, &[0x00, 0x00, 0xAA]) {
                Ok(r) => {
                    println!("  dev_idx={dev_idx:#04x} PING OK: ver={}.{} raw=[{}]", r[4], r[5], hex(&r));
                    if dev_idx != 0xFF && active_idx.is_none() {
                        active_idx = Some(dev_idx);
                    }
                }
                Err(e) => println!("  dev_idx={dev_idx:#04x} PING err: {e:?}"),
            }
        }

        let Some(dev_idx) = active_idx else {
            println!("  No active HID++ 2.0 device responded (check if mouse is turned ON / awake, or connected by USB).");
            continue;
        };

        println!("\n== Feature discovery on dev_idx={dev_idx:#04x} ==");
        let Ok(r_fs) = ch.call_20(dev_idx, 0x00, 0, &[0x00, 0x01, 0x00]) else {
            eprintln!("  IFeatureSet lookup failed");
            continue;
        };
        let fs_idx = r_fs[4];
        println!("  IFeatureSet (0x0001) at index {fs_idx} (type={:#04x})", r_fs[5]);
        let Ok(r_cnt) = ch.call_20(dev_idx, fs_idx, 0, &[]) else {
            eprintln!("  IFeatureSet::getCount failed");
            continue;
        };
        let count = r_cnt[4];
        println!("  Feature count = {count}");

        let mut features: Vec<(u8, u16, u8)> = Vec::new();
        for i in 0..=count {
            match ch.call_20(dev_idx, fs_idx, 1, &[i]) {
                Ok(r) => {
                    let fid = u16::from_be_bytes([r[4], r[5]]);
                    let ftype = r[6];
                    println!("    [{i:2}] 0x{fid:04X} ({:<28}) type={ftype:#04x}", feature_name(fid));
                    features.push((i, fid, ftype));
                }
                Err(e) => println!("    [{i:2}] err={e:?}"),
            }
        }

        let find_feat = |target: u16| -> Option<u8> {
            features.iter().find(|(_, id, _)| *id == target).map(|(idx, _, _)| *idx)
        };

        if let Some(idx) = find_feat(0x0005) {
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
                let dev_name = String::from_utf8_lossy(&name_bytes).to_string();
                let dev_type = ch.call_20(dev_idx, idx, 2, &[]).ok().map(|r| r[4]);
                println!("\n== DeviceName (0x0005): len={len} name={dev_name:?} type={dev_type:?}");
            }
        }

        if let Some(idx) = find_feat(0x0003) {
            if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
                let n = r[4];
                println!("\n== DeviceFwInfo (0x0003): entities={n}");
                for e in 0..n {
                    if let Ok(rf) = ch.call_20(dev_idx, idx, 1, &[e]) {
                        println!("  entity[{e}]: raw=[{}]", hex(&rf[4..]));
                    }
                }
            }
        }

        if let Some(idx) = find_feat(0x1004) {
            if let Ok(rc) = ch.call_20(dev_idx, idx, 0, &[]) {
                println!("\n== UnifiedBattery (0x1004) caps: [{}]", hex(&rc[4..10]));
            }
            if let Ok(r) = ch.call_20(dev_idx, idx, 1, &[]) {
                println!(
                    "   status: pct={}% level={} status={} ext={} raw=[{}]",
                    r[4], r[5], r[6], r[7], hex(&r[4..10])
                );
            }
        }
        if let Some(idx) = find_feat(0x1000) {
            if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
                println!("\n== BatteryStatus (0x1000): raw=[{}]", hex(&r[4..10]));
            }
        }
        if let Some(idx) = find_feat(0x1001) {
            if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
                println!("\n== BatteryVoltage (0x1001): raw=[{}]", hex(&r[4..10]));
            }
        }

        if let Some(idx) = find_feat(0x2201) {
            let cnt = ch.call_20(dev_idx, idx, 0, &[]).ok().map(|r| r[4]);
            let list = ch.call_20(dev_idx, idx, 1, &[0]).ok();
            let cur = ch.call_20(dev_idx, idx, 2, &[0]).ok();
            println!("\n== AdjustableDpi (0x2201): count={cnt:?}");
            if let Some(r) = list { println!("   dpiList: [{}]", hex(&r[4..])); }
            if let Some(r) = cur { println!("   curDpi:  [{}]", hex(&r[4..12])); }
        }

        if let Some(idx) = find_feat(0x8060) {
            let list = ch.call_20(dev_idx, idx, 0, &[]).ok();
            let cur = ch.call_20(dev_idx, idx, 1, &[]).ok();
            println!("\n== ReportRate (0x8060):");
            if let Some(r) = list { println!("   list: [{}]", hex(&r[4..8])); }
            if let Some(r) = cur { println!("   cur:  [{}]", hex(&r[4..8])); }
        }
        if let Some(idx) = find_feat(0x8061) {
            let list = ch.call_20(dev_idx, idx, 0, &[]).ok();
            let cur = ch.call_20(dev_idx, idx, 1, &[]).ok();
            println!("\n== ExtReportRate (0x8061):");
            if let Some(r) = list { println!("   list: [{}]", hex(&r[4..8])); }
            if let Some(r) = cur { println!("   cur:  [{}]", hex(&r[4..8])); }
        }

        if let Some(idx) = find_feat(0x8070) {
            println!("\n== ColorLedEffects (0x8070):");
            if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
                println!("   getInfo: [{}]", hex(&r[4..]));
                let zones = r[4];
                for z in 0..zones {
                    if let Ok(rz) = ch.call_20(dev_idx, idx, 1, &[z]) {
                        println!("   zone[{z}] info: [{}]", hex(&rz[4..]));
                        let n_eff = rz[7];
                        for e in 0..n_eff {
                            if let Ok(re) = ch.call_20(dev_idx, idx, 2, &[z, e]) {
                                println!("     eff[{e}]: [{}]", hex(&re[4..]));
                            }
                        }
                    }
                }
            }
        }
        if let Some(idx) = find_feat(0x8071) {
            println!("\n== RgbEffects (0x8071):");
            if let Ok(r) = ch.call_20(dev_idx, idx, 0, &[]) {
                println!("   getInfo: [{}]", hex(&r[4..]));
                let zones = r[4];
                for z in 0..zones {
                    if let Ok(rz) = ch.call_20(dev_idx, idx, 1, &[z]) {
                        println!("   zone[{z}] info: [{}]", hex(&rz[4..]));
                    }
                    if let Ok(rc) = ch.call_20(dev_idx, idx, 4, &[z]) {
                        println!("   zone[{z}] cur:  [{}]", hex(&rc[4..]));
                    }
                }
            }
        }

        if let Some(idx) = find_feat(0x8100) {
            println!("\n== OnboardProfiles (0x8100) at index {idx} ==");
            let desc = ch.call_20(dev_idx, idx, 0, &[]).expect("0x8100 getDescription");
            let mem_model = desc[4];
            let prof_fmt = desc[5];
            let macro_fmt = desc[6];
            let prof_cnt = desc[7];
            let prof_cnt_oob = desc[8];
            let btn_cnt = desc[9];
            let sec_cnt = desc[10];
            let sec_size = u16::from_be_bytes([desc[11], desc[12]]);
            let mech_layout = desc[13];
            let various_info = desc[14];
            println!("  desc raw: [{}]", hex(&desc[4..]));
            println!(
                "  mem_model={mem_model} prof_fmt={prof_fmt} macro_fmt={macro_fmt} prof_cnt={prof_cnt} prof_cnt_oob={prof_cnt_oob} btn_cnt={btn_cnt} sec_cnt={sec_cnt} sec_size={sec_size} mech_layout={mech_layout:#04x} various_info={various_info:#04x}"
            );

            let mode = ch.call_20(dev_idx, idx, 2, &[]).ok().map(|r| r[4]);
            let cur_prof = ch.call_20(dev_idx, idx, 4, &[]).ok().map(|r| u16::from_be_bytes([r[4], r[5]]));
            let cur_dpi = ch.call_20(dev_idx, idx, 11, &[]).ok().map(|r| r[4]);
            println!(
                "  mode={mode:?} (1=Onboard, 2=Host) cur_profile={cur_prof:?} ({:#06x}) cur_dpi_idx={cur_dpi:?}",
                cur_prof.unwrap_or(0)
            );

            let backup_dir = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .map(|p| {
                    p.join("DeviceHub")
                        .join("backups")
                        .join("logitech")
                        .join(format!("probe_{pid:04x}"))
                });
            if let Some(dir) = &backup_dir {
                let _ = fs::create_dir_all(dir);
            }

            let mut sectors_to_read: Vec<u16> = vec![0x0000];
            for s in 1..=prof_cnt as u16 {
                sectors_to_read.push(s);
            }
            if prof_cnt_oob > 0 {
                sectors_to_read.push(0x0100);
                for s in 1..=prof_cnt_oob as u16 {
                    sectors_to_read.push(0x0100 | s);
                }
            }

            for sector in sectors_to_read {
                let mut data = Vec::with_capacity(sec_size as usize);
                let mut ok = true;
                let mut off = 0u16;
                while off < sec_size {
                    let read_off = if sec_size - off < 16 { sec_size - 16 } else { off };
                    let s_bytes = sector.to_be_bytes();
                    let o_bytes = read_off.to_be_bytes();
                    match ch.call_20(dev_idx, idx, 5, &[s_bytes[0], s_bytes[1], o_bytes[0], o_bytes[1]]) {
                        Ok(r) => {
                            let skip = (off - read_off) as usize;
                            let take = (sec_size - off).min(16) as usize;
                            data.extend_from_slice(&r[4 + skip..4 + skip + take]);
                            off += take as u16;
                        }
                        Err(e) => {
                            println!("  sector 0x{sector:04X} off={off} read err: {e:?}");
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok { continue; }
                if let Some(dir) = &backup_dir {
                    let _ = fs::write(dir.join(format!("sector_{sector:04x}.bin")), &data);
                }
                let stored_crc = if data.len() >= 2 {
                    u16::from_be_bytes([data[data.len() - 2], data[data.len() - 1]])
                } else { 0 };
                let calc_crc = if data.len() >= 2 {
                    crc16_ccitt_false(&data[..data.len() - 2])
                } else { 0 };
                println!(
                    "\n  -- Sector 0x{sector:04X} ({} bytes) CRC stored=0x{stored_crc:04X} calc=0x{calc_crc:04X} match={} --",
                    data.len(),
                    stored_crc == calc_crc
                );
                for (row_idx, chunk) in data.chunks(16).enumerate() {
                    println!("    {:04x}: {}", row_idx * 16, hex(chunk));
                }
            }
        }
    }
}
