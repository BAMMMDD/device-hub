//! DD ピン留め解除: 全セルの isCustom を 0 → [5,2] 再送 (修正後 stop と同じ手順)
fn main() {
    let api = hidapi::HidApi::new().unwrap();
    let dev = api
        .device_list()
        .find(|d| d.vendor_id() == 0x1ca6 && d.usage_page() == 0xFFB0)
        .or_else(|| {
            api.device_list()
                .find(|d| d.vendor_id() == 0x1ca6 && d.usage_page() & 0xff00 == 0xff00)
        })
        .expect("SU66 — デバイスハブを閉じて");
    let d = dev.open_device(&api).unwrap();
    d.set_blocking_mode(false).unwrap();
    let mut drain = [0u8; 65];
    while d.read_timeout(&mut drain, 20).unwrap_or(0) > 0 {}
    let cmd = |grp: u8, sub: u8, args: &[u8]| -> Option<Vec<u8>> {
        let mut drain = [0u8; 65];
        while d.read_timeout(&mut drain, 10).unwrap_or(0) > 0 {}
        let mut w = [0u8; 65];
        w[1] = grp;
        w[2] = sub;
        w[3..3 + args.len()].copy_from_slice(args);
        d.write(&w).ok()?;
        let mut r = [0u8; 65];
        let n = d.read_timeout(&mut r, 400).unwrap_or(0);
        if n == 0 {
            None
        } else {
            Some(r[..n].to_vec())
        }
    };
    // 全セル isCustom=0 (色バイトは読んだ現値を保持)
    for c in 0..9u8 {
        if let Some(rb) = cmd(5, 3, &[0, c]) {
            let mut w = [0u8; 65];
            w[1] = 5;
            w[2] = 4;
            w[3] = 0;
            w[4] = c;
            w[5..65].copy_from_slice(&rb[4..64]);
            for i in 0..15 {
                w[8 + i * 4] = 0;
            }
            let _ = d.write(&w);
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }
    // [5,1] → [5,2] 再送でファーム駆動復帰
    if let Some(r) = cmd(5, 1, &[0, 0]) {
        let mut args = vec![0u8];
        args.extend_from_slice(&r[4..10]);
        let _ = cmd(5, 2, &args);
    }
    println!("ピン解除 + ファーム復帰完了 — ' がアニメーションに戻ったか確認してください");
}
