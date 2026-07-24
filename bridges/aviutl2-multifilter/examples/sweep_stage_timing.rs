//! Breaks down where the per-plug-in discovery staging time goes (issue #381):
//! closure resolution, reading + hashing the plug-in binary itself, and
//! sealed-load-tree staging (hard-link/copy plus the verification re-hash),
//! each timed separately. Read-only for the plug-in: it never dispatches a
//! worker.
//!
//! Output is a single machine-readable JSON document on stdout carrying the
//! input identity (basename, canonical path, size, SHA-256), the per-stage
//! timings, and the staging statistics (hard-link vs copy counts, stale
//! cleanup result, manifest digest), usable as comparison evidence for
//! #381/#399. Every fallible step fails closed: it prints a JSON object with
//! "failure_stage" and the error string to stdout and exits non-zero, so a
//! failed measurement is never mistaken for a successful one.
//!
//! Run:
//!   cargo run --release --example sweep_stage_timing -- "<effect.aex>" "<support-files-dir>"

use std::path::PathBuf;
use std::time::Instant;

use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, resolve_dependency_closure,
};
use aexcompat_broker::sealed_load_tree::{LoadEntry, SealedLoadTree};
use serde_json::json;
use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn fail(stage: &str, error: impl std::fmt::Display) -> ! {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "failure_stage": stage,
            "error": error.to_string(),
        }))
        .expect("failure JSON must serialize")
    );
    std::process::exit(1);
}

fn main() {
    let usage = "usage: sweep_stage_timing <effect.aex> <support-files-dir>";
    let mut args = std::env::args().skip(1);
    let Some(plugin) = args.next().map(PathBuf::from) else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    let Some(support) = args.next().map(PathBuf::from) else {
        eprintln!("{usage}");
        std::process::exit(2);
    };

    let plugin = std::fs::canonicalize(&plugin).unwrap_or_else(|e| fail("plugin_read", e));
    let name = plugin
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| fail("plugin_basename", "plugin path has no basename"));

    // The plug-in's parent is already canonical because `plugin` is. The
    // support dir must canonicalize too: dropping it silently would shrink
    // the search roots and measure an incomplete dependency closure.
    let mut roots: Vec<PathBuf> = plugin.parent().map(PathBuf::from).into_iter().collect();
    roots.push(std::fs::canonicalize(&support).unwrap_or_else(|e| fail("support_path", e)));

    let t = Instant::now();
    let closure = resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots))
        .unwrap_or_else(|e| fail("closure_resolve", e));
    let resolve = t.elapsed();

    // Measures read + SHA-256 of the plug-in binary itself, not pure hashing
    // time: the same bytes are also read inside closure resolution above, and
    // the digest computed here is reused for both the input identity and the
    // main LoadEntry's expected hash.
    let t = Instant::now();
    let bytes = std::fs::read(&plugin).unwrap_or_else(|e| fail("plugin_read", e));
    let sha: [u8; 32] = Sha256::digest(&bytes).into();
    let plugin_read_hash = t.elapsed();

    let main_entry = LoadEntry {
        source: plugin.clone(),
        relative_basename: name.clone(),
        expected_sha256: sha,
        expected_size: bytes.len() as u64,
    };
    let deps: Vec<LoadEntry> = closure
        .dependencies()
        .iter()
        .map(|artifact| LoadEntry {
            source: artifact.path.clone(),
            relative_basename: artifact
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| {
                    fail(
                        "closure_resolve",
                        format!("dependency {:?} has no basename", artifact.path),
                    )
                }),
            expected_sha256: artifact.expected_sha256,
            expected_size: artifact.expected_size,
        })
        .collect();

    let t = Instant::now();
    let (tree, stats) = SealedLoadTree::create_with_stats(main_entry, deps)
        .unwrap_or_else(|e| fail("sealed_stage", e));
    let stage = t.elapsed();
    let manifest_digest = tree.manifest_digest();
    let manifest_entries = tree.manifest_basenames().len();
    drop(tree);

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "input": {
                "name": name,
                "path": plugin.to_string_lossy(),
                "size": bytes.len() as u64,
                "sha256": hex(&sha),
            },
            "resolve": {
                "elapsed_ms": resolve.as_millis(),
                "dependencies": closure.dependencies().len(),
                "total_bytes": closure.total_bytes(),
                "unresolved": closure.unresolved(),
            },
            "plugin_read_hash": {
                "elapsed_ms": plugin_read_hash.as_millis(),
            },
            "stage": {
                "elapsed_ms": stage.as_millis(),
                "hard_linked": stats.hard_linked,
                "copied": stats.copied,
                "stale_cleanup_ok": stats.stale_cleanup_ok,
                "manifest_digest": hex(&manifest_digest),
                "manifest_entries": manifest_entries,
            },
        }))
        .expect("timing JSON must serialize")
    );
}
