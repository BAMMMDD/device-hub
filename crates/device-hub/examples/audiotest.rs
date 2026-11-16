//! オーディオキャプチャの確認 — 5 秒間バンド値を表示する。
//! `cargo run -p device-hub --example audiotest` で実行し、
//! 音楽を流した状態で値が 0 以外になるか見る。

fn main() {
    device_hub::audio::start();
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let b = device_hub::audio::bands();
        let max = b.iter().copied().fold(0f32, f32::max);
        let sum: f32 = b.iter().sum();
        println!(
            "max={max:.3} sum={sum:.3}  [{}]",
            b.iter()
                .map(|v| format!("{:.2}", v))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
}
