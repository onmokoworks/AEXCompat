//! Diagnostic: run the multifilter's real discovery pass (`discover_all`)
//! over a directory of AEX and report per-plugin outcomes and the total wall
//! time for the in-place-only discovery route (#816).
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discovery_ab_diag -- "<effects dir>" [deps-dir...]
//!
//! Clustering, sessions, and fallbacks are the code the bridge ships. The
//! former staged A/B environment variable was removed by #816.

use std::path::PathBuf;
use std::time::Instant;

use aexcompat_aviutl2_multifilter::discover_all_for_diagnostics;

fn main() {
    let mut args = std::env::args().skip(1);
    let directory = PathBuf::from(
        args.next()
            .expect("usage: discovery_ab_diag <effects dir> [deps-dir...]"),
    );
    let dependency_dirs: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_MULTIFILTER_REPOSITORY")
            .expect("set AEXCOMPAT_MULTIFILTER_REPOSITORY"),
    );
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .expect("read the effects directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("aex"))
        })
        .collect();
    paths.sort();
    eprintln!(
        "discovering {} plug-in(s) via the in-place pipeline",
        paths.len(),
    );
    let started = Instant::now();
    let mut results = discover_all_for_diagnostics(&repository, &paths, dependency_dirs);
    let elapsed = started.elapsed();
    results.sort_by(|left, right| left.0.cmp(&right.0));
    let mut ok = 0usize;
    for (path, discovered, classification) in &results {
        if *discovered {
            ok += 1;
        }
        println!(
            "{}\t{}\t{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?"),
            if *discovered { "ok" } else { "failed" },
            classification.as_deref().unwrap_or("-"),
        );
    }
    println!(
        "total={} ok={} failed={} elapsed={:.1}s",
        results.len(),
        ok,
        results.len() - ok,
        elapsed.as_secs_f64()
    );
}
