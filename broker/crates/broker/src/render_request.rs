use crate::fixture_profiles::maskoffset::{
    mask_scene_argb8_hash, rectangle_mask_argb8_hash, source_argb8_hash,
};
use crate::fixture_profiles::scattermap::expected_argb8_hash;
use crate::fixture_profiles::{ParameterizedRenderAdapter, RegisteredProfile};
use crate::host_core::descriptor_manifest::{load as load_manifest, LoadedManifest};
use crate::host_core::parameter::{
    apply_defaults, encode_worker_payload, validate_assignments, ParameterValue, PluginProfile,
    ValidatedAssignments, ValidationError, ValueKind,
};
use crate::windows_process::run_isolated;
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const REQUEST_LIMIT: u64 = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    plugin_id: String,
    assignments: Assignments,
}

struct Assignments(BTreeMap<String, ParameterValue>);

impl<'de> Deserialize<'de> for Assignments {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AssignmentsVisitor;

        impl<'de> Visitor<'de> for AssignmentsVisitor {
            type Value = Assignments;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a map of unique parameter ids to numeric values")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut values = BTreeMap::new();
                while let Some((id, value)) = map.next_entry::<String, ParameterValue>()? {
                    if values.insert(id.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate parameter id: {id}"
                        )));
                    }
                }
                Ok(Assignments(values))
            }
        }

        deserializer.deserialize_map(AssignmentsVisitor)
    }
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    gate: &'static str,
    plugin_id: String,
    assignment_count: usize,
    accepted: bool,
    native_dispatch_permitted: bool,
    native_process_started: bool,
    errors: Vec<ValidationError>,
}

fn evaluate(
    request: &Request,
    profile: &PluginProfile,
) -> io::Result<(ValidatedAssignments, usize, Vec<ValidationError>)> {
    if !matches!(request.schema_version, 2 | 3)
        || request.schema_version == 2
            && request
                .assignments
                .0
                .values()
                .any(|value| !matches!(value, ParameterValue::Numeric(_)))
    {
        return Err(invalid("render request identity mismatch"));
    }
    let assignment_count = request.assignments.0.len();
    let (validated, errors) =
        validate_assignments(profile, &request.assignments.0).map_err(invalid)?;
    let effective = apply_defaults(profile, &validated);
    Ok((effective, assignment_count, errors))
}

fn expected_hash(adapter: ParameterizedRenderAdapter, effective: &ValidatedAssignments) -> String {
    match adapter {
        ParameterizedRenderAdapter::ScatterMap => expected_argb8_hash(effective),
        ParameterizedRenderAdapter::MaskOffsetRectangle => rectangle_mask_argb8_hash(effective),
    }
}

fn descriptors(
    repository: &Path,
    plugin_id: &str,
    registered: &RegisteredProfile,
) -> io::Result<LoadedManifest> {
    load_manifest(repository, plugin_id, registered.descriptor_manifest)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn worker_echo_matches(
    report: &Value,
    profile: &PluginProfile,
    effective: &ValidatedAssignments,
) -> bool {
    let Some(items) = report.get("requested_parameters").and_then(Value::as_array) else {
        return false;
    };
    items.len() == profile.descriptors.len()
        && items
            .iter()
            .zip(&profile.descriptors)
            .all(|(item, descriptor)| {
                let expected_kind = match descriptor.kind {
                    ValueKind::Integer => "integer",
                    ValueKind::Float => "float",
                    ValueKind::Color => "color",
                };
                let value_matches = match descriptor.kind {
                    ValueKind::Integer | ValueKind::Float => {
                        item.get("value").and_then(Value::as_f64)
                            == effective
                                .get(&descriptor.id)
                                .and_then(|value| value.numeric())
                    }
                    ValueKind::Color => {
                        item.get("value")
                            == effective
                                .get(&descriptor.id)
                                .and_then(|value| serde_json::to_value(value).ok())
                                .as_ref()
                    }
                };
                item.get("id").and_then(Value::as_str) == Some(descriptor.id.as_str())
                    && item.get("slot").and_then(Value::as_u64) == Some(descriptor.slot as u64)
                    && item.get("kind").and_then(Value::as_str) == Some(expected_kind)
                    && value_matches
            })
}

fn resolve_inside(
    repository: &Path,
    path: &Path,
    relative_root: &str,
    create_parent: bool,
) -> io::Result<PathBuf> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid("path traversal forbidden"));
    }
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repository.join(path)
    };
    if resolved.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(invalid("JSON path required"));
    }
    let root = repository.join(relative_root);
    if create_parent {
        fs::create_dir_all(&root)?;
        fs::create_dir_all(
            resolved
                .parent()
                .ok_or_else(|| invalid("path parent missing"))?,
        )?;
    }
    let parent = resolved
        .parent()
        .ok_or_else(|| invalid("path parent missing"))?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid("path outside broker-owned root"));
    }
    if !create_parent && !resolved.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid("input resolves outside broker-owned root"));
    }
    Ok(resolved)
}

