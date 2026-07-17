use crate::fixture_profiles::find_observation;
use crate::host_core::approved_artifact::load;
use crate::windows_process::run_isolated;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path};
use std::time::Duration;

const WORKER_REPORT_KEYS: [&str; 9] = [
    "schema_version",
    "stage",
    "status",
    "identity_verified",
    "module_loaded",
    "entrypoint_resolved",
    "selectors_executed",
    "render_performed",
    "win32_error",
];

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn failure_classification(report: Option<&Value>) -> Option<&'static str> {
    let report = report?;
    let status = report.get("status")?.as_str()?;
    let error = report.get("win32_error")?.as_u64()?;
    match (status, error) {
        ("loaded_and_unloaded", 0) => None,
        ("load_failed", 126) => Some("load_dependency_not_found"),
        ("load_failed", 127) => Some("load_dependency_import_missing"),
        ("load_failed", 193) => Some("load_bad_image_format"),
        ("load_failed", _) => Some("load_failed_other"),
        ("entrypoint_missing", 127) => Some("entrypoint_not_found"),
        ("entrypoint_missing", _) => Some("entrypoint_lookup_unexpected_error"),
        ("dll_policy_failed", _) => Some("dll_policy_failed"),
        ("identity_mismatch", _) => Some("identity_mismatch"),
        ("identity_read_failed", _) => Some("identity_read_failed"),
        ("invalid_request", _) => Some("invalid_request"),
        _ => Some("worker_report_inconsistent"),
    }
}

fn worker_report_evidence(
    report: Option<&Value>,
    classification: &str,
    exit_code: u32,
    stdout_truncated: bool,
) -> Vec<String> {
    let mut evidence = Vec::new();
    if classification != "ok" {
        evidence.push("classification_must_be_ok".into());
    }
    if exit_code != 0 {
        evidence.push("exit_code_must_be_zero".into());
    }
    if stdout_truncated {
        evidence.push("stdout_must_not_be_truncated".into());
    }

    let Some(object) = report.and_then(Value::as_object) else {
        evidence.push("worker_report_must_be_json_object".into());
        return evidence;
    };
    if object.len() != WORKER_REPORT_KEYS.len()
        || WORKER_REPORT_KEYS
            .iter()
            .any(|key| !object.contains_key(*key))
    {
        evidence.push("worker_report_keys_must_match_schema_exactly".into());
    }
    let checks = [
        (
            "schema_version",
            object.get("schema_version").and_then(Value::as_u64) == Some(1),
        ),
        (
            "stage",
            object.get("stage").and_then(Value::as_str) == Some("L1"),
        ),
        (
            "status",
            object.get("status").and_then(Value::as_str) == Some("loaded_and_unloaded"),
        ),
        (
            "identity_verified",
            object.get("identity_verified").and_then(Value::as_bool) == Some(true),
        ),
        (
            "module_loaded",
            object.get("module_loaded").and_then(Value::as_bool) == Some(true),
        ),
        (
            "entrypoint_resolved",
            object.get("entrypoint_resolved").and_then(Value::as_bool) == Some(true),
        ),
        (
            "selectors_executed",
            object.get("selectors_executed").and_then(Value::as_bool) == Some(false),
        ),
        (
            "render_performed",
            object.get("render_performed").and_then(Value::as_bool) == Some(false),
        ),
        (
            "win32_error",
            object.get("win32_error").and_then(Value::as_u64) == Some(0),
        ),
    ];
    for (key, valid) in checks {
        if !valid {
            evidence.push(format!("invalid_{key}"));
        }
    }
    evidence
}

