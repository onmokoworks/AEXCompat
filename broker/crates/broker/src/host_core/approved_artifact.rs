//! Reads the file that selects which plug-in a fixture route loads.
//!
//! This was an approval-receipt gate: the file's `approved_stage`,
//! `receipt_id`, and `expires` had to equal compiled-in constants, its
//! `timeout_ms` had to sit under a policy ceiling, and its recorded
//! `sha256`/`byte_size` had to match the file on disk or the route refused to
//! run. Issue #732 (from the #678 decision) removed that ceremony: the
//! recorded identity is provenance to compare against, not a precondition to
//! enforce, and `expires` never expired anything because it was a string
//! equality against a constant. What remains is a selection file that names
//! the plug-in, its dependencies, the worker, and a timeout; the bytes that
//! actually load are hashed here and that observed identity is what travels
//! onward. A recorded identity that no longer matches the file is logged and
//! the run continues.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub struct SelectionPolicy {
    pub allowlist_path: &'static str,
}

/// Upper bound on a selection file's `timeout_ms`. Transport sanity (a route
/// must not wait forever on a caller that cannot), not an approval ceiling.
const MAX_SELECTION_TIMEOUT_MS: u64 = 600_000;

/// Hashes `path`, returning the observed digest and size.
fn observe_identity(path: &Path) -> io::Result<([u8; 32], u64)> {
    let bytes = fs::read(path)?;
    Ok((Sha256::digest(&bytes).into(), bytes.len() as u64))
}

/// Warns when a selection file's recorded identity no longer describes the
/// file on disk. Recorded, never enforced (issue #732): a rebuilt fixture is
/// the ordinary cause, and the observed identity is what the report carries.
fn warn_on_recorded_identity_drift(
    label: &str,
    recorded_sha256: &str,
    recorded_size: u64,
    observed_sha256: [u8; 32],
    observed_size: u64,
) {
    let observed_hex = observed_sha256
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !recorded_sha256.eq_ignore_ascii_case(&observed_hex) || recorded_size != observed_size {
        tracing::warn!(
            entry = label,
            "the selection file's recorded identity does not describe the file on disk;              the observed identity is what ran"
        );
    }
}

