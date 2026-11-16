//! TP55 PEQ E2E: 実機で 10 バンド読み取り → バンド書き換え往復 → 復元。
//! 最後に no-op PeqAll (現在値をそのまま 10 フレーム書き込み) で
//! 全バンド書き込み + 即時適用 + フラッシュ保存のシーケンスを検証する。

use devhub_core::*;

fn show(tag: &str, st: &DeviceState) {
    let Some(peq) = &st.peq else {
        println!("{tag}: peq なし");
        return;
    };
    for (i, b) in peq.iter().enumerate() {
        println!(
            "  {tag} b{i}: {}Hz Q{:.2} {:+.1}dB {}",
            b.freq,
            b.q,
            b.gain,
            b.filter.label()
        );
    }
}

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
        .find(|d| d.driver == DriverKind::Walkplay);
    let Some(info) = info else {
        eprintln!("TP55 が見つかりません");
        return;
    };
    println!("== {}", info.name);
    let drv = hub.driver(&info.id).expect("driver");
    let st = drv.read().expect("read");
    show("read", &st);
    let Some(orig) = st.peq.clone() else {
        eprintln!("peq を読み取れませんでした");
        return;
    };
    let mut fails = 0u32;

    // --- バンド 4 の書き換え往復 (中域。freq/gain/type を同時に変える) ---
    let mut t = orig[4];
    t.freq = if t.freq == 700 { 900 } else { 700 };
    t.gain = if (t.gain - 3.).abs() < 0.5 { -3. } else { 3. };
    t.q = if (t.q - 1.5).abs() < 0.1 { 2. } else { 1.5 };
    t.filter = if t.filter == PeqFilter::HighShelf {
        PeqFilter::Peak
    } else {
        PeqFilter::HighShelf
    };
    print!("b4 write: ");
    match drv.apply(&Change::Peq { band: 4, value: t }) {
        Err(e) => {
            println!("apply err: {e}");
            fails += 1;
        }
        Ok(_) => {
            std::thread::sleep(std::time::Duration::from_millis(120));
            match drv.read().ok().and_then(|s| s.peq?.get(4).copied()) {
                Some(b) => {
                    let ok = b.freq == t.freq
                        && (b.q - t.q).abs() < 0.02
                        && (b.gain - t.gain).abs() < 0.05
                        && b.filter == t.filter;
                    println!(
                        "read {}Hz Q{:.2} {:+.1}dB {} → {}",
                        b.freq,
                        b.q,
                        b.gain,
                        b.filter.label(),
                        if ok { "OK" } else { "MISMATCH" }
                    );
                    if !ok {
                        fails += 1;
                    }
                }
                None => {
                    println!("readback failed");
                    fails += 1;
                }
            }
        }
    }

    // --- 復元 ---
    print!("b4 restore: ");
    match drv.apply(&Change::Peq {
        band: 4,
        value: orig[4],
    }) {
        Err(e) => {
            println!("apply err: {e}");
            fails += 1;
        }
        Ok(_) => {
            std::thread::sleep(std::time::Duration::from_millis(120));
            match drv.read().ok().and_then(|s| s.peq?.get(4).copied()) {
                Some(b) => {
                    let ok = b == orig[4];
                    println!("{}", if ok { "OK" } else { "MISMATCH" });
                    if !ok {
                        fails += 1;
                    }
                }
                None => {
                    println!("readback failed");
                    fails += 1;
                }
            }
        }
    }

    // --- no-op PeqAll: 現在値のまま 10 フレーム全書き込み (全シーケンス検証) ---
    print!("peqall noop: ");
    match drv.apply(&Change::PeqAll {
        bands: orig.clone(),
    }) {
        Err(e) => {
            println!("apply err: {e}");
            fails += 1;
        }
        Ok(_) => {
            std::thread::sleep(std::time::Duration::from_millis(120));
            match drv.read().ok().and_then(|s| s.peq) {
                Some(b) if b == orig => println!("OK"),
                Some(b) => {
                    println!("MISMATCH");
                    for (i, (a, o)) in b.iter().zip(orig.iter()).enumerate() {
                        if a != o {
                            println!("    b{i}: got {a:?} want {o:?}");
                        }
                    }
                    fails += 1;
                }
                None => {
                    println!("readback failed");
                    fails += 1;
                }
            }
        }
    }
    println!("=> {fails} 件の不一致");
}
