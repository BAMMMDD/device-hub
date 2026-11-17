//! ライト lightness no-op 書き込み検証 + マクロ read の安全確認。

use hidapi::HidApi;
use std::time::Duration;

fn raw65(d: &hidapi::HidDevice, body: &[u8]) -> Option<Vec<u8>> {
    let mut w = [0u8; 65];
    for (i, &b) in body.iter().enumerate() {
        w[1 + i] = b;
    }
    d.send_feature_report(&w).ok()?;
    std::thread::sleep(Duration::from_millis(120));
    let mut r = [0u8; 65];
    match d.get_feature_report(&mut r) {
        Ok(n) => Some(r[..n.min(32)].to_vec()),
        Err(_) => None,
    }
}

fn light(d: &hidapi::HidDevice, dev: u8, sub: u8, cmd: u8, args: &[u8]) -> Option<Vec<u8>> {
    // 実機確認済みレイアウト: [0, dev, 0, sub, 2, cmd, args...]
    let mut body = vec![0u8, dev, 0, sub, 2, cmd];
    body.extend_from_slice(args);
    raw65(d, &body)
}

fn feat(d: &hidapi::HidDevice, b2: u8, sub: u8, f: u8, cmd: u8, args: &[u8]) -> Option<Vec<u8>> {
    let mut body = vec![0u8, 0, b2, sub, f, cmd];
    body.extend_from_slice(args);
    raw65(d, &body)
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

    // lightness get → 同値 set (no-op)
    let g = light(&d, 1, 3, 130, &[1, 0]);
    println!("lightness get r1: {:02x?}", g);
    // JS: SetLightness [0,dev,3,2,2,r,n,o]; get r,n = a[8..10]
    let (r, n, o) = g.as_ref().map(|v| (v[8], v[9], v[10])).unwrap_or((0, 0xa9, 0));
    println!("lightness set same (r={r},n={n},o={o}): {:02x?}", light(&d, 1, 3, 2, &[r, n, o]));
    println!("lightness get again: {:02x?}", light(&d, 1, 3, 130, &[1, 0]));

    // マクロ read のみ (安全) — 未割当スロットの応答確認
    for slot in [0u8, 1, 2] {
        println!("macrosize s{slot}: {:02x?}", feat(&d, 2, 6, 4, 129, &[1, 0, slot]));
    }
    println!("-- done --");
}
