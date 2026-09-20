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
    #[serde(default)]
    #[allow(dead_code)]
    execution_images: Option<serde::de::IgnoredAny>,
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

// Parses the worker's final report as the FIRST JSON value on stdout,
// tolerating non-JSON teardown output the closure may print after the report
// when its modules unload at process exit (issue #474: e.g. an Adobe
// allocator's exit-time stats lines). A broken or truncated report still
// fails closed.
pub(crate) fn parse_report_prefix(stdout: &str) -> io::Result<Value> {
    let trimmed = stdout.trim();
    let mut stream = serde_json::Deserializer::from_str(trimmed).into_iter::<Value>();
    match stream.next() {
        Some(Ok(value)) => Ok(value),
        _ => Err(invalid_classified(
            "module_audit_report_invalid_json",
            "secure worker report is not valid JSON",
        )),
    }
}

/// The audit as a recorded observation (issue #730): `None` when the report
/// confirms that only known modules loaded, `Some(reason)` otherwise. The
/// classifier below is unchanged; what moved is the boundary — an unknown or
/// unconfirmable module list rides the launch result as a warning instead of
/// failing the dispatch. The module list explains observations (a runtime DLL
/// difference can change pixels); it does not decide their validity.
pub fn observe_required_worker_audit(stdout: &str, stdout_truncated: bool) -> Option<String> {
    validate_required_worker_audit(stdout, stdout_truncated)
        .err()
        .map(|error| error.to_string())
}

/// In-place cluster-session observation (issue #751): the worker records the
/// loaded-module set (search-root classification, no declared narrowing) and
/// this checks only that the record exists, parses, and classified every
/// module. Any shortfall rides the close report as a warning — record, never
/// enforce.
pub fn observe_in_place_cluster_audit(stdout: &str, stdout_truncated: bool) -> Option<String> {
    let observe = || -> io::Result<()> {
        if stdout_truncated {
            return Err(invalid("secure worker report was truncated"));
        }
        let report = parse_report_prefix(stdout)?;
        // The cluster shape: the base report plus optional per-swap epochs.
        let audit: ClusterAuditReport = serde_json::from_value(
            report
                .get("module_audit")
                .cloned()
                .ok_or_else(|| invalid("secure worker module audit is missing"))?,
        )
        .map_err(|_| invalid("secure worker module audit schema is invalid"))?;
        if audit.schema != 1 {
            return Err(invalid("secure worker module audit schema is invalid"));
        }
        if audit.status != "passed" || audit.unknown_count != 0 {
            return Err(invalid(
                "the recorded in-place module audit left unclassified modules",
            ));
        }
        Ok(())
    };
    observe().err().map(|error| error.to_string())
}

pub fn validate_required_worker_audit(stdout: &str, stdout_truncated: bool) -> io::Result<()> {
    if stdout_truncated {
        return Err(invalid("secure worker report was truncated"));
    }
    let report = parse_report_prefix(stdout)?;
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
// §5). The report carries the same shape as the one-shot audit plus per-swap
// epochs; it is recorded, not enforced, by `observe_in_place_cluster_audit`.
// ---------------------------------------------------------------------------

/// The observer reads only `schema`, `status` and `unknown_count`; the other
/// fields are the schema itself. Being required (and `deny_unknown_fields`)
/// is what makes a reshaped or partial record fail to deserialize instead of
/// passing as a classified audit, so they are declared but never read.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
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
    #[serde(default)]
    execution_images: Option<serde::de::IgnoredAny>,
}

/// One swap epoch (design §5): `pre_unload` is the snapshot taken right
/// before `plugins[plugin_index]` was released, `post_load` the snapshot
/// right after the next plugin finished GLOBAL_SETUP. Schema-only, like the
/// unread fields above.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct AuditEpoch {
    plugin_index: u32,
    pre_unload: AuditSnapshot,
    post_load: AuditSnapshot,
}

