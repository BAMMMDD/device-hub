//! 通知フレーム [4,type] 探索: 全 if の input report を監視しながら
//! プロファイル切替/DPI 切替相当のイベントを発生させる。

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
    std::thread::sleep(Duration::from_millis(100));
    let mut r = [0u8; 65];
    let _ = d.get_feature_report(&mut r);
    Some(r[..24].to_vec())
}

fn main() {
    let api = HidApi::new().unwrap();
    let mut listeners = Vec::new();
    let mut feat_dev = None;
    for info in api.device_list() {
        if info.vendor_id() != 0x37b0 || info.product_id() != 0x0044 {
            continue;
        }
        let tag = format!(
            "if{} up={:04x} u={:02x}",
            info.interface_number(),
            info.usage_page(),
            info.usage()
        );
        match info.open_device(&api) {
            Ok(d) => {
                d.set_blocking_mode(false).ok();
                if info.interface_number() == 2 && info.usage_page() == 0xffff {
                    feat_dev = Some(d);
                    println!("feature dev: {tag}");
                } else {
                    println!("listener: {tag}");
                    listeners.push((tag, d));
                }
            }
            Err(e) => println!("open fail {tag}: {e}"),
        }
    }
    let feat_dev = feat_dev.expect("feature dev");

    // 全リスナーで3秒 listen (切替前ベースライン)
    println!("== baseline listen 2s ==");
    let mut b = [0u8; 128];
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        for (tag, d) in &listeners {
            if let Ok(n) = d.read_timeout(&mut b, 10) {
                if n > 0 {
                    println!("  [{tag}] {:02x?}", &b[..n.min(32)]);
                }
            }
        }
    }

    // プロファイル切替 (デバイス内イベント) → 通知が出るか
    println!("== send profile set 2 ==");
    feat(&feat_dev, 2, 1, 0, 5, &[2]);
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        for (tag, d) in &listeners {
            if let Ok(n) = d.read_timeout(&mut b, 10) {
                if n > 0 {
                    println!("  [{tag}] {:02x?}", &b[..n.min(32)]);
                }
            }
        }
        // feature if2 自身の input report も見る
        if let Ok(n) = feat_dev.read_timeout(&mut b, 1) {
            if n > 0 {
                println!("  [if2 feat] {:02x?}", &b[..n.min(32)]);
            }
        }
    }
    println!("== send profile set 1 (restore) ==");
    feat(&feat_dev, 2, 1, 0, 5, &[1]);
    println!("-- done --");
}
