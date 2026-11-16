//! mode16 が「isCustom セルのみ表示モード」か検証:
//!  Q..P 行セルにカスタム色を書き → mode16 で表示されるか観察 → 復元。
use devhub_core::{Change, DeviceKind, Rgb};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn main() {
    let hub = devhub_hid::connect().expect("connect");
    let info = hub
        .scan()
        .unwrap_or_default()
        .into_iter()
        .find(|d| d.kind == DeviceKind::Keyboard && d.supported)
        .expect("SU66");
    let d = hub.driver(&info.id).expect("driver");
    let st0 = d.read().expect("read");
    let base = st0.lighting.get(&0u8).cloned().expect("lighting");

    // per_key: セル index → 色。上位行 (cell 0-20) を赤, 21-41 を緑に塗る
    let mut s = base.clone();
    s.on = true;
    s.zone_on = vec![true, true];
    s.brightness = 80;
    s.speed = 80;
    s.effect = 16;
    let mut pk = BTreeMap::new();
    for i in 0..21usize {
        pk.insert(i, Rgb { r: 255, g: 0, b: 0 });
    }
    for i in 21..42usize {
        pk.insert(i, Rgb { r: 0, g: 255, b: 0 });
    }
    s.per_key = pk;
    println!("SET mode16+perkey {}", ms());
    let _ = d.apply(&Change::Lighting { area: 0, state: s });
    std::thread::sleep(Duration::from_secs(30));

    // per_key 消して mode16 のまま観察 (暗転するか)
    let mut s2 = base.clone();
    s2.on = true;
    s2.zone_on = vec![true, true];
    s2.brightness = 80;
    s2.speed = 80;
    s2.effect = 16;
    println!("CLEAR perkey keep mode16 {}", ms());
    let _ = d.apply(&Change::Lighting { area: 0, state: s2 });
    std::thread::sleep(Duration::from_secs(10));

    println!("RESTORE {}", ms());
    let _ = d.apply(&Change::Lighting {
        area: 0,
        state: base,
    });
    println!("DONE {}", ms());
}
