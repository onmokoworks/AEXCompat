use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::io;
use std::path::{Component, Path};

/// Matches the native worker's fixed `kMaxAuditedModules` enumeration buffer.
/// Both sides stay bounded and fail closed when a report exceeds this count.
pub const MAX_AUDITED_MODULES: usize = 512;
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
    /// OS DriverStore (`System32\DriverStore\FileRepository`) modules
    /// (issue #362): absent in reports from pre-#362 workers.
    #[serde(default)]
    driverstore: Vec<String>,
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
    require_subset(
        &audit.post_load.driverstore,
        &audit.observed_union.driverstore,
    )?;
    require_subset(&audit.pre_unload.worker, &audit.observed_union.worker)?;
    require_subset(&audit.pre_unload.plugin, &audit.observed_union.plugin)?;
    require_subset(&audit.pre_unload.system32, &audit.observed_union.system32)?;
    require_subset(&audit.pre_unload.winsxs, &audit.observed_union.winsxs)?;
    require_subset(
        &audit.pre_unload.driverstore,
        &audit.observed_union.driverstore,
    )?;
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
        .chain(&snapshot.driverstore)
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
        + snapshot.driverstore.len()
        + snapshot.policy.as_ref().map_or(0, Vec::len)
        + snapshot.unknown.len();
    if count > MAX_AUDITED_MODULES {
        return Err(module_audit_limit_error(
            snapshot,
            label,
            count,
            MAX_AUDITED_MODULES,
        ));
    }
    Ok(())
}

