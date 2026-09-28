//! Diagnostic: run the multifilter's real discovery pass (`discover_all`)
//! over a directory of AEX and report per-plugin outcomes and the total wall
//! time for the in-place-only discovery route (#816).
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discovery_ab_diag -- [--one-shot] [--skip N] [--limit N] "<effects dir>" [deps-dir...]
//!
//! Clustering, sessions, and fallbacks are the code the bridge ships. The
//! former staged A/B environment variable was removed by #816.

use std::path::PathBuf;
use std::time::Instant;

use aexcompat_aviutl2_multifilter::{
    discover_all_one_shot_for_diagnostics, discover_records_for_diagnostics,
};

// Discovery's fallback reason is free-form and may contain local paths. This
// shareable A/B output only needs the fixed resolution code.
fn fallback_resolution(note: &str) -> &'static str {
    if note.ends_with("/one_shot_fallback") {
        "one_shot_fallback"
    } else if note.ends_with("/invalidated") {
        "invalidated"
    } else {
        "unknown"
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut first = args
        .next()
        .expect("usage: discovery_ab_diag [--one-shot] [--limit N] <effects dir> [deps-dir...]");
    let one_shot = first == "--one-shot";
    if one_shot {
        first = args.next().expect("--one-shot needs an effects dir");
    }
    let mut skip = 0usize;
    let mut limit = None;
    while first == "--skip" || first == "--limit" {
        let option = first;
        let count: usize = args
            .next()
            .unwrap_or_else(|| panic!("{option} needs a count"))
            .parse()
            .unwrap_or_else(|_| panic!("{option} takes an integer"));
        if option == "--skip" {
            skip = count;
        } else {
            assert!(count > 0, "--limit takes a positive count");
            limit = Some(count);
        }
        first = args.next().expect("slice option needs an effects dir");
    }
    let directory = PathBuf::from(first);
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
    paths = paths.into_iter().skip(skip).collect();
    if let Some(limit) = limit {
        paths.truncate(limit);
    }
    eprintln!(
        "discovering {} plug-in(s) via the in-place {} pipeline",
        paths.len(),
        if one_shot { "one-shot" } else { "clustered" },
    );
    let started = Instant::now();
    let mut results = if one_shot {
        discover_all_one_shot_for_diagnostics(&repository, &paths, dependency_dirs)
            .into_iter()
            .map(|(path, ok, classification)| (path, ok, classification, None))
            .collect::<Vec<_>>()
    } else {
        discover_records_for_diagnostics(&repository, &paths, dependency_dirs)
            .into_iter()
            .map(|record| {
                (
                    record.path,
                    record.ok,
                    record.failure_classification,
                    record.cluster_fallback,
                )
            })
            .collect::<Vec<_>>()
    };
    let elapsed = started.elapsed();
    results.sort_by(|left, right| left.0.cmp(&right.0));
    let mut ok = 0usize;
    for (path, discovered, classification, fallback) in &results {
        if *discovered {
            ok += 1;
        }
        println!(
            "{}\t{}\t{}\t{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?"),
            if *discovered { "ok" } else { "failed" },
            classification.as_deref().unwrap_or("-"),
            fallback.as_deref().map(fallback_resolution).unwrap_or("-"),
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

#[cfg(test)]
mod tests {
    use super::fallback_resolution;

    #[test]
    fn fallback_output_does_not_expose_free_form_reason() {
        assert_eq!(
            fallback_resolution(r"open failed at C:\private\effects\foo.aex/one_shot_fallback"),
            "one_shot_fallback"
        );
        assert_eq!(fallback_resolution("unexpected local path"), "unknown");
    }
}
