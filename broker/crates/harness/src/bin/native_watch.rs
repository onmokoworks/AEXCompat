//! Attach to an already isolated, idle native worker. Never launches plug-ins.
#[path = "../native_watch/mod.rs"]
mod native_watch;

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    if let Err(error) = run() {
        eprintln!("native_watch_error: {error}");
        std::process::exit(1);
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn run() -> Result<(), String> {
    use std::{
        fs::OpenOptions,
        io::{Read, Write},
    };
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 5 {
        return Err("usage: native-watch <worker-pid> <spec.json> <new-report.json> <timeout-ms (1..300000)>".into());
    }
    let pid = args[1]
        .to_str()
        .ok_or("PID encoding")?
        .parse()
        .map_err(|_| "invalid PID")?;
    let timeout: u64 = args[4]
        .to_str()
        .ok_or("timeout encoding")?
        .parse()
        .map_err(|_| "invalid timeout")?;
    if !(1..=300_000).contains(&timeout) {
        return Err("timeout must be 1..300000 ms".into());
    }
    let mut source = Vec::new();
    std::fs::File::open(&args[2])
        .map_err(|e| e.to_string())?
        .take(16385)
        .read_to_end(&mut source)
        .map_err(|e| e.to_string())?;
    if source.len() > 16384 {
        return Err("spec exceeds 16 KiB".into());
    }
    let config: native_watch::Config =
        serde_json::from_slice(&source).map_err(|e| e.to_string())?;
    let observer = native_watch::Observer::new(config)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])
        .map_err(|e| e.to_string())?;
    native_watch::windows::require_worker(pid)?;
    let report = native_watch::windows::observe(
        pid,
        observer,
        std::time::Duration::from_millis(timeout),
        || {
            println!("native_watch_armed");
            let _ = std::io::stdout().flush();
        },
    );
    serde_json::to_writer_pretty(&mut output, &report).map_err(|e| e.to_string())?;
    output.write_all(b"\n").map_err(|e| e.to_string())?;
    if !report.success {
        return Err(report.reason);
    }
    Ok(())
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!(
        "native-watch requires x86_64 Windows; portable core tests do not execute the Windows adapter"
    );
    std::process::exit(2);
}
