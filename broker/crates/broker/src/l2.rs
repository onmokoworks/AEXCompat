use crate::fixture_profiles::{find_observation, L2ObservationPolicy};
use crate::host_core::approved_artifact::load_v2_load_tree;
use crate::sealed_load_tree::SealedLoadTree;
use crate::secure_launch::{secure_launch, SecureLaunchRequest};
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
    crate::trace_policy::validate_broker_trace_directory(repository)?;
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
    let approved = load_v2_load_tree(repository, id, policy.approval)?;
    let plugin_basename = approved.main.relative_basename.clone();
    let plugin_sha256 = approved.main.expected_sha256;
    let tree = SealedLoadTree::create(approved.main, approved.dependencies)?;
    let before = ["--l2".to_owned()];
    let after = [hex_sha256(plugin_sha256)];
    let request = SecureLaunchRequest {
        worker_program: worker,
        worker_expected_sha256: approved.worker_sha256,
        worker_expected_size: approved.worker_byte_size,
        plugin_basename: &plugin_basename,
        args_before_plugin: &before,
        args_after_plugin: &after,
        require_module_audit: true,
    };
    let result = secure_launch(tree, request, Duration::from_millis(approved.timeout_ms))?;
    let worker_report: Value = serde_json::from_str(result.stdout.trim()).unwrap_or_else(|_| {
        json!({
            "status": "worker_report_unavailable",
            "selectors_executed": "unknown",
            "render_performed": false
        })
    });
    let passed = result.classification.as_str() == "ok" && worker_passed(&worker_report, policy);
    let report = json!({"schema_version":1,"stage":"L2","plugin_id":id,
        "receipt_id":policy.approval.receipt_id,"expected_sha256":hex_sha256(plugin_sha256).to_ascii_uppercase(),
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

fn hex_sha256(digest: [u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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

    #[test]
    fn plugin_hash_argument_is_lowercase_sha256() {
        assert_eq!(hex_sha256([0xab; 32]), "ab".repeat(32));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires locally approved real L2 fixtures"]
    fn real_registered_l2_fixtures_use_secure_production_launch_when_present() {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap()
            .to_path_buf();
        let worker = repository.join("target/minihost-build/aex_l2_worker.exe");
        if !worker.is_file() {
            return;
        }

        let mut failed = Vec::new();
        for id in ["scattermap", "maskoffset"] {
            let policy = find_observation(id).unwrap().l2.approval;
            let approved = load_v2_load_tree(&repository, id, policy).unwrap();
            if !approved.main.source.is_file() {
                continue;
            }
            let output = PathBuf::from(format!(
                "target/l2-results/{id}-secure-launch-test-{:032x}.json",
                rand::random::<u128>()
            ));
            if !run(&repository, &worker, id, &output).unwrap() {
                failed.push(id);
            }
        }
        assert!(failed.is_empty(), "failed fixtures: {failed:?}");
    }
}
