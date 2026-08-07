use crate::fixture_profiles::find_observation;
use crate::host_core::approved_artifact::load_v2_load_tree;
use crate::secure_launch::{SecureLaunchRequest, secure_launch_in_place};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path};
use std::time::Duration;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Whether the observation satisfies the host's L2 contract.
///
/// This used to also assert fixture identity: the ABOUT text had to contain
/// the fixture's version string and the reported `out_flags` pair had to equal
/// constants compiled into the broker, so a plug-in that changed its flags
/// failed L2 and an unregistered plug-in could never pass at all (issue #733).
/// Those values are observations, and the report already carries them; what is
/// checked here is what the host owes any plug-in.
fn worker_passed(worker_report: &Value) -> bool {
    // The advertisement is the plug-in's to make. The host contract is that
    // conditional UI selectors are dispatched exactly when it advertised them.
    let advertised = |key: &str| {
        worker_report
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let conditional_ui_expected =
        advertised("update_params_ui_advertised") || advertised("query_dynamic_flags_advertised");
    worker_report.get("status") == Some(&Value::String("selectors_completed".into()))
        && worker_report
            .get("about_message")
            .and_then(Value::as_str)
            .is_some_and(|message| !message.trim().is_empty())
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
        && worker_report.get("out_flags").is_some_and(Value::is_number)
        && worker_report
            .get("out_flags2")
            .is_some_and(Value::is_number)
        && worker_report.get("conditional_ui_selectors_dispatched")
            == Some(&Value::Bool(conditional_ui_expected))
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
    let approved = load_v2_load_tree(repository, id, policy.selection)?;
    let plugin_sha256 = approved.main.expected_sha256;
    let plugin_path = fs::canonicalize(&approved.main.source)?;
    let search_dirs = approved.dependency_search_dirs()?;
    let joined = crate::secure_image_dispatch::joined_dependency_search_dirs(&search_dirs)?;
    let before = ["--l2".to_owned()];
    let after = [
        hex_sha256(plugin_sha256),
        "--dependency-dirs-v1".to_owned(),
        joined,
    ];
    let request = SecureLaunchRequest {
        worker_program: worker,
        worker_expected_sha256: approved.worker_sha256,
        worker_expected_size: approved.worker_byte_size,
        args_before_plugin: &before,
        args_after_plugin: &after,
        repository,
        require_module_audit: true,
        launch_environment: Default::default(),
    };
    let result = secure_launch_in_place(
        &plugin_path,
        request,
        Some(Duration::from_millis(approved.timeout_ms)),
        None,
    )?;
    let worker_report: Value = serde_json::from_str(result.stdout.trim()).unwrap_or_else(|_| {
        json!({
            "status": "worker_report_unavailable",
            "selectors_executed": "unknown",
            "render_performed": false
        })
    });
    let passed = result.classification.as_str() == "ok" && worker_passed(&worker_report);
    let report = json!({"schema_version":1,"stage":"L2","plugin_id":id,
        "receipt_id":approved.receipt_id,"expected_sha256":hex_sha256(plugin_sha256).to_ascii_uppercase(),
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

    /// The contract is the host's behaviour, not the fixture's identity: a
    /// plug-in that reports different flags still passes (issue #733), while a
    /// host that dispatched the wrong selectors does not.
    #[test]
    fn l2_contract_accepts_any_plugin_identity_and_rejects_host_drift() {
        let mut value = report();
        assert!(worker_passed(&value));

        // Different flags are a different plug-in, not a failure.
        value["out_flags"] = json!(5);
        value["out_flags2"] = json!(9);
        value["about_message"] = json!("Some Other Effect v3");
        assert!(worker_passed(&value));

        // The host must dispatch the conditional UI selectors exactly when the
        // plug-in advertised them.
        value["update_params_ui_advertised"] = json!(true);
        assert!(!worker_passed(&value));
        value["conditional_ui_selectors_dispatched"] = json!(true);
        assert!(worker_passed(&value));

        // A selector error is still a failure.
        value["global_setup_error"] = json!(25);
        assert!(!worker_passed(&value));
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
            let policy = find_observation(id).unwrap().l2.selection;
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
