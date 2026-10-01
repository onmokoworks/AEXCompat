#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod licenses;

mod shared_ui;

mod gui_state;
#[cfg(any(target_os = "macos", test))]
mod shared_descriptor;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
mod macos_worker_controller;

#[cfg(any(target_os = "macos", test))]
mod process_group_cleanup;

#[cfg(windows)]
include!("windows.rs");

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
    macos::run()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("aexcompat-harness currently supports Windows and macOS");
}