pub fn run(repository: &Path, request_path: &Path, output_path: &Path) -> io::Result<bool> {
    let request_path = resolve_inside(repository, request_path, "target/render-requests", false)?;
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/render-request-results",
        true,
    )?;
    let metadata = fs::metadata(&request_path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > REQUEST_LIMIT {
        return Err(invalid("render request size invalid"));
    }
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)
        .map_err(|error| invalid(format!("invalid render request: {error}")))?;
    let profile = crate::fixture_profiles::find(&request.plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let manifest = descriptors(repository, &request.plugin_id, profile)?;
    let (_, assignment_count, errors) = evaluate(&request, &manifest.profile)?;
    let accepted = errors.is_empty();
    let report = Report {
        schema_version: 1,
        gate: "pre_dispatch_parameter_validation",
        plugin_id: request.plugin_id,
        assignment_count,
        accepted,
        native_dispatch_permitted: accepted,
        native_process_started: false,
        errors,
    };
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(accepted)
}

pub fn execute(repository: &Path, request_path: &Path, output_path: &Path) -> io::Result<bool> {
    let request_path = resolve_inside(repository, request_path, "target/render-requests", false)?;
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/render-request-render-results",
        true,
    )?;
    let metadata = fs::metadata(&request_path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > REQUEST_LIMIT {
        return Err(invalid("render request size invalid"));
    }
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)
        .map_err(|error| invalid(format!("invalid render request: {error}")))?;
    let profile = crate::fixture_profiles::find(&request.plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker_spec = profile
        .classic_worker
        .ok_or_else(|| invalid("classic render is not supported for plugin profile"))?;
    let manifest = descriptors(repository, &request.plugin_id, profile)?;
    let (effective, assignment_count, errors) = evaluate(&request, &manifest.profile)?;
    if !errors.is_empty() {
        let report = json!({"schema_version":1,"stage":"parameterized_classic_render",
            "plugin_id":request.plugin_id,"assignment_count":assignment_count,"accepted":false,
            "native_process_started":false,"errors":errors,"passed":false});
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output_path)?;
        serde_json::to_writer_pretty(&mut output, &report)
            .map_err(|error| invalid(error.to_string()))?;
        output.write_all(b"\n")?;
        return Ok(false);
    }
    let approved = crate::render::entry(repository, &request.plugin_id)?;
    if !manifest
        .plugin_sha256
        .eq_ignore_ascii_case(&approved.sha256)
    {
        return Err(invalid("descriptor manifest plugin digest mismatch"));
    }
    let worker = repository.join(worker_spec.executable);
    let expected = expected_hash(profile.parameterized_render, &effective);
    let args = [
        worker_spec.request_mode.to_string(),
        approved.plugin_path.to_string_lossy().into_owned(),
        approved.sha256.to_ascii_lowercase(),
        encode_worker_payload(&manifest.profile, &effective).map_err(invalid)?,
    ];
    let mut runs = Vec::new();
    for _ in 0..2 {
        let isolated = run_isolated(&worker, &args, Duration::from_millis(approved.timeout_ms))?;
        let report: Value = serde_json::from_str(isolated.stdout.trim())
            .unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        runs.push((isolated.classification, report));
    }
    let valid_run = |classification: crate::ExitClassification, report: &Value| {
        classification.as_str() == "ok"
            && report.get("request_mode") == Some(&Value::Bool(true))
            && report.get("render_error") == Some(&json!(0))
            && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
            && worker_echo_matches(report, &manifest.profile, &effective)
            && report
                .get("output_sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&expected))
    };
    let deterministic = runs[0].1.get("output_sha256") == runs[1].1.get("output_sha256");
    let passed = deterministic
        && runs
            .iter()
            .all(|(classification, report)| valid_run(*classification, report));
    let summarize = |item: &(crate::ExitClassification, Value)| {
        json!({
        "classification":item.0.as_str(),"render_error":item.1.get("render_error"),
        "output_sha256":item.1.get("output_sha256"),"guard_bytes_intact":item.1.get("guard_bytes_intact"),
        "request_mode":item.1.get("request_mode"),
        "requested_parameters":item.1.get("requested_parameters")})
    };
    let report = json!({"schema_version":1,"stage":"parameterized_classic_render",
        "plugin_id":request.plugin_id,"receipt_id":approved.receipt_id,
        "fixture_sha256":approved.sha256.to_ascii_uppercase(),"assignment_count":assignment_count,
        "accepted":true,"native_process_started":true,"parameters":effective,
        "expected_oracle_sha256":expected,
        "run_1":summarize(&runs[0]),"run_2":summarize(&runs[1]),"deterministic":deterministic,
        "broker_survived":true,"passed":passed});
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(passed)
}

