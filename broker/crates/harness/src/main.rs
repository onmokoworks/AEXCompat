#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod gui_state;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(windows)]
include!("windows.rs");

#[cfg(target_os = "macos")]
fn main() -> eframe::Result<()> {
    macos::run()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("aexcompat-harness currently supports Windows and macOS");
}
