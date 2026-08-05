//! Diagnostic: run the multifilter's real discovery pass (`discover_all`)
//! over a directory of AEX and report per-plugin outcomes and the total wall
//! time — the A/B lever for issue #751's in-place discovery.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discovery_ab_diag -- "<effects dir>" [deps-dir...]
//!
//! `AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY=1` selects the staged (closure
//! walk + sealed tree) pipeline; unset runs the in-place default. Everything
//! else — clustering, sessions, fallbacks — is the code the bridge ships.

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
    let staged =
        std::env::var("AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY").is_ok_and(|value| value == "1");
    eprintln!(
        "discovering {} plug-in(s) via the {} pipeline",
        paths.len(),
        if staged { "staged" } else { "in-place" }
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