pub fn execute_smart(
    repository: &Path,
    request_path: &Path,
    output_path: &Path,
) -> io::Result<bool> {
    let request_path = resolve_inside(repository, request_path, "target/render-requests", false)?;
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/smart-request-render-results",
        true,
    )?;
    let metadata = fs::metadata(&request_path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > REQUEST_LIMIT {
        return Err(invalid("render request size invalid"));
    }
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)
        .map_err(|error| invalid(format!("invalid render request: {error}")))?;
    let profile = crate::fixture_profiles::find(&request.plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker_spec = profile
        .smart_worker
        .ok_or_else(|| invalid("SmartFX render is not supported for plugin profile"))?;
    let manifest = descriptors(repository, &request.plugin_id, profile)?;
    let (effective, assignment_count, errors) = evaluate(&request, &manifest.profile)?;
    if !errors.is_empty() {
        let report = json!({"schema_version":1,"stage":"parameterized_smartfx_render",
            "plugin_id":request.plugin_id,"assignment_count":assignment_count,"accepted":false,
            "native_process_started":false,"errors":errors,"passed":false});
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output_path)?;
        serde_json::to_writer_pretty(&mut output, &report)
            .map_err(|error| invalid(error.to_string()))?;
        output.write_all(b"\n")?;
        return Ok(false);
    }
    let approved = crate::smart::approved_entry(repository, &request.plugin_id)?;
    if !manifest
        .plugin_sha256
        .eq_ignore_ascii_case(&approved.sha256)
    {
        return Err(invalid("descriptor manifest plugin digest mismatch"));
    }
    let worker = repository.join(worker_spec.executable);
    let expected = expected_hash(profile.parameterized_render, &effective);
    let args = [
        worker_spec.request_mode.to_string(),
        approved.plugin_path.to_string_lossy().into_owned(),
        approved.sha256.to_ascii_lowercase(),
        encode_worker_payload(&manifest.profile, &effective).map_err(invalid)?,
    ];
    let mut runs = Vec::new();
    for _ in 0..2 {
        let isolated = run_isolated(&worker, &args, Duration::from_millis(approved.timeout_ms))?;
        let report: Value = serde_json::from_str(isolated.stdout.trim())
            .unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        runs.push((isolated.classification, report));
    }
    let valid_run = |classification: crate::ExitClassification, report: &Value| {
        classification.as_str() == "ok"
            && report.get("request_mode") == Some(&Value::Bool(true))
            && report.get("pre_render_error") == Some(&json!(0))
            && report.get("smart_render_error") == Some(&json!(0))
            && report.get("result_rects_valid") == Some(&Value::Bool(true))
            && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
            && worker_echo_matches(report, &manifest.profile, &effective)
            && report
                .get("output_sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&expected))
    };
    let deterministic = runs[0].1.get("output_sha256") == runs[1].1.get("output_sha256");
    let passed = deterministic
        && runs
            .iter()
            .all(|(classification, report)| valid_run(*classification, report));
    let summarize = |item: &(crate::ExitClassification, Value)| {
        json!({
        "classification":item.0.as_str(),"pre_render_error":item.1.get("pre_render_error"),
        "smart_render_error":item.1.get("smart_render_error"),"output_sha256":item.1.get("output_sha256"),
        "result_rects_valid":item.1.get("result_rects_valid"),
        "guard_bytes_intact":item.1.get("guard_bytes_intact"),"request_mode":item.1.get("request_mode"),
        "requested_parameters":item.1.get("requested_parameters")})
    };
    let report = json!({"schema_version":1,"stage":"parameterized_smartfx_render",
        "plugin_id":request.plugin_id,"receipt_id":approved.receipt_id,
        "fixture_sha256":approved.sha256.to_ascii_uppercase(),"assignment_count":assignment_count,
        "accepted":true,"native_process_started":true,"parameters":effective,
        "expected_oracle_sha256":expected,
        "run_1":summarize(&runs[0]),"run_2":summarize(&runs[1]),"deterministic":deterministic,
        "broker_survived":true,"passed":passed});
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(passed)
}

