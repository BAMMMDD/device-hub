//! LAMZU MayaX V2/LM20 (37b0:0044) — 最終検証: ボタン読み・DPI ステージ同値書き込み
//! ・MotionSync/activeDPI 往復。

use hidapi::HidApi;
use std::time::Duration;

fn feat(d: &hidapi::HidDevice, b2: u8, sub: u8, f: u8, cmd: u8, args: &[u8]) -> Option<Vec<u8>> {
    let mut w = [0u8; 65];
    w[3] = b2;
    w[4] = sub;
    w[5] = f;
    w[6] = cmd;
    for (i, &b) in args.iter().enumerate() {
        w[7 + i] = b;
    }
    if d.send_feature_report(&w).is_err() {
        return None;
    }
    std::thread::sleep(Duration::from_millis(120));
    let mut r = [0u8; 65];
    match d.get_feature_report(&mut r) {
        Ok(n) => Some(r[..n.min(40)].to_vec()),
        Err(_) => None,
    }
}

fn getbtn(d: &hidapi::HidDevice, dev: u8, btn: u8) -> Option<Vec<u8>> {
    let mut w = [0u8; 65];
    w[3] = 2;
    w[4] = 15; // sub = 5+len(10)
    w[5] = 3;
    w[6] = 128;
    w[7] = dev;
    w[8] = btn;
    w[9] = 0;
    w[10] = 255; // type = all
    w[11] = 10; // len
    if d.send_feature_report(&w).is_err() {
        return None;
    }
    std::thread::sleep(Duration::from_millis(120));
    let mut r = [0u8; 65];
    match d.get_feature_report(&mut r) {
        Ok(n) => Some(r[..n.min(40)].to_vec()),
        Err(_) => None,
    }
}

fn main() {
    let api = HidApi::new().unwrap();
    let info = api
        .device_list()
        .find(|d| {
            d.vendor_id() == 0x37b0
                && d.product_id() == 0x0044
                && d.interface_number() == 2
                && d.usage_page() == 0xffff
                && d.usage() == 0
        })
        .expect("ffff:00 if2");
    let d = info.open_device(&api).expect("open");
    d.set_blocking_mode(false).ok();
    let dev = 1u8;

    println!("== buttons 1..5 (type=255,len=10) ==");
    for b in 1..=5u8 {
        match getbtn(&d, dev, b) {
            Some(v) => println!("  btn{b}: {:02x?}", &v[..v.len().min(30)]),
            None => println!("  btn{b}: FAIL"),
        }
    }

    println!("== dpi stage write-same round-trip ==");
    // 現在: 5ステージ 400/800/1600/3200/6400 — 同値を sub26 で書き戻す
    let stages: [u8; 20] = [
        0x01, 0x90, 0x01, 0x90, // 400
        0x03, 0x20, 0x03, 0x20, // 800
        0x06, 0x40, 0x06, 0x40, // 1600
        0x0c, 0x80, 0x0c, 0x80, // 3200
        0x19, 0x00, 0x19, 0x00, // 6400
    ];
    let mut args = vec![dev, 5];
    args.extend_from_slice(&stages);
    match feat(&d, 2, 26, 1, 1, &args) {
        Some(v) => println!("  set stages: {:02x?}", &v[..v.len().min(20)]),
        None => println!("  set stages: FAIL"),
    }
    match feat(&d, 2, 10, 1, 129, &[dev, 1]) {
        Some(v) => println!("  get stages: {:02x?}", &v[..v.len().min(30)]),
        None => println!("  get stages: FAIL"),
    }

    println!("== activeDPI 3 -> 1 -> 3 ==");
    println!("  set1: {:02x?}", feat(&d, 2, 2, 1, 2, &[dev, 1]).map(|v| v[..12].to_vec()));
    println!("  get : {:02x?}", feat(&d, 2, 2, 1, 130, &[dev]).map(|v| v[..12].to_vec()));
    println!("  set3: {:02x?}", feat(&d, 2, 2, 1, 2, &[dev, 3]).map(|v| v[..12].to_vec()));
    println!("  get : {:02x?}", feat(&d, 2, 2, 1, 130, &[dev]).map(|v| v[..12].to_vec()));

    println!("== motionsync 0 -> 1 -> 0 ==");
    println!("  set1: {:02x?}", feat(&d, 2, 2, 1, 9, &[dev, 1]).map(|v| v[..12].to_vec()));
    println!("  get : {:02x?}", feat(&d, 2, 2, 1, 137, &[dev]).map(|v| v[..12].to_vec()));
    println!("  set0: {:02x?}", feat(&d, 2, 2, 1, 9, &[dev, 0]).map(|v| v[..12].to_vec()));
    println!("  get : {:02x?}", feat(&d, 2, 2, 1, 137, &[dev]).map(|v| v[..12].to_vec()));
    println!("-- done --");
}
