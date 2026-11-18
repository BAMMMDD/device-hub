//! EP 自動検出の確認用プローブ (要管理者)。
//! `usbfind <hub> <addr> [秒]` — 対象デバイスの interrupt IN 転送を
//! EP 別に数えて多い順に出す。対象のマウスを動かしながら実行すること。
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hub: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2);
    let addr: u8 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5);
    eprintln!("USBPcap{hub} addr {addr} を {secs} 秒スキャン — 対象を動かしてください");
    match devhub_polling::scan_interrupt(hub, addr, std::time::Duration::from_secs(secs)) {
        Ok(eps) if eps.is_empty() => eprintln!("転送なし"),
        Ok(eps) => {
            for (ep, n) in eps {
                println!("EP 0x{ep:02X} -> {n} transfers");
            }
        }
        Err(e) => eprintln!("error: {e} (os {:?})", e.raw_os_error()),
    }
}