pub fn execute_smart_suite_fault(
    repository: &Path,
    plugin_id: &str,
    fault_id: &str,
    output_path: &Path,
) -> io::Result<bool> {
    let (worker_mode, expect_crash) = match fault_id {
        "mask_count_error" => ("--smart-mask-count-error-request", false),
        "mask_count_crash" => ("--smart-mask-count-crash-request", true),
        _ => return Err(invalid("unknown fixed suite fault")),
    };
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/smart-suite-fault-results",
        true,
    )?;
    let profile = crate::fixture_profiles::find(plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker_spec = profile
        .smart_worker
        .filter(|spec| spec.request_mode == "--smart-mask-request")
        .ok_or_else(|| invalid("profile has no approved mask-suite fault capability"))?;
    let manifest = descriptors(repository, plugin_id, profile)?;
    let effective = apply_defaults(&manifest.profile, &ValidatedAssignments::new());
    let approved = crate::smart::approved_entry(repository, plugin_id)?;
    if !manifest
        .plugin_sha256
        .eq_ignore_ascii_case(&approved.sha256)
    {
        return Err(invalid("descriptor manifest plugin digest mismatch"));
    }
    let worker = repository.join(worker_spec.executable);
    let args = [
        worker_mode.to_string(),
        approved.plugin_path.to_string_lossy().into_owned(),
        approved.sha256.to_ascii_lowercase(),
        encode_worker_payload(&manifest.profile, &effective).map_err(invalid)?,
    ];
    let mut runs = Vec::new();
    for _ in 0..2 {
        let isolated = run_isolated(&worker, &args, Duration::from_millis(approved.timeout_ms))?;
        let report: Value = serde_json::from_str(isolated.stdout.trim())
            .unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        runs.push((isolated.classification, report));
    }
    let fallback_hash = source_argb8_hash();
    let fallback_valid = |classification: crate::ExitClassification, report: &Value| {
        classification.as_str() == "ok"
            && report.get("pre_render_error") == Some(&json!(0))
            && report.get("smart_render_error") == Some(&json!(0))
            && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
            && worker_echo_matches(report, &manifest.profile, &effective)
            && report
                .get("output_sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&fallback_hash))
    };
    let passed = if expect_crash {
        runs.iter()
            .all(|(classification, _)| classification.as_str() == "crashed")
    } else {
        runs.iter()
            .all(|(classification, report)| fallback_valid(*classification, report))
    };
    let summarize = |item: &(crate::ExitClassification, Value)| {
        json!({
            "classification":item.0.as_str(),
            "pre_render_error":item.1.get("pre_render_error"),
            "smart_render_error":item.1.get("smart_render_error"),
            "output_sha256":item.1.get("output_sha256"),
            "guard_bytes_intact":item.1.get("guard_bytes_intact")
        })
    };
    let report = json!({
        "schema_version":1,"stage":"smartfx_suite_fault","plugin_id":plugin_id,
        "receipt_id":approved.receipt_id,"fixture_sha256":approved.sha256.to_ascii_uppercase(),
        "fault_id":fault_id,"expected_outcome":if expect_crash { "worker_crash" } else { "plugin_fallback" },
        "expected_fallback_sha256":if expect_crash { Value::Null } else { json!(fallback_hash) },
        "run_1":summarize(&runs[0]),"run_2":summarize(&runs[1]),
        "broker_survived":true,"passed":passed
    });
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(passed)
}

pub fn execute_smart_mask_scene(
    repository: &Path,
    plugin_id: &str,
    scene_case_id: &str,
    output_path: &Path,
) -> io::Result<bool> {
    let (scene_id, mask_index, expected_count) = match scene_case_id {
        "empty" => ("empty", 1.0, 0),
        "translated_rectangle" => ("translated_rectangle", 1.0, 1),
        "two_rectangles_first" => ("two_rectangles", 1.0, 2),
        "two_rectangles_second" => ("two_rectangles", 2.0, 2),
        _ => return Err(invalid("unknown fixed mask scene")),
    };
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/smart-mask-scene-results",
        true,
    )?;
    let profile = crate::fixture_profiles::find(plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker_spec = profile
        .smart_worker
        .filter(|spec| spec.request_mode == "--smart-mask-request")
        .ok_or_else(|| invalid("profile has no approved mask-scene capability"))?;
    let manifest = descriptors(repository, plugin_id, profile)?;
    let mut effective = apply_defaults(&manifest.profile, &ValidatedAssignments::new());
    effective.insert("mask_index".into(), ParameterValue::Numeric(mask_index));
    let expected = mask_scene_argb8_hash(&effective, scene_id)
        .ok_or_else(|| invalid("mask scene has no independent oracle"))?;
    let approved = crate::smart::approved_entry(repository, plugin_id)?;
    if !manifest
        .plugin_sha256
        .eq_ignore_ascii_case(&approved.sha256)
    {
        return Err(invalid("descriptor manifest plugin digest mismatch"));
    }
    let worker = repository.join(worker_spec.executable);
    let args = [
        "--smart-mask-scene-request".to_string(),
        approved.plugin_path.to_string_lossy().into_owned(),
        approved.sha256.to_ascii_lowercase(),
        encode_worker_payload(&manifest.profile, &effective).map_err(invalid)?,
        scene_id.to_string(),
    ];
    let mut runs = Vec::new();
    for _ in 0..2 {
        let isolated = run_isolated(&worker, &args, Duration::from_millis(approved.timeout_ms))?;
        let report: Value = serde_json::from_str(isolated.stdout.trim())
            .unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        runs.push((isolated.classification, report));
    }
    let valid_run = |classification: crate::ExitClassification, report: &Value| {
        classification.as_str() == "ok"
            && report.get("pre_render_error") == Some(&json!(0))
            && report.get("smart_render_error") == Some(&json!(0))
            && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
            && report.get("mask_scene_id").and_then(Value::as_str) == Some(scene_id)
            && report.get("mask_count").and_then(Value::as_u64) == Some(expected_count)
            && worker_echo_matches(report, &manifest.profile, &effective)
            && report
                .get("output_sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&expected))
    };
    let deterministic = runs[0].1.get("output_sha256") == runs[1].1.get("output_sha256");
    let passed = deterministic
        && runs
            .iter()
            .all(|(classification, report)| valid_run(*classification, report));
    let summarize = |item: &(crate::ExitClassification, Value)| {
        json!({
            "classification":item.0.as_str(),
            "pre_render_error":item.1.get("pre_render_error"),
            "smart_render_error":item.1.get("smart_render_error"),
            "output_sha256":item.1.get("output_sha256"),
            "guard_bytes_intact":item.1.get("guard_bytes_intact"),
            "mask_scene_id":item.1.get("mask_scene_id"),
            "mask_count":item.1.get("mask_count")
        })
    };
    let report = json!({
        "schema_version":1,"stage":"smartfx_mask_scene","plugin_id":plugin_id,
        "receipt_id":approved.receipt_id,"fixture_sha256":approved.sha256.to_ascii_uppercase(),
        "scene_case_id":scene_case_id,"host_scene_id":scene_id,"mask_index":mask_index,
        "expected_mask_count":expected_count,"expected_oracle_sha256":expected,
        "run_1":summarize(&runs[0]),"run_2":summarize(&runs[1]),
        "deterministic":deterministic,"broker_survived":true,"passed":passed
    });
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn repository() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-render-request-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("target/render-requests")).unwrap();
        fs::create_dir_all(root.join("profiles/scattermap")).unwrap();
        fs::create_dir_all(root.join("profiles/maskoffset")).unwrap();
        fs::write(
            root.join("profiles/scattermap/parameter_descriptors.json"),
            include_bytes!("../../../../profiles/scattermap/parameter_descriptors.json"),
        )
        .unwrap();
        fs::write(
            root.join("profiles/maskoffset/parameter_descriptors.json"),
            include_bytes!("../../../../profiles/maskoffset/parameter_descriptors.json"),
        )
        .unwrap();
        root
    }

    #[test]
    fn generic_worker_echo_is_descriptor_order_and_value_bound() {
        let profile = PluginProfile {
            id: "example".into(),
            descriptors: vec![crate::host_core::parameter::Descriptor {
                id: "radius".into(),
                display_name: "Radius".into(),
                slot: 3,
                observed_type: 10,
                minimum: Some(0.0),
                maximum: Some(10.0),
                default_value: ParameterValue::Numeric(2.5),
                kind: ValueKind::Float,
            }],
        };
        let effective =
            ValidatedAssignments::from([("radius".into(), ParameterValue::Numeric(2.5))]);
        let valid = json!({"requested_parameters":[
            {"id":"radius","slot":3,"kind":"float","value":2.5}
        ]});
        assert!(worker_echo_matches(&valid, &profile, &effective));
        let drifted = json!({"requested_parameters":[
            {"id":"radius","slot":4,"kind":"float","value":2.5}
        ]});
        assert!(!worker_echo_matches(&drifted, &profile, &effective));
    }

    #[test]
    fn accepted_request_still_does_not_start_native_code() {
        let root = repository();
        let request = root.join("target/render-requests/valid.json");
        let output = root.join("target/render-request-results/valid.json");
        fs::write(&request, br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"amount":500,"direction":1,"seed":10000,"mix":0,"invert_map":1}}"#).unwrap();
        assert!(run(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(report["native_dispatch_permitted"], true);
        assert_eq!(report["native_process_started"], false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn color_requires_v3_and_is_accepted_without_native_dispatch() {
        let root = repository();
        let request = root.join("target/render-requests/color.json");
        let output = root.join("target/render-request-results/color.json");
        let assignments = r#""fill_color":{"alpha":255,"red":20,"green":180,"blue":70}"#;
        fs::write(
            &request,
            format!(
                r#"{{"schema_version":2,"plugin_id":"maskoffset","assignments":{{{assignments}}}}}"#
            ),
        )
        .unwrap();
        assert!(run(&root, &request, &output).is_err());
        assert!(!output.exists());
        fs::write(
            &request,
            format!(
                r#"{{"schema_version":3,"plugin_id":"maskoffset","assignments":{{{assignments}}}}}"#
            ),
        )
        .unwrap();
        assert!(run(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(report["accepted"], true);
        assert_eq!(report["native_process_started"], false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_suite_fault_fails_before_output_or_native_lookup() {
        let root = repository();
        let output = root.join("target/smart-suite-fault-results/unknown.json");
        assert!(execute_smart_suite_fault(&root, "maskoffset", "arbitrary", &output).is_err());
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_mask_scene_fails_before_output_or_native_lookup() {
        let root = repository();
        let output = root.join("target/smart-mask-scene-results/unknown.json");
        assert!(execute_smart_mask_scene(&root, "maskoffset", "arbitrary", &output).is_err());
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejected_request_records_no_native_process_and_create_new_output() {
        let root = repository();
        let request = root.join("target/render-requests/rejected.json");
        let output = root.join("target/render-request-results/rejected.json");
        fs::write(
            &request,
            br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"direction":4}}"#,
        )
        .unwrap();
        assert!(!run(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(report["errors"][0]["code"], "parameter_out_of_range");
        assert_eq!(report["native_process_started"], false);
        assert!(run(&root, &request, &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strict_json_rejects_unknown_and_duplicate_assignments() {
        for (name, body) in [
            ("unknown", br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"other":1}}"#.as_slice()),
            ("duplicate", br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"direction":1,"direction":2}}"#.as_slice()),
        ] {
            let root = repository();
            let request = root.join(format!("target/render-requests/{name}.json"));
            let output = root.join(format!("target/render-request-results/{name}.json"));
            fs::write(&request, body).unwrap();
            assert!(run(&root, &request, &output).is_err());
            assert!(!output.exists());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn unknown_plugin_profile_fails_before_any_output() {
        let root = repository();
        let request = root.join("target/render-requests/unknown-plugin.json");
        let output = root.join("target/render-request-results/unknown-plugin.json");
        fs::write(
            &request,
            br#"{"schema_version":2,"plugin_id":"unknown-aex","assignments":{}}"#,
        )
        .unwrap();
        assert!(run(&root, &request, &output).is_err());
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn requires_json_paths_inside_broker_owned_roots() {
        let root = repository();
        let request = root.join("target/render-requests/request.txt");
        let output = root.join("target/render-request-results/report.json");
        fs::write(&request, b"{}").unwrap();
        assert!(run(&root, &request, &output).is_err());
        assert!(!output.exists());
        assert!(run(&root, &root.join("outside.json"), &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn execution_route_rejects_before_worker_launch() {
        let root = repository();
        let request = root.join("target/render-requests/rejected-execution.json");
        let output = root.join("target/render-request-render-results/rejected.json");
        fs::write(
            &request,
            br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"direction":4}}"#,
        )
        .unwrap();
        assert!(!execute(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(report["native_process_started"], false);
        assert_eq!(report["passed"], false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn smart_execution_route_rejects_before_worker_launch() {
        let root = repository();
        let request = root.join("target/render-requests/rejected-smart-execution.json");
        let output = root.join("target/smart-request-render-results/rejected.json");
        fs::write(
            &request,
            br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"mix":100.1}}"#,
        )
        .unwrap();
        assert!(!execute_smart(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(report["stage"], "parameterized_smartfx_render");
        assert_eq!(report["native_process_started"], false);
        fs::remove_dir_all(root).unwrap();
    }
}
