use crate::windows_process::run_isolated;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    schema_version: u32,
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    plugin_path: PathBuf,
    sha256: String,
    byte_size: u64,
    approved_stage: String,
    receipt_id: String,
    expires: String,
    timeout_ms: u64,
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn read_entry(repository: &Path, id: &str) -> io::Result<Entry> {
    let path = repository.join("target/l1-allowlist/active.local.json");
    let bytes = fs::read(path)?;
    let allowlist: Allowlist =
        serde_json::from_slice(&bytes).map_err(|error| invalid_data(error.to_string()))?;
    if allowlist.schema_version != 1 || allowlist.entries.len() != 1 {
        return Err(invalid_data("allowlist must contain exactly one schema-v1 entry"));
    }
    let entry = allowlist.entries.into_iter().next().unwrap();
    if entry.id != id || entry.approved_stage != "L1" {
        return Err(invalid_data("requested id or stage is not approved"));
    }
    if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid_data("invalid expected digest"));
    }
    if entry.byte_size == 0 || entry.timeout_ms == 0 || entry.timeout_ms > 30_000 {
        return Err(invalid_data("invalid resource limits"));
    }
    let metadata = fs::metadata(&entry.plugin_path)?;
    if !metadata.is_file() || metadata.len() != entry.byte_size {
        return Err(invalid_data("allowlisted identity size mismatch"));
    }
    if entry.receipt_id.is_empty() || entry.expires != "2026-08-12T23:59:59+09:00" {
        return Err(invalid_data("receipt binding or expiry is invalid"));
    }
    Ok(entry)
}

pub fn run(repository: &Path, worker: &Path, id: &str, output: &Path) -> io::Result<bool> {
    if output.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir)) {
        return Err(invalid_data("output traversal is forbidden"));
    }
    let root = repository.join("target/l1-results");
    fs::create_dir_all(&root)?;
    let output = if output.is_absolute() { output.to_path_buf() } else { repository.join(output) };
    let parent = output.parent().ok_or_else(|| invalid_data("output parent missing"))?;
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid_data("output is outside the L1 result root"));
    }
    let entry = read_entry(repository, id)?;
    let result = run_isolated(
        worker,
        &["--l1".into(), entry.plugin_path.to_string_lossy().into_owned(), entry.sha256.to_ascii_lowercase()],
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
    let mut file = OpenOptions::new().write(true).create_new(true).open(output)?;
    serde_json::to_writer_pretty(&mut file, &report)
        .map_err(|error| invalid_data(error.to_string()))?;
    file.write_all(b"\n")?;
    Ok(passed)
}
