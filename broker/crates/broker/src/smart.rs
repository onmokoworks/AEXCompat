use crate::windows_process::run_isolated;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const EXPECTED: &str = "19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist { schema_version: u32, entries: Vec<Entry> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry { id: String, plugin_path: PathBuf, sha256: String, byte_size: u64,
    approved_stage: String, receipt_id: String, expires: String, timeout_ms: u64 }
fn invalid(s: impl Into<String>) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, s.into()) }

pub fn run(repository: &Path, worker: &Path, id: &str, output: &Path) -> io::Result<bool> {
    if output.components().any(|p| matches!(p, Component::ParentDir | Component::CurDir)) {
        return Err(invalid("output traversal forbidden"));
    }
    let list: Allowlist = serde_json::from_slice(&fs::read(repository.join("target/smart-allowlist/active.local.json"))?)
        .map_err(|e| invalid(e.to_string()))?;
    if list.schema_version != 1 || list.entries.len() != 1 { return Err(invalid("one SmartFX entry required")); }
    let entry = list.entries.into_iter().next().unwrap();
    if entry.id != id || entry.approved_stage != "smartfx_render" ||
       entry.receipt_id != "scattermap-smartfx-20260713-001" || entry.expires != "2026-08-12T23:59:59+09:00" ||
       entry.timeout_ms == 0 || entry.timeout_ms > 5_000 || entry.sha256.len() != 64 {
        return Err(invalid("SmartFX allowlist mismatch"));
    }
    let metadata = fs::metadata(&entry.plugin_path)?;
    if !metadata.is_file() || metadata.len() != entry.byte_size { return Err(invalid("SmartFX fixture size mismatch")); }
    let root = repository.join("target/smart-results"); fs::create_dir_all(&root)?;
    let output = if output.is_absolute() { output.to_path_buf() } else { repository.join(output) };
    let parent = output.parent().ok_or_else(|| invalid("output parent missing"))?; fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) { return Err(invalid("output outside SmartFX root")); }
    let args = ["--smart".into(), entry.plugin_path.to_string_lossy().into_owned(), entry.sha256.to_ascii_lowercase()];
    let mut reports = Vec::new();
    for _ in 0..2 {
        let isolated = run_isolated(worker, &args, Duration::from_millis(entry.timeout_ms))?;
        let report: Value = serde_json::from_str(isolated.stdout.trim()).unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        reports.push((isolated.classification, report));
    }
    let hashes: Vec<String> = reports.iter().map(|(_, r)| r.get("output_sha256").and_then(Value::as_str).unwrap_or("").to_ascii_uppercase()).collect();
    let deterministic = hashes[0] == hashes[1];
    let passed = deterministic && hashes.iter().all(|h| h == EXPECTED) && reports.iter().all(|(c, r)|
        c.as_str() == "ok" && r.get("pre_render_error") == Some(&json!(0)) && r.get("smart_render_error") == Some(&json!(0)) &&
        r.get("result_rects_valid") == Some(&Value::Bool(true)) && r.get("guard_bytes_intact") == Some(&Value::Bool(true)));
    let summary = json!({"schema_version":1,"stage":"smartfx_render","plugin_id":id,"receipt_id":entry.receipt_id,
        "fixture_sha256":entry.sha256.to_ascii_uppercase(),"expected_oracle_sha256":EXPECTED,
        "run_1":{"classification":reports[0].0.as_str(),"output_sha256":hashes[0]},
        "run_2":{"classification":reports[1].0.as_str(),"output_sha256":hashes[1]},
        "deterministic":deterministic,"oracle_match":hashes.iter().all(|h| h == EXPECTED),"broker_survived":true,"passed":passed});
    let mut file = OpenOptions::new().write(true).create_new(true).open(output)?;
    serde_json::to_writer_pretty(&mut file, &summary).map_err(|e| invalid(e.to_string()))?; file.write_all(b"\n")?;
    Ok(passed)
}
