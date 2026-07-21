//! Experiment (issue #304): try to load a real AE effect by sealing its
//! dependency-DLL closure into the sealed load tree, resolving imports against
//! the AE Support Files folder.
//!
//! Run:
//!   set AEXCOMPAT_MULTIFILTER_REPOSITORY=C:\path\to\AEXCompat
//!   cargo run --release --example discover_with_deps -- "<effect.aex>" "<support-files-dir>"

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_diagnostics;
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use object::LittleEndian;
use object::read::pe::PeFile64;
use sha2::{Digest, Sha256};

/// The DLL names a PE imports.
fn imports(path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let Ok(pe) = PeFile64::parse(&*bytes) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    if let Ok(Some(table)) = pe.import_table() {
        if let Ok(mut descs) = table.descriptors() {
            while let Ok(Some(desc)) = descs.next() {
                if let Ok(name) = table.name(desc.name.get(LittleEndian)) {
                    names.push(String::from_utf8_lossy(name).to_string());
                }
            }
        }
    }
    names
}

/// The recursive closure of `effect`'s imports that resolve to files in
/// `support` (the AE Support Files folder). System DLLs (kernel32, etc.) are not
/// there and are skipped (resolved from System32 at load).
fn dependency_closure(effect: &Path, support: &Path) -> Vec<PathBuf> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut closure: Vec<PathBuf> = Vec::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(effect.to_path_buf());
    while let Some(current) = queue.pop_front() {
        for name in imports(&current) {
            let folded = name.to_lowercase();
            if !seen.insert(folded) {
                continue;
            }
            let candidate = support.join(&name);
            if candidate.is_file() {
                closure.push(candidate.clone());
                queue.push_back(candidate);
            }
        }
    }
    closure
}

fn artifact(path: &Path) -> Option<ApprovedImageArtifact> {
    let bytes = std::fs::read(path).ok()?;
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&Sha256::digest(&bytes));
    Some(ApprovedImageArtifact {
        path: path.to_path_buf(),
        expected_sha256: sha,
        expected_size: bytes.len() as u64,
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let effect = PathBuf::from(args.next().expect("usage: discover_with_deps <effect.aex> <support-dir>"));
    let support = PathBuf::from(args.next().expect("need the AE Support Files dir"));
    let repository = PathBuf::from(
        std::env::var_os("AEXCOMPAT_MULTIFILTER_REPOSITORY").expect("set AEXCOMPAT_MULTIFILTER_REPOSITORY"),
    );

    let closure = dependency_closure(&effect, &support);
    eprintln!("dependency closure ({} DLLs) for {:?}:", closure.len(), effect.file_name().unwrap());
    for dep in &closure {
        eprintln!("  {}", dep.file_name().unwrap().to_string_lossy());
    }

    let deps: Vec<ApprovedImageArtifact> = closure.iter().filter_map(|p| artifact(p)).collect();
    let bytes = std::fs::read(&effect).expect("read effect");
    let sha = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();

    eprintln!("\ndiscovering with {} sealed dependencies...", deps.len());
    match inspect_experimental_with_approved_dependencies_and_diagnostics(&repository, &effect, &sha, deps) {
        Ok((params, _)) => println!("\n*** LOADED: {} parameters discovered ***", params.len()),
        Err(error) => {
            let detail = format!("{error}");
            println!("\nFAILED: {}", &detail[..detail.len().min(700)]);
        }
    }
}
