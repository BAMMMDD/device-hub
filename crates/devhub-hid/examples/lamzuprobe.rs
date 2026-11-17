//! ボタン read 最終バリアント + p2/p3 元値リストア。

use hidapi::HidApi;
use std::time::Duration;

fn raw(d: &hidapi::HidDevice, req: &[u8]) -> Option<Vec<u8>> {
    let mut w = [0u8; 65];
    for (i, &b) in req.iter().enumerate() {
        w[1 + i] = b;
    }
    d.send_feature_report(&w).ok()?;
    std::thread::sleep(Duration::from_millis(110));
    let mut r = [0u8; 65];
    match d.get_feature_report(&mut r) {
        Ok(n) => Some(r[..n.min(32)].to_vec()),
        Err(_) => None,
    }
}

fn feat(d: &hidapi::HidDevice, b2: u8, sub: u8, f: u8, cmd: u8, args: &[u8]) -> Option<Vec<u8>> {
    let mut w = [0u8; 65];
    w[3] = b2;
    w[4] = sub;
    w[5] = f;
    w[6] = cmd;
    for (i, &b) in args.iter().enumerate() {
        w[7 + i] = b;
    }
    d.send_feature_report(&w).ok()?;
    std::thread::sleep(Duration::from_millis(110));
    let mut r = [0u8; 65];
    match d.get_feature_report(&mut r) {
        Ok(n) => Some(r[..n.min(32)].to_vec()),
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

    println!("== btn read variants (btn1, devID1) ==");
    // [0,0,2,sub,f,128,dev,btn,d8,type,len]
    for (tag, req) in [
        ("type=0", &[0, 0, 2, 15, 3, 128, 1, 1, 0, 0, 10][..]),
        ("d8=1", &[0, 0, 2, 15, 3, 128, 1, 1, 1, 255, 10][..]),
        ("d8=btn", &[0, 0, 2, 15, 3, 128, 1, 1, 1, 1, 1][..]),
        ("len=0", &[0, 0, 2, 5, 3, 128, 1, 1, 0, 255, 0][..]),
        ("len=1 t1", &[0, 0, 2, 6, 3, 128, 1, 1, 0, 1, 1][..]),
        ("sub=15 no9", &[0, 0, 2, 15, 3, 128, 1, 1, 1, 255, 10][..]),
        ("cmd128 f=4 b2", &[0, 0, 2, 15, 4, 128, 1, 2, 0, 255, 10][..]),
        ("cmd128 f=4 b3", &[0, 0, 2, 15, 4, 128, 1, 3, 0, 255, 10][..]),
        ("cmd128 f=4 b5", &[0, 0, 2, 15, 4, 128, 1, 5, 0, 255, 10][..]),
        ("cmd128 f=5 b1", &[0, 0, 2, 15, 5, 128, 1, 1, 0, 255, 10][..]),
    ] {
        println!("  {tag:12}: {:02x?}", raw(&d, req).map(|v| v[..28].to_vec()));
    }

    println!("== restore p2/p3 (rate=1,stage=2) ==");
    for p in [2u8, 3] {
        println!("  p{p} rate1: {:02x?}", feat(&d, 2, 2, 1, 0, &[p, 1]).map(|v| v[..12].to_vec()));
        println!("  p{p} stage2: {:02x?}", feat(&d, 2, 2, 1, 2, &[p, 2]).map(|v| v[..12].to_vec()));
        // p2/p3 ステージも p1 と同じ5個に戻す (壊れた stage count=2 を修復)
        let mut a = vec![p, 5u8];
        a.extend_from_slice(&[
            0x01, 0x90, 0x01, 0x90, 0x03, 0x20, 0x03, 0x20, 0x06, 0x40, 0x06, 0x40, 0x0c, 0x80,
            0x0c, 0x80, 0x19, 0x00, 0x19, 0x00,
        ]);
        println!("  p{p} stages: {:02x?}", feat(&d, 2, 26, 1, 1, &a).map(|v| v[..14].to_vec()));
    }
    println!("== verify ==");
    for p in 1..=4u8 {
        println!(
            "  p{p}: rate={:02x?} stage={:02x?} cnt={:02x?}",
            feat(&d, 2, 2, 1, 128, &[p]).map(|v| v[8]),
            feat(&d, 2, 2, 1, 130, &[p]).map(|v| v[8]),
            feat(&d, 2, 10, 1, 129, &[p, 1]).map(|v| v[8])
        );
    }
    println!("-- done --");
}
