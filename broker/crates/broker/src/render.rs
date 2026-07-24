use crate::host_core::approved_artifact::{ApprovedLoadTree, load_v2_load_tree};
use crate::sealed_load_tree::SealedLoadTree;
use crate::secure_launch::{SecureLaunchRequest, secure_launch};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path};
use std::time::{Duration, Instant};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
pub(crate) fn secure_entry(repository: &Path, id: &str) -> io::Result<ApprovedLoadTree> {
    let profile =
        crate::fixture_profiles::find(id).ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker = profile
        .classic_worker
        .ok_or_else(|| invalid("classic render is not approved for profile"))?;
    load_v2_load_tree(repository, id, worker.approval)
}
fn classification(value: &str) -> &str {
    match value {
        "ok" => "ok",
        "crashed" => "crashed",
        "timeout_killed" => "timeout_killed",
        _ => "internal_error",
    }
}
fn expected(case_id: &str) -> Option<&'static str> {
    match case_id {
        "default" => Some("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9"),
        "identity" | "mix_zero" => {
            Some("863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7")
        }
        "horizontal" => Some("82E72A2E7C05E831A45980FA4042940B8B84C4B6CCF020057968E88ACA323779"),
        "vertical_no_repeat" => {
            Some("F35007B74ED682D78EF72A53737BDA0BB4F321EAEB09B76A682B09733EC19351")
        }
        "mixed" => Some("19736FAE645A7CD3BEBE865344E8A066E41040C7AB80042670A8B2EC6F0E1F3D"),
        "amount_max" => Some("8E535435C74A9521D816A3B836DB578A2AE942EFBD80A55447B97610DC26B794"),
        "seed_max" => Some("E31BA13264E801DE7CCCE4D6863215E54C0DC0C7FF4A918E45EE75BC59E817EC"),
        "odd_dimensions" | "padded_stride" => {
            Some("85AC7EB4759281BC81BA60994B58055369CD2224078383D4CAB6A8B685DECE26")
        }
        "connected_map" => Some("A38568761441C209940F81A8C2792DAD50566C66EDA1463BDCF071CCA614891B"),
        "inverted_map" => Some("3BC0C5172B880A8A83CEC24177B78721E9F0619D5330F6A26AAA02B9CC057A08"),
        "partial_extent_hint" => {
            Some("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9")
        }
        "threaded_default" => {
            Some("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9")
        }
        _ => None,
    }
}
pub fn run(
    repository: &Path,
    worker: &Path,
    id: &str,
    case_id: &str,
    output: &Path,
) -> io::Result<bool> {
    crate::trace_policy::validate_broker_trace_directory(repository)?;
    if output
        .components()
        .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid("output traversal forbidden"));
    }
    let root = repository.join("target/render-results");
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
        return Err(invalid("output outside render root"));
    }
    let expected = expected(case_id).ok_or_else(|| invalid("unknown fixed render case"))?;
    let mut reports = Vec::new();
    let mut runs = Vec::new();
    let mut approved_identity = None;
    let mut fixture_sha256 = String::new();
    for _ in 0..2 {
        // secure_launch consumes the tree, so rebuild it from the authenticated
        // receipt for each determinism run rather than reusing mutable state.
        let entry = secure_entry(repository, id)?;
        if entry.worker_path != worker {
            return Err(invalid(
                "render worker differs from approved trusted worker",
            ));
        }
        let plugin_basename = entry.main.relative_basename.clone();
        let plugin_sha256 = entry
            .main
            .expected_sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let worker_sha256 = entry.worker_sha256;
        let worker_byte_size = entry.worker_byte_size;
        let timeout_ms = entry.timeout_ms;
        let tree = SealedLoadTree::create(entry.main, entry.dependencies)?;
        let identity = (
            tree.manifest_digest(),
            worker_sha256,
            worker_byte_size,
            timeout_ms,
        );
        if approved_identity
            .as_ref()
            .is_some_and(|approved| approved != &identity)
        {
            return Err(invalid("render approval changed between determinism runs"));
        }
        approved_identity = Some(identity);
        if fixture_sha256.is_empty() {
            fixture_sha256 = plugin_sha256.to_ascii_uppercase();
        }
        let args_before_plugin = ["--render".into()];
        let args_after_plugin = [plugin_sha256, case_id.to_string()];
        let request = SecureLaunchRequest {
            worker_program: worker,
            worker_expected_sha256: worker_sha256,
            worker_expected_size: worker_byte_size,
            plugin_basename: Some(&plugin_basename),
            args_before_plugin: &args_before_plugin,
            args_after_plugin: &args_after_plugin,
            repository,
            require_module_audit: true,
        };
        let start = Instant::now();
        let result = secure_launch(tree, request, Some(Duration::from_millis(timeout_ms)))?;
        let elapsed = start.elapsed().as_millis().min(30_000) as u64;
        let worker_report: Value =
            serde_json::from_str(result.stdout.trim()).unwrap_or_else(|_| {
                json!({
            "status":"worker_report_unavailable","render_error":-1,"output_sha256":""})
            });
        let hash = worker_report
            .get("output_sha256")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_uppercase();
        let render_error = worker_report
            .get("render_error")
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        runs.push(
            json!({"classification":classification(result.classification.as_str()),
            "render_error":render_error,"output_sha256":hash,"elapsed_ms":elapsed,
            "concurrent_render":worker_report.get("concurrent_render"),
            "thread_1_error":worker_report.get("thread_1_error"),
            "thread_2_error":worker_report.get("thread_2_error"),
            "thread_1_sha256":worker_report.get("thread_1_sha256"),
            "thread_2_sha256":worker_report.get("thread_2_sha256"),
            "thread_1_guards_intact":worker_report.get("thread_1_guards_intact"),
            "thread_2_guards_intact":worker_report.get("thread_2_guards_intact")}),
        );
        reports.push(worker_report);
    }
    let input_hash = reports[0]
        .get("input_sha256")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_uppercase();
    let deterministic = runs[0]["output_sha256"] == runs[1]["output_sha256"];
    let oracle_match = runs.iter().all(|r| r["output_sha256"] == expected);
    let guards = reports
        .iter()
        .all(|r| r.get("guard_bytes_intact") == Some(&Value::Bool(true)));
    let thread_hash_matches = |r: &Value, key: &str| {
        r.get(key)
            .and_then(Value::as_str)
            .is_some_and(|hash| hash.eq_ignore_ascii_case(expected))
    };
    let threaded_valid = case_id != "threaded_default"
        || reports.iter().all(|r| {
            r.get("concurrent_render") == Some(&Value::Bool(true))
                && r.get("thread_1_error") == Some(&json!(0))
                && r.get("thread_2_error") == Some(&json!(0))
                && thread_hash_matches(r, "thread_1_sha256")
                && thread_hash_matches(r, "thread_2_sha256")
                && r.get("thread_1_guards_intact") == Some(&Value::Bool(true))
                && r.get("thread_2_guards_intact") == Some(&Value::Bool(true))
        });
    let passed = deterministic
        && oracle_match
        && guards
        && threaded_valid
        && runs
            .iter()
            .all(|r| r["classification"] == "ok" && r["render_error"] == 0);
    let width = reports[0].get("width").and_then(Value::as_i64).unwrap_or(0);
    let height = reports[0]
        .get("height")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let rowbytes = reports[0]
        .get("rowbytes")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let layout_valid = u64::try_from(width)
        .ok()
        .zip(u64::try_from(height).ok())
        .zip(u64::try_from(rowbytes).ok())
        .is_some_and(|((width, height), rowbytes)| {
            crate::render_request::validate_image_buffer_layout(
                width, height, rowbytes, 4, None, 4096, 16_777_216, 67_108_864,
            )
            .is_ok()
        });
    let passed = passed && layout_valid;
    let report = json!({"schema_version":1,"stage":"classic_render","plugin_id":id,
        "receipt_id":crate::fixture_profiles::find(id).unwrap().classic_worker.unwrap().approval.receipt_id,
        "fixture_sha256":fixture_sha256,
        "case_id":case_id,"pixel_format":"argb8","width":width,"height":height,"rowbytes":rowbytes,
        "input_sha256":input_hash,"run_1":runs[0],"run_2":runs[1],
        "expected_oracle_sha256":expected,"deterministic":deterministic,"oracle_match":oracle_match,
        "guard_bytes_intact":guards,"threaded_render_valid":threaded_valid,"image_layout_valid":layout_valid,
        "broker_survived":true,"passed":passed});
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
    use super::expected;
    #[test]
    fn oracle_table_rejects_unknown_cases() {
        assert!(expected("default").is_some());
        assert!(expected("amount_max").is_some());
        assert!(expected("partial_extent_hint").is_some());
        assert!(expected("threaded_default").is_some());
        assert!(expected("arbitrary").is_none());
    }
}
