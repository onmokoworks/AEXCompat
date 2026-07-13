use crate::fixture_profiles::{find_observation, L2ObservationPolicy};
use crate::host_core::approved_artifact::load;
use crate::windows_process::run_isolated;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path};
use std::time::Duration;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn worker_passed(worker_report: &Value, policy: L2ObservationPolicy) -> bool {
    worker_report.get("status") == Some(&Value::String("selectors_completed".into()))
        && worker_report
            .get("about_message")
            .and_then(Value::as_str)
            .is_some_and(|message| {
                policy
                    .about_substrings
                    .iter()
                    .all(|part| message.contains(part))
            })
        && [
            "global_setup_error",
            "params_setup_error",
            "sequence_setup_error",
            "sequence_resetup_error",
            "frame_setup_error",
            "frame_setdown_error",
            "sequence_setdown_error",
            "global_setdown_error",
        ]
        .iter()
        .all(|key| worker_report.get(*key) == Some(&json!(0)))
        && worker_report.get("out_flags") == Some(&json!(policy.out_flags))
        && worker_report.get("out_flags2") == Some(&json!(policy.out_flags2))
        && worker_report.get("update_params_ui_advertised")
            == Some(&Value::Bool(policy.update_params_ui_advertised))
        && worker_report.get("query_dynamic_flags_advertised")
            == Some(&Value::Bool(policy.query_dynamic_flags_advertised))
        && worker_report.get("conditional_ui_selectors_dispatched")
            == Some(&Value::Bool(
                policy.update_params_ui_advertised || policy.query_dynamic_flags_advertised,
            ))
        && worker_report.get("lifecycle_data_null") == Some(&Value::Bool(true))
        && worker_report.get("render_performed") == Some(&Value::Bool(false))
}

pub fn run(repository: &Path, worker: &Path, id: &str, output: &Path) -> io::Result<bool> {
    let policy = find_observation(id)
        .ok_or_else(|| invalid("requested L2 profile is not registered"))?
        .l2;
    if output
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid("output traversal forbidden"));
    }
    let root = repository.join("target/l2-results");
    fs::create_dir_all(&root)?;
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        repository.join(output)
    };
    let parent = output
        .parent()
        .ok_or_else(|| invalid("output parent missing"))?;
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid("output outside L2 result root"));
    }
    let entry = load(repository, id, policy.approval)?;
    let result = run_isolated(
        worker,
        &[
            "--l2".into(),
            entry.plugin_path.to_string_lossy().into_owned(),
            entry.sha256.to_ascii_lowercase(),
        ],
        Duration::from_millis(entry.timeout_ms),
    )?;
    let worker_report: Value = serde_json::from_str(result.stdout.trim()).unwrap_or_else(|_| {
        json!({
            "status": "worker_report_unavailable",
            "selectors_executed": "unknown",
            "render_performed": false
        })
    });
    let passed = result.classification.as_str() == "ok" && worker_passed(&worker_report, policy);
    let report = json!({"schema_version":1,"stage":"L2","plugin_id":id,
        "receipt_id":entry.receipt_id,"expected_sha256":entry.sha256.to_ascii_uppercase(),
        "worker_exit":result.classification.as_str(),"worker_exit_code":result.exit_code,
        "stdout_truncated":result.stdout_truncated,"stderr_truncated":result.stderr_truncated,
        "stderr":result.stderr,
        "worker_report":worker_report,"passed":passed});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(&mut file, &report).map_err(|e| invalid(e.to_string()))?;
    file.write_all(b"\n")?;
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: L2ObservationPolicy = L2ObservationPolicy {
        approval: crate::host_core::approved_artifact::ApprovalPolicy {
            allowlist_path: "unused",
            stage: "L2",
            receipt_id: "receipt",
            expires: "expiry",
            max_timeout_ms: 1,
        },
        about_substrings: &["Example", "v1"],
        out_flags: 4,
        out_flags2: 8,
        update_params_ui_advertised: false,
        query_dynamic_flags_advertised: false,
    };

    fn report() -> Value {
        json!({
            "status": "selectors_completed", "about_message": "Example v1",
            "global_setup_error": 0, "params_setup_error": 0,
            "sequence_setup_error": 0, "sequence_resetup_error": 0,
            "frame_setup_error": 0, "frame_setdown_error": 0,
            "sequence_setdown_error": 0, "global_setdown_error": 0,
            "out_flags": 4, "out_flags2": 8,
            "update_params_ui_advertised": false,
            "query_dynamic_flags_advertised": false,
            "conditional_ui_selectors_dispatched": false,
            "lifecycle_data_null": true, "render_performed": false
        })
    }

    #[test]
    fn generic_l2_policy_accepts_matching_observation_and_rejects_drift() {
        let mut value = report();
        assert!(worker_passed(&value, POLICY));
        value["out_flags"] = json!(5);
        assert!(!worker_passed(&value, POLICY));
    }
}
