//! USB デバイス列挙の確認用プローブ — `cargo run -p devhub-polling --example usbenum`
//! 権限不要。`devices()` の結果を表示する。
fn main() {
    for (n, root) in devhub_polling::discover::hubs() {
        println!("USBPcap{n} -> {root}");
    }
    for d in devhub_polling::discover::devices() {
        println!(
            "hub={:2} addr={:3} {:04x}:{:04x} mouse={} {:?}\n    {} | {}",
            d.hub,
            d.address,
            d.vid,
            d.pid,
            d.is_mouse,
            d.name,
            d.instance_id,
            d.location
        );
    }
}