/// Extracts the worker's bounded, path-free plug-in execution identities for
/// public diagnostics. Invalid or unsafe observations are omitted; they never
/// change dispatch, audit, or render verdicts.
pub(crate) fn execution_images_summary(report: &Value) -> Option<Value> {
    let images = report
        .get("module_audit")?
        .get("execution_images")?
        .as_array()?;
    if images.is_empty() || images.len() > 64 {
        return None;
    }
    let mut seen = HashSet::new();
    let mut safe = Vec::with_capacity(images.len());
    for image in images {
        let object = image.as_object()?;
        let expected = [
            "plugin_index",
            "basename",
            "sha256",
            "size_bytes",
            "binding_status",
        ];
        if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
            return None;
        }
        let plugin_index = object.get("plugin_index")?.as_u64()?;
        if plugin_index > 4095 || !seen.insert(plugin_index) {
            return None;
        }
        let basename = object.get("basename")?.as_str()?;
        validate_basename(basename).ok()?;
        let sha256 = object.get("sha256")?.as_str()?;
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let size_bytes = object.get("size_bytes")?.as_u64()?;
        if size_bytes == 0 {
            return None;
        }
        let binding_status = object.get("binding_status")?.as_str()?;
        if !matches!(
            binding_status,
            "same_file_identity_matches_loaded_module"
                | "loaded_module_path_unavailable"
                | "loaded_module_identity_unavailable"
                | "loaded_module_identity_mismatch"
        ) {
            return None;
        }
        safe.push(serde_json::json!({
            "plugin_index": plugin_index,
            "basename": basename,
            "sha256": sha256.to_ascii_lowercase(),
            "size_bytes": size_bytes,
            "binding_status": binding_status,
        }));
    }
    Some(Value::Array(safe))
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

    #[test]
    fn execution_images_are_path_free_and_record_only() {
        let mut report: Value = serde_json::from_str(&valid_report()).unwrap();
        report["module_audit"]["execution_images"] = json!([{
            "plugin_index": 0,
            "basename": "Fixture.aex",
            "sha256": "AB".repeat(32),
            "size_bytes": 12345,
            "binding_status": "loaded_module_identity_mismatch"
        }]);
        let summary = execution_images_summary(&report).unwrap();
        assert_eq!(summary[0]["plugin_index"], 0);
        assert_eq!(summary[0]["basename"], "Fixture.aex");
        assert_eq!(summary[0]["sha256"], "ab".repeat(32));
        assert_eq!(
            summary[0]["binding_status"],
            "loaded_module_identity_mismatch"
        );
        assert!(validate_required_worker_audit(&report.to_string(), false).is_ok());

        report["module_audit"]["execution_images"][0]["basename"] =
            json!(r"C:\private\Fixture.aex");
        assert!(execution_images_summary(&report).is_none());
        assert!(validate_required_worker_audit(&report.to_string(), false).is_ok());

        // Optional observations must not change the audit verdict even when a
        // newer or faulty worker emits a schema the summary cannot publish.
        for malformed in [json!(null), json!("not an array"), json!([{}])] {
            report["module_audit"]["execution_images"] = malformed;
            assert!(execution_images_summary(&report).is_none());
            assert!(validate_required_worker_audit(&report.to_string(), false).is_ok());
        }
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

    /// The recording boundary (issue #730): a passing audit adds nothing, and
    /// every classifier rejection becomes a recorded reason instead of an
    /// error the dispatch would have died on.
    #[test]
    fn observation_wrappers_record_instead_of_failing() {
        assert_eq!(observe_required_worker_audit(&valid_report(), false), None);
        let warning = observe_required_worker_audit("{}", false)
            .expect("a missing audit is recorded, not enforced");
        assert!(warning.contains("module audit"), "{warning}");
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
        // into their own category and are held to the same union/dedup
        // invariants as every other category.
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

    /// The in-place observation (issue #751): a present, parsed, fully
    /// classified record passes silently; anything less rides back as a
    /// warning string — never an error the dispatch acts on.
    #[test]
    fn in_place_cluster_audit_observation_warns_without_enforcing() {
        assert_eq!(
            observe_in_place_cluster_audit(&valid_cluster_report(), false),
            None
        );
        // Truncated stdout, a missing record, and unclassified modules all
        // surface as warnings.
        assert!(observe_in_place_cluster_audit(&valid_cluster_report(), true).is_some());
        assert!(
            observe_in_place_cluster_audit(&json!({"status": "ok"}).to_string(), false).is_some()
        );
        let mut failed: Value = serde_json::from_str(&valid_cluster_report()).unwrap();
        failed["module_audit"]["status"] = json!("failed");
        failed["module_audit"]["unknown_count"] = json!(2);
        assert!(observe_in_place_cluster_audit(&failed.to_string(), false).is_some());
    }
}
