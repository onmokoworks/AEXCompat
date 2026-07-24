use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use aex_guest_worker::classic::ClassicHost;
use aex_guest_worker::pe::PeImage;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(command) = args.next() else {
        eprintln!("usage: aex-guest-worker <inspect|setup|render> <x64.aex>");
        return ExitCode::from(2);
    };
    let Some(path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: aex-guest-worker <inspect|setup|render> <x64.aex>");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: aex-guest-worker <inspect|setup|render> <x64.aex>");
        return ExitCode::from(2);
    }

    let result = fs::read(&path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| PeImage::parse_and_map(&bytes).map_err(|error| error.to_string()))
        .and_then(|image| match command.to_str() {
            Some("inspect") => {
                serde_json::to_string_pretty(&image.report()).map_err(|error| error.to_string())
            }
            Some("setup") => ClassicHost::new(&image)
                .and_then(|mut host| host.setup())
                .and_then(|report| {
                    serde_json::to_string_pretty(&report).map_err(|error| {
                        aex_guest_worker::classic::ClassicError::Guest(
                            aex_guest_worker::x64::GuestError::Callback(error.to_string()),
                        )
                    })
                })
                .map_err(|error| error.to_string()),
            Some("render") => ClassicHost::new(&image)
                .and_then(|mut host| host.render_default_2x2())
                .and_then(|report| {
                    serde_json::to_string_pretty(&report).map_err(|error| {
                        aex_guest_worker::classic::ClassicError::Guest(
                            aex_guest_worker::x64::GuestError::Callback(error.to_string()),
                        )
                    })
                })
                .map_err(|error| error.to_string()),
            _ => Err("command must be inspect, setup, or render".to_string()),
        });
    match result {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            ExitCode::from(1)
        }
    }
}
