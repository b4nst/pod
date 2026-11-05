// no console window on Windows; the flash helper reports through a file
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod deck;
mod theme;

fn main() -> iced::Result {
    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == pod_core::flash::HELPER_ARG) {
        std::process::exit(pod_core::flash::helper_main(args));
    }
    app::run()
}
