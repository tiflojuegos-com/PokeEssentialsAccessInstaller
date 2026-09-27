mod cli;
mod console;
mod core;
#[cfg(feature = "gui")]
mod gui;
mod i18n;
#[cfg(feature = "gui")]
mod uia;

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    #[cfg(feature = "gui")]
    if let Some(dropped) = cli::window_start(&args) {
        console::leave_for_gui();
        gui::run(dropped);
        return;
    }
    std::process::exit(cli::run(args));
}
