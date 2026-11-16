//! SU66 ライティング検証プローブ第2弾:
//!  A) mode0 が「選択パレットスロットの静的表示」か確定 (slot0=虹/他=固色)
//!  B) effect 20-23 の受理可否 (書き込み→読み戻し)
//!  C) modes 16-19 がキー反応型か (ユーザーに打鍵してもらい撮影)
//! 終了時に元の LightingState (パレット含む) を復元する。
use devhub_core::{Change, DeviceKind, LightingState, Rgb};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

fn apply(d: &std::sync::Arc<dyn devhub_core::DeviceDriver>, s: &LightingState) {
    match d.apply(&Change::Lighting {
        area: 0,
        state: s.clone(),
    }) {
        Ok(()) => {}
        Err(e) => println!("  apply ERR {e}"),
    }
}

fn step(
    d: &std::sync::Arc<dyn devhub_core::DeviceDriver>,
    label: &str,
    s: &LightingState,
    dwell_ms: u64,
) {
    apply(d, s);
    println!("STEP {label} {}", ms());
    std::thread::sleep(Duration::from_millis(dwell_ms));
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
        "SNAPSHOT {} effect={} sel={} palette={:?}",
        ms(),
        base.effect,
        base.palette_sel,
        base.palette
    );

    let mut s = base.clone();
    s.on = true;
    s.zone_on = vec![true, true];
    s.brightness = 80;
    s.speed = 80;
    s.direction = 0;
    s.color_correction = None;

    // ---- A. mode0 = 静的パレット表示か ----
    // 8スロットを判別色に書き換え (終了時に復元)
    let distinct = [
        rgb(255, 0, 0),     // 0: 赤   (公式UIでは「more/虹」スロット)
        rgb(255, 128, 0),   // 1: 橙
        rgb(255, 255, 0),   // 2: 黄
        rgb(0, 255, 0),     // 3: 緑
        rgb(0, 255, 255),   // 4: 水色
        rgb(0, 0, 255),     // 5: 青
        rgb(160, 0, 255),   // 6: 紫
        rgb(255, 255, 255), // 7: 白
    ];
    s.effect = 0;
    s.palette = distinct.to_vec();
    for (sel, dwell) in [
        (0usize, 8000u64),
        (1, 4000),
        (3, 4000),
        (5, 4000),
        (7, 4000),
    ] {
        let mut p = s.clone();
        p.palette_sel = sel as u8;
        p.color = p.palette[sel];
        step(&d, &format!("m0_sel{sel}"), &p, dwell);
    }
    // 静的モードの輝度応答 (sel=1 橙)
    for bri in [20u8, 100] {
        let mut p = s.clone();
        p.palette_sel = 1;
        p.color = p.palette[1];
        p.brightness = bri;
        step(&d, &format!("m0_bri{bri}"), &p, 4000);
    }
    s.brightness = 80;

    // ---- B. effect 20-23 受理可否 ----
    s.palette_sel = 1;
    s.color = s.palette[1];
    for m in 20u8..=23 {
        let mut p = s.clone();
        p.effect = m;
        apply(&d, &p);
        let rb = d
            .read()
            .ok()
            .and_then(|st| st.lighting.get(&0u8).map(|l| l.effect));
        println!("STEP mode{m} written rb={rb:?} {}", ms());
        std::thread::sleep(Duration::from_millis(4000));
    }

    // ---- C. modes 16-19 キー反応テスト ----
    println!("=== TYPE_PHASE_START {} ===", ms());
    println!("*** ここから80秒間、キーボードを連続でタイプしてください ***");
    for m in [16u8, 17, 18, 19] {
        let mut p = s.clone();
        p.effect = m;
        apply(&d, &p);
        println!("REACT mode{m} {}", ms());
        std::thread::sleep(Duration::from_millis(20000));
    }
    println!("=== TYPE_PHASE_END {} ===", ms());

    println!("RESTORE {}", ms());
    apply(&d, &base);
    println!("DONE {}", ms());
}
