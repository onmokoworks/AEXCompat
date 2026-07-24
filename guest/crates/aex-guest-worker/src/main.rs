use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use aex_guest_worker::pe::PeImage;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(command) = args.next() else {
        eprintln!("usage: aex-guest-worker inspect <x64.aex>");
        return ExitCode::from(2);
    };
    let Some(path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: aex-guest-worker inspect <x64.aex>");
        return ExitCode::from(2);
    };
    if args.next().is_some() || command != "inspect" {
        eprintln!("usage: aex-guest-worker inspect <x64.aex>");
        return ExitCode::from(2);
    }

    match fs::read(&path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| PeImage::parse_and_map(&bytes).map_err(|error| error.to_string()))
    {
        Ok(image) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&image.report())
                    .expect("PE report is always serializable")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            ExitCode::from(1)
        }
    }
}
