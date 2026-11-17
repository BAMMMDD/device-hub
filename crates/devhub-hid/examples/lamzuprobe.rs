//! motionsync=0 に戻す + 残設定の最終確認。

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
    std::thread::sleep(Duration::from_millis(140));
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

    println!("motionsync set 0: {:02x?}", feat(&d, 2, 2, 1, 9, &[1, 0]).map(|v| v[..12].to_vec()));
    println!("speeden set 1   : {:02x?}", feat(&d, 2, 3, 0, 26, &[1, 1]).map(|v| v[..12].to_vec()));
    std::thread::sleep(Duration::from_secs(2));

    println!("== final state (p1) ==");
    for (tag, b2, sub, f, cmd, args) in [
        ("poll", 2u8, 2, 1, 128, &[1u8][..]),
        ("lod", 2, 2, 1, 136, &[1]),
        ("activedpi", 2, 2, 1, 130, &[1]),
        ("motionsync", 2, 2, 1, 137, &[1]),
        ("anglesnap", 2, 2, 1, 132, &[1]),
        ("ripple", 2, 2, 1, 138, &[1]),
        ("hyper", 2, 2, 1, 139, &[1]),
        ("track", 2, 2, 1, 147, &[1]),
        ("dpixy", 2, 2, 1, 141, &[1]),
        ("angletune", 2, 2, 1, 148, &[1]),
        ("debounce", 2, 2, 0, 136, &[1]),
        ("sleep", 2, 3, 0, 135, &[1]),
        ("btncomb", 2, 3, 3, 129, &[1]),
        ("sensoroff", 2, 3, 1, 149, &[1]),
        ("speeden", 2, 3, 0, 154, &[1]),
        ("scrollhp", 2, 4, 0, 153, &[1]),
        ("glass", 2, 1, 1, 142, &[0]),
        ("battery", 2, 2, 0, 131, &[1]),
        ("profile", 2, 1, 0, 133, &[1]),
    ] {
        println!("  {tag:10}: {:02x?}", feat(&d, b2, sub, f, cmd, args).map(|v| v[6..14].to_vec()));
    }
    println!("  stages    : {:02x?}", feat(&d, 2, 10, 1, 129, &[1, 1]).map(|v| v[6..26].to_vec()));
    println!("-- done --");
}
