//! SU66 ライティング・パラメータ掃引 — 実機に個別パラメータ変更を順に書き込み、
//! 各ステップの開始時刻をミリ秒で標準出力する。連続撮影したカメラ映像と
//! タイムスタンプで対応付けて、ゾーン/パレット/輝度/方向/速度/色補正の
//! 実機挙動を分類する。終了時に元の LightingState を復元する。
use devhub_core::{Change, DeviceKind, LightingState};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

const STEP_MS: u64 = 4000;

fn step(d: &std::sync::Arc<dyn devhub_core::DeviceDriver>, label: &str, s: &LightingState) {
    let t = ms();
    match d.apply(&Change::Lighting {
        area: 0,
        state: s.clone(),
    }) {
        Ok(()) => println!("STEP {label} OK {t}"),
        Err(e) => println!("STEP {label} ERR {t} {e}"),
    }
    std::thread::sleep(Duration::from_millis(STEP_MS));
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
        "SNAPSHOT {} effect={} bri={} spd={} dir={} sel={} zone={:?} cc={:?} palette={:?}",
        ms(),
        base.effect,
        base.brightness,
        base.speed,
        base.direction,
        base.palette_sel,
        base.zone_on,
        base.color_correction,
        base.palette
    );

    let mut s = base.clone();
    s.on = true;
    s.zone_on = vec![true, true];
    // 各スロット本来の色を palette[sel] に合わせる (F4: sel 変更で slot が上書きされるのを回避)
    let keep_slot = |s: &mut LightingState| {
        let sel = s.palette_sel as usize;
        if let Some(c) = s.palette.get(sel).cloned() {
            s.color = c;
        }
    };

    // ---- A. ゾーン検証 (effect=8 均一系, sel 維持) ----
    s.effect = 8;
    s.brightness = 80;
    s.speed = 80;
    s.direction = 0;
    keep_slot(&mut s);
    for (label, zones) in [
        ("zone_none", vec![false, false]),
        ("zone_lower", vec![false, true]),
        ("zone_upper", vec![true, false]),
        ("zone_both", vec![true, true]),
    ] {
        let mut z = s.clone();
        z.zone_on = zones;
        step(&d, label, &z);
    }
    s.zone_on = vec![true, true];

    // ---- B. パレット sel 0..7 (effect=8) ----
    for sel in 0..8u8 {
        let mut p = s.clone();
        p.palette_sel = sel;
        keep_slot(&mut p);
        step(&d, &format!("pal{sel}"), &p);
    }
    s.palette_sel = 0;
    keep_slot(&mut s);

    // ---- C. effect 0 の色応答 (sel 0 vs sel 5) ----
    for sel in [0u8, 5] {
        let mut p = s.clone();
        p.effect = 0;
        p.palette_sel = sel;
        keep_slot(&mut p);
        step(&d, &format!("m0pal{sel}"), &p);
    }

    // ---- D. 輝度掃引 (effect=8) ----
    for bri in [0u8, 20, 40, 60, 100] {
        let mut p = s.clone();
        p.brightness = bri;
        step(&d, &format!("bri{bri}"), &p);
    }
    s.brightness = 80;

    // ---- E. 方向 (effect=11 sine) ----
    for dir in [0u8, 1] {
        let mut p = s.clone();
        p.effect = 11;
        p.direction = dir;
        step(&d, &format!("dir{dir}"), &p);
    }
    s.direction = 0;

    // ---- F. 速度 (effect=10 shift) ----
    for spd in [5u8, 50, 100] {
        let mut p = s.clone();
        p.effect = 10;
        p.speed = spd;
        step(&d, &format!("spd{spd}"), &p);
    }
    s.speed = 80;

    // ---- G. 色補正 (effect=8) ----
    for (label, cc) in [
        ("cc_red", [100u8, 0, 0]),
        ("cc_grn", [0, 100, 0]),
        ("cc_blu", [0, 0, 100]),
        ("cc_off", [0, 0, 0]),
    ] {
        let mut p = s.clone();
        p.effect = 8;
        p.color_correction = Some(devhub_core::Rgb {
            r: cc[0],
            g: cc[1],
            b: cc[2],
        });
        step(&d, label, &p);
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
