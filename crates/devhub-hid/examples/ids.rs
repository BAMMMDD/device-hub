fn main() {
    let hub = devhub_hid::connect().unwrap();
    for d in hub.scan().unwrap() {
        println!("{} | {:?}", d.id.0, d.name);
    }
}
