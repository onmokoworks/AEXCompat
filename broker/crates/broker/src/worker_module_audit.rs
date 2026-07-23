use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::io;
use std::path::{Component, Path};

const MAX_AUDITED_MODULES: usize = 128;
const MIN_REQUIRED_PHASES: u32 = 3;
const MAX_DIAGNOSTIC_SAMPLES_PER_CATEGORY: usize = 4;

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
    #[serde(default)]
    winsxs: Vec<String>,
    #[serde(default)]
    policy: Option<Vec<String>>,
    #[serde(default)]
    unknown: Vec<String>,
}

pub fn validate_required_worker_audit(stdout: &str, stdout_truncated: bool) -> io::Result<()> {
    if stdout_truncated {
        return Err(invalid("secure worker report was truncated"));
    }
    let report: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        invalid_classified(
            "module_audit_report_invalid_json",
            "secure worker report is not valid JSON",
        )
    })?;
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
    require_subset(&audit.post_load.winsxs, &audit.observed_union.winsxs)?;
    require_subset(&audit.pre_unload.worker, &audit.observed_union.worker)?;
    require_subset(&audit.pre_unload.plugin, &audit.observed_union.plugin)?;
    require_subset(&audit.pre_unload.system32, &audit.observed_union.system32)?;
    require_subset(&audit.pre_unload.winsxs, &audit.observed_union.winsxs)?;
    require_optional_subset(&audit.post_load.policy, &audit.observed_union.policy)?;
    require_optional_subset(&audit.pre_unload.policy, &audit.observed_union.policy)?;
    require_subset(&audit.post_load.unknown, &audit.observed_union.unknown)?;
    require_subset(&audit.pre_unload.unknown, &audit.observed_union.unknown)?;
    Ok(())
}

fn validate_snapshot(snapshot: &AuditSnapshot, label: &str) -> io::Result<()> {
    if snapshot.status != "passed" || snapshot.unknown_count != 0 || !snapshot.unknown.is_empty() {
        return Err(invalid(format!(
            "secure worker {label} module audit failed"
        )));
    }
    let mut names = HashSet::new();
    for name in snapshot
        .worker
        .iter()
        .chain(&snapshot.plugin)
        .chain(&snapshot.system32)
        .chain(&snapshot.winsxs)
        .chain(snapshot.policy.iter().flatten())
        .chain(&snapshot.unknown)
    {
        validate_basename(name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(invalid("secure worker module audit contains duplicates"));
        }
    }
    let count = snapshot.worker.len()
        + snapshot.plugin.len()
        + snapshot.system32.len()
        + snapshot.winsxs.len()
        + snapshot.policy.as_ref().map_or(0, Vec::len)
        + snapshot.unknown.len();
    if count > MAX_AUDITED_MODULES {
        return Err(module_audit_limit_error(snapshot, label, count));
    }
    Ok(())
}

fn module_audit_limit_error(snapshot: &AuditSnapshot, label: &str, total: usize) -> io::Error {
    let samples = |values: &[String]| {
        values
            .iter()
            .take(MAX_DIAGNOSTIC_SAMPLES_PER_CATEGORY)
            .cloned()
            .collect::<Vec<_>>()
    };
    invalid_diagnostics(serde_json::json!({
        "classification": "module_audit_limit_exceeded",
        "failure_stage": "module_audit_validation",
        "reason": format!("secure worker {label} module audit limit exceeded"),
        "limit": MAX_AUDITED_MODULES,
        "total": total,
        "category_counts": {
            "worker": snapshot.worker.len(),
            "plugin": snapshot.plugin.len(),
            "system32": snapshot.system32.len(),
            "winsxs": snapshot.winsxs.len(),
            "policy": snapshot.policy.as_ref().map_or(0, Vec::len),
            "unknown": snapshot.unknown.len(),
        },
        "sample_basenames": {
            "worker": samples(&snapshot.worker),
            "plugin": samples(&snapshot.plugin),
            "system32": samples(&snapshot.system32),
            "winsxs": samples(&snapshot.winsxs),
            "policy": snapshot.policy.as_ref().map_or_else(Vec::new, |values| samples(values)),
            "unknown": samples(&snapshot.unknown),
        },
    }))
}

