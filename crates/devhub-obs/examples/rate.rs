//! 実測 input report レートを 1 秒ごとに表示 (バックグラウンド計測の検証用)。
//!   cargo run -p devhub-obs --example rate
//! マウスを動かしながら見る。窓を他アプリに隠しても INPUTSINK で計測できるはず。
fn main() {
    devhub_obs::rawinput::start(); // マウス+キーボード両方登録
    let targets: &[(u16, u16, &str)] = &[
        (0x33E4, 0x3854, "HTS Plus Pro (2.4G)"),
        (0x33E4, 0x5219, "HTS Plus Pro (有線)"),
        (0x1CA6, 0x3009, "SU66 Max"),
        (0x373B, 0x1278, "ATK ドングル"),
        (0x373B, 0x12A7, "ATK 有線"),
    ];
    loop {
        for &(v, p, name) in targets {
            let st = devhub_obs::rawinput::stats(v, p);
            println!(
                "{name}: {}",
                st.map(|s| format!(
                    "{:.0} Hz (avg) · seg {} · peak {} · med {}",
                    s.hz,
                    s.seg_hz.map(|x| format!("{x:.0}")).unwrap_or_else(|| "—".into()),
                    s.peak_hz.map(|x| format!("{x:.0}")).unwrap_or_else(|| "—".into()),
                    s.gap_stats
                        .map(|(m, _)| format!("{m:.3}ms → ~{:.0}Hz", 1000. / m))
                        .unwrap_or_else(|| "—".into()),
                ))
                .unwrap_or_else(|| "—".into()),
            );
        }
        println!("---");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
