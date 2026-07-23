//! Diagnostic: discover a sample of AEX **serially** and print per-AEX timing +
//! result, to tell whether the multi-filter cold-scan timeouts are caused by the
//! 8-way parallel resource contention or by genuinely slow/hanging plug-ins.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discover_timing -- "<dir>" [max]

use std::path::{Path, PathBuf};
use std::time::Instant;

use aexcompat_broker::image_render::inspect_experimental_with_diagnostics;
use sha2::{Digest, Sha256};

fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            collect(&path, out, depth + 1);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("aex"))
        {
            out.push(path);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: discover_timing <dir> [max]"));
    let max: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(30);
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_MULTIFILTER_REPOSITORY")
            .expect("set AEXCOMPAT_MULTIFILTER_REPOSITORY"),
    );

    let mut paths = Vec::new();
    collect(&dir, &mut paths, 0);
    paths.sort();
    paths.truncate(max);
    eprintln!("serial discovery of {} AEX under {dir:?}\n", paths.len());

    let (mut ok, mut fail_fast, mut fail_slow) = (0u32, 0u32, 0u32);
    for plugin in &paths {
        let name = plugin.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let Ok(bytes) = std::fs::read(plugin) else {
            continue;
        };
        let sha = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let started = Instant::now();
        let result = inspect_experimental_with_diagnostics(&repository, plugin, &sha);
        let ms = started.elapsed().as_millis();
        match result {
            Ok((params, _)) => {
                ok += 1;
                println!("{ms:>6} ms  OK ({:>3} params)  {name}", params.len());
            }
            Err(error) if ms >= 4500 => {
                fail_slow += 1;
                let detail = format!("{error}");
                println!(
                    "{ms:>6} ms  TIMEOUT/slow  {name}\n    {}",
                    &detail[..detail.len().min(600)]
                );
            }
            Err(error) => {
                fail_fast += 1;
                let detail = format!("{error}");
                println!(
                    "{ms:>6} ms  FAIL  {name}\n    {}",
                    &detail[..detail.len().min(600)]
                );
            }
        }
    }
    eprintln!(
        "\nserial: ok={ok}  fast-fail={fail_fast}  slow/timeout={fail_slow}  (of {})",
        paths.len()
    );
}
