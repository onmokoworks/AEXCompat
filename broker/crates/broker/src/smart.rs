use crate::windows_process::run_isolated;
use crate::host_core::approved_artifact::{load, ApprovedArtifact};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path};
use std::time::Duration;

fn expected(case_id: &str) -> Option<&'static str> { match case_id {
    "default" => Some("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9"),
    "identity" => Some("863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7"),
    "horizontal" => Some("82E72A2E7C05E831A45980FA4042940B8B84C4B6CCF020057968E88ACA323779"),
    "vertical_no_repeat" => Some("F35007B74ED682D78EF72A53737BDA0BB4F321EAEB09B76A682B09733EC19351"),
    "mixed" => Some("19736FAE645A7CD3BEBE865344E8A066E41040C7AB80042670A8B2EC6F0E1F3D"),
    "amount_max" => Some("8E535435C74A9521D816A3B836DB578A2AE942EFBD80A55447B97610DC26B794"),
    "seed_max" => Some("E31BA13264E801DE7CCCE4D6863215E54C0DC0C7FF4A918E45EE75BC59E817EC"),
    "mix_zero" => Some("863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7"),
    "odd_dimensions" | "padded_stride" => Some("85AC7EB4759281BC81BA60994B58055369CD2224078383D4CAB6A8B685DECE26"),
    "connected_map" => Some("A38568761441C209940F81A8C2792DAD50566C66EDA1463BDCF071CCA614891B"),
    "inverted_map" => Some("3BC0C5172B880A8A83CEC24177B78721E9F0619D5330F6A26AAA02B9CC057A08"),
    "deep16_default" => Some("FDC0BC732683E9353F9A855D6EA2589B17D43D29D7B538093B474BEC6D5AD026"),
    "float32_default" => Some("D707B9B7BD7C923182A0BEFCA60985896E473AFF3D0191FC310FC07CAD3FE90B"),
    "gpu_fallback_float32" => Some("D707B9B7BD7C923182A0BEFCA60985896E473AFF3D0191FC310FC07CAD3FE90B"),
    "error_missing_input" => Some("346790DBFE3BE4137B9B0B36606E504BCE22FF351867A183DE2FE8481BE1006E"),
    "crash_null_output_world" => Some(""),
    "temporal_context" => Some("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9"),
    "partial_output_request" => Some("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9"),
    _ => None,
} }
fn invalid(s: impl Into<String>) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, s.into()) }

