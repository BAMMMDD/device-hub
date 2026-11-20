//! Read-only probe for Razer DeathAdder V4 Pro (90B Feature Report).
//! `cargo run -p devhub-hid --example razerprobe`

use hidapi::HidApi;
use std::time::Duration;

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn razer_crc(report: &[u8; 90]) -> u8 {
    let mut crc = 0u8;
    for &b in &report[2..88] {
        crc ^= b;
    }
    crc
}

fn build_report(tx_id: u8, data_size: u8, cmd_class: u8, cmd_id: u8, args: &[u8]) -> [u8; 90] {
    let mut r = [0u8; 90];
    r[0] = 0x00; // status = new command
    r[1] = tx_id;
    r[2] = 0x00; // remaining_packets hi
    r[3] = 0x00; // remaining_packets lo
    r[4] = 0x00; // protocol_type
    r[5] = data_size;
    r[6] = cmd_class;
    r[7] = cmd_id;
    let n = args.len().min(80);
    r[8..8 + n].copy_from_slice(&args[..n]);
    r[88] = razer_crc(&r);
    r[89] = 0x00;
    r
}

fn send_recv(dev: &hidapi::HidDevice, req: &[u8; 90]) -> Result<(usize, [u8; 90]), String> {
    let mut w = [0u8; 91];
    w[0] = 0x00; // Report ID 0
    w[1..91].copy_from_slice(req);
    dev.send_feature_report(&w)
        .map_err(|e| format!("send_feature_report: {e}"))?;

    for attempt in 0..5 {
        std::thread::sleep(Duration::from_millis(if attempt == 0 { 15 } else { 20 }));
        let mut r = [0u8; 91];
        r[0] = 0x00;
        let len = dev
            .get_feature_report(&mut r)
            .map_err(|e| format!("get_feature_report: {e}"))?;
        let resp: [u8; 90] = if len >= 91 {
            let mut out = [0u8; 90];
            out.copy_from_slice(&r[1..91]);
            out
        } else if len == 90 {
            let mut out = [0u8; 90];
            out.copy_from_slice(&r[0..90]);
            out
        } else {
            return Err(format!("short feature report len={len}: {}", hex(&r[..len])));
        };

        if resp[0] == 0x01 {
            // BUSY
            continue;
        }
        return Ok((len, resp));
    }
    Err("device stayed BUSY (status=0x01)".to_string())
}

