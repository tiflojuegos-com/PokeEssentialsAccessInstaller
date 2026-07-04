#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod gui;
mod i18n;

fn main() {
    gui::run();
}
