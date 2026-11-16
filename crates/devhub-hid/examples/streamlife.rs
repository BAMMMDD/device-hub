//! 軸ストリームの自己終了時間を測る — [4,3,1,1] を 1 発送って、
//! その後一切送らずフレームが止まるまで読み続ける。
//! 実行: cargo run -p devhub-hid --example streamlife

fn main() -> devhub_core::backend::HubResult<()> {
    let api = hidapi::HidApi::new().expect("hidapi init");
    let mut path = None;
    for d in api.device_list() {
        if d.vendor_id() == 0x1CA6 && d.product_id() == 0x3009 && d.usage_page() == 0xFFB0 {
            path = Some(d.path().to_owned());
        }
    }
    let Some(path) = path else {
        println!("SU66 not found");
        return Ok(());
    };
    let dev = api.open_path(&path).expect("open");
    dev.set_blocking_mode(false).ok();

    let mut buf = [0u8; 65];
    while dev.read_timeout(&mut buf, 0).unwrap_or(0) > 0 {}

    let mut req = [0u8; 65];
    req[1..5].copy_from_slice(&[4, 3, 1, 1]);
    dev.write(&req).expect("arm stream");
    let start = std::time::Instant::now();
    println!("armed at t=0 — reading until stream dies (max 150s)");

    let (mut n, mut last) = (0u64, start);
    while start.elapsed() < std::time::Duration::from_secs(150) {
        if dev.read_timeout(&mut buf, 20).unwrap_or(0) > 0 {
            n += 1;
            last = std::time::Instant::now();
        }
        // 直近 10 秒フレームが無ければ死んだとみなす。
        if last.elapsed() > std::time::Duration::from_secs(10) {
            break;
        }
    }
    println!(
        "total {n} frames; last frame at t={:.1}s — stream lifetime ≈ that value",
        last.duration_since(start).as_secs_f64()
    );
    Ok(())
}
