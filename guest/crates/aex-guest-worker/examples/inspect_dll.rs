//! Static dependency inspection; never executes DLL code.
use aex_guest_worker::pe::PeImage;
use std::{env, fs, process::ExitCode};

fn main() -> ExitCode {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut args = env::args().skip(1);
        let path = args
            .next()
            .ok_or("usage: inspect_dll <dll> [hex-guest-base]")?;
        if fs::metadata(&path)?.len() > 128 * 1024 * 1024 {
            return Err("DLL exceeds 128 MiB input bound".into());
        }
        let mut image = PeImage::parse_library(&fs::read(path)?)?;
        if let Some(base) = args.next() {
            image = image.rebase(u64::from_str_radix(base.trim_start_matches("0x"), 16)?)?;
        }
        if args.next().is_some() {
            return Err("unexpected argument".into());
        }
        println!("{}", serde_json::to_string_pretty(&image.report())?);
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