pub fn run(repository: &Path, worker: &Path, id: &str, output: &Path) -> io::Result<bool> {
    let policy = find_observation(id)
        .ok_or_else(|| invalid_data("requested L1 profile is not registered"))?
        .l1_approval;
    if output
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid_data("output traversal is forbidden"));
    }
    let root = repository.join("target/l1-results");
    fs::create_dir_all(&root)?;
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        repository.join(output)
    };
    let parent = output
        .parent()
        .ok_or_else(|| invalid_data("output parent missing"))?;
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid_data("output is outside the L1 result root"));
    }
    let entry = load(repository, id, policy)?;
    let result = run_isolated(
        worker,
        &[
            "--l1".into(),
            entry.plugin_path.to_string_lossy().into_owned(),
            entry.sha256.to_ascii_lowercase(),
        ],
        Duration::from_millis(entry.timeout_ms),
    )?;
    let parsed_report = serde_json::from_str::<Value>(result.stdout.trim());
    let worker_report = parsed_report.as_ref().ok();
    let mut evidence = worker_report_evidence(
        worker_report,
        result.classification.as_str(),
        result.exit_code,
        result.stdout_truncated,
    );
    if let Err(error) = &parsed_report {
        evidence.push(format!("worker_report_json_invalid: {error}"));
    }
    let passed = evidence.is_empty();
    let worker_report = worker_report.cloned().unwrap_or(Value::Null);
    let failure_classification = failure_classification(parsed_report.as_ref().ok());
    let report = json!({
        "schema_version": 1,
        "stage": "L1",
        "plugin_id": id,
        "receipt_id": entry.receipt_id,
        "expected_sha256": entry.sha256.to_ascii_uppercase(),
        "worker_exit": result.classification.as_str(),
        "worker_exit_code": result.exit_code,
        "stdout_truncated": result.stdout_truncated,
        "stderr_truncated": result.stderr_truncated,
        "worker_report": worker_report,
        "failure_classification": failure_classification,
        "evidence": evidence,
        "passed": passed
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(&mut file, &report)
        .map_err(|error| invalid_data(error.to_string()))?;
    file.write_all(b"\n")?;
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::{failure_classification, worker_report_evidence};
    use serde_json::{json, Value};

    fn valid_report() -> Value {
        json!({"schema_version":1,"stage":"L1","status":"loaded_and_unloaded",
            "identity_verified":true,"module_loaded":true,"entrypoint_resolved":true,
            "selectors_executed":false,"render_performed":false,"win32_error":0})
    }

    #[test]
    fn success_requires_every_process_and_report_invariant() {
        let cases = [
            ("valid", None, "ok", 0, false, true),
            (
                "extra_key",
                Some(("extra", json!(true))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "wrong_type",
                Some(("schema_version", json!("1"))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "wrong_stage",
                Some(("stage", json!("L2"))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "wrong_status",
                Some(("status", json!("entrypoint_missing"))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "identity",
                Some(("identity_verified", json!(false))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "module",
                Some(("module_loaded", json!(false))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "entrypoint",
                Some(("entrypoint_resolved", json!(false))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "selectors",
                Some(("selectors_executed", json!(true))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "render",
                Some(("render_performed", json!(true))),
                "ok",
                0,
                false,
                false,
            ),
            (
                "win32",
                Some(("win32_error", json!(127))),
                "ok",
                0,
                false,
                false,
            ),
            ("classification", None, "crashed", 0, false, false),
            ("exit", None, "ok", 12, false, false),
            ("truncated", None, "ok", 0, true, false),
        ];
        for (name, mutation, classification, exit, truncated, expected) in cases {
            let mut report = valid_report();
            if let Some((key, value)) = mutation {
                report.as_object_mut().unwrap().insert(key.into(), value);
            }
            assert_eq!(
                worker_report_evidence(Some(&report), classification, exit, truncated).is_empty(),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn malformed_reports_produce_evidence() {
        assert!(!worker_report_evidence(None, "ok", 0, false).is_empty());
        assert!(!worker_report_evidence(Some(&json!([])), "ok", 0, false).is_empty());
    }

    #[test]
    fn loader_and_entrypoint_errors_have_stage_specific_classifications() {
        for (status, error, expected) in [
            ("load_failed", 126, Some("load_dependency_not_found")),
            ("load_failed", 127, Some("load_dependency_import_missing")),
            ("load_failed", 193, Some("load_bad_image_format")),
            ("entrypoint_missing", 127, Some("entrypoint_not_found")),
            (
                "entrypoint_missing",
                193,
                Some("entrypoint_lookup_unexpected_error"),
            ),
            ("loaded_and_unloaded", 0, None),
        ] {
            let report = json!({"status":status,"win32_error":error});
            assert_eq!(failure_classification(Some(&report)), expected);
        }
    }
}