fn module_audit_limit_error(
    snapshot: &AuditSnapshot,
    label: &str,
    total: usize,
    limit: usize,
) -> io::Error {
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
        "limit": limit,
        "total": total,
        "overflow": true,
        "truncated": false,
        "category_counts": {
            "worker": snapshot.worker.len(),
            "plugin": snapshot.plugin.len(),
            "system32": snapshot.system32.len(),
            "winsxs": snapshot.winsxs.len(),
            "driverstore": snapshot.driverstore.len(),
            "policy": snapshot.policy.as_ref().map_or(0, Vec::len),
            "unknown": snapshot.unknown.len(),
        },
        "sample_basenames": {
            "worker": samples(&snapshot.worker),
            "plugin": samples(&snapshot.plugin),
            "system32": samples(&snapshot.system32),
            "winsxs": samples(&snapshot.winsxs),
            "driverstore": samples(&snapshot.driverstore),
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

// ---------------------------------------------------------------------------
// Cluster session audit (issue #405, docs/CLOSURE_SESSION_PROTOCOL_2026-07-23
// §5). The one-shot validator above is unchanged; cluster sessions validate
// the same report shape plus per-swap epochs against the launch-authenticated
// manifest declaration instead of the fixed MAX_AUDITED_MODULES cap.
// ---------------------------------------------------------------------------

/// The launch-authenticated declaration a cluster session's module audit is
/// validated against: the basenames a `plugin`-class snapshot entry may carry
/// (the manifest's plugins ∪ dependencies), the plugin count bounding epoch
/// indices, and the declared module bound replacing the fixed one-shot cap.
#[derive(Clone, Debug)]
pub struct ClusterAuditDeclaration {
    declared_basenames: HashSet<String>,
    plugin_count: usize,
    module_bound: usize,
}

impl ClusterAuditDeclaration {
    pub fn new(
        declared_basenames: impl IntoIterator<Item = String>,
        plugin_count: usize,
        module_bound: usize,
    ) -> io::Result<Self> {
        if plugin_count == 0 || module_bound == 0 {
            return Err(invalid("cluster audit declaration is empty"));
        }
        let mut folded = HashSet::new();
        for name in declared_basenames {
            validate_basename(&name)?;
            folded.insert(name.to_ascii_lowercase());
        }
        if folded.len() > module_bound {
            return Err(invalid(
                "cluster audit declaration exceeds its own module bound",
            ));
        }
        Ok(Self {
            declared_basenames: folded,
            plugin_count,
            module_bound,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClusterAuditReport {
    schema: u32,
    status: String,
    post_load: AuditSnapshot,
    pre_unload: AuditSnapshot,
    observed_union: AuditSnapshot,
    #[serde(default)]
    epochs: Vec<AuditEpoch>,
    phase_count: u32,
    unknown_count: u32,
}

/// One swap epoch (design §5): `pre_unload` is the snapshot taken right
/// before `plugins[plugin_index]` was released, `post_load` the snapshot
/// right after the next plugin finished GLOBAL_SETUP.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditEpoch {
    plugin_index: u32,
    pre_unload: AuditSnapshot,
    post_load: AuditSnapshot,
}

/// Cluster-session variant of `validate_required_worker_audit`. Everything
/// the one-shot validator enforces stays fail-closed here (schema, unknown
/// count, duplicate names, unsafe names, subset relations against the
/// observed union); the only change is that the module-count cap and the
/// allowed `plugin`-class set come from the launch-authenticated manifest
/// declaration rather than the fixed 512-module one-shot bound.
pub fn validate_cluster_worker_audit(
    stdout: &str,
    stdout_truncated: bool,
    declaration: &ClusterAuditDeclaration,
) -> io::Result<()> {
    if stdout_truncated {
        return Err(invalid("secure worker report was truncated"));
    }
    let report: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        invalid_classified(
            "module_audit_report_invalid_json",
            "secure worker report is not valid JSON",
        )
    })?;
    let audit: ClusterAuditReport = serde_json::from_value(
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
    validate_cluster_snapshot(&audit.post_load, "post-load", declaration)?;
    validate_cluster_snapshot(&audit.pre_unload, "pre-unload", declaration)?;
    validate_cluster_snapshot(&audit.observed_union, "observed union", declaration)?;
    if audit.observed_union.worker.is_empty() || audit.observed_union.plugin.is_empty() {
        return Err(invalid("secure worker module audit lacks required images"));
    }
    for epoch in &audit.epochs {
        if epoch.plugin_index as usize >= declaration.plugin_count {
            return Err(invalid(
                "secure worker audit epoch plugin index is outside the manifest",
            ));
        }
        validate_cluster_snapshot(&epoch.pre_unload, "epoch pre-unload", declaration)?;
        validate_cluster_snapshot(&epoch.post_load, "epoch post-load", declaration)?;
    }
    // The union stays the cumulative union of every snapshot (design §5):
    // each terminal and epoch snapshot must be a subset of it in every
    // category, exactly like the one-shot path.
    let epoch_snapshots = audit
        .epochs
        .iter()
        .flat_map(|epoch| [&epoch.pre_unload, &epoch.post_load]);
    for snapshot in [&audit.post_load, &audit.pre_unload]
        .into_iter()
        .chain(epoch_snapshots)
    {
        require_subset(&snapshot.worker, &audit.observed_union.worker)?;
        require_subset(&snapshot.plugin, &audit.observed_union.plugin)?;
        require_subset(&snapshot.system32, &audit.observed_union.system32)?;
        require_subset(&snapshot.winsxs, &audit.observed_union.winsxs)?;
        require_subset(&snapshot.driverstore, &audit.observed_union.driverstore)?;
        require_optional_subset(&snapshot.policy, &audit.observed_union.policy)?;
        require_subset(&snapshot.unknown, &audit.observed_union.unknown)?;
    }
    Ok(())
}

fn validate_cluster_snapshot(
    snapshot: &AuditSnapshot,
    label: &str,
    declaration: &ClusterAuditDeclaration,
) -> io::Result<()> {
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
        .chain(&snapshot.driverstore)
        .chain(snapshot.policy.iter().flatten())
        .chain(&snapshot.unknown)
    {
        validate_basename(name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(invalid("secure worker module audit contains duplicates"));
        }
    }
    // Declared-set check (design §5): a plugin-class entry is a module loaded
    // from the sealed root, so it must be one of the manifest's plugins or
    // pinned dependencies. Anything else was never authenticated at launch.
    if snapshot.plugin.iter().any(|name| {
        !declaration
            .declared_basenames
            .contains(&name.to_ascii_lowercase())
    }) {
        return Err(invalid(
            "secure worker module audit carries an undeclared plugin-class module",
        ));
    }
    let count = snapshot.worker.len()
        + snapshot.plugin.len()
        + snapshot.system32.len()
        + snapshot.winsxs.len()
        + snapshot.driverstore.len()
        + snapshot.policy.as_ref().map_or(0, Vec::len)
        + snapshot.unknown.len();
    if count > declaration.module_bound {
        return Err(module_audit_limit_error(
            snapshot,
            label,
            count,
            declaration.module_bound,
        ));
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

    fn report_with_module_total(total: usize) -> String {
        assert!(total >= 3);
        let system32: Vec<_> = (0..total - 3)
            .map(|index| format!("system-{index}.dll"))
            .collect();
        let snapshot = json!({
            "status": "passed",
            "unknown_count": 0,
            "worker": ["trusted-worker.exe"],
            "plugin": ["fixture.plugin"],
            "system32": system32,
            "winsxs": ["comctl32.dll"],
            "unknown": []
        });
        json!({
            "status": "rendered",
            "module_audit": {
                "schema": 1,
                "status": "passed",
                "post_load": snapshot,
                "pre_unload": snapshot,
                "observed_union": snapshot,
                "phase_count": 7,
                "unknown_count": 0
            }
        })
        .to_string()
    }

    #[test]
    fn accepts_complete_cumulative_audit() {
        validate_required_worker_audit(&valid_report(), false).unwrap();
    }

    #[test]
    fn accepts_observed_176_module_process_and_exact_limit() {
        validate_required_worker_audit(&report_with_module_total(176), false)
            .expect("the installed 176-module process fits the bounded audit");
        validate_required_worker_audit(&report_with_module_total(MAX_AUDITED_MODULES), false)
            .expect("the exact native producer bound remains valid");
    }

    #[test]
    fn accepts_driverstore_modules_like_winsxs_assemblies() {
        // Issue #362: user-mode driver DLLs from the OS DriverStore classify
        // into their own category, accepted on both the one-shot and the
        // cluster paths, and held to the same union/dedup invariants.
        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        for snapshot in ["post_load", "pre_unload", "observed_union"] {
            report["module_audit"][snapshot]["driverstore"] = json!(["nvoglv64.dll"]);
        }
        validate_required_worker_audit(&report.to_string(), false).unwrap();

        // A snapshot entry missing from the union fails closed.
        let mut inconsistent = report.clone();
        inconsistent["module_audit"]["observed_union"]["driverstore"] = json!([]);
        assert!(validate_required_worker_audit(&inconsistent.to_string(), false).is_err());

        // The same basename in two categories is a duplicate.
        let mut duplicate = report.clone();
        duplicate["module_audit"]["post_load"]["system32"] =
            json!(["kernel32.dll", "NVOGLV64.dll"]);
        assert!(validate_required_worker_audit(&duplicate.to_string(), false).is_err());

        // The cluster validator counts and accepts the category too.
        let mut cluster_report: Value = serde_json::from_str(&valid_cluster_report()).unwrap();
        for snapshot in ["post_load", "pre_unload", "observed_union"] {
            cluster_report["module_audit"][snapshot]["driverstore"] = json!(["nvoglv64.dll"]);
        }
        validate_cluster_worker_audit(&cluster_report.to_string(), false, &cluster_declaration())
            .unwrap();
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
        let names: Vec<_> = (0..509)
            .map(|index| format!("runtime-{index}.dll"))
            .collect();
        report["module_audit"]["observed_union"]["policy"] = json!(names);
        assert!(validate_required_worker_audit(&report.to_string(), false).is_err());
    }

    #[test]
    fn classifies_limit_plus_one_as_explicit_bounded_overflow() {
        let error = validate_required_worker_audit(
            &report_with_module_total(MAX_AUDITED_MODULES + 1),
            false,
        )
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
        assert_eq!(diagnostics["total"], MAX_AUDITED_MODULES + 1);
        assert_eq!(
            diagnostics["category_counts"]["system32"],
            MAX_AUDITED_MODULES - 2
        );
        assert_eq!(diagnostics["overflow"], true);
        assert_eq!(diagnostics["truncated"], false);
        assert_eq!(
            diagnostics["sample_basenames"]["system32"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert!(!error_text.contains("D:\\"));
        assert!(!error_text.contains("system-509.dll"));
    }

    // ---- Cluster session audit (issue #405) ----

    fn cluster_declaration() -> ClusterAuditDeclaration {
        ClusterAuditDeclaration::new(
            [
                "alpha.plugin".to_owned(),
                "beta.plugin".to_owned(),
                "helper.dll".to_owned(),
            ],
            2,
            200,
        )
        .unwrap()
    }

    fn cluster_snapshot(plugins: &[&str]) -> Value {
        json!({
            "status": "passed",
            "unknown_count": 0,
            "worker": ["trusted-worker.exe"],
            "plugin": plugins,
            "system32": ["kernel32.dll"],
            "winsxs": ["comctl32.dll"],
            "unknown": []
        })
    }

    fn valid_cluster_report() -> String {
        json!({
            "status": "session_completed",
            "module_audit": {
                "schema": 1,
                "status": "passed",
                "phase_count": 5,
                "unknown_count": 0,
                "post_load": cluster_snapshot(&["beta.plugin", "helper.dll"]),
                "pre_unload": cluster_snapshot(&["beta.plugin", "helper.dll"]),
                "observed_union": cluster_snapshot(&["alpha.plugin", "beta.plugin", "helper.dll"]),
                "epochs": [{
                    "plugin_index": 0,
                    "pre_unload": cluster_snapshot(&["alpha.plugin", "helper.dll"]),
                    "post_load": cluster_snapshot(&["beta.plugin", "helper.dll"])
                }]
            }
        })
        .to_string()
    }

    #[test]
    fn cluster_audit_accepts_epochs_within_the_declared_set() {
        validate_cluster_worker_audit(&valid_cluster_report(), false, &cluster_declaration())
            .unwrap();
        // A cluster audit may legitimately exceed the fixed one-shot cap as
        // long as it stays within the declared module bound.
        let declaration =
            ClusterAuditDeclaration::new(["alpha.plugin".to_owned()], 1, 700).unwrap();
        let system32: Vec<String> = (0..600).map(|index| format!("sys-{index}.dll")).collect();
        let mut snapshot = cluster_snapshot(&["alpha.plugin"]);
        snapshot["system32"] = json!(system32);
        let report = json!({
            "status": "session_completed",
            "module_audit": {
                "schema": 1,
                "status": "passed",
                "phase_count": 3,
                "unknown_count": 0,
                "post_load": snapshot,
                "pre_unload": snapshot,
                "observed_union": snapshot
            }
        });
        let report = report.to_string();
        assert!(validate_required_worker_audit(&report, false).is_err());
        validate_cluster_worker_audit(&report, false, &declaration).unwrap();
    }

    #[test]
    fn cluster_audit_rejects_modules_outside_the_declared_set() {
        let mut report: Value = serde_json::from_str(&valid_cluster_report()).unwrap();
        report["module_audit"]["post_load"]["plugin"] = json!(["evil.dll", "helper.dll"]);
        report["module_audit"]["observed_union"]["plugin"] =
            json!(["alpha.plugin", "beta.plugin", "evil.dll", "helper.dll"]);
        assert!(
            validate_cluster_worker_audit(&report.to_string(), false, &cluster_declaration())
                .is_err()
        );
    }

    #[test]
    fn cluster_audit_rejects_snapshots_beyond_the_declared_bound() {
        let declaration = ClusterAuditDeclaration::new(["alpha.plugin".to_owned()], 1, 4).unwrap();
        // worker + plugin + system32 + winsxs = 4 modules passes...
        let report = json!({
            "status": "session_completed",
            "module_audit": {
                "schema": 1,
                "status": "passed",
                "phase_count": 3,
                "unknown_count": 0,
                "post_load": cluster_snapshot(&["alpha.plugin"]),
                "pre_unload": cluster_snapshot(&["alpha.plugin"]),
                "observed_union": cluster_snapshot(&["alpha.plugin"])
            }
        });
        validate_cluster_worker_audit(&report.to_string(), false, &declaration).unwrap();
        // ...but one more module crosses the declared bound.
        let mut over = report.clone();
        over["module_audit"]["observed_union"]["system32"] = json!(["kernel32.dll", "ntdll.dll"]);
        assert!(validate_cluster_worker_audit(&over.to_string(), false, &declaration).is_err());
        // The declaration itself cannot exceed its own bound.
        assert!(
            ClusterAuditDeclaration::new(["a.plugin".to_owned(), "b.plugin".to_owned()], 1, 1,)
                .is_err()
        );
    }

    #[test]
    fn cluster_audit_rejects_union_violations_and_foreign_epoch_indices() {
        // An epoch snapshot outside the observed union breaks the cumulative
        // union contract (monotonic accumulation).
        let mut report: Value = serde_json::from_str(&valid_cluster_report()).unwrap();
        report["module_audit"]["epochs"][0]["pre_unload"] =
            cluster_snapshot(&["alpha.plugin", "beta.plugin", "helper.dll"]);
        report["module_audit"]["observed_union"] =
            cluster_snapshot(&["alpha.plugin", "helper.dll"]);
        assert!(
            validate_cluster_worker_audit(&report.to_string(), false, &cluster_declaration())
                .is_err()
        );

        let mut report: Value = serde_json::from_str(&valid_cluster_report()).unwrap();
        report["module_audit"]["epochs"][0]["plugin_index"] = json!(2);
        assert!(
            validate_cluster_worker_audit(&report.to_string(), false, &cluster_declaration())
                .is_err()
        );

        let mut report: Value = serde_json::from_str(&valid_cluster_report()).unwrap();
        report["module_audit"]["epochs"][0]["surprise"] = json!(true);
        assert!(
            validate_cluster_worker_audit(&report.to_string(), false, &cluster_declaration())
                .is_err()
        );

        assert!(
            validate_cluster_worker_audit(&valid_cluster_report(), true, &cluster_declaration())
                .is_err()
        );
        assert!(validate_cluster_worker_audit("not-json", false, &cluster_declaration()).is_err());
    }
}
