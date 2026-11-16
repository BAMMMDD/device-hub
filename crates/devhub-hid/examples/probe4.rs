//! DirectDrive 検証: [5,5,area,chunk, ...15cell×4B(B,G,R,flag)] を連続送信。
//! 応答不要 (SDK: sendDataNoResponse = sendReport のみ)。
//! 上半分=赤, 下半分=青 を流して実機 LED が従うか見る (ユーザ目視確認用)。

fn open(api: &hidapi::HidApi) -> Option<hidapi::HidDevice> {
    let dev = api
        .device_list()
        .find(|d| d.vendor_id() == 0x1ca6 && d.usage_page() == 0xFFB0)
        .or_else(|| {
            api.device_list()
                .find(|d| d.vendor_id() == 0x1ca6 && d.usage_page() & 0xff00 == 0xff00)
        })?;
    let d = dev.open_device(api).ok()?;
    d.set_blocking_mode(false).ok();
    Some(d)
}

fn main() {
    let api = hidapi::HidApi::new().unwrap();
    let Some(d) = open(&api) else {
        println!("no device");
        return;
    };
    // 135 セル: 前半 68 セルを赤、後半を青 (B,G,R,flag)
    let mut cells = Vec::new();
    for i in 0..135u8 {
        let (b, g, r) = if i < 68 {
            (0u8, 0u8, 255u8)
        } else {
            (255, 0, 0)
        };
        cells.extend_from_slice(&[b, g, r, 0]);
    }
    let area = 0u8;
    let mut ok = true;
    for (chunk, part) in cells.chunks(60).enumerate() {
        let mut w = [0u8; 65];
        w[1] = 5;
        w[2] = 5; // DirectDrive
        w[3] = area;
        w[4] = chunk as u8;
        w[5..5 + part.len()].copy_from_slice(part);
        if let Err(e) = d.write(&w) {
            println!("chunk {chunk}: send err {e}");
            ok = false;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    println!("direct drive frames sent: {ok}");
    // 5秒後に通常書き込みでファーム側モードに戻す (点灯状態の復帰確認用に読みだけ)
    std::thread::sleep(std::time::Duration::from_secs(3));
    let mut r = [0u8; 65];
    let n = d.read_timeout(&mut r, 200).unwrap_or(0);
    println!("post-read: {n} bytes {:?}", &r[..n.min(8)]);
}
