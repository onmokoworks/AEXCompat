use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub struct ApprovalPolicy {
    pub allowlist_path: &'static str,
    pub stage: &'static str,
    pub receipt_id: &'static str,
    pub expires: &'static str,
    pub max_timeout_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    schema_version: u32,
    entries: Vec<ApprovedArtifact>,
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
    approved_stage: String,
    receipt_id: String,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedArtifact {
    pub id: String,
    pub plugin_path: PathBuf,
    pub sha256: String,
    pub byte_size: u64,
    approved_stage: String,
    pub receipt_id: String,
    expires: String,
    pub timeout_ms: u64,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub fn load(repository: &Path, id: &str, policy: ApprovalPolicy) -> io::Result<ApprovedArtifact> {
    let bytes = fs::read(repository.join(policy.allowlist_path))?;
    let list: Allowlist =
        serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    if list.schema_version != 1 || list.entries.len() != 1 {
        return Err(invalid(
            "allowlist must contain exactly one schema-v1 entry",
        ));
    }
    let entry = list.entries.into_iter().next().unwrap();
    if entry.id != id
        || entry.approved_stage != policy.stage
        || entry.receipt_id != policy.receipt_id
        || entry.expires != policy.expires
    {
        return Err(invalid(
            "approval identity, stage, receipt, or expiry mismatch",
        ));
    }
    if entry.timeout_ms == 0
        || entry.timeout_ms > policy.max_timeout_ms
        || entry.sha256.len() != 64
        || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(invalid("approval digest or resource limit invalid"));
    }
    let metadata = fs::metadata(&entry.plugin_path)?;
    if !metadata.is_file() || metadata.len() != entry.byte_size {
        return Err(invalid("approved artifact size mismatch"));
    }
    Ok(entry)
}

/// Parses an approved schema-v2 receipt into inputs for `SealedLoadTree`.
///
/// This only authenticates receipt structure and policy fields. `SealedLoadTree::create`
/// remains responsible for opening and authenticating the source files.
pub fn load_v2_load_tree(
    repository: &Path,
    id: &str,
    policy: ApprovalPolicy,
) -> io::Result<ApprovedLoadTree> {
    let bytes = fs::read(repository.join(policy.allowlist_path))?;
    let list: AllowlistV2 =
        serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    if list.schema_version != 2 || list.entries.len() != 1 {
        return Err(invalid(
            "allowlist must contain exactly one schema-v2 entry",
        ));
    }
    let entry = list.entries.into_iter().next().unwrap();
    if entry.id != id
        || entry.approved_stage != policy.stage
        || entry.receipt_id != policy.receipt_id
        || entry.expires != policy.expires
    {
        return Err(invalid(
            "approval identity, stage, receipt, or expiry mismatch",
        ));
    }
    if entry.timeout_ms == 0 || entry.timeout_ms > policy.max_timeout_ms {
        return Err(invalid("approval resource limit invalid"));
    }
    if entry.trusted_worker.byte_size == 0 {
        return Err(invalid("trusted worker byte size must be nonzero"));
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
    let worker_sha256 = decode_sha256(&entry.trusted_worker.sha256)?;
    if entry.load_tree.dependencies.len() > MAX_LOAD_TREE_DEPENDENCIES {
        return Err(invalid("load tree dependency limit exceeded"));
    }

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
        timeout_ms: entry.timeout_ms,
        worker_path: entry.trusted_worker.path,
        worker_sha256,
        worker_byte_size: entry.trusted_worker.byte_size,
    })
}

