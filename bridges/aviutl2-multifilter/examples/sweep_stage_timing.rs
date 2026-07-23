//! Breaks down where the per-plug-in discovery staging time goes (issue #381):
//! closure resolution, the plug-in's own approval hash, and sealed-load-tree
//! staging (hard-link/copy plus the verification re-hash), each timed
//! separately. Read-only for the plug-in: it never dispatches a worker.
//!
//! Run:
//!   cargo run --release --example sweep_stage_timing -- "<effect.aex>" "<support-files-dir>"

use std::path::PathBuf;
use std::time::Instant;

use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, resolve_dependency_closure,
};
use aexcompat_broker::sealed_load_tree::{LoadEntry, SealedLoadTree};
use sha2::{Digest, Sha256};

fn main() {
    let mut args = std::env::args().skip(1);
    let plugin = PathBuf::from(
        args.next()
            .expect("usage: stage_timing <aex> <support-dir>"),
    );
    let support = PathBuf::from(args.next().expect("need support dir"));

    let mut roots: Vec<PathBuf> = plugin
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .into_iter()
        .collect();
    if let Ok(s) = std::fs::canonicalize(&support) {
        roots.push(s);
    }

    let t = Instant::now();
    let closure =
        resolve_dependency_closure(DependencyClosureRequest::new(&plugin, &roots)).unwrap();
    let resolve = t.elapsed();

    let t = Instant::now();
    let bytes = std::fs::read(&plugin).unwrap();
    let sha: [u8; 32] = Sha256::digest(&bytes).into();
    let plugin_hash = t.elapsed();

    let main_entry = LoadEntry {
        source: plugin.clone(),
        relative_basename: plugin.file_name().unwrap().to_string_lossy().into_owned(),
        expected_sha256: sha,
        expected_size: bytes.len() as u64,
    };
    let deps: Vec<LoadEntry> = closure
        .dependencies()
        .iter()
        .map(|a| LoadEntry {
            source: a.path.clone(),
            relative_basename: a.path.file_name().unwrap().to_string_lossy().into_owned(),
            expected_sha256: a.expected_sha256,
            expected_size: a.expected_size,
        })
        .collect();

    let t = Instant::now();
    let tree = SealedLoadTree::create(main_entry, deps).unwrap();
    let stage = t.elapsed();
    drop(tree);

    println!(
        "resolve={:.2}s plugin_hash={:.3}s stage={:.2}s deps={} bytes={:.1}MB",
        resolve.as_secs_f64(),
        plugin_hash.as_secs_f64(),
        stage.as_secs_f64(),
        closure.dependencies().len(),
        closure.total_bytes() as f64 / 1e6,
    );
}
