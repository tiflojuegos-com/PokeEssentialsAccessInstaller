#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
#[cfg(feature = "gui")]
mod gui;
mod i18n;

#[cfg(feature = "gui")]
fn main() {
    gui::run();
}

#[cfg(not(feature = "gui"))]
fn main() {}
