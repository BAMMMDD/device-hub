//! E2E verification for Logitech HID++ 2.0 mice (safe reversible read-write-restore).
//! `cargo run -p devhub-hid --example logie2e`

use devhub_core::backend::DeviceDriver;
use devhub_core::model::*;
use devhub_core::registry;
use devhub_hid::logitech;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn main() {
    let api = hidapi::HidApi::new().expect("HidApi::new");
    let dev_meta = api
        .device_list()
        .find(|d| {
            d.vendor_id() == 0x046D
                && registry::match_device(
                    d.vendor_id(),
                    d.product_id(),
                    d.usage_page(),
                    d.usage(),
                )
                .is_some()
        })
        .expect("No supported Logitech 046D mouse/receiver found");

    let spec = registry::match_device(
        dev_meta.vendor_id(),
        dev_meta.product_id(),
        dev_meta.usage_page(),
        dev_meta.usage(),
    )
    .expect("registry match");

    let dev = api.open_path(dev_meta.path()).expect("open_path");
    let info = DeviceInfo {
        id: DeviceId(format!(
            "hid:{:04x}:{:04x}:e2e",
            spec.vid, spec.pid
        )),
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
    println!("info: {:?}", cur_info);
    if cur_info.asleep {
        panic!("Logitech mouse is asleep/off; wake it up before running logie2e");
    }

    let st0 = drv.read().expect("initial read");
    println!("initial state: {:?}", st0);
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
        println!("ALL LOGITECH E2E VERIFICATIONS PASSED!");
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
        println!("ALL LOGITECH E2E VERIFICATIONS PASSED!");
    }
}
