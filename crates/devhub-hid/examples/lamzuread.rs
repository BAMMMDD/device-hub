//! LAMZU ドライバ経路の実機検証 — lamzu::open → caps → read → apply(読み戻し) → 復元。
//! `cargo run -p devhub-hid --example lamzuread`
//!
//! Maya X V2 / LM20 有線 (37b0:0044) 専用。往復は全て元値に戻す安全なものだけ。

use devhub_core::backend::DeviceDriver;
use devhub_core::model::*;
use devhub_hid::lamzu;
use devhub_hid::transport;

fn main() {
    let api = transport::open().expect("hidapi");
    // 有線 0044 優先、無ければ Orbital 8K ドングル 0048 (同じ feature 経路)。
    let info_raw = api
        .device_list()
        .find(|d| {
            d.vendor_id() == 0x37b0
                && (d.product_id() == 0x0044 || d.product_id() == 0x0048)
                && d.usage_page() == 0xffff
                && d.usage() == 0
        })
        .expect("LAMZU FFFF:00 collection");
    let dev = info_raw.open_device(&api).expect("open");
    let info = DeviceInfo {
        id: DeviceId("lamzu-lm20".into()),
        kind: DeviceKind::Mouse,
        name: "LAMZU?".into(),
        vendor: "LAMZU".into(),
        model: "maya-x-v2".into(),
        vid: 0x37b0,
        pid: info_raw.product_id(),
        transport: if info_raw.product_id() == 0x0048 {
            Transport::Wireless24G
        } else {
            Transport::Wired
        },
        firmware: None,
        dongle_firmware: None,
        serial: None,
        driver: DriverKind::Lamzu,
        supported: false,
        asleep: false,
        battery: None,
        board_id: None,
    };
    let d = lamzu::open(dev, info, None).expect("driver");
    let i = d.info();
    println!(
        "== info: fw={:?} serial={:?} bat={:?}",
        i.firmware, i.serial, i.battery
    );

    let c = d.caps();
    println!(
        "== caps: settings={} buttons={}",
        c.settings.len(),
        c.mouse.as_ref().map(|m| m.buttons.len()).unwrap_or(0)
    );

    let s = d.read().expect("read");
    let ms = s.mouse.as_ref().unwrap();
    println!("== profile: active={}", s.profile.active);
    println!("== stages: active={}", ms.active_stage);
    for (k, st) in ms.stages.iter().enumerate() {
        println!("   {k}: {}x{:?} rgb={:?}", st.x, st.y, st.color);
    }
    println!("== buttons:");
    for (k, b) in ms.buttons.iter().enumerate() {
        println!("   {k}: {b:?}");
    }
    println!("== settings:");
    for (k, v) in &s.settings {
        println!("   {k} = {v:?}");
    }

    // トグル往復: motion_sync を反転 → 読み戻し → 復元。
    for id in ["motion_sync"] {
        let cur = matches!(s.settings.get(id), Some(SettingValue::Bool(true)));
        println!("== apply {id} -> {}", !cur);
        d.apply(&Change::Setting {
            id: id.into(),
            value: SettingValue::Bool(!cur),
        })
        .expect("apply");
        let s2 = d.read().expect("read2");
        let now = matches!(s2.settings.get(id), Some(SettingValue::Bool(true)));
        println!("   readback: {now} (expect {})", !cur);
        assert_eq!(now, !cur, "{id} roundtrip failed");
        d.apply(&Change::Setting {
            id: id.into(),
            value: SettingValue::Bool(cur),
        })
        .expect("restore");
    }

    // ポーリング: 1000Hz → 読み戻し → 復元。
    if let Some(SettingValue::Choice(cur)) = s.settings.get("polling_wireless") {
        let cur = *cur;
        println!("== apply polling {cur} -> 1000");
        d.apply(&Change::Setting {
            id: "polling_wireless".into(),
            value: SettingValue::Choice(1000),
        })
        .expect("apply");
        let s2 = d.read().expect("read2");
        let now = match s2.settings.get("polling_wireless") {
            Some(SettingValue::Choice(v)) => *v,
            _ => -1,
        };
        println!("   readback: {now}");
        assert_eq!(now, 1000);
        d.apply(&Change::Setting {
            id: "polling_wireless".into(),
            value: SettingValue::Choice(cur),
        })
        .expect("restore");
    }

    // LOD: 1.0mm (Choice 10) → 読み戻し → 復元。
    if let Some(SettingValue::Choice(cur)) = s.settings.get("lod") {
        let cur = *cur;
        println!("== apply lod {cur} -> 10 (1.0mm)");
        d.apply(&Change::Setting {
            id: "lod".into(),
            value: SettingValue::Choice(10),
        })
        .expect("apply");
        let s2 = d.read().expect("read2");
        let now = match s2.settings.get("lod") {
            Some(SettingValue::Choice(v)) => *v,
            _ => -1,
        };
        println!("   readback: {now}");
        assert_eq!(now, 10);
        d.apply(&Change::Setting {
            id: "lod".into(),
            value: SettingValue::Choice(cur),
        })
        .expect("restore");
    }

    // デバウンス: ±1 → 読み戻し → 復元。
    if let Some(SettingValue::Num(cur)) = s.settings.get("debounce") {
        let t = if *cur < 20.0 { *cur + 1.0 } else { *cur - 1.0 };
        println!("== apply debounce {cur} -> {t}");
        d.apply(&Change::Setting {
            id: "debounce".into(),
            value: SettingValue::Num(t),
        })
        .expect("apply");
        let s2 = d.read().expect("read2");
        let now = match s2.settings.get("debounce") {
            Some(SettingValue::Num(v)) => *v,
            _ => -1.0,
        };
        println!("   readback: {now}");
        assert_eq!(now, t);
        d.apply(&Change::Setting {
            id: "debounce".into(),
            value: SettingValue::Num(*cur),
        })
        .expect("restore");
    }

    // アクティブステージ: 0 → 読み戻し → 復元。
    let orig_stage = ms.active_stage;
    let target = if orig_stage == 0 { 1 } else { 0 };
    println!("== apply active_stage {orig_stage} -> {target}");
    d.apply(&Change::ActiveStage(target)).expect("apply");
    let s2 = d.read().expect("read2");
    assert_eq!(s2.mouse.as_ref().unwrap().active_stage, target);
    println!("   readback: {target}");
    d.apply(&Change::ActiveStage(orig_stage)).expect("restore");

    // サイドボタン (slot3): Key('a' usage=4) → 読み戻し → 元に戻す。
    if let Some(orig) = ms.buttons.get(3).cloned() {
        let a = ButtonAction::Key(KeyStroke { mods: 0, usage: 4 });
        println!("== apply button3 -> {a:?}");
        d.apply(&Change::Button { slot: 3, action: a })
            .expect("apply");
        let s2 = d.read().expect("read2");
        println!("   readback: {:?}", s2.mouse.as_ref().unwrap().buttons[3]);
        d.apply(&Change::Button {
            slot: 3,
            action: orig,
        })
        .expect("restore");
    }
    println!("OK");
}
