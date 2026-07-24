//! Diagnostic: open one cluster DiscoverySession over two plug-ins and print
//! the full close report (worker classification, exit code, stdout final
//! report, stderr), so a worker that dies before the first inspect_plugin
//! can be attributed (issue #405 real-environment failure).
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example cluster_diag -- <plugin1> <plugin2> [deps-dir]

use std::path::PathBuf;

use aexcompat_broker::plugin_dependency_closure::{
    DependencyClosureRequest, resolve_dependency_closure,
};
use aexcompat_broker::render_session::{
    DiscoverySession, DiscoverySessionOpenRequest, InspectOutcome,
};
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use sha2::{Digest, Sha256};

fn artifact(path: &std::path::Path) -> ApprovedImageArtifact {
    let bytes = std::fs::read(path).expect("read plugin");
    ApprovedImageArtifact {
        path: path.to_path_buf(),
        expected_sha256: Sha256::digest(&bytes).into(),
        expected_size: bytes.len() as u64,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let plugin1 = PathBuf::from(args.next().expect("usage: cluster_diag <p1> <p2> [deps-dir]"));
    let plugin2 = PathBuf::from(args.next().expect("usage: cluster_diag <p1> <p2> [deps-dir]"));
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_MULTIFILTER_REPOSITORY")
            .expect("set AEXCOMPAT_MULTIFILTER_REPOSITORY"),
    );
    let mut roots: Vec<PathBuf> = plugin1
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .into_iter()
        .collect();
    roots.extend(args.map(|dir| {
        std::fs::canonicalize(&dir).unwrap_or_else(|_| PathBuf::from(dir))
    }));

    let closure = resolve_dependency_closure(DependencyClosureRequest::new(&plugin1, &roots))
        .expect("resolve closure for plugin1");
    eprintln!(
        "closure: {} deps, {} unresolved",
        closure.dependencies().len(),
        closure.unresolved().len()
    );
    let dependency_count = closure.dependencies().len();
    let declared = 2 + dependency_count;
    let module_bound = (declared + 256) as u32;

    let mut session = DiscoverySession::open(DiscoverySessionOpenRequest {
        repository: &repository,
        plugins: vec![artifact(&plugin1), artifact(&plugin2)],
        dependencies: closure.into_dependencies(),
        module_bound,
        inspect_deadline: std::time::Duration::from_secs(300),
    })
    .expect("open discovery session");
    eprintln!("session opened (module_bound {module_bound})");

    for index in 0..2u32 {
        match session.inspect_plugin(index, index) {
            Ok(InspectOutcome::Inspected { report }) => {
                let params = report
                    .get("parameters")
                    .and_then(|value| value.as_array())
                    .map_or(0, |rows| rows.len());
                eprintln!("inspect {index}: ok ({params} params)");
            }
            Ok(InspectOutcome::InspectError { error_kind, .. }) => {
                eprintln!("inspect {index}: plugin-local error: {error_kind}");
            }
            Err(error) => {
                eprintln!("inspect {index}: SESSION INVALIDATED: {error}");
                break;
            }
        }
    }
    let close = session.close();
    println!("{}", serde_json::to_string_pretty(&close).unwrap());
}
