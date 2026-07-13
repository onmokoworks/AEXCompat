use serde::Deserialize;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            "aexcompat-approved-artifact-{}-{nonce}",
            std::process::id()
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
}
