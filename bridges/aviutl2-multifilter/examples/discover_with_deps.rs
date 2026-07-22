//! Loads one AEX with its dependency-DLL closure sealed into the load tree
//! (issue #304), printing the closure the broker resolved and the discovery
//! outcome. Single-plug-in counterpart to `discover_sweep`.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discover_with_deps -- "<effect.aex>" "<support-files-dir>"

use std::path::{Path, PathBuf};

use aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_diagnostics;
use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, resolve_dependency_closure,
};
use sha2::{Digest, Sha256};

fn main() {
    let mut args = std::env::args().skip(1);
    let effect = PathBuf::from(
        args.next()
            .expect("usage: discover_with_deps <effect.aex> <support-dir>"),
    );
    let support = PathBuf::from(args.next().expect("need the AE Support Files dir"));
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_MULTIFILTER_REPOSITORY")
            .expect("set AEXCOMPAT_MULTIFILTER_REPOSITORY"),
    );

    let mut roots: Vec<PathBuf> = effect.parent().map(Path::to_path_buf).into_iter().collect();
    if !roots.contains(&support) {
        roots.push(support);
    }
    let closure = resolve_dependency_closure(DependencyClosureRequest::new(&effect, &roots))
        .expect("resolve the dependency closure");
    eprintln!(
        "dependency closure ({} DLLs, {} bytes) for {:?}:",
        closure.dependencies().len(),
        closure.total_bytes(),
        effect.file_name().unwrap()
    );
    for dependency in closure.dependencies() {
        eprintln!(
            "  {}",
            dependency.path.file_name().unwrap().to_string_lossy()
        );
    }
    if !closure.unresolved().is_empty() {
        eprintln!("unresolved imports (left to System32 / api sets):");
        for name in closure.unresolved() {
            eprintln!("  {name}");
        }
    }

    let bytes = std::fs::read(&effect).expect("read effect");
    let sha = format!("{:x}", Sha256::digest(&bytes));
    eprintln!(
        "\ndiscovering with {} sealed dependencies...",
        closure.dependencies().len()
    );
    match inspect_experimental_with_approved_dependencies_and_diagnostics(
        &repository,
        &effect,
        &sha,
        closure.into_dependencies(),
    ) {
        Ok((parameters, _)) => println!(
            "\n*** LOADED: {} parameters discovered ***",
            parameters.len()
        ),
        Err(error) => {
            let detail = format!("{error}");
            println!("\nFAILED: {}", &detail[..detail.len().min(700)]);
        }
    }
}