fn convert_load_entry(
    receipt: LoadEntryReceipt,
    names: &mut HashSet<String>,
) -> io::Result<crate::sealed_load_tree::LoadEntry> {
    if receipt.byte_size == 0 {
        return Err(invalid("load entry byte size must be nonzero"));
    }
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
    let expected_sha256 = decode_sha256(&receipt.sha256)?;
    Ok(crate::sealed_load_tree::LoadEntry {
        source: receipt.source_path,
        relative_basename: receipt.basename,
        expected_sha256,
        expected_size: receipt.byte_size,
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

fn decode_sha256(value: &str) -> io::Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("load entry SHA-256 must be 32 hex-encoded bytes"));
    }
    let mut digest = [0u8; 32];
    for (output, pair) in digest.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).map_err(|_| invalid("invalid SHA-256 encoding"))?;
        *output = u8::from_str_radix(pair, 16).map_err(|_| invalid("invalid SHA-256 encoding"))?;
    }
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn v2_policy() -> ApprovalPolicy {
        ApprovalPolicy {
            allowlist_path: "allowlist.json",
            stage: "render",
            receipt_id: "reviewed-002",
            expires: "2099-01-01T00:00:00Z",
            max_timeout_ms: 5_000,
        }
    }

    fn v2_allowlist(
        main: serde_json::Value,
        dependencies: Vec<serde_json::Value>,
    ) -> serde_json::Value {
        json!({"schema_version":2,"entries":[{
            "id":"fixture","approved_stage":"render","receipt_id":"reviewed-002",
            "expires":"2099-01-01T00:00:00Z","timeout_ms":5000,
            "trusted_worker":{"path":"C:/approved/worker.exe","sha256":"0b".repeat(32),"byte_size":9},
            "load_tree":{"main":main,"dependencies":dependencies}
        }]})
    }

    fn receipt(path: &str, basename: &str) -> serde_json::Value {
        json!({
            "source_path":path,"basename":basename,"sha256":"0a".repeat(32),"byte_size":7
        })
    }

    fn parse_v2(value: serde_json::Value) -> io::Result<ApprovedLoadTree> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-approved-artifact-v2-{}-{nonce}-{}",
            std::process::id(),
            TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("allowlist.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let result = load_v2_load_tree(&root, "fixture", v2_policy());
        fs::remove_dir_all(root).unwrap();
        result
    }

    #[test]
    fn policy_is_target_independent() {
        let policy = ApprovalPolicy {
            allowlist_path: "target/example/active.local.json",
            stage: "render",
            receipt_id: "reviewed-001",
            expires: "2099-01-01T00:00:00Z",
            max_timeout_ms: 5_000,
        };
        assert_eq!(policy.stage, "render");
        assert_eq!(policy.max_timeout_ms, 5_000);
    }

    #[test]
    fn loader_accepts_only_the_reviewed_identity_and_limits() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-approved-artifact-{}-{nonce}-{}",
            std::process::id(),
            TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let plugin = root.join("fixture.bin");
        fs::write(&plugin, b"fixture").unwrap();
        let policy = ApprovalPolicy {
            allowlist_path: "allowlist.json",
            stage: "render",
            receipt_id: "reviewed-001",
            expires: "2099-01-01T00:00:00Z",
            max_timeout_ms: 5_000,
        };
        let write_allowlist = |receipt: &str, timeout_ms: u64| {
            fs::write(
                root.join("allowlist.json"),
                serde_json::to_vec(&json!({"schema_version":1,"entries":[{
                    "id":"fixture","plugin_path":plugin,"sha256":"A".repeat(64),
                    "byte_size":7,"approved_stage":"render","receipt_id":receipt,
                    "expires":"2099-01-01T00:00:00Z","timeout_ms":timeout_ms
                }]}))
                .unwrap(),
            )
            .unwrap();
        };

        write_allowlist("reviewed-001", 5_000);
        assert_eq!(load(&root, "fixture", policy).unwrap().byte_size, 7);
        write_allowlist("tampered", 5_000);
        assert!(load(&root, "fixture", policy).is_err());
        write_allowlist("reviewed-001", 5_001);
        assert!(load(&root, "fixture", policy).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v2_converts_strict_load_tree_receipt() {
        let approved = parse_v2(v2_allowlist(
            receipt("C:/approved/main.plugin", "main.plugin"),
            vec![receipt("C:/approved/helper.dll", "helper.dll")],
        ))
        .unwrap();

        assert_eq!(
            approved.main.source,
            PathBuf::from("C:/approved/main.plugin")
        );
        assert_eq!(approved.main.relative_basename, "main.plugin");
        assert_eq!(approved.main.expected_sha256, [0x0a; 32]);
        assert_eq!(approved.main.expected_size, 7);
        assert_eq!(approved.dependencies.len(), 1);
        assert_eq!(approved.dependencies[0].relative_basename, "helper.dll");
        assert_eq!(approved.timeout_ms, 5_000);
        assert_eq!(approved.worker_sha256, [0x0b; 32]);
        assert_eq!(approved.worker_byte_size, 9);
    }

    #[test]
    fn v2_rejects_unknown_fields_and_dependency_overflow() {
        let mut main = receipt("C:/approved/main.plugin", "main.plugin");
        main.as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        assert!(parse_v2(v2_allowlist(main, vec![])).is_err());

        let dependencies = (0..=MAX_LOAD_TREE_DEPENDENCIES)
            .map(|index| {
                receipt(
                    &format!("C:/approved/d{index}.dll"),
                    &format!("d{index}.dll"),
                )
            })
            .collect();
        assert!(parse_v2(v2_allowlist(
            receipt("C:/approved/main.plugin", "main.plugin"),
            dependencies,
        ))
        .is_err());
    }

    #[test]
    fn v2_rejects_unsafe_or_ambiguous_entries() {
        let invalid_entries = [
            receipt("C:/approved/sub/main.plugin", "sub/main.plugin"),
            receipt("C:/approved/sub\\main.plugin", "sub\\main.plugin"),
            json!({"source_path":"C:/approved/main.plugin","basename":"main.plugin","sha256":"gg".repeat(32),"byte_size":7}),
            json!({"source_path":"C:/approved/main.plugin","basename":"main.plugin","sha256":"00".repeat(32),"byte_size":0}),
            receipt("C:/approved/different.plugin", "main.plugin"),
        ];
        for invalid_main in invalid_entries {
            assert!(parse_v2(v2_allowlist(invalid_main, vec![])).is_err());
        }

        assert!(parse_v2(v2_allowlist(
            receipt("C:/approved/Main.plugin", "Main.plugin"),
            vec![receipt("C:/approved/main.plugin", "main.plugin")],
        ))
        .is_err());
    }

    #[test]
    fn v2_rejects_windows_unsafe_basenames_fail_closed() {
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
            let source = format!("C:/approved/{basename}");
            assert!(
                parse_v2(v2_allowlist(receipt(&source, basename), vec![])).is_err(),
                "v2 accepted unsafe basename {basename:?}"
            );
        }
    }

    #[test]
    fn v2_allows_names_that_only_resemble_dos_devices() {
        for basename in ["COM0.dll", "COM10.dll", "LPT0.plugin", "NULsafe.txt"] {
            let source = format!("C:/approved/{basename}");
            assert!(
                parse_v2(v2_allowlist(receipt(&source, basename), vec![])).is_ok(),
                "v2 rejected safe basename {basename:?}"
            );
        }
    }
}
