use devhub_core::backend::*;
use devhub_core::model::DeviceKind;
fn main(){
 let hub = devhub_hid::connect().unwrap();
 let list = hub.scan().unwrap();
 let mouse = list.iter().find(|d| d.kind==DeviceKind::Mouse).unwrap();
 let drv = hub.driver(&mouse.id).unwrap();
 let mut cfg = drv.mouse_config().unwrap();
 println!("before: msync={} asnap={} ripple={} deb={}", cfg.motion_sync, cfg.angle_snap, cfg.ripple_control, cfg.debounce_ms);
 cfg.motion_sync = !cfg.motion_sync;
 match drv.apply_mouse(&cfg) {
   Ok(())=>println!("apply ok"),
   Err(e)=>{println!("apply err: {e}"); return}
 }
 let cfg2 = drv.mouse_config().unwrap();
 println!("after:  msync={} (expect {})", cfg2.motion_sync, cfg.motion_sync);
 let mut back = cfg2.clone(); back.motion_sync = !back.motion_sync;
 match drv.apply_mouse(&back) {
   Ok(())=>println!("restored"),
   Err(e)=>println!("restore err: {e}"),
 }
}
