//! ハイブリッド解放の実機確認:
//! 1. 7キーを赤で 30fps ストリーム 5秒 → 赤が出るはず
//! 2. 空マップを送信 → 追跡ピンが [5,4] で解放 → ファームアニメに戻るはず
use devhub_core::{DeviceKind, Rgb};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn main() {
    let hub = match devhub_hid::connect() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("connect: {e}");
            return;
        }
    };
    let info = hub
        .scan()
        .unwrap_or_default()
        .into_iter()
        .find(|d| d.kind == DeviceKind::Keyboard && d.supported)
        .expect("SU66");
    let d = hub.driver(&info.id).expect("driver");
    let area = d
        .caps()
        .lighting
        .iter()
        .find(|a| a.direct_drive)
        .map(|a| a.id)
        .expect("direct_drive エリア");
    let mut cells = BTreeMap::new();
    for i in 0..7usize {
        cells.insert(i, Rgb::new(255, 0, 0));
    }
    println!("5秒間 7キー赤ストリーム");
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(5) {
        d.direct_drive(area, &cells).unwrap();
        std::thread::sleep(Duration::from_millis(33));
    }
    println!("空マップ送信 — 7キーがファームに戻るはず (解放)");
    d.direct_drive(area, &BTreeMap::new()).unwrap();
    std::thread::sleep(Duration::from_secs(8));
    println!("終了");
}
