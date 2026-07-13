use crate::fixture_profiles::find_observation;
use crate::host_core::approved_artifact::load;
use crate::windows_process::run_isolated;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path};
use std::time::Duration;

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
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
    let worker_report: Value = serde_json::from_str(result.stdout.trim())
        .map_err(|error| invalid_data(error.to_string()))?;
    let passed = result.classification.as_str() == "ok"
        && worker_report.get("status") == Some(&Value::String("loaded_and_unloaded".into()))
        && worker_report.get("selectors_executed") == Some(&Value::Bool(false));
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
