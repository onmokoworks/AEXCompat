use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::io;
use std::path::{Component, Path};

const MAX_AUDITED_MODULES: usize = 128;
const MIN_REQUIRED_PHASES: u32 = 3;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditReport {
    schema: u32,
    status: String,
    post_load: AuditSnapshot,
    pre_unload: AuditSnapshot,
    observed_union: AuditSnapshot,
    phase_count: u32,
    unknown_count: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditSnapshot {
    status: String,
    unknown_count: u32,
    worker: Vec<String>,
    plugin: Vec<String>,
    system32: Vec<String>,
}

pub fn validate_required_worker_audit(stdout: &str, stdout_truncated: bool) -> io::Result<()> {
    if stdout_truncated {
        return Err(invalid("secure worker report was truncated"));
    }
    let report: Value = serde_json::from_str(stdout.trim())
        .map_err(|_| invalid("secure worker report is not valid JSON"))?;
    let audit: AuditReport = serde_json::from_value(
        report
            .get("module_audit")
            .cloned()
            .ok_or_else(|| invalid("secure worker module audit is missing"))?,
    )
    .map_err(|_| invalid("secure worker module audit schema is invalid"))?;

    if audit.schema != 1 || audit.status != "passed" || audit.unknown_count != 0 {
        return Err(invalid("secure worker module audit did not pass"));
    }
    if audit.phase_count < MIN_REQUIRED_PHASES {
        return Err(invalid("secure worker module audit is incomplete"));
    }
    validate_snapshot(&audit.post_load, "post-load")?;
    validate_snapshot(&audit.pre_unload, "pre-unload")?;
    validate_snapshot(&audit.observed_union, "observed union")?;
    if audit.observed_union.worker.is_empty() || audit.observed_union.plugin.is_empty() {
        return Err(invalid("secure worker module audit lacks required images"));
    }
    require_subset(&audit.post_load.worker, &audit.observed_union.worker)?;
    require_subset(&audit.post_load.plugin, &audit.observed_union.plugin)?;
    require_subset(&audit.post_load.system32, &audit.observed_union.system32)?;
    require_subset(&audit.pre_unload.worker, &audit.observed_union.worker)?;
    require_subset(&audit.pre_unload.plugin, &audit.observed_union.plugin)?;
    require_subset(&audit.pre_unload.system32, &audit.observed_union.system32)?;
    Ok(())
}

fn validate_snapshot(snapshot: &AuditSnapshot, label: &str) -> io::Result<()> {
    if snapshot.status != "passed" || snapshot.unknown_count != 0 {
        return Err(invalid(format!(
            "secure worker {label} module audit failed"
        )));
    }
    let count = snapshot.worker.len() + snapshot.plugin.len() + snapshot.system32.len();
    if count > MAX_AUDITED_MODULES {
        return Err(invalid("secure worker module audit limit exceeded"));
    }
    let mut names = HashSet::new();
    for name in snapshot
        .worker
        .iter()
        .chain(&snapshot.plugin)
        .chain(&snapshot.system32)
    {
        validate_basename(name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(invalid("secure worker module audit contains duplicates"));
        }
    }
    Ok(())
}

fn validate_basename(name: &str) -> io::Result<()> {
    let mut parts = Path::new(name).components();
    if name.is_empty()
        || name.len() > 260
        || !matches!(parts.next(), Some(Component::Normal(_)))
        || parts.next().is_some()
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || name.ends_with(['.', ' '])
    {
        return Err(invalid(
            "secure worker module audit contains an unsafe name",
        ));
    }
    Ok(())
}

fn require_subset(values: &[String], union: &[String]) -> io::Result<()> {
    let union: HashSet<_> = union
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect();
    if values
        .iter()
        .any(|value| !union.contains(&value.to_ascii_lowercase()))
    {
        return Err(invalid("secure worker observed union is inconsistent"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_report() -> String {
        json!({
            "status": "rendered",
            "module_audit": {
                "schema": 1,
                "status": "passed",
                "post_load": snapshot(),
                "pre_unload": snapshot(),
                "observed_union": snapshot(),
                "phase_count": 7,
                "unknown_count": 0
            }
        })
        .to_string()
    }

    fn snapshot() -> Value {
        json!({
            "status": "passed",
            "unknown_count": 0,
            "worker": ["trusted-worker.exe"],
            "plugin": ["fixture.plugin"],
            "system32": ["kernel32.dll"]
        })
    }

    #[test]
    fn accepts_complete_cumulative_audit() {
        validate_required_worker_audit(&valid_report(), false).unwrap();
    }

    #[test]
    fn rejects_missing_failed_truncated_and_incomplete_audits() {
        assert!(validate_required_worker_audit("{}", false).is_err());
        assert!(validate_required_worker_audit(&valid_report(), true).is_err());
        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        report["module_audit"]["observed_union"]["unknown_count"] = json!(1);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());
        report["module_audit"]["observed_union"]["unknown_count"] = json!(0);
        report["module_audit"]["phase_count"] = json!(0);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());
    }

    #[test]
    fn rejects_unsafe_names_duplicates_and_inconsistent_union() {
        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        report["module_audit"]["post_load"]["plugin"] = json!(["../fixture.plugin"]);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());

        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        report["module_audit"]["post_load"]["system32"] = json!(["kernel32.dll", "KERNEL32.DLL"]);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());

        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        report["module_audit"]["post_load"]["plugin"] = json!(["other.plugin"]);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());
    }
}
