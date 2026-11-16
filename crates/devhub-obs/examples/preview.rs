//! OBS 無しで描画を確認する: 実機を ~5 秒ポーリングし、
//! キーを押しながら `obs-preview.png` に焼く。
//!   cargo run -p devhub-obs --example preview

fn main() {
    let poller = devhub_obs::live::Poller::start();
    println!("5 秒間ポーリングします — キーやボタンを押してください");
    std::thread::sleep(std::time::Duration::from_secs(5));
    let font = devhub_obs::render::font();
    let live = poller.shared.lock().unwrap().clone();
    eprintln!("status: {:?}", live.status);
    eprintln!(
        "kb: {:?} mouse: {:?}",
        live.kb.as_ref().map(|k| k.samples.len()),
        live.mouse.as_ref().map(|m| m.depths.len())
    );
    let (cfg, _) = devhub_obs::config::current();
    let vis = devhub_obs::render::Vis::default().with_cfg(&cfg);
    for (mode, file) in [
        (devhub_obs::render::Mode::Both, "obs-preview.png"),
        (devhub_obs::render::Mode::Kb, "obs-preview-kb.png"),
        (devhub_obs::render::Mode::Mouse, "obs-preview-mouse.png"),
    ] {
        let pix = devhub_obs::render::draw(&live, font.as_ref(), mode, &vis);
        image::save_buffer(
            file,
            &pix.buf,
            pix.w,
            pix.h,
            image::ExtendedColorType::Rgba8,
        )
        .expect("png write");
        println!("wrote {file} ({}x{})", pix.w, pix.h);
    }
}
