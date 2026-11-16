//! 各モードが選択パレット色を使うか検証: palette[1]=赤, sel=1 で各モードを4秒表示。
//! 赤に染まるモード = .color() フラグあり。終了時に復元。
use devhub_core::{Change, DeviceKind, Rgb};
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

    let mut s = base.clone();
    s.on = true;
    s.zone_on = vec![true, true];
    s.brightness = 80;
    s.speed = 80;
    s.direction = 0;
    s.palette_sel = 1;
    s.palette[1] = Rgb { r: 255, g: 0, b: 0 };
    s.color = s.palette[1];

    for m in 1u8..=15 {
        let mut p = s.clone();
        p.effect = m;
        let _ = d.apply(&Change::Lighting { area: 0, state: p });
        println!("C mode{m} {}", ms());
        std::thread::sleep(Duration::from_millis(4000));
    }
    println!("RESTORE {}", ms());
    let _ = d.apply(&Change::Lighting {
        area: 0,
        state: base,
    });
    println!("DONE {}", ms());
}
