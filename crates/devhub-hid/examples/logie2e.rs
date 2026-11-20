//! E2E verification for ALL connected Logitech HID++ 2.0 mice (safe reversible read-write-restore).
//! `cargo run -p devhub-hid --example logie2e`

use devhub_core::backend::DeviceDriver;
use devhub_core::model::*;
use devhub_core::registry;
use devhub_hid::logitech;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn main() {
    let api = hidapi::HidApi::new().expect("HidApi::new");
    let mut seen_pids = HashSet::new();
    let mut verified_count = 0usize;

    for dev_meta in api.device_list() {
        if dev_meta.vendor_id() != 0x046D {
            continue;
        }
        let Some(spec) = registry::match_device(
            dev_meta.vendor_id(),
            dev_meta.product_id(),
            dev_meta.usage_page(),
            dev_meta.usage(),
        ) else {
            continue;
        };
        if !seen_pids.insert((spec.vid, spec.pid)) {
            continue;
        }

        println!("\n=== E2E Testing {:04X}:{:04X} ({}) ===", spec.vid, spec.pid, spec.name);
        let dev = api.open_path(dev_meta.path()).expect("open_path");
        let id_str = format!("hid:{:04x}:{:04x}:e2e", spec.vid, spec.pid);
        let info = DeviceInfo {
            id: DeviceId(id_str.clone()),
            kind: spec.kind,
            name: spec.name.into(),
            vendor: spec.vendor.into(),
            model: spec.model.into(),
            vid: spec.vid,
            pid: spec.pid,
            transport: spec.transport,
            firmware: None,
            dongle_firmware: None,
            serial: dev_meta.serial_number().map(str::to_string),
            driver: spec.driver,
            battery: None,
            supported: true,
            asleep: false,
            board_id: None,
        };

        let detected = Arc::new(Mutex::new(HashMap::new()));
        let drv = logitech::open(dev, info, None, detected).expect("logitech::open");
        let cur_info = drv.info();
        println!("info: name={:?} model={} fw={:?} dongle={:?} asleep={}",
            cur_info.name, cur_info.model, cur_info.firmware, cur_info.dongle_firmware, cur_info.asleep);
        if cur_info.asleep {
            panic!("Logitech mouse {} ({:04X}:{:04X}) is asleep/off; wake it up before running logie2e", cur_info.name, spec.vid, spec.pid);
        }

        let caps = drv.caps();
        let bat = drv.battery();
        println!("battery: {bat:?}, caps lighting areas: {}", caps.lighting.len());
        let st0 = drv.read().expect("initial read");
        println!("initial state: profile={}, rate={:?}, mouse={:?}",
            st0.profile.active,
            st0.settings.get("polling_rate"),
            st0.mouse.as_ref().map(|m| (m.active_stage, m.stages.iter().map(|s| s.x).collect::<Vec<_>>())));

        // Verify onboard sector backup exists under %APPDATA%\DeviceHub\backups\logitech\
        let backup_dir = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .expect("APPDATA env missing")
            .join("DeviceHub")
            .join("backups")
            .join("logitech")
            .join(sanitize_id(&id_str));
        let backup_files: Vec<_> = std::fs::read_dir(&backup_dir)
            .unwrap_or_else(|e| panic!("backup dir {backup_dir:?} unreadable: {e}"))
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            !backup_files.is_empty(),
            "expected onboard sector backup files in {backup_dir:?}"
        );
        println!("verified {} sector backup file(s) in {backup_dir:?}", backup_files.len());

        // 1) Reversible ActiveStage or DPI test
        let m0 = st0.mouse.as_ref().expect("mouse state");
        if m0.stages.len() >= 2 {
            let orig_stage = m0.active_stage;
            let test_stage = if orig_stage == 0 { 1 } else { 0 };
            println!("Testing active_stage: {orig_stage} -> {test_stage} -> {orig_stage}");
            drv.apply(&Change::ActiveStage(test_stage))
                .expect("apply test_stage");
            let st1 = drv.read().expect("read after test_stage");
            assert_eq!(st1.mouse.as_ref().unwrap().active_stage, test_stage);
            drv.apply(&Change::ActiveStage(orig_stage))
                .expect("restore active_stage");
            let st2 = drv.read().expect("read after restore");
            assert_eq!(st2.mouse.as_ref().unwrap().active_stage, orig_stage);
            println!("active_stage roundtrip OK!");
        } else {
            println!("Single DPI stage only; testing DPI write/restore on stage 0");
            let orig = m0.stages.clone();
            let mut mod_stages = orig.clone();
            mod_stages[0].x = if orig[0].x == 800 { 1000 } else { 800 };
            drv.apply(&Change::DpiStages {
                stages: mod_stages.clone(),
                active: 0,
            })
            .expect("apply mod dpi");
            let st1 = drv.read().expect("read after mod dpi");
            assert_eq!(st1.mouse.as_ref().unwrap().stages[0].x, mod_stages[0].x);
            drv.apply(&Change::DpiStages {
                stages: orig.clone(),
                active: 0,
            })
            .expect("restore dpi");
            let st2 = drv.read().expect("read after restore dpi");
            assert_eq!(st2.mouse.as_ref().unwrap().stages[0].x, orig[0].x);
            println!("DPI roundtrip OK!");
        }

        // 2) Reversible polling_rate test (exercises 255B sector write + CRC-16 + readback + restore)
        let orig_hz = st0
            .settings
            .get("polling_rate")
            .map(|v| v.as_choice())
            .expect("polling_rate present");
        let test_hz = if orig_hz == 500 { 1000 } else { 500 };
        println!("Testing polling_rate: {orig_hz} -> {test_hz} -> {orig_hz}");
        drv.apply(&Change::Setting {
            id: "polling_rate".into(),
            value: SettingValue::Choice(test_hz),
        })
        .expect("apply test polling_rate");
        let st_hz1 = drv.read().expect("read after test polling_rate");
        assert_eq!(
            st_hz1.settings.get("polling_rate").unwrap().as_choice(),
            test_hz
        );
        drv.apply(&Change::Setting {
            id: "polling_rate".into(),
            value: SettingValue::Choice(orig_hz),
        })
        .expect("restore polling_rate");
        let st_hz2 = drv.read().expect("read after restore polling_rate");
        assert_eq!(
            st_hz2.settings.get("polling_rate").unwrap().as_choice(),
            orig_hz
        );
        println!("polling_rate 255B sector write/CRC/readback/restore roundtrip OK!");

        verified_count += 1;
    }

    assert!(verified_count > 0, "No Logitech 046D mice verified!");
    println!("\nALL {verified_count} LOGITECH MICE E2E VERIFICATIONS PASSED!");
}
