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
        || entry.receipt_id != "scattermap-extended-render-20260713-001"
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
fn expected(case_id: &str) -> Option<&'static str> { match case_id {
    "default" => Some("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9"),
    "identity" | "mix_zero" => Some("863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7"),
    "horizontal" => Some("82E72A2E7C05E831A45980FA4042940B8B84C4B6CCF020057968E88ACA323779"),
    "vertical_no_repeat" => Some("F35007B74ED682D78EF72A53737BDA0BB4F321EAEB09B76A682B09733EC19351"),
    "mixed" => Some("19736FAE645A7CD3BEBE865344E8A066E41040C7AB80042670A8B2EC6F0E1F3D"),
    "amount_max" => Some("8E535435C74A9521D816A3B836DB578A2AE942EFBD80A55447B97610DC26B794"),
    "seed_max" => Some("E31BA13264E801DE7CCCE4D6863215E54C0DC0C7FF4A918E45EE75BC59E817EC"),
    "odd_dimensions" | "padded_stride" => Some("85AC7EB4759281BC81BA60994B58055369CD2224078383D4CAB6A8B685DECE26"),
    "connected_map" => Some("A38568761441C209940F81A8C2792DAD50566C66EDA1463BDCF071CCA614891B"),
    "inverted_map" => Some("3BC0C5172B880A8A83CEC24177B78721E9F0619D5330F6A26AAA02B9CC057A08"),
    "partial_extent_hint" => Some("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9"),
    _ => None,
} }
pub fn run(repository: &Path, worker: &Path, id: &str, case_id: &str, output: &Path) -> io::Result<bool> {
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
    let expected = expected(case_id).ok_or_else(|| invalid("unknown fixed render case"))?;
    let args = ["--render".into(), entry.plugin_path.to_string_lossy().into_owned(),
        entry.sha256.to_ascii_lowercase(), case_id.to_string()];
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
    let oracle_match = runs.iter().all(|r| r["output_sha256"] == expected);
    let guards = reports.iter().all(|r| r.get("guard_bytes_intact") == Some(&Value::Bool(true)));
    let passed = deterministic && oracle_match && guards && runs.iter().all(|r| r["classification"] == "ok" && r["render_error"] == 0);
    let width = reports[0].get("width").and_then(Value::as_i64).unwrap_or(0);
    let height = reports[0].get("height").and_then(Value::as_i64).unwrap_or(0);
    let rowbytes = reports[0].get("rowbytes").and_then(Value::as_i64).unwrap_or(0);
    let report = json!({"schema_version":1,"stage":"classic_render","plugin_id":id,
        "receipt_id":entry.receipt_id,"fixture_sha256":entry.sha256.to_ascii_uppercase(),
        "case_id":case_id,"pixel_format":"argb8","width":width,"height":height,"rowbytes":rowbytes,
        "input_sha256":input_hash,"run_1":runs[0],"run_2":runs[1],
        "expected_oracle_sha256":expected,"deterministic":deterministic,"oracle_match":oracle_match,
        "guard_bytes_intact":guards,"broker_survived":true,"passed":passed});
    let mut file = OpenOptions::new().write(true).create_new(true).open(output)?;
    serde_json::to_writer_pretty(&mut file, &report).map_err(|e| invalid(e.to_string()))?;
    file.write_all(b"\n")?;
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::expected;
    #[test]
    fn oracle_table_rejects_unknown_cases() {
        assert!(expected("default").is_some());
        assert!(expected("amount_max").is_some());
        assert!(expected("partial_extent_hint").is_some());
        assert!(expected("arbitrary").is_none());
    }
}
