fn main() {
    let hub = devhub_hid::connect().unwrap();
    for info in hub.scan().unwrap_or_default() {
        if !info.supported {
            continue;
        }
        let drv = match hub.driver(&info.id) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if drv.caps().keyboard.is_none() {
            continue;
        }
        let st = drv.read().unwrap();
        let kb = st.keyboard.as_ref().unwrap();
        for (l, layer) in kb.keymap.iter().enumerate() {
            println!("--- layer{l} ---");
            for (i, a) in layer.iter().enumerate().take(60) {
                print!("{i}:{a:?} ");
                if i % 8 == 7 {
                    println!()
                }
            }
            println!();
        }
        for (k, v) in &st.settings {
            println!("{k} = {v:?}");
        }
    }
}