pub const MAX_LOAD_TREE_DEPENDENCIES: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowlistV2 {
    schema_version: u32,
    entries: Vec<ApprovedArtifactV2>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovedArtifactV2 {
    id: String,
    // Parsed so existing selection files keep loading, and echoed into
    // reports as provenance; no longer compared against a constant (#732).
    #[allow(dead_code)]
    approved_stage: String,
    receipt_id: String,
    #[allow(dead_code)]
    expires: String,
    timeout_ms: u64,
    trusted_worker: TrustedWorkerReceipt,
    load_tree: LoadTreeReceipt,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedWorkerReceipt {
    path: PathBuf,
    sha256: String,
    byte_size: u64,
}

pub struct ApprovedLoadTree {
    pub main: crate::sealed_load_tree::LoadEntry,
    pub dependencies: Vec<crate::sealed_load_tree::LoadEntry>,
    /// Echoed from the selection file into reports as provenance for which
    /// record selected this plug-in. Not checked against anything (#732).
    pub receipt_id: String,
    pub timeout_ms: u64,
    pub worker_path: PathBuf,
    pub worker_sha256: [u8; 32],
    pub worker_byte_size: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadTreeReceipt {
    main: LoadEntryReceipt,
    dependencies: Vec<LoadEntryReceipt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadEntryReceipt {
    source_path: PathBuf,
    basename: String,
    sha256: String,
    byte_size: u64,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Parses an approved schema-v2 receipt into inputs for `SealedLoadTree`.
///
/// This only authenticates receipt structure and policy fields. `SealedLoadTree::create`
/// remains responsible for opening and authenticating the source files.
pub fn load_v2_load_tree(
    repository: &Path,
    id: &str,
    policy: SelectionPolicy,
) -> io::Result<ApprovedLoadTree> {
    let bytes = fs::read(repository.join(policy.allowlist_path))?;
    let list: AllowlistV2 =
        serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    if list.schema_version != 2 || list.entries.len() != 1 {
        return Err(invalid(
            "selection file must contain exactly one schema-v2 entry",
        ));
    }
    let entry = list.entries.into_iter().next().unwrap();
    if entry.id != id {
        return Err(invalid("selection entry does not name the requested id"));
    }
    if entry.timeout_ms == 0 || entry.timeout_ms > MAX_SELECTION_TIMEOUT_MS {
        return Err(invalid("selection timeout is outside the bounded range"));
    }
    if !entry
        .trusted_worker
        .path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        return Err(invalid("trusted worker must be an EXE"));
    }
    // The worker binary is hashed where it is, so the identity handed to the
    // launch is the one that will execute; the recorded pair only warns.
    let worker_path = if entry.trusted_worker.path.is_absolute() {
        entry.trusted_worker.path.clone()
    } else {
        repository.join(&entry.trusted_worker.path)
    };
    let (worker_sha256, worker_byte_size) = observe_identity(&worker_path)?;
    if worker_byte_size == 0 {
        return Err(invalid("trusted worker is empty"));
    }
    warn_on_recorded_identity_drift(
        "trusted_worker",
        &entry.trusted_worker.sha256,
        entry.trusted_worker.byte_size,
        worker_sha256,
        worker_byte_size,
    );
    if entry.load_tree.dependencies.len() > MAX_LOAD_TREE_DEPENDENCIES {
        return Err(invalid("load tree dependency limit exceeded"));
    }

    let receipt_id = entry.receipt_id;
    let mut names = HashSet::with_capacity(entry.load_tree.dependencies.len() + 1);
    let main = convert_load_entry(entry.load_tree.main, &mut names)?;
    let dependencies = entry
        .load_tree
        .dependencies
        .into_iter()
        .map(|dependency| convert_load_entry(dependency, &mut names))
        .collect::<io::Result<Vec<_>>>()?;
    Ok(ApprovedLoadTree {
        main,
        dependencies,
        receipt_id,
        timeout_ms: entry.timeout_ms,
        worker_path: entry.trusted_worker.path,
        worker_sha256,
        worker_byte_size,
    })
}

fn convert_load_entry(
    receipt: LoadEntryReceipt,
    names: &mut HashSet<String>,
) -> io::Result<crate::sealed_load_tree::LoadEntry> {
    validate_v2_basename(&receipt.basename)?;
    if receipt
        .source_path
        .file_name()
        .and_then(|name| name.to_str())
        != Some(receipt.basename.as_str())
    {
        return Err(invalid("source filename differs from basename"));
    }
    if !names.insert(receipt.basename.to_lowercase()) {
        return Err(invalid("duplicate or case-insensitive basename collision"));
    }
    // Hash the file that is actually about to be staged. `SealedLoadTree`
    // still verifies its copy against this identity, so staging keeps proving
    // "the bytes that ran are the bytes we hashed"; what changed is that the
    // identity comes from the file rather than from the selection record.
    let (observed_sha256, observed_size) = observe_identity(&receipt.source_path)?;
    if observed_size == 0 {
        return Err(invalid("load entry file is empty"));
    }
    warn_on_recorded_identity_drift(
        &receipt.basename,
        &receipt.sha256,
        receipt.byte_size,
        observed_sha256,
        observed_size,
    );
    Ok(crate::sealed_load_tree::LoadEntry {
        source: receipt.source_path,
        relative_basename: receipt.basename,
        expected_sha256: observed_sha256,
        expected_size: observed_size,
    })
}

fn validate_v2_basename(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || name.ends_with(['.', ' '])
        || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(invalid("load entry basename is not a safe path component"));
    }

    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || matches!(stem.as_bytes(), [b'C', b'O', b'M', b'1'..=b'9'])
        || matches!(stem.as_bytes(), [b'L', b'P', b'T', b'1'..=b'9']);
    if reserved {
        return Err(invalid("load entry basename is a reserved DOS device name"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    const MAIN_BYTES: &[u8] = b"fixture";
    const WORKER_BYTES: &[u8] = b"worker.exe";

    fn policy() -> SelectionPolicy {
        SelectionPolicy {
            allowlist_path: "allowlist.json",
        }
    }

    fn scratch(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-approved-artifact-{label}-{}-{nonce}-{}",
            std::process::id(),
            TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn v2_allowlist(
        main: serde_json::Value,
        dependencies: Vec<serde_json::Value>,
        worker: &Path,
    ) -> serde_json::Value {
        json!({"schema_version":2,"entries":[{
            "id":"fixture","approved_stage":"render","receipt_id":"reviewed-002",
            "expires":"2099-01-01T00:00:00Z","timeout_ms":5000,
            "trusted_worker":{
                "path":worker,"sha256":hex(WORKER_BYTES),"byte_size":WORKER_BYTES.len()
            },
            "load_tree":{"main":main,"dependencies":dependencies}
        }]})
    }

    /// A load-tree entry whose file exists, so the identity the loader
    /// observes is the one this records.
    fn entry(root: &Path, basename: &str) -> serde_json::Value {
        let source = root.join(basename);
        if let Some(parent) = source.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&source, MAIN_BYTES).unwrap();
        json!({
            "source_path":source,"basename":basename,
            "sha256":hex(MAIN_BYTES),"byte_size":MAIN_BYTES.len()
        })
    }

    /// The same shape without creating the file: for cases that must fail on
    /// the record itself, before any file is read.
    fn absent_entry(source: &str, basename: &str) -> serde_json::Value {
        json!({
            "source_path":source,"basename":basename,
            "sha256":hex(MAIN_BYTES),"byte_size":MAIN_BYTES.len()
        })
    }

    fn parse_v2_in(root: &Path, value: serde_json::Value) -> io::Result<ApprovedLoadTree> {
        fs::write(
            root.join("allowlist.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        load_v2_load_tree(root, "fixture", policy())
    }

    fn worker_in(root: &Path) -> PathBuf {
        let worker = root.join("worker.exe");
        fs::write(&worker, WORKER_BYTES).unwrap();
        worker
    }

    #[test]
    fn v2_converts_strict_load_tree_selection() {
        let root = scratch("v2-convert");
        let worker = worker_in(&root);
        let approved = parse_v2_in(
            &root,
            v2_allowlist(
                entry(&root, "main.plugin"),
                vec![entry(&root, "helper.dll")],
                &worker,
            ),
        )
        .unwrap();

        assert_eq!(approved.main.source, root.join("main.plugin"));
        assert_eq!(approved.main.relative_basename, "main.plugin");
        assert_eq!(
            approved.main.expected_sha256,
            Sha256::digest(MAIN_BYTES).as_slice()
        );
        assert_eq!(approved.main.expected_size, MAIN_BYTES.len() as u64);
        assert_eq!(approved.dependencies.len(), 1);
        assert_eq!(approved.dependencies[0].relative_basename, "helper.dll");
        assert_eq!(approved.receipt_id, "reviewed-002");
        assert_eq!(approved.timeout_ms, 5_000);
        assert_eq!(
            approved.worker_sha256,
            Sha256::digest(WORKER_BYTES).as_slice()
        );
        assert_eq!(approved.worker_byte_size, WORKER_BYTES.len() as u64);
        fs::remove_dir_all(root).unwrap();
    }

    /// The identity comes from the file, so a record that disagrees with it
    /// loads anyway and the staged tree is bound to what was read.
    #[test]
    fn v2_takes_the_identity_from_the_file_not_the_record() {
        let root = scratch("v2-drift");
        let worker = worker_in(&root);
        let mut main = entry(&root, "main.plugin");
        main.as_object_mut()
            .unwrap()
            .insert("sha256".into(), json!("a".repeat(64)));
        main.as_object_mut()
            .unwrap()
            .insert("byte_size".into(), json!(4096));
        let approved = parse_v2_in(&root, v2_allowlist(main, vec![], &worker)).unwrap();
        assert_eq!(
            approved.main.expected_sha256,
            Sha256::digest(MAIN_BYTES).as_slice()
        );
        assert_eq!(approved.main.expected_size, MAIN_BYTES.len() as u64);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v2_rejects_unknown_fields_and_dependency_overflow() {
        let root = scratch("v2-overflow");
        let worker = worker_in(&root);
        let mut main = entry(&root, "main.plugin");
        main.as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        assert!(parse_v2_in(&root, v2_allowlist(main, vec![], &worker)).is_err());

        let dependencies = (0..=MAX_LOAD_TREE_DEPENDENCIES)
            .map(|index| entry(&root, &format!("d{index}.dll")))
            .collect();
        assert!(
            parse_v2_in(
                &root,
                v2_allowlist(entry(&root, "main.plugin"), dependencies, &worker),
            )
            .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v2_rejects_unsafe_or_ambiguous_entries() {
        let root = scratch("v2-unsafe");
        let worker = worker_in(&root);
        let invalid_entries = [
            absent_entry("C:/approved/sub/main.plugin", "sub/main.plugin"),
            absent_entry("C:/approved/sub\\main.plugin", "sub\\main.plugin"),
            absent_entry("C:/approved/different.plugin", "main.plugin"),
        ];
        for invalid_main in invalid_entries {
            assert!(parse_v2_in(&root, v2_allowlist(invalid_main, vec![], &worker)).is_err());
        }

        // An empty file has no identity worth staging.
        let empty = root.join("empty.plugin");
        fs::write(&empty, b"").unwrap();
        assert!(
            parse_v2_in(
                &root,
                v2_allowlist(
                    json!({"source_path":empty,"basename":"empty.plugin",
                           "sha256":hex(b""),"byte_size":0}),
                    vec![],
                    &worker,
                ),
            )
            .is_err()
        );

        // Two spellings of one basename still collide.
        assert!(
            parse_v2_in(
                &root,
                v2_allowlist(
                    entry(&root, "Main.plugin"),
                    vec![entry(&root, "main.plugin")],
                    &worker,
                ),
            )
            .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v2_rejects_windows_unsafe_basenames_fail_closed() {
        let root = scratch("v2-names");
        let worker = worker_in(&root);
        let invalid_names = [
            "drive:name.dll",
            "NUL",
            "nul.txt",
            "NUL .txt",
            "CON.plugin",
            "PRN.log",
            "AUX.dll",
            "COM1.dll",
            "com9",
            "LPT1.plugin",
            "lpt9.txt",
            "trailing.",
            "trailing ",
            "control\u{0001}.dll",
            "delete\u{007f}.dll",
        ];

        for basename in invalid_names {
            let main = absent_entry(&format!("C:/approved/{basename}"), basename);
            assert!(
                parse_v2_in(&root, v2_allowlist(main, vec![], &worker)).is_err(),
                "v2 accepted unsafe basename {basename:?}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v2_allows_names_that_only_resemble_dos_devices() {
        for basename in ["COM0.dll", "COM10.dll", "LPT0.plugin", "NULsafe.txt"] {
            let root = scratch("v2-safe");
            let worker = worker_in(&root);
            assert!(
                parse_v2_in(&root, v2_allowlist(entry(&root, basename), vec![], &worker)).is_ok(),
                "v2 rejected safe basename {basename:?}"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }
}