pub fn run(repository: &Path, worker: &Path, id: &str, case_id: &str, output: &Path) -> io::Result<bool> {
    let expected = expected(case_id).ok_or_else(|| invalid("unknown fixed SmartFX case"))?;
    if output.components().any(|p| matches!(p, Component::ParentDir | Component::CurDir)) {
        return Err(invalid("output traversal forbidden"));
    }
    let entry = approved_entry(repository, id)?;
    let root = repository.join("target/smart-results"); fs::create_dir_all(&root)?;
    let output = if output.is_absolute() { output.to_path_buf() } else { repository.join(output) };
    let parent = output.parent().ok_or_else(|| invalid("output parent missing"))?; fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) { return Err(invalid("output outside SmartFX root")); }
    let args = ["--smart".into(), entry.plugin_path.to_string_lossy().into_owned(), entry.sha256.to_ascii_lowercase(), case_id.into()];
    let mut reports = Vec::new();
    for _ in 0..2 {
        let isolated = run_isolated(worker, &args, Duration::from_millis(entry.timeout_ms))?;
        let report: Value = serde_json::from_str(isolated.stdout.trim()).unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        reports.push((isolated.classification, report));
    }
    let hashes: Vec<String> = reports.iter().map(|(_, r)| r.get("output_sha256").and_then(Value::as_str).unwrap_or("").to_ascii_uppercase()).collect();
    let deterministic = hashes[0] == hashes[1];
    let gpu_valid = case_id != "gpu_fallback_float32" || reports.iter().all(|(_, r)|
        r.get("gpu_device_setup_error") == Some(&json!(0)) && r.get("gpu_device_setdown_error") == Some(&json!(0)) &&
        r.get("gpu_render_possible") == Some(&Value::Bool(false)) && r.get("gpu_render_dispatched") == Some(&Value::Bool(false)));
    let expected_error = case_id == "error_missing_input";
    let expected_crash = case_id == "crash_null_output_world";
    let temporal_valid = case_id != "temporal_context" || reports.iter().all(|(_, r)|
        r.get("checkout_time") == Some(&json!(42)) && r.get("checkout_time_step") == Some(&json!(2)) &&
        r.get("checkout_time_scale") == Some(&json!(24)));
    let roi_valid = case_id != "partial_output_request" || reports.iter().all(|(_, r)|
        r.get("roi_contract_valid") == Some(&Value::Bool(true)) &&
        r.get("input_checkout_request") == Some(&json!([3, 2, 11, 8])) &&
        r.get("map_checkout_request") == Some(&json!([3, 2, 11, 8])));
    let error_valid = !expected_error || reports.iter().all(|(c, r)|
        c.as_str() == "nonzero_exit" && r.get("pre_render_error") == Some(&json!(0)) &&
        r.get("smart_render_error").and_then(Value::as_i64).is_some_and(|e| e != 0) &&
        r.get("guard_bytes_intact") == Some(&Value::Bool(true)));
    let crash_valid = !expected_crash || reports.iter().all(|(c, _)| c.as_str() == "crashed");
    let success_valid = expected_error || expected_crash || reports.iter().all(|(c, r)|
        c.as_str() == "ok" && r.get("pre_render_error") == Some(&json!(0)) && r.get("smart_render_error") == Some(&json!(0)) &&
        r.get("result_rects_valid") == Some(&Value::Bool(true)) && r.get("guard_bytes_intact") == Some(&Value::Bool(true)));
    let oracle_valid = expected_crash || hashes.iter().all(|h| h == expected);
    let passed = deterministic && gpu_valid && temporal_valid && roi_valid && error_valid && crash_valid && success_valid && oracle_valid;
    let summary = json!({"schema_version":1,"stage":"smartfx_render","plugin_id":id,"receipt_id":entry.receipt_id,
        "fixture_sha256":entry.sha256.to_ascii_uppercase(),"case_id":case_id,"expected_oracle_sha256":expected,
        "run_1":{"classification":reports[0].0.as_str(),"output_sha256":hashes[0],
            "smart_render_error":reports[0].1.get("smart_render_error"),
            "guard_bytes_intact":reports[0].1.get("guard_bytes_intact"),
            "input_checkout_request":reports[0].1.get("input_checkout_request"),
            "map_checkout_request":reports[0].1.get("map_checkout_request")},
        "run_2":{"classification":reports[1].0.as_str(),"output_sha256":hashes[1],
            "smart_render_error":reports[1].1.get("smart_render_error"),
            "guard_bytes_intact":reports[1].1.get("guard_bytes_intact"),
            "input_checkout_request":reports[1].1.get("input_checkout_request"),
            "map_checkout_request":reports[1].1.get("map_checkout_request")},
        "deterministic":deterministic,"oracle_match":oracle_valid,
        "gpu_negotiation_valid":gpu_valid,"expected_error":expected_error,"error_contract_valid":error_valid,
        "temporal_context_valid":temporal_valid,"roi_contract_valid":roi_valid,"expected_crash":expected_crash,
        "crash_contract_valid":crash_valid,"broker_survived":true,"passed":passed});
    let mut file = OpenOptions::new().write(true).create_new(true).open(output)?;
    serde_json::to_writer_pretty(&mut file, &summary).map_err(|e| invalid(e.to_string()))?; file.write_all(b"\n")?;
    Ok(passed)
}

pub(crate) fn approved_entry(repository: &Path, id: &str) -> io::Result<ApprovedArtifact> {
    let profile = crate::fixture_profiles::find(id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker = profile.smart_worker
        .ok_or_else(|| invalid("SmartFX render is not approved for profile"))?;
    load(repository, id, worker.approval)
}

#[cfg(test)]
mod tests {
    use super::expected;
    #[test]
    fn oracle_table_covers_the_fixed_smartfx_matrix() {
        for case_id in ["default", "identity", "horizontal", "vertical_no_repeat", "mixed", "amount_max", "seed_max", "mix_zero",
                        "odd_dimensions", "padded_stride", "connected_map", "inverted_map", "deep16_default",
                        "float32_default", "gpu_fallback_float32", "error_missing_input"] {
            assert_eq!(expected(case_id).unwrap().len(), 64);
        }
        assert_eq!(expected("crash_null_output_world"), Some(""));
        assert_eq!(expected("temporal_context").unwrap().len(), 64);
        assert_eq!(expected("partial_output_request").unwrap().len(), 64);
        assert!(expected("arbitrary").is_none());
    }
}
