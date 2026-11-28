// Shared Windows/macOS GUI. Only the CLI and native Windows tests are OS-specific.
#![cfg_attr(target_os = "macos", allow(dead_code))]

mod app;
mod effect_chain;
mod live_session;
mod preflight;
mod render_contract;
mod ui;
mod viewer;

#[cfg(windows)]
mod cli;
#[cfg(windows)]
mod independent_cases;
#[cfg(all(windows, test))]
mod tests;

#[cfg(all(target_os = "macos", test))]
#[path = "macos_unified_gui_tests.rs"]
mod macos_unified_gui_tests;

#[cfg(windows)]
pub(crate) fn run() -> eframe::Result {
    cli::run()
}

#[cfg(target_os = "macos")]
pub(crate) fn run(repository: std::path::PathBuf) -> eframe::Result {
    let result = viewer::run_gui(repository);
    // Statics are never dropped, so close the resident guest explicitly.
    crate::macos::backend::close_resident_session_blocking();
    result
}
