//! USB 実測の手動確認 — 管理者で実行し、対象マウスを動かす。
//!
//! ```text
//! cargo run --release -p devhub-polling --example usbrate -- <hub> <addr> <ep> [report_id]
//! # 例: usbrate 1 2 1        (USBPcap1 / addr 2 / EP 0x81 / Report ID 全件)
//! # 例: usbrate 1 2 1 3      (Report ID 3 のみ)
//! ```
//!
//! 1 秒ごとに「現在 Hz / 期間内ピーク」を出す。Ctrl+C で終了。

use devhub_polling::{Capture, Status, Target};

fn main() {
    let mut a = std::env::args().skip(1);
    let (hub, addr, ep): (u16, u8, u8) = match (a.next(), a.next(), a.next()) {
        (Some(h), Some(d), Some(e)) => (
            h.parse().unwrap(),
            d.parse().unwrap(),
            e.parse().unwrap(),
        ),
        _ => {
            eprintln!("usage: usbrate <hub> <addr> <ep> [report_id]");
            std::process::exit(2);
        }
    };
    let report: Option<u8> = a
        .next()
        .and_then(|s| s.parse::<i16>().ok())
        .and_then(|v| u8::try_from(v).ok());
    let secs: u64 = a.next().and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
    let cap = Capture::start(Target {
        hub,
        device: addr,
        endpoint: 0x80 | ep,
        report,
    });
    println!("USBPcap{hub} addr {addr} EP 0x{:02X} — 対象のマウスを動かしてください", 0x80 | ep);
    for _ in 0..secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let s = cap.snapshot();
        match s.status {
            Status::Live | Status::Waiting => {
                println!("{:>5} Hz  (peak {} Hz)", s.hz, s.peak_hz)
            }
            st => {
                println!("status: {st:?}");
                std::process::exit(1);
            }
        }
    }
}
