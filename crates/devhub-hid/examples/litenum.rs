//! SU66 ライティングモード掃引 — 実機に effect 0..=19 を順に書き込み、
//! 各モードの開始時刻をミリ秒で標準出力する。連続撮影したカメラ映像と
//! タイムスタンプで対応付けて、実機の実際の光り方を分類する。
//! 終了時に元の LightingState を復元する。
use devhub_core::{Change, DeviceKind};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn main() {
    let hub = match devhub_hid::connect() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("connect: {e}");
            return;
        }
    };
    let info = hub
        .scan()
        .unwrap_or_default()
        .into_iter()
        .find(|d| d.kind == DeviceKind::Keyboard && d.supported)
        .expect("SU66");
    let d = hub.driver(&info.id).expect("driver");
    let st0 = d.read().expect("initial read");
    let base = st0.lighting.get(&0u8).cloned().expect("lighting area0");
    println!(
        "SNAPSHOT {} on={} effect={} brightness={} speed={} dir={} sel={} zone={:?} perkey={}",
        ms(),
        base.on,
        base.effect,
        base.brightness,
        base.speed,
        base.direction,
        base.palette_sel,
        base.zone_on,
        base.per_key.len()
    );
    for m in 0..=19u8 {
        let mut s = base.clone();
        s.effect = m;
        s.on = true;
        if !s.zone_on.is_empty() {
            s.zone_on = vec![true, true];
        }
        let t = ms();
        match d.apply(&Change::Lighting { area: 0, state: s }) {
            Ok(()) => println!("MODE {m} OK {t}"),
            Err(e) => println!("MODE {m} ERR {t} {e}"),
        }
        std::thread::sleep(Duration::from_millis(3500));
    }
    println!("RESTORE {}", ms());
    if let Err(e) = d.apply(&Change::Lighting {
        area: 0,
        state: base,
    }) {
        println!("RESTORE ERR {e}");
    }
    println!("DONE {}", ms());
}
