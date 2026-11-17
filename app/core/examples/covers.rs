//! Write `/.pod/covers.bin` for a folder laid out like an iPod, as SYNC does
//! (the simulator's test library, see scripts/sim-setup.sh):
//!   cargo run -p pod-core --example covers -- build/sim/simdisk [ipod6g]
use std::path::Path;
use std::sync::atomic::AtomicBool;

use pod_core::{Model, covers};

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(mount) = args.next() else {
        return eprintln!("usage: covers MOUNT [TARGET]");
    };
    let model = args.next().and_then(|t| Model::from_target(&t.to_string_lossy()));
    println!("{:?}", covers::write(Path::new(&mount), model, &mut |_| {}, &AtomicBool::new(false)));
}
