//! Stages an AEX plus its resolved dependency closure into one folder, the way
//! the sealed load tree lays them out (issue #304).
//!
//! This is the input for `tools/observe-staged-aex-modules.ps1`, which loads the
//! staged plug-in with the worker's search flags and reports which modules the
//! Adobe runtime pulls in from outside that folder — the modules the worker's
//! sealed-tree module audit counts as unknown (L2).
//!
//! Run:
//!   cargo run --release --example stage_closure -- "<effect.aex>" "<support-files-dir>" "<stage-dir>"

use std::path::PathBuf;

use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, resolve_dependency_closure,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let effect = PathBuf::from(
        args.next()
            .expect("usage: stage_closure <effect.aex> <support-dir> <stage-dir>"),
    );
    let support = PathBuf::from(args.next().expect("need the AE Support Files dir"));
    let stage = PathBuf::from(args.next().expect("need the staging dir"));

    // The resolver requires absolute roots, so canonicalize what the command line
    // gave us, exactly as the multi-filter does for its configured folders.
    let mut roots: Vec<PathBuf> = effect
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .into_iter()
        .collect();
    if let Ok(support) = std::fs::canonicalize(&support)
        && !roots.contains(&support)
    {
        roots.push(support);
    }
    let closure = resolve_dependency_closure(DependencyClosureRequest::new(&effect, &roots))
        .expect("resolve the dependency closure");

    // Clear the staging folder first. The observer loads from it with the
    // worker's DLL-load-directory flag, so a leftover from an earlier run would
    // satisfy an import that the real sealed tree does not contain and be
    // reported as staged — the opposite of what this tool is for. Only direct
    // children are removed, and only files, so a mistyped path cannot take a
    // directory tree with it.
    std::fs::create_dir_all(&stage).expect("create the staging dir");
    for entry in std::fs::read_dir(&stage)
        .expect("read the staging dir")
        .flatten()
    {
        if entry.file_type().is_ok_and(|kind| kind.is_file()) {
            std::fs::remove_file(entry.path()).expect("clear the staging dir");
        }
    }
    let staged_plugin = stage.join(effect.file_name().expect("effect basename"));
    std::fs::copy(&effect, &staged_plugin).expect("stage the plug-in");
    for dependency in closure.dependencies() {
        let name = dependency.path.file_name().expect("dependency basename");
        std::fs::copy(&dependency.path, stage.join(name)).expect("stage a dependency");
    }
    eprintln!(
        "staged {} dependencies ({} bytes)",
        closure.dependencies().len(),
        closure.total_bytes()
    );
    println!("{}", staged_plugin.display());
}
