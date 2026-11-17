//! プロファイル切替往復: p1→p2→p1 + active profile get + 値確認。

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
    d.send_feature_report(&w).ok()?;
    std::thread::sleep(Duration::from_millis(150));
    let mut r = [0u8; 65];
    match d.get_feature_report(&mut r) {
        Ok(n) => Some(r[..n.min(24)].to_vec()),
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

    println!("active: {:02x?}", feat(&d, 2, 1, 0, 133, &[1]).map(|v| v[6..10].to_vec()));
    println!("-> set profile 2:");
    println!("  {:02x?}", feat(&d, 2, 1, 0, 5, &[2]).map(|v| v[..14].to_vec()));
    std::thread::sleep(Duration::from_millis(400));
    println!("active: {:02x?}", feat(&d, 2, 1, 0, 133, &[1]).map(|v| v[6..10].to_vec()));
    println!("-> set profile 1:");
    println!("  {:02x?}", feat(&d, 2, 1, 0, 5, &[1]).map(|v| v[..14].to_vec()));
    std::thread::sleep(Duration::from_millis(400));
    println!("active: {:02x?}", feat(&d, 2, 1, 0, 133, &[1]).map(|v| v[6..10].to_vec()));
    println!("p1 rate after roundtrip: {:02x?}", feat(&d, 2, 2, 1, 128, &[1]).map(|v| v[8]));
    println!("-- done --");
}