fn main() {
    let api = HidApi::new().expect("HidApi::new");
    println!("== Razer 1532 interfaces ==");
    let mut candidates = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() == 0x1532 {
            println!(
                "  {:04x}:{:04x} if={} up={:04x} u={:02x} name={:?} serial={:?} path={:?}",
                d.vendor_id(),
                d.product_id(),
                d.interface_number(),
                d.usage_page(),
                d.usage(),
                d.product_string(),
                d.serial_number(),
                d.path(),
            );
            candidates.push((
                d.product_id(),
                d.interface_number(),
                d.usage_page(),
                d.usage(),
                d.path().to_owned(),
            ));
        }
    }

    if candidates.is_empty() {
        println!("No Razer 1532 interfaces found!");
        return;
    }

    let mut working = None;
    for (pid, if_num, up, u, path) in &candidates {
        println!(
            "\n-- Trying GetFirmwareVersion on 1532:{pid:04x} if={if_num} up={up:04x} u={u:02x} --"
        );
        match api.open_path(path) {
            Ok(dev) => {
                let req = build_report(0x1F, 0x02, 0x00, 0x81, &[]);
                match send_recv(&dev, &req) {
                    Ok((len, resp)) => {
                        println!(
                            "  len={len} status={:#04x} tx={:#04x} class={:#04x} cmd={:#04x} args[0..4]={}",
                            resp[0],
                            resp[1],
                            resp[6],
                            resp[7],
                            hex(&resp[8..12])
                        );
                        if resp[0] == 0x02 && resp[6] == 0x00 && resp[7] == 0x81 {
                            println!("  ==> WORKING INTERFACE FOUND! (v{}.{})", resp[8], resp[9]);
                            working = Some((*pid, *if_num, *up, *u, dev));
                            break;
                        }
                    }
                    Err(e) => println!("  feature err: {e}"),
                }
            }
            Err(e) => println!("  open_path err: {e}"),
        }
    }

    let Some((pid, if_num, up, u, dev)) = working else {
        println!("\nNo working Razer feature interface found!");
        return;
    };

    println!(
        "\n== Full Read-Only Probe on 1532:{pid:04x} if={if_num} up={up:04x} u={u:02x} =="
    );

    let cmds: &[(&str, u8, u8, u8, &[u8])] = &[
        ("Get Firmware Version", 0x02, 0x00, 0x81, &[]),
        ("Get Serial", 0x16, 0x00, 0x82, &[]),
        ("Get Polling Rate 2", 0x01, 0x00, 0xC0, &[]),
        ("Get DPI XY (NOSTORE=0x00)", 0x07, 0x04, 0x85, &[0x00]),
        ("Get DPI XY (VARSTORE=0x01)", 0x07, 0x04, 0x85, &[0x01]),
        ("Get DPI Stages (VARSTORE=0x01)", 0x26, 0x04, 0x86, &[0x01]),
        ("Get Battery Level", 0x02, 0x07, 0x80, &[]),
        ("Get Charging Status", 0x02, 0x07, 0x84, &[]),
        ("Get Idle Time", 0x02, 0x07, 0x83, &[]),
        ("Get Low Battery Threshold", 0x01, 0x07, 0x81, &[]),
        ("Get Dongle Multi Indicator LEDs", 0x03, 0x07, 0x95, &[]),
    ];

    for (label, size, class, cmd, args) in cmds {
        let req = build_report(0x1F, *size, *class, *cmd, args);
        match send_recv(&dev, &req) {
            Ok((len, resp)) => {
                let dlen = (resp[5] as usize).min(80);
                println!("\n[{label}] len={len} status={:#04x} tx={:#04x} rem={:#06x} proto={:#04x} dsize={:#04x} class={:#04x} cmd={:#04x}",
                    resp[0], resp[1], u16::from_be_bytes([resp[2], resp[3]]), resp[4], resp[5], resp[6], resp[7]);
                println!("  args[0..{}]: {}", dlen.max(16).min(80), hex(&resp[8..8 + dlen.max(16).min(80)]));

                match (*class, *cmd) {
                    (0x00, 0x81) => println!("  -> FW: v{}.{}", resp[8], resp[9]),
                    (0x00, 0x82) => {
                        let s = String::from_utf8_lossy(&resp[8..8 + 22])
                            .trim_end_matches('\0')
                            .to_string();
                        println!("  -> Serial: {s:?}");
                    }
                    (0x00, 0xC0) => {
                        let hz = match resp[9] {
                            0x01 => 8000,
                            0x02 => 4000,
                            0x04 => 2000,
                            0x08 => 1000,
                            0x10 => 500,
                            0x20 => 250,
                            0x40 => 125,
                            _ => 0,
                        };
                        println!("  -> Polling rate code={:#04x} ({hz} Hz) arg0={:#04x}", resp[9], resp[8]);
                    }
                    (0x04, 0x85) => {
                        let x = u16::from_be_bytes([resp[9], resp[10]]);
                        let y = u16::from_be_bytes([resp[11], resp[12]]);
                        println!("  -> DPI XY: store={:#04x} x={x} y={y}", resp[8]);
                    }
                    (0x04, 0x86) => {
                        let store = resp[8];
                        let active = resp[9];
                        let count = resp[10];
                        println!("  -> DPI Stages: store={store:#04x} active={active} count={count}");
                        let mut off = 11usize;
                        for i in 0..count {
                            if off + 7 <= 88 {
                                let id = resp[off];
                                let x = u16::from_be_bytes([resp[off + 1], resp[off + 2]]);
                                let y = u16::from_be_bytes([resp[off + 3], resp[off + 4]]);
                                println!(
                                    "     stage[{i}]: id={id} x={x} y={y} rsv=[{:02x} {:02x}]",
                                    resp[off + 5],
                                    resp[off + 6]
                                );
                            }
                            off += 7;
                        }
                    }
                    (0x07, 0x80) => {
                        let raw = resp[9];
                        let pct = (raw as u32 * 100) / 255;
                        println!("  -> Battery: raw={raw}/255 ({pct}%) arg0={:#04x}", resp[8]);
                    }
                    (0x07, 0x84) => {
                        println!("  -> Charging: {} (arg0={:#04x})", resp[9] != 0, resp[8]);
                    }
                    (0x07, 0x83) => {
                        let sec = u16::from_be_bytes([resp[8], resp[9]]);
                        println!("  -> Idle time: {sec} s ({} min)", sec / 60);
                    }
                    (0x07, 0x81) => {
                        let raw = resp[8];
                        let pct = (raw as u32 * 100) / 255;
                        println!("  -> Low battery threshold: raw={raw:#04x} (~{pct}%)", );
                    }
                    (0x07, 0x95) => {
                        println!(
                            "  -> Dongle multi indicator LEDs: left={} center={} right={}",
                            resp[8], resp[9], resp[10]
                        );
                    }
                    _ => {}
                }
            }
            Err(e) => println!("\n[{label}] err: {e}"),
        }
    }
}
