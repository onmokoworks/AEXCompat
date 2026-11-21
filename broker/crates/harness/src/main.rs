#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod licenses;

mod shared_ui;

#[cfg(any(target_os = "macos", test))]
mod guest_parameter_transport;
mod gui_state;
#[cfg(any(target_os = "macos", test))]
mod shared_descriptor;
#[cfg(any(windows, target_os = "macos"))]
mod worker_backend;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
mod macos_worker_controller;

#[cfg(any(target_os = "macos", test))]
mod process_group_cleanup;

#[cfg(windows)]
include!("windows.rs");

// macOS uses the same GUI; only the worker behind crate::worker_backend differs.
#[cfg(target_os = "macos")]
mod unified_gui {
    #![allow(dead_code)]
    include!("windows/ui.rs");
    include!("windows/preflight.rs");
    include!("windows/render_contract.rs");
    include!("windows/live_session.rs");
    include!("windows/app.rs");
    include!("windows/effect_chain.rs");
    include!("windows/viewer.rs");
    include!("macos_unified_gui_tests.rs");

    pub(crate) fn run(repository: PathBuf) -> eframe::Result {
        let result = run_gui(repository);
        // Statics are never dropped, so close the resident guest explicitly.
        crate::macos::backend::close_resident_session_blocking();
        result
    }
}

#[cfg(target_os = "macos")]
fn main() -> eframe::Result<()> {
    let mut args = std::env::args().collect::<Vec<_>>();
    args.retain(|argument| argument != "--headless");
    if args
        .get(1)
        .is_some_and(|argument| argument == "--describe-aex")
    {
        if args.len() != 3 {
            eprintln!("aexcompat_description_error: expected exactly one AEX path");
            std::process::exit(64);
        }
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("harness lives beneath the repository");
        match macos::describe_aex_headless(repository, std::path::Path::new(&args[2])) {
            Ok(description) => {
                println!("{}", serde_json::to_string_pretty(&description).unwrap());
                return Ok(());
            }
            Err(classification) => {
                eprintln!("aexcompat_description_error: {classification}");
                std::process::exit(1);
            }
        }
    }
    if args.len() == 5 && args[1] == "--render-fixture" {
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("harness lives beneath the repository");
        match macos::render_fixture_headless(
            repository,
            std::path::Path::new(&args[2]),
            std::path::Path::new(&args[3]),
            std::path::Path::new(&args[4]),
        ) {
            Ok(report) => {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
                return Ok(());
            }
            Err(error) => {
                eprintln!("aexcompat_fixture_error: {error}");
                std::process::exit(1);
            }
        }
    }
    if std::env::var_os("AEXCOMPAT_LEGACY_MAC_GUI").is_some_and(|value| value == "1") {
        return macos::run();
    }
    unified_gui::run(macos::gui_repository_root())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("aexcompat-harness currently supports Windows and macOS");
}
