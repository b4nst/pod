//! Write `/.pod/covers.bin` for a folder laid out like an iPod, as SYNC does
//! (the simulator's test library, see scripts/sim-setup.sh):
//!   cargo run -p pod-core --example covers -- build/sim/simdisk
use std::path::Path;
use std::sync::atomic::AtomicBool;

use pod_core::covers;

fn main() {
    let Some(mount) = std::env::args_os().nth(1) else {
        return eprintln!("usage: covers MOUNT");
    };
    println!("{:?}", covers::write(Path::new(&mount), &mut |_| {}, &AtomicBool::new(false)));
}