fn require_optional_subset(
    values: &Option<Vec<String>>,
    union: &Option<Vec<String>>,
) -> io::Result<()> {
    match (values, union) {
        (None, None) => Ok(()),
        (Some(values), Some(union)) => require_subset(values, union),
        (None, Some(_)) => Ok(()),
        (Some(_), None) => Err(invalid(
            "secure worker observed union policy is inconsistent",
        )),
    }
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

fn invalid_classified(classification: &str, reason: &str) -> io::Error {
    invalid_diagnostics(serde_json::json!({
        "classification": classification,
        "failure_stage": "module_audit_validation",
        "reason": reason,
    }))
}

fn invalid_diagnostics(diagnostics: Value) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("secure worker module audit failed: diagnostics={diagnostics}"),
    )
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
            "system32": ["kernel32.dll"],
            "winsxs": ["comctl32.dll"],
            "unknown": []
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
    fn classifies_invalid_json_without_exposing_worker_output() {
        let error = validate_required_worker_audit("not-json", false)
            .expect_err("malformed worker output must fail closed");
        let error_text = error.to_string();
        let diagnostics = error_text
            .split_once("diagnostics=")
            .map(|(_, value)| value)
            .expect("classified diagnostic marker");
        let diagnostics: Value =
            serde_json::from_str(diagnostics).expect("diagnostics remain valid JSON");
        assert_eq!(
            diagnostics["classification"],
            "module_audit_report_invalid_json"
        );
        assert_eq!(diagnostics["failure_stage"], "module_audit_validation");
        assert_eq!(
            diagnostics["reason"],
            "secure worker report is not valid JSON"
        );
        assert!(!error_text.contains("not-json"));
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

        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        report["module_audit"]["observed_union"]["unknown"] = json!(["outside.dll"]);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());

        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        report["module_audit"]["observed_union"]["unknown"] = json!(["..\\outside.dll"]);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());
    }

    #[test]
    fn validates_optional_policy_names_bounds_and_union_consistency() {
        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        for phase in ["post_load", "pre_unload", "observed_union"] {
            report["module_audit"][phase]["policy"] = json!(["gpu-runtime.dll"]);
        }
        validate_required_worker_audit(&report.to_string(), false).unwrap();

        report["module_audit"]["post_load"]["policy"] = json!(["kernel32.dll"]);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());

        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        report["module_audit"]["post_load"]["policy"] = json!(["gpu-runtime.dll"]);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());

        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        let names: Vec<_> = (0..126)
            .map(|index| format!("runtime-{index}.dll"))
            .collect();
        report["module_audit"]["observed_union"]["policy"] = json!(names);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());
    }
    #[test]
    fn classifies_module_audit_limit_with_bounded_counts_and_samples() {
        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        let names: Vec<_> = (0..129)
            .map(|index| format!("sealed-{index}.dll"))
            .collect();
        report["module_audit"]["observed_union"]["system32"] = json!(names);

        let error = validate_required_worker_audit(&report.to_string(), false)
            .expect_err("an over-limit audit must fail closed");
        let error_text = error.to_string();
        let diagnostics = error_text
            .split_once("diagnostics=")
            .map(|(_, value)| value)
            .expect("structured diagnostic marker");
        let diagnostics: Value =
            serde_json::from_str(diagnostics).expect("diagnostic must be valid JSON");
        assert_eq!(diagnostics["classification"], "module_audit_limit_exceeded");
        assert_eq!(diagnostics["failure_stage"], "module_audit_validation");
        assert_eq!(diagnostics["limit"], MAX_AUDITED_MODULES);
        assert_eq!(diagnostics["total"], 132);
        assert_eq!(diagnostics["category_counts"]["system32"], 129);
        assert_eq!(
            diagnostics["sample_basenames"]["system32"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert!(!error_text.contains("D:\\"));
        assert!(!error_text.contains("sealed-128.dll"));
    }
}
