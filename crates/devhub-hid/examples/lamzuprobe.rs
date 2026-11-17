//! AdaptiveDPI no-op 書き込み検証 (get の同値を set に流す)。

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
    std::thread::sleep(Duration::from_millis(120));
    let mut r = [0u8; 65];
    match d.get_feature_report(&mut r) {
        Ok(n) => Some(r[..n.min(44)].to_vec()),
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

    let g = feat(&d, 2, 9, 1, 154, &[1]);
    println!("get: {:02x?}", g);
    if let Some(v) = &g {
        let enable = v[8];
        let count = v[12] as usize;
        // data = a[13..13+count*4] (u16BE ×2 per point, count ポイント)
        let mut args = vec![1u8, enable, 0, count as u8];
        args.extend_from_slice(&v[13..13 + count * 4]);
        println!("set same (enable={enable} count={count}): {:02x?}",
            feat(&d, 2, (4 + count * 4) as u8, 1, 26, &args));
        std::thread::sleep(Duration::from_millis(300));
        println!("get again: {:02x?}", feat(&d, 2, 9, 1, 154, &[1]));
    }
    println!("-- done --");
}
