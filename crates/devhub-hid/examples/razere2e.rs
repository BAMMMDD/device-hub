//! E2E verification for Razer DeathAdder V4 Pro (safe reversible read-write-restore).
//! `cargo run -p devhub-hid --example razere2e`

use devhub_core::backend::DeviceDriver;
use devhub_core::model::*;
use devhub_core::registry;
use devhub_hid::razer;

fn main() {
    let api = hidapi::HidApi::new().expect("HidApi::new");
    let dev_meta = api
        .device_list()
        .find(|d| {
            d.vendor_id() == 0x1532
                && (d.product_id() == 0x00BF || d.product_id() == 0x00BE)
                && d.usage_page() == 0x0001
                && d.usage() == 0x02
        })
        .expect("Razer DeathAdder V4 Pro (1532:00BF/00BE) not found");

    let spec = registry::match_device(
        dev_meta.vendor_id(),
        dev_meta.product_id(),
        dev_meta.usage_page(),
        dev_meta.usage(),
    )
    .expect("registry match");

    let dev = api.open_path(dev_meta.path()).expect("open_path");
    let info = DeviceInfo {
        id: DeviceId("hid:1532:00bf:e2e".into()),
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

    let drv = razer::open(dev, info, None).expect("razer::open");
    println!("info: {:?}", drv.info());
    let caps = drv.caps();
    println!("caps settings: {}", caps.settings.len());
    let st0 = drv.read().expect("initial read");
    println!("initial state: {:?}", st0);
    let bat = drv.battery().expect("battery");
    println!("battery: {:?}", bat);

    // 1) Reversible sleep_time test
    let orig_sleep = st0
        .settings
        .get("sleep_time")
        .map(|v| v.as_num())
        .expect("sleep_time present");
    let test_sleep = if (orig_sleep - 6.0).abs() < 0.1 { 5.0 } else { 6.0 };
    println!("Testing sleep_time: {orig_sleep} -> {test_sleep} -> {orig_sleep}");
    drv.apply(&Change::Setting {
        id: "sleep_time".into(),
        value: SettingValue::Num(test_sleep),
    })
    .expect("apply test_sleep");
    let st1 = drv.read().expect("read after test_sleep");
    let got_sleep = st1.settings.get("sleep_time").unwrap().as_num();
    assert!(
        (got_sleep - test_sleep).abs() < 0.1,
        "sleep_time mismatch: got {got_sleep}, expected {test_sleep}"
    );
    drv.apply(&Change::Setting {
        id: "sleep_time".into(),
        value: SettingValue::Num(orig_sleep),
    })
    .expect("restore sleep_time");
    let st2 = drv.read().expect("read after restore sleep_time");
    let rest_sleep = st2.settings.get("sleep_time").unwrap().as_num();
    assert!(
        (rest_sleep - orig_sleep).abs() < 0.1,
        "sleep_time restore mismatch: got {rest_sleep}, expected {orig_sleep}"
    );
    println!("sleep_time roundtrip OK!");

    // 2) Reversible ActiveStage test
    let m0 = st0.mouse.as_ref().expect("mouse state");
    if m0.stages.len() >= 2 {
        let orig_stage = m0.active_stage;
        let test_stage = if orig_stage == 0 { 1 } else { 0 };
        println!("Testing active_stage: {orig_stage} -> {test_stage} -> {orig_stage}");
        drv.apply(&Change::ActiveStage(test_stage))
            .expect("apply test_stage");
        let st_mid = drv.read().expect("read after test_stage");
        let got_stage = st_mid.mouse.as_ref().unwrap().active_stage;
        assert_eq!(
            got_stage, test_stage,
            "active_stage mismatch: got {got_stage}, expected {test_stage}"
        );
        drv.apply(&Change::ActiveStage(orig_stage))
            .expect("restore active_stage");
        let st_end = drv.read().expect("read after restore stage");
        let rest_stage = st_end.mouse.as_ref().unwrap().active_stage;
        assert_eq!(
            rest_stage, orig_stage,
            "active_stage restore mismatch: got {rest_stage}, expected {orig_stage}"
        );
        println!("active_stage roundtrip OK!");
    }

    // 3) Reversible polling_rate test
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
    .expect("apply test_hz");
    let st_hz1 = drv.read().expect("read after test_hz");
    let got_hz = st_hz1.settings.get("polling_rate").unwrap().as_choice();
    assert_eq!(
        got_hz, test_hz,
        "polling_rate mismatch: got {got_hz}, expected {test_hz}"
    );
    drv.apply(&Change::Setting {
        id: "polling_rate".into(),
        value: SettingValue::Choice(orig_hz),
    })
    .expect("restore polling_rate");
    let st_hz2 = drv.read().expect("read after restore polling_rate");
    let rest_hz = st_hz2.settings.get("polling_rate").unwrap().as_choice();
    assert_eq!(
        rest_hz, orig_hz,
        "polling_rate restore mismatch: got {rest_hz}, expected {orig_hz}"
    );
    println!("polling_rate roundtrip OK!");

    println!("ALL RAZER E2E VERIFICATIONS PASSED!");
}
