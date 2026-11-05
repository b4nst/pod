//! Try the privileged flash path end to end on a fake iPod disk image:
//!   cargo run -p pod-core --features fixtures --example flash-image -- make disk.img boot.ipod
//!   hdiutil attach -nomount -imagekey diskimage-class=CRawDiskImage disk.img   # macOS
//!   cargo run -p pod-core --features fixtures --example flash-image -- flash /dev/diskN boot.ipod
use pod_core::Model;
use pod_core::flash::{self, Action};
use pod_core::ipod::fixtures::{fake_bootloader, fake_ipod};

fn main() {
    let mut args = std::env::args_os().skip(1);
    let cmd = args.next().unwrap_or_default();
    if cmd == flash::HELPER_ARG {
        std::process::exit(flash::helper_main(args));
    }
    let args: Vec<String> = args.map(|a| a.to_string_lossy().into_owned()).collect();
    match (cmd.to_str(), args.as_slice()) {
        (Some("make"), [img, boot]) => {
            std::fs::write(img, fake_ipod(Model::Mini2g)).unwrap();
            std::fs::write(boot, fake_bootloader(Model::Mini2g, "pod-vtest")).unwrap();
        }
        (Some("flash"), [disk, boot]) => println!("{:?}", flash::run_elevated(disk, &Action::Install(boot.into()))),
        _ => eprintln!("usage: flash-image make IMG BOOT | flash DISK BOOT"),
    }
}
