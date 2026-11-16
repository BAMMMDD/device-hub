//! パラメトリック EQ の biquad 計算 — WalkPlay CB 系 (peq.szwalkplay.com) の
//! 公式実装から移植。ドライバ側の wire 係数生成と UI の周波数特性グラフで
//! 共有し、両者の曲線が一致することを保証する。
//!
//! TP55 の DAC EQ 係数は **96 kHz** で計算される (実機応答の係数と
//! バイト単位で一致することを確認済み)。グラフ評価も同じ 96 kHz を使う。

use crate::model::{PeqBand, PeqFilter};

/// TP55 DAC EQ の係数サンプルレート。
pub const FS: f64 = 96_000.;

/// 正規化 biquad 係数 `[b0, b1, b2, -a1, -a2]` (a0=1)。
/// 公式フィルタークラス (H1/j1/X1/U1/t2) を忠実に再現する:
/// - PK/LP/HP … 標準 RBJ 公式
/// - LS (j1) … **Q を使わない**固定スロープの shelf (公式は引数に Q を取るが無視)
/// - HS (X1) … ゲインを ±12dB・Q を 0.5..5 にクランプし、さらに
///   **ゲイン × 0.85 (headroom)** を掛けた後に計算。負ゲイン時は分子の符号を反転
pub fn coefs(filter: PeqFilter, freq: f32, q: f32, gain_db: f32, fs: f64) -> [f64; 5] {
    let f = freq as f64;
    match filter {
        PeqFilter::Peak => {
            let s = (10f64).powf(gain_db as f64 / 40.);
            let w = 2. * std::f64::consts::PI * f / fs;
            let u = w.sin() / (2. * q as f64);
            let c = 1. + u / s;
            let cw = w.cos();
            [
                (1. + u * s) / c,
                -2. * cw / c,
                (1. - u * s) / c,
                2. * cw / c,
                -(1. - u / s) / c,
            ]
        }
        PeqFilter::LowShelf => {
            // j1: Q は無視 (公式も q 引数を使っていない)。
            let a = (10f64).powf(gain_db as f64 / 40.);
            let w = 2. * std::f64::consts::PI * f / fs;
            let s = w.sin() / 2. * (0.25 * (a + 1. / a) + 2.).sqrt();
            let (cw, sa) = (w.cos(), a.sqrt());
            let i = a * (a + 1. - (a - 1.) * cw + 2. * sa * s);
            let c = 2. * a * (a - 1. - (a + 1.) * cw);
            let d = a * (a + 1. - (a - 1.) * cw - 2. * sa * s);
            let f0 = a + 1. + (a - 1.) * cw + 2. * sa * s;
            [
                i / f0,
                c / f0,
                d / f0,
                2. * (a - 1. + (a + 1.) * cw) / f0,
                -(a + 1. + (a - 1.) * cw - 2. * sa * s) / f0,
            ]
        }
        PeqFilter::HighShelf => {
            // X1: 公式の _validateParameters + headroom を再現。
            let f = f.clamp(20., 0.99 * fs / 2.);
            let q = (q as f64).clamp(0.5, 5.);
            let g = (gain_db as f64).clamp(-12., 12.) * 0.85;
            let r = (10f64).powf(g.abs() / 40.);
            let o = r.sqrt();
            let w = 2. * std::f64::consts::PI * f / fs;
            let i = w.cos();
            let u = w.sin() / 2. * ((r + 1. / r) * (1. / q - 1.) + 2.).sqrt();
            let c = r + 1. - (r - 1.) * i + 2. * o * u;
            let d = if g >= 0. { 1. } else { -1. };
            [
                d * (r * (r + 1. + (r - 1.) * i + 2. * o * u)) / c,
                d * (-2. * r * (r - 1. + (r + 1.) * i)) / c,
                d * (r * (r + 1. + (r - 1.) * i - 2. * o * u)) / c,
                -2. * (r - 1. - (r + 1.) * i) / c,
                -(r + 1. - (r - 1.) * i - 2. * o * u) / c,
            ]
        }
        PeqFilter::LowPass => {
            let w = 2. * std::f64::consts::PI * f / fs;
            let s = w.sin() / (2. * q as f64);
            let i = 1. + s;
            let cw = w.cos();
            [
                (1. - cw) / 2. / i,
                (1. - cw) / i,
                (1. - cw) / 2. / i,
                2. * cw / i,
                -(1. - s) / i,
            ]
        }
        PeqFilter::HighPass => {
            let w = 2. * std::f64::consts::PI * f / fs;
            let l = w.sin() / (2. * q as f64);
            let s = 1. + l;
            let cw = w.cos();
            [
                (1. + cw) / 2. / s,
                -(1. + cw) / s,
                (1. + cw) / 2. / s,
                2. * cw / s,
                -(1. - l) / s,
            ]
        }
    }
}

/// `at` Hz における全バンド合成の応答 (dB)。UI のグラフ描画用。
pub fn response_db(bands: &[PeqBand], at: f64) -> f64 {
    use std::f64::consts::PI;
    let w = 2. * PI * at / FS;
    let (cw1, cw2) = (w.cos(), (2. * w).cos());
    let (sw1, sw2) = (w.sin(), (2. * w).sin());
    let mut db = 0f64;
    for b in bands {
        if b.gain.abs() < 0.01 && b.filter == PeqFilter::Peak {
            continue;
        }
        let [b0, b1, b2, na1, na2] = coefs(b.filter, b.freq as f32, b.q, b.gain, FS);
        let a1 = -na1;
        let a2 = -na2;
        // |H(e^jw)| = |b0 + b1 e^-jw + b2 e^-2jw| / |1 + a1 e^-jw + a2 e^-2jw|
        let nr = b0 + b1 * cw1 + b2 * cw2;
        let ni = -(b1 * sw1 + b2 * sw2);
        let dr = 1. + a1 * cw1 + a2 * cw2;
        let di = -(a1 * sw1 + a2 * sw2);
        let mag = ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt();
        db += 20. * mag.max(1e-9).log10();
    }
    db
}
