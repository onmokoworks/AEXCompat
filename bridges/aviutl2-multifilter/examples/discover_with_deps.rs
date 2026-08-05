//! Loads one AEX with its dependency-DLL closure sealed into the load tree
//! (issue #304), printing the closure the broker resolved and the discovery
//! outcome. Also seals the extra authenticated inputs (issue #362): BIB.dll
//! for closures that never link it statically, and `Film Stocks`-type data
//! resources staged into `<sealed root>/<subdir>/`.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discover_with_deps -- "<effect.aex>" "<support-files-dir>"

use std::path::PathBuf;

use aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_resources;
use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, resolve_dependency_closure,
};
use aexcompat_broker::sealed_load_tree::SealedResourceEntry;
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
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

    // Extra sealed inputs (issue #362): BIB.dll when the closure never links
    // it, and `Film Stocks` data files beside the plug-in.
    let mut dependencies = closure.into_dependencies();
    let has_bib = dependencies.iter().any(|dependency| {
        dependency
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("bib.dll"))
    });
    if !has_bib {
        let mut candidates: Vec<PathBuf> = effect
            .parent()
            .map(|parent| parent.join("BIB.dll"))
            .into_iter()
            .collect();
        candidates.extend(roots.iter().map(|root| root.join("BIB.dll")));
        if let Some(bib) = candidates.into_iter().find(|path| path.is_file()) {
            let bytes = std::fs::read(&bib).expect("read BIB.dll");
            eprintln!("extra sealed dependency: BIB.dll ({})", bib.display());
            dependencies.push(ApprovedImageArtifact {
                path: bib,
                expected_sha256: Sha256::digest(&bytes).into(),
                expected_size: bytes.len() as u64,
            });
        }
    }
    let mut resources = Vec::new();
    let film_stocks = effect
        .parent()
        .map(|parent| parent.join("Film Stocks"))
        .filter(|dir| dir.is_dir());
    if let Some(dir) = film_stocks {
        for entry in std::fs::read_dir(&dir).expect("read Film Stocks") {
            let path = entry.expect("Film Stocks entry").path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = std::fs::read(&path).expect("read grain file");
            resources.push(SealedResourceEntry {
                source: path,
                relative_path: format!("Film Stocks/{name}"),
                expected_sha256: Sha256::digest(&bytes).into(),
                expected_size: bytes.len() as u64,
            });
        }
        if !resources.is_empty() {
            eprintln!(
                "sealed data resources: {} files under Film Stocks/",
                resources.len()
            );
        }
    }

    let bytes = std::fs::read(&effect).expect("read effect");
    let sha = format!("{:x}", Sha256::digest(&bytes));
    eprintln!(
        "\ndiscovering with {} sealed dependencies, {} data resources...",
        dependencies.len(),
        resources.len()
    );
    match inspect_experimental_with_approved_dependencies_and_resources(
        &repository,
        &effect,
        &sha,
        dependencies,
        resources,
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
