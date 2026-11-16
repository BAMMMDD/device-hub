//! 解析プローブ #10: ↑ と Enter の正しい LED セル index を実機で特定する。
//! 行3 (Caps〜Enter 段) と行4 (Shift〜↑ 段) の候補セルを虹色グラデで点灯。
//! セル index → 色相 が一対一なので、Enter/↑ の上の色から index が分かる。

fn open(api: &hidapi::HidApi) -> Option<hidapi::HidDevice> {
    let dev = api
        .device_list()
        .find(|d| d.vendor_id() == 0x1ca6 && d.usage_page() == 0xFFB0)
        .or_else(|| {
            api.device_list()
                .find(|d| d.vendor_id() == 0x1ca6 && d.usage_page() & 0xff00 == 0xff00)
        })?;
    let d = dev.open_device(api).ok()?;
    d.set_blocking_mode(false).ok()?;
    let mut drain = [0u8; 65];
    while d.read_timeout(&mut drain, 20).unwrap_or(0) > 0 {}
    Some(d)
}

fn send_frame(d: &hidapi::HidDevice, cells: &[[u8; 4]]) -> bool {
    for (chunk, part) in cells.chunks(15).enumerate() {
        let mut w = [0u8; 65];
        w[1] = 5;
        w[2] = 5;
        w[3] = 0;
        w[4] = chunk as u8;
        for (i, c) in part.iter().enumerate() {
            w[5 + i * 4..9 + i * 4].copy_from_slice(c);
        }
        if d.write(&w).is_err() {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    true
}

fn cmd(d: &hidapi::HidDevice, grp: u8, sub: u8, args: &[u8]) -> Option<Vec<u8>> {
    let mut drain = [0u8; 65];
    while d.read_timeout(&mut drain, 10).unwrap_or(0) > 0 {}
    let mut w = [0u8; 65];
    w[1] = grp;
    w[2] = sub;
    w[3..3 + args.len()].copy_from_slice(args);
    d.write(&w).ok()?;
    let mut r = [0u8; 65];
    let n = d.read_timeout(&mut r, 300).unwrap_or(0);
    if n == 0 {
        None
    } else {
        Some(r[..n].to_vec())
    }
}

fn hue(i: usize) -> [u8; 3] {
    // cell index → HSV 色相で B,G,R を返す (135 セルで 1 周)
    let h = (i as f32 / 135.0) * 360.0;
    let x = 1.0 - ((h / 60.0) % 2.0 - 1.0).abs();
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [(b * 255.0) as u8, (g * 255.0) as u8, (r * 255.0) as u8]
}

fn main() {
    let api = hidapi::HidApi::new().unwrap();
    let d = open(&api).expect("SU66 が見つからない");

    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("rows");

    let mut cells = [[0u8; 4]; 135];
    match mode {
        // 行3+行4 全体を色相スイープ — Enter/↑ の色から index 特定
        "rows" => {
            for i in 42..105 {
                let [b, g, r] = hue(i);
                cells[i] = [b, g, r, 255];
            }
            println!("行3+行4 (cells 42-104) を色相スイープで点灯 — 30秒");
            println!("  cell42=黄緑 → 色相は index に比例 (cell÷135×360°)");
            println!(
                "  色→index: 黄≈22-33 緑≈45-56 水色≈67-78 青≈90-101 紫≈112-124 赤≈0-11,124-134"
            );
        }
        // 容疑セル同時識別: Enter 候補 73-77 / ↑ 候補 96-99 を別色で点灯
        "suspects" => {
            // (cell, B,G,R) — 離れた index 同士は区別しやすい色で
            for &(cell, b, g, r) in &[
                (73usize, 0u8, 0u8, 255u8), // 赤
                (74, 0, 255, 0),            // 青
                (75, 255, 0, 0),            // 緑  ← 現行の Enter 推測
                (76, 255, 255, 255),        // 白
                (77, 0, 255, 255),          // 黄
                (78, 255, 0, 255),          // 紫
                (79, 255, 255, 0),          // 水色
            ] {
                cells[cell] = [b, g, r, 255];
            }
            for &(cell, b, g, r) in &[
                (95usize, 255u8, 255u8, 255u8), // 白
                (96, 0, 0, 255),                // 赤
                (97, 0, 255, 0),                // 青
                (98, 255, 0, 0),                // 緑  ← 現行の ↑ 推測
                (99, 0, 255, 255),              // 黄
                (100, 255, 0, 255),             // 紫
                (101, 255, 255, 0),             // 水色
            ] {
                cells[cell] = [b, g, r, 255];
            }
            println!("Enter 候補: cells 73-79 = 赤/青/緑/白/黄/紫/水色 (現行推測=75:緑)");
            println!("↑ 候補:    cells 95-101 = 白/赤/青/緑/黄/紫/水色 (現行推測=98:緑)");
            println!("→ Enter と ↑ に乗ってる色を教えてください");
        }
        // 74/75 と 97/98 の帰属を赤/青で確定
        "split" => {
            cells[74] = [0, 0, 255, 255]; // 赤
            cells[75] = [255, 0, 0, 255]; // 青 (BGR 順!)
            cells[97] = [0, 0, 255, 255]; // 赤
            cells[98] = [255, 0, 0, 255]; // 青
            println!("cell74=赤 cell75=青 / cell97=赤 cell98=青");
            println!("→ ' が赤なら '=74・青なら75。↑が赤なら97・Delが青なら98");
        }
        // 修正検証 (matrix_col 正解): '=74+75赤, Enter=76緑, ↑=97黄, Del=98青
        "verify" => {
            cells[74] = [0, 0, 255, 255]; // ' = 赤
            cells[75] = [0, 0, 255, 255]; // ' 右端pad = 赤
            cells[76] = [0, 255, 0, 255]; // Enter = 緑 (BGR 順!)
            cells[97] = [0, 255, 255, 255]; // ↑ = 黄
            cells[98] = [255, 0, 0, 255]; // Del = 青
            println!("'=赤 Enter=緑 ↑=黄 Del=青 — この4キーだけ光れば修正OK");
        }
        // 個別セル確認: probe10 single <n> で cell n だけ白点灯
        "single" => {
            let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(98);
            cells[n] = [255, 255, 255, 255];
            println!("cell {n} のみ白点灯 — 30秒");
        }
        _ => {}
    }

    // 30 秒間フレーム維持
    for _ in 0..300 {
        send_frame(&d, &cells);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if let Some(r) = cmd(&d, 5, 1, &[0, 0]) {
        let mut args = vec![0u8];
        args.extend_from_slice(&r[4..r.len().min(10)]);
        let _ = cmd(&d, 5, 2, &args);
        println!("firmware lighting restored");
    }
}
