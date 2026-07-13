use crate::windows_process::run_isolated;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist { schema_version: u32, entries: Vec<Entry> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String, plugin_path: PathBuf, sha256: String, byte_size: u64,
    approved_stage: String, receipt_id: String, expires: String, timeout_ms: u64,
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn entry(repository: &Path, id: &str) -> io::Result<Entry> {
    let bytes = fs::read(repository.join("target/render-allowlist/active.local.json"))?;
    let list: Allowlist = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    if list.schema_version != 1 || list.entries.len() != 1 { return Err(invalid("one render entry required")); }
    let entry = list.entries.into_iter().next().unwrap();
    if entry.id != id || entry.approved_stage != "classic_render"
        || entry.receipt_id != "scattermap-render-20260713-001"
        || entry.expires != "2026-08-12T23:59:59+09:00" {
        return Err(invalid("render stage, receipt, or expiry mismatch"));
    }
    if entry.timeout_ms == 0 || entry.timeout_ms > 5_000 || entry.sha256.len() != 64
        || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(invalid("invalid render limits")); }
    let metadata = fs::metadata(&entry.plugin_path)?;
    if !metadata.is_file() || metadata.len() != entry.byte_size { return Err(invalid("render size mismatch")); }
    Ok(entry)
}
fn classification(value: &str) -> &str {
    match value { "ok" => "ok", "crashed" => "crashed", "timeout_killed" => "timeout_killed", _ => "internal_error" }
}
pub fn run(repository: &Path, worker: &Path, id: &str, output: &Path) -> io::Result<bool> {
    if output.components().any(|p| matches!(p, Component::ParentDir | Component::CurDir)) {
        return Err(invalid("output traversal forbidden"));
    }
    let root = repository.join("target/render-results");
    fs::create_dir_all(&root)?;
    let output = if output.is_absolute() { output.to_path_buf() } else { repository.join(output) };
    let parent = output.parent().ok_or_else(|| invalid("output parent missing"))?;
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) { return Err(invalid("output outside render root")); }
    let entry = entry(repository, id)?;
    let args = ["--render".into(), entry.plugin_path.to_string_lossy().into_owned(), entry.sha256.to_ascii_lowercase()];
    let mut reports = Vec::new();
    let mut runs = Vec::new();
    for _ in 0..2 {
        let start = Instant::now();
        let result = run_isolated(worker, &args, Duration::from_millis(entry.timeout_ms))?;
        let elapsed = start.elapsed().as_millis().min(30_000) as u64;
        let worker_report: Value = serde_json::from_str(result.stdout.trim()).unwrap_or_else(|_| json!({
            "status":"worker_report_unavailable","render_error":-1,"output_sha256":""}));
        let hash = worker_report.get("output_sha256").and_then(Value::as_str).unwrap_or("").to_ascii_uppercase();
        let render_error = worker_report.get("render_error").and_then(Value::as_i64).unwrap_or(-1);
        runs.push(json!({"classification":classification(result.classification.as_str()),
            "render_error":render_error,"output_sha256":hash,"elapsed_ms":elapsed}));
        reports.push(worker_report);
    }
    let input_hash = reports[0].get("input_sha256").and_then(Value::as_str).unwrap_or("").to_ascii_uppercase();
    let deterministic = runs[0]["output_sha256"] == runs[1]["output_sha256"];
    let guards = reports.iter().all(|r| r.get("guard_bytes_intact") == Some(&Value::Bool(true)));
    let passed = deterministic && guards && runs.iter().all(|r| r["classification"] == "ok" && r["render_error"] == 0);
    let report = json!({"schema_version":1,"stage":"classic_render","plugin_id":id,
        "receipt_id":entry.receipt_id,"fixture_sha256":entry.sha256.to_ascii_uppercase(),
        "pixel_format":"argb8","width":16,"height":12,"rowbytes":64,
        "input_sha256":input_hash,"run_1":runs[0],"run_2":runs[1],
        "deterministic":deterministic,"guard_bytes_intact":guards,"broker_survived":true,"passed":passed});
    let mut file = OpenOptions::new().write(true).create_new(true).open(output)?;
    serde_json::to_writer_pretty(&mut file, &report).map_err(|e| invalid(e.to_string()))?;
    file.write_all(b"\n")?;
    Ok(passed)
}
