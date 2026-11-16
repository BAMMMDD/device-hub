//! E2E through the real backend: scan → driver → info/caps/config/battery.
//! cargo run -p devhub-hid --example probe

use devhub_hid::connect;

fn main() {
    let hub = connect().expect("connect");
    let devs = hub.scan().expect("scan");
    println!("=== scan: {} devices ===", devs.len());
    for d in &devs {
        println!(
            "{:30} kind={:?} driver={:?} supported={} serial={:?}",
            d.name, d.kind, d.driver, d.supported, d.serial
        );
    }
    for d in &devs {
        if !d.supported {
            continue;
        }
        println!("\n=== {} ({}) ===", d.name, d.id);
        match hub.driver(&d.id) {
            Err(e) => println!("  driver: ERR {e}"),
            Ok(drv) => {
                let i = drv.info();
                println!("  info: fw={:?} supported={}", i.firmware, i.supported);
                match drv.battery() {
                    Ok(b) => println!("  battery: {:?}% charging={} mv={:?}", b.percent, b.charging, b.voltage_mv),
                    Err(e) => println!("  battery: {e}"),
                }
                match drv.mouse_config() {
                    Ok(c) => println!(
                        "  mouse: poll={}Hz stages={:?} active={} lod={} deb={}ms sleep={}",
                        c.polling_wired,
                        c.stages.iter().map(|s| s.dpi).collect::<Vec<_>>(),
                        c.active_stage, c.lod_mm, c.debounce_ms, c.sleep_min
                    ),
                    Err(e) => println!("  mouse_config: {e}"),
                }
                match drv.keyboard_config() {
                    Ok(c) => {
                        println!("  keyboard: layers={} light={:?} b={} s={}", c.layers.len(), c.lighting.mode, c.lighting.brightness, c.lighting.speed);
                        if let Some(l) = c.layers.first() {
                            let named: Vec<String> = l
                                .keys
                                .iter()
                                .filter_map(|k| match k {
                                    devhub_core::model::KeyAction::Standard(s) => Some(s.clone()),
                                    _ => None,
                                })
                                .collect();
                            println!("  layer0 keys: {:?}", &named[..named.len().min(40)]);
                        }
                    }
                    Err(e) => println!("  keyboard_config: {e}"),
                }
            }
        }
    }
    println!("\ndone");
}
