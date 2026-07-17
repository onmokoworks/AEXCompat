#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant, SystemTime},
};

const SCATTERMAP_HASH: &str = "223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB";
const MASKOFFSET_HASH: &str = "B7C41F4F906FCE74B26BD2F06520F6BFDF75DCD1A2682DBB85D50D1DE833877B";

fn profile_for_hash(hash: &str) -> Option<&'static str> {
    match hash {
        SCATTERMAP_HASH => Some("scattermap"),
        MASKOFFSET_HASH => Some("maskoffset"),
        _ => None,
    }
}

fn decode_sha256(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("SHA-256 must be exactly 64 hexadecimal characters".into());
    }
    let mut digest = [0; 32];
    for (output, pair) in digest.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
        *output = u8::from_str_radix(pair, 16).map_err(|error| error.to_string())?;
    }
    Ok(digest)
}

struct Selection {
    path: PathBuf,
    size: u64,
    sha256: String,
    profile: Option<&'static str>,
    modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
struct SessionDependency {
    path: PathBuf,
    size: u64,
    sha256: String,
}

fn discover_adjacent_imports(aex_path: &Path) -> Result<Vec<SessionDependency>, String> {
    const MAX_DEPENDENCIES: usize = 64;
    let directory = aex_path
        .parent()
        .ok_or("Selected AEX has no parent directory")?;
    let mut adjacent = std::collections::HashMap::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("dll"))
        {
            let Some(name) = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            let key = name.to_ascii_lowercase();
            if adjacent.insert(key, path).is_some() {
                return Err(format!(
                    "Ambiguous case-insensitive dependency name: {name}"
                ));
            }
        }
    }

    let mut pending = vec![aex_path.to_path_buf()];
    let mut visited = std::collections::HashSet::new();
    let mut dependencies = Vec::new();
    while let Some(module_path) = pending.pop() {
        let bytes = fs::read(&module_path).map_err(|error| error.to_string())?;
        let pe = goblin::pe::PE::parse(&bytes).map_err(|error| {
            format!(
                "Could not inspect PE imports for {}: {error}",
                module_path.display()
            )
        })?;
        for imported_name in pe.libraries {
            let key = imported_name.to_ascii_lowercase();
            let Some(path) = adjacent.get(&key) else {
                continue;
            };
            if !visited.insert(key) {
                continue;
            }
            if dependencies.len() == MAX_DEPENDENCIES {
                return Err(format!(
                    "Adjacent dependency graph exceeds {MAX_DEPENDENCIES} DLLs"
                ));
            }
            let dependency_bytes = fs::read(path).map_err(|error| error.to_string())?;
            dependencies.push(SessionDependency {
                path: path.clone(),
                size: dependency_bytes.len() as u64,
                sha256: format!("{:X}", Sha256::digest(&dependency_bytes)),
            });
            pending.push(path.clone());
        }
    }
    dependencies.sort_by(|left, right| {
        left.path
            .to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.path.to_string_lossy().to_ascii_lowercase())
    });
    Ok(dependencies)
}

struct TaskResult {
    success: bool,
    body: String,
    output: Option<PathBuf>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum TaskKind {
    #[default]
    Generic,
    IdentifyAex,
    InspectParameters,
}

#[derive(Debug, Default, PartialEq)]
struct RenderDiagnostics {
    render_path: String,
    pixel_format: String,
    worker_classification: String,
    gpu_fallback_used: bool,
    gpu_attempt_classification: Option<String>,
    gpu_failure_stage: Option<String>,
    final_stages: Vec<String>,
    gpu_stages: Vec<String>,
}

#[derive(Debug, PartialEq)]
struct FailureDiagnostics {
    classification: String,
    failure_stage: Option<String>,
    exit_code: Option<i64>,
    elapsed_ms: Option<u64>,
    selector_error: Option<i64>,
    stages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct PixelComparison {
    width: u32,
    height: u32,
    differing_pixels: u64,
    max_channel_error: u8,
    mean_absolute_error: f64,
}

impl PixelComparison {
    fn exact(&self) -> bool {
        self.differing_pixels == 0
    }

    fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "stage": "ae_reference_pixel_comparison",
            "status": if self.exact() { "pixel_exact" } else { "pixel_difference" },
            "pixel_exact": self.exact(),
            "width": self.width,
            "height": self.height,
            "pixel_count": u64::from(self.width) * u64::from(self.height),
            "differing_pixels": self.differing_pixels,
            "max_channel_error": self.max_channel_error,
            "mean_absolute_error": self.mean_absolute_error,
        })
    }
}

fn compare_images(reference: &Path, output: &Path) -> Result<PixelComparison, String> {
    let reference = image::open(reference)
        .map_err(|error| format!("Reference image could not be decoded: {error}"))?
        .into_rgba8();
    let output = image::open(output)
        .map_err(|error| format!("AEX output could not be decoded: {error}"))?
        .into_rgba8();
    if reference.dimensions() != output.dimensions() {
        return Err(format!(
            "Image size mismatch: AE reference is {}x{}, AEX output is {}x{}",
            reference.width(),
            reference.height(),
            output.width(),
            output.height()
        ));
    }
    let mut differing_pixels = 0u64;
    let mut absolute_error = 0u64;
    let mut max_channel_error = 0u8;
    for (reference, output) in reference
        .as_raw()
        .chunks_exact(4)
        .zip(output.as_raw().chunks_exact(4))
    {
        let mut pixel_differs = false;
        for channel in 0..4 {
            let error = reference[channel].abs_diff(output[channel]);
            absolute_error += u64::from(error);
            max_channel_error = max_channel_error.max(error);
            pixel_differs |= error != 0;
        }
        differing_pixels += u64::from(pixel_differs);
    }
    let channel_count = u64::from(reference.width()) * u64::from(reference.height()) * 4;
    Ok(PixelComparison {
        width: reference.width(),
        height: reference.height(),
        differing_pixels,
        max_channel_error,
        mean_absolute_error: absolute_error as f64 / channel_count.max(1) as f64,
    })
}

fn assign_layer_paths(
    parameters: &mut [aexcompat_broker::image_render::InteractiveParameter],
    assignments: &[std::ffi::OsString],
) -> Result<(), String> {
    if assignments.is_empty() || assignments.len() % 2 != 0 {
        return Err("layer assignments must be SLOT IMAGE pairs".into());
    }
    let mut assigned_slots = Vec::new();
    let mut planned = Vec::new();
    for assignment in assignments.chunks_exact(2) {
        let slot = assignment[0]
            .to_string_lossy()
            .parse::<u32>()
            .map_err(|_| "layer slot must be a positive integer".to_owned())?;
        if slot == 0 {
            return Err("layer slot must be a positive integer".into());
        }
        if assigned_slots.contains(&slot) {
            return Err(format!("layer slot {slot} was assigned more than once"));
        }
        let index = parameters
            .iter()
            .position(|item| item.slot == slot)
            .ok_or_else(|| format!("AEX exposes no parameter at slot {slot}"))?;
        if parameters[index].kind != "layer" {
            return Err(format!("parameter slot {slot} is not a Layer input"));
        }
        planned.push((index, PathBuf::from(&assignment[1])));
        assigned_slots.push(slot);
    }
    for (index, path) in planned {
        parameters[index].layer_path = Some(path);
    }
    Ok(())
}

fn apply_typed_assignments(
    parameters: &mut Vec<aexcompat_broker::image_render::InteractiveParameter>,
    document: &serde_json::Value,
) -> Result<(), String> {
    let root = document
        .as_object()
        .ok_or_else(|| "assignment document must be an object".to_owned())?;
    if root.keys().any(|key| {
        !matches!(
            key.as_str(),
            "schema_version" | "assignments" | "timing" | "host_context"
        )
    }) || root
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err("assignment document schema is invalid".into());
    }
    let assignments = root
        .get("assignments")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "assignment document has no assignments array".to_owned())?;
    if assignments.len() > 1024 {
        return Err("assignment count exceeds the parameter limit".into());
    }
    let mut updated = parameters.clone();
    let mut assigned_slots = Vec::new();
    for assignment in assignments {
        let object = assignment
            .as_object()
            .ok_or_else(|| "each assignment must be an object".to_owned())?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "slot" | "value" | "color" | "components" | "layer" | "text"
            )
        }) {
            return Err("assignment contains an unknown field".into());
        }
        let slot = object
            .get("slot")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value != 0)
            .ok_or_else(|| "assignment slot must be a positive integer".to_owned())?;
        if assigned_slots.contains(&slot) {
            return Err(format!("parameter slot {slot} was assigned more than once"));
        }
        let parameter = updated
            .iter_mut()
            .find(|item| item.slot == slot)
            .ok_or_else(|| format!("AEX exposes no parameter at slot {slot}"))?;
        let value_field_count = ["value", "color", "components", "layer", "text"]
            .into_iter()
            .filter(|field| object.contains_key(*field))
            .count();
        if value_field_count != 1 {
            return Err(format!(
                "parameter slot {slot} must have exactly one typed value"
            ));
        }
        match parameter.kind.as_str() {
            "integer" | "float" | "path" => {
                let value = object
                    .get("value")
                    .and_then(serde_json::Value::as_f64)
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| format!("parameter slot {slot} requires a finite value"))?;
                if value < parameter.minimum
                    || value > parameter.maximum
                    || (matches!(parameter.kind.as_str(), "integer" | "path")
                        && value.fract() != 0.0)
                {
                    return Err(format!("parameter slot {slot} value is out of range"));
                }
                parameter.value = value;
            }
            "color" => {
                let values = object
                    .get("color")
                    .and_then(serde_json::Value::as_array)
                    .filter(|values| values.len() == 4)
                    .ok_or_else(|| format!("parameter slot {slot} requires ARGB8 color"))?;
                let mut color = [0u8; 4];
                for (index, value) in values.iter().enumerate() {
                    color[index] = value
                        .as_u64()
                        .and_then(|value| u8::try_from(value).ok())
                        .ok_or_else(|| format!("parameter slot {slot} color is invalid"))?;
                }
                parameter.color = color;
            }
            "angle" | "point" | "point3d" => {
                let expected = match parameter.kind.as_str() {
                    "angle" => 1,
                    "point" => 2,
                    _ => 3,
                };
                let values = object
                    .get("components")
                    .and_then(serde_json::Value::as_array)
                    .filter(|values| values.len() == expected)
                    .ok_or_else(|| format!("parameter slot {slot} component count is invalid"))?;
                for (index, value) in values.iter().enumerate() {
                    let value = value
                        .as_f64()
                        .filter(|value| {
                            value.is_finite() && *value >= -32768.0 && *value <= 32768.0
                        })
                        .ok_or_else(|| format!("parameter slot {slot} component is invalid"))?;
                    parameter.components[index] = value;
                }
            }
            "layer" => {
                let path = object
                    .get("layer")
                    .and_then(serde_json::Value::as_str)
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| format!("parameter slot {slot} requires a layer path"))?;
                parameter.layer_path = Some(PathBuf::from(path));
            }
            "arbitrary_data" => {
                let text = object
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .filter(|text| !text.is_empty() && text.len() <= 4096 && !text.contains('\0'))
                    .ok_or_else(|| format!("parameter slot {slot} requires bounded text"))?;
                parameter.debug_summary = Some(text.to_owned());
            }
            _ => return Err(format!("parameter slot {slot} is not assignable")),
        }
        assigned_slots.push(slot);
    }
    *parameters = updated;
    Ok(())
}

fn typed_request_timing(
    document: &serde_json::Value,
) -> Result<aexcompat_broker::image_render::RenderTiming, String> {
    let Some(timing) = document.get("timing") else {
        return Ok(aexcompat_broker::image_render::RenderTiming::default());
    };
    let timing = timing
        .as_object()
        .ok_or_else(|| "timing must be an object".to_owned())?;
    if timing.keys().any(|key| {
        !matches!(
            key.as_str(),
            "frame" | "fps" | "time_scale" | "time_step" | "duration_frames"
        )
    }) {
        return Err("timing contains an unknown field".into());
    }
    let frame = timing
        .get("frame")
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| (0..=10_000_000).contains(value))
        .ok_or_else(|| "timing frame must be an integer within 0..=10000000".to_owned())?;
    let (time_scale, time_step) = match (
        timing.get("fps"),
        timing.get("time_scale"),
        timing.get("time_step"),
    ) {
        (Some(fps), None, None) => (
            fps.as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| (1..=1000).contains(value))
                .ok_or_else(|| "timing fps must be an integer within 1..=1000".to_owned())?,
            1,
        ),
        (None, Some(time_scale), Some(time_step)) => (
            time_scale
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| (1..=1_000_000).contains(value))
                .ok_or_else(|| {
                    "timing time_scale must be an integer within 1..=1000000".to_owned()
                })?,
            time_step
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .filter(|value| (1..=100_000).contains(value))
                .ok_or_else(|| {
                    "timing time_step must be an integer within 1..=100000".to_owned()
                })?,
        ),
        _ => return Err("timing requires either fps or the time_scale/time_step pair".to_owned()),
    };
    let duration_frames = match timing.get("duration_frames") {
        Some(value) => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .filter(|value| *value > frame && *value <= 10_000_001)
            .ok_or_else(|| {
                "timing duration_frames must be an integer greater than frame and at most 10000001"
                    .to_owned()
            })?,
        None => frame.saturating_add(1),
    };
    render_timing(frame, duration_frames, time_scale, time_step)
}

fn typed_request_host_context(
    document: &serde_json::Value,
) -> Result<Option<aexcompat_broker::render_request::HostContext>, String> {
    document
        .get("host_context")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| format!("host_context is invalid: {error}"))
}

fn render_timing(
    frame: i32,
    duration_frames: i32,
    time_scale: u32,
    time_step: i32,
) -> Result<aexcompat_broker::image_render::RenderTiming, String> {
    if frame < 0
        || duration_frames <= frame
        || time_scale == 0
        || time_step <= 0
        || time_scale > 1_000_000
        || time_step > 100_000
    {
        return Err("render timing is outside the supported range".into());
    }
    let current_time = frame
        .checked_mul(time_step)
        .ok_or_else(|| "current_time exceeds the 32-bit AE time range".to_owned())?;
    let total_time = duration_frames
        .checked_mul(time_step)
        .ok_or_else(|| "total_time exceeds the 32-bit AE time range".to_owned())?;
    Ok(aexcompat_broker::image_render::RenderTiming {
        current_time,
        time_step,
        total_time,
        time_scale,
    })
}

fn typed_request_document(
    parameters: &[aexcompat_broker::image_render::InteractiveParameter],
    frame: i32,
    time_scale: u32,
    time_step: i32,
    duration_frames: i32,
    host_context: Option<&aexcompat_broker::render_request::HostContext>,
) -> serde_json::Value {
    let assignments = parameters
        .iter()
        .filter_map(|parameter| {
            let mut assignment = serde_json::Map::new();
            assignment.insert("slot".into(), serde_json::json!(parameter.slot));
            match parameter.kind.as_str() {
                "integer" | "float" | "path" => {
                    assignment.insert("value".into(), serde_json::json!(parameter.value));
                }
                "color" => {
                    assignment.insert("color".into(), serde_json::json!(parameter.color));
                }
                "angle" | "point" | "point3d" => {
                    let components = parameter.components.get(..parameter.component_count)?;
                    assignment.insert("components".into(), serde_json::json!(components));
                }
                "layer" => {
                    let path = parameter.layer_path.as_ref()?;
                    assignment.insert("layer".into(), serde_json::json!(path.to_string_lossy()));
                }
                "arbitrary_data" => {
                    let text = parameter.debug_summary.as_ref()?;
                    assignment.insert("text".into(), serde_json::json!(text));
                }
                _ => return None,
            }
            Some(serde_json::Value::Object(assignment))
        })
        .collect::<Vec<_>>();
    let timing = if time_step == 1 && time_scale <= 1000 {
        serde_json::json!({
            "frame": frame, "fps": time_scale, "duration_frames": duration_frames
        })
    } else {
        serde_json::json!({
            "frame": frame, "time_scale": time_scale, "time_step": time_step,
            "duration_frames": duration_frames
        })
    };
    let mut document = serde_json::json!({
        "schema_version": 1,
        "timing": timing,
        "assignments": assignments,
    });
    if let Some(context) = host_context {
        document["host_context"] = serde_json::to_value(context).unwrap_or_default();
    }
    document
}

impl Default for FailureDiagnostics {
    fn default() -> Self {
        Self {
            classification: "host_validation_error".into(),
            failure_stage: None,
            exit_code: None,
            elapsed_ms: None,
            selector_error: None,
            stages: Vec::new(),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct MatrixCase {
    render_path: String,
    pixel_format: String,
    passed: bool,
    applicable: bool,
    classification: String,
    failure_stage: Option<String>,
    selector_error: Option<i64>,
    output_png: Option<String>,
    output_relation: Option<String>,
    differing_input_pixels: Option<u64>,
    error: Option<String>,
}

fn compatibility_matrix(report: &serde_json::Value) -> Option<Vec<MatrixCase>> {
    (report.get("stage")?.as_str()? == "effect_compatibility_matrix").then(|| {
        report["cases"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|case| MatrixCase {
                render_path: case["render_path"].as_str().unwrap_or("unknown").to_owned(),
                pixel_format: case["pixel_format"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
                passed: case["passed"].as_bool().unwrap_or(false),
                applicable: case["applicable"].as_bool().unwrap_or(true),
                classification: case["classification"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_owned(),
                failure_stage: case["failure_stage"].as_str().map(str::to_owned),
                selector_error: case["selector_error"].as_i64(),
                output_png: case["output_png"].as_str().map(str::to_owned),
                output_relation: case["output_relation"].as_str().map(str::to_owned),
                differing_input_pixels: case["differing_input_pixels"].as_u64(),
                error: case["error"].as_str().map(str::to_owned),
            })
            .collect()
    })
}

fn run_effect_matrix(
    repository: &Path,
    plugin_path: &Path,
    hash: &str,
    input: &Path,
    output_root: &Path,
    parameters: &[aexcompat_broker::image_render::InteractiveParameter],
    timing: aexcompat_broker::image_render::RenderTiming,
    reference_root: Option<&Path>,
    host_context: Option<&aexcompat_broker::render_request::HostContext>,
) -> serde_json::Value {
    use aexcompat_broker::image_render::RenderPixelFormat;
    let formats = [
        (RenderPixelFormat::Argb8, "argb8"),
        (RenderPixelFormat::Argb16, "argb16"),
        (RenderPixelFormat::Argb32f, "argb32f"),
    ];
    let mut cases = Vec::with_capacity(6);
    for smart in [false, true] {
        for (pixel_format, format_name) in formats {
            let path_name = if smart { "smartfx" } else { "classic" };
            let output = output_root.join(format!("{path_name}-{format_name}.png"));
            let result =
                aexcompat_broker::image_render::render_experimental_image_at_time_with_format_and_context(
                    repository,
                    plugin_path,
                    hash,
                    input,
                    &output,
                    parameters,
                    timing,
                    smart,
                    pixel_format,
                    host_context,
                );
            cases.push(match result {
                Ok(report) => {
                    let mut case = serde_json::json!({
                        "render_path": path_name,
                        "pixel_format": format_name,
                        "render_completed": true,
                        "applicable": true,
                        "passed": true,
                        "classification": report["worker_classification"],
                        "failure_stage": serde_json::Value::Null,
                        "selector_error": serde_json::Value::Null,
                        "output_png": output,
                    });
                    if report["width"] != report["input_width"]
                        || report["height"] != report["input_height"]
                    {
                        case["output_relation"] = serde_json::json!("different_dimensions");
                        case["differing_input_pixels"] = serde_json::Value::Null;
                    } else {
                        match compare_images(input, &output) {
                            Ok(comparison) => {
                                case["output_relation"] =
                                    serde_json::json!(if comparison.exact() {
                                        "pixel_exact_passthrough"
                                    } else {
                                        "pixels_changed"
                                    });
                                case["differing_input_pixels"] =
                                    serde_json::json!(comparison.differing_pixels);
                            }
                            Err(error) => {
                                case["output_relation"] =
                                    serde_json::json!("comparison_unavailable");
                                case["output_comparison_error"] = serde_json::json!(error);
                            }
                        }
                    }
                    if let Some(reference_root) = reference_root {
                        let reference =
                            reference_root.join(format!("{path_name}-{format_name}.png"));
                        case["reference_png"] = serde_json::json!(reference);
                        match compare_images(&reference, &output) {
                            Ok(comparison) => {
                                case["pixel_exact"] = serde_json::json!(comparison.exact());
                                case["differing_pixels"] =
                                    serde_json::json!(comparison.differing_pixels);
                                case["max_channel_error"] =
                                    serde_json::json!(comparison.max_channel_error);
                                case["mean_absolute_error"] =
                                    serde_json::json!(comparison.mean_absolute_error);
                                if !comparison.exact() {
                                    case["passed"] = serde_json::json!(false);
                                    case["classification"] = serde_json::json!("pixel_difference");
                                    case["failure_stage"] = serde_json::json!("pixel_comparison");
                                }
                            }
                            Err(error) => {
                                case["passed"] = serde_json::json!(false);
                                case["pixel_exact"] = serde_json::Value::Null;
                                case["classification"] =
                                    serde_json::json!("reference_validation_error");
                                case["failure_stage"] = serde_json::json!("pixel_comparison");
                                case["error"] = serde_json::json!(error);
                            }
                        }
                    }
                    case
                }
                Err(error) => {
                    let message = error.to_string();
                    let diagnostics = failure_diagnostics(&message).unwrap_or_default();
                    serde_json::json!({
                        "render_path": path_name,
                        "pixel_format": format_name,
                        "render_completed": false,
                        "applicable": !matches!(
                            diagnostics.classification.as_str(),
                            "unsupported_pixel_depth" | "unsupported_render_path" |
                                "unsupported_media_type"
                        ),
                        "passed": false,
                        "classification": diagnostics.classification,
                        "failure_stage": diagnostics.failure_stage,
                        "selector_error": diagnostics.selector_error,
                        "error": matrix_error_summary(&message),
                    })
                }
            });
        }
    }
    let passed = cases.iter().filter(|case| case["passed"] == true).count();
    let unsupported = cases
        .iter()
        .filter(|case| case["applicable"] == false)
        .count();
    let applicable = cases.len() - unsupported;
    let failed = applicable - passed;
    serde_json::json!({
        "schema_version": 1,
        "stage": "effect_compatibility_matrix",
        "case_count": cases.len(),
        "applicable_count": applicable,
        "passed_count": passed,
        "failed_count": failed,
        "unsupported_count": unsupported,
        "reference_mode": reference_root.is_some(),
        "cases": cases,
    })
}

fn json_after_marker(text: &str, marker: &str) -> Option<serde_json::Value> {
    let tail = text.split_once(marker)?.1;
    serde_json::Deserializer::from_str(tail)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
}

fn failure_diagnostics(message: &str) -> Option<FailureDiagnostics> {
    let diagnostics = json_after_marker(message, "diagnostics=")
        .or_else(|| json_after_marker(message, "worker report unavailable: "))?;
    let report = json_after_marker(message, "report=");
    let selector_error = report.as_ref().and_then(|value| {
        [
            "render_error",
            "smart_render_error",
            "pre_render_error",
            "gpu_device_setup_error",
            "gpu_device_setdown_error",
        ]
        .into_iter()
        .find_map(|field| value[field].as_i64().filter(|error| *error != 0))
    });
    let failure_stage = diagnostics["failure_stage"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            (report
                .as_ref()
                .and_then(|value| value["result_rects_valid"].as_bool())
                == Some(false))
            .then(|| "result_rect_validation".to_owned())
        });
    let unsupported_depth = report
        .as_ref()
        .and_then(|value| value["depth_supported"].as_bool())
        == Some(false);
    let unsupported_render_path = report
        .as_ref()
        .and_then(|value| value["smart_render_supported"].as_bool())
        == Some(false);
    let unsupported_media_type = report
        .as_ref()
        .and_then(|value| value["image_render_supported"].as_bool())
        == Some(false);
    Some(FailureDiagnostics {
        classification: if unsupported_media_type {
            "unsupported_media_type".to_owned()
        } else if unsupported_render_path {
            "unsupported_render_path".to_owned()
        } else if unsupported_depth {
            "unsupported_pixel_depth".to_owned()
        } else {
            diagnostics["classification"]
                .as_str()
                .unwrap_or("worker_error")
                .to_owned()
        },
        failure_stage: if unsupported_media_type {
            Some("media_type_negotiation".to_owned())
        } else if unsupported_render_path {
            Some("render_path_negotiation".to_owned())
        } else if unsupported_depth {
            Some("pixel_depth_negotiation".to_owned())
        } else {
            failure_stage
        },
        exit_code: diagnostics["exit_code"].as_i64(),
        elapsed_ms: diagnostics["elapsed_ms"].as_u64(),
        selector_error: if unsupported_render_path || unsupported_depth {
            None
        } else {
            selector_error
        },
        stages: completed_stages(Some(&diagnostics)),
    })
}

fn matrix_error_summary(message: &str) -> String {
    if let Some(report) = json_after_marker(message, "report=") {
        if report["smart_render_supported"].as_bool() == Some(false) {
            return "AEX did not advertise SmartFX render support".into();
        }
        if report["depth_supported"].as_bool() == Some(false) {
            return "AEX did not advertise support for the requested pixel depth".into();
        }
        if report["result_rects_valid"].as_bool() == Some(false) {
            return "SmartFX did not return a valid result rectangle".into();
        }
    }
    message.chars().take(512).collect()
}

fn completed_stages(diagnostics: Option<&serde_json::Value>) -> Vec<String> {
    diagnostics
        .and_then(|value| value.get("stage_events"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|event| event.get("state").and_then(serde_json::Value::as_str) == Some("end"))
        .filter_map(|event| {
            let stage = event.get("stage")?.as_str()?;
            let errors = event.get("errors").cloned().unwrap_or_default();
            Some(
                if errors.as_object().is_some_and(|values| !values.is_empty()) {
                    format!("{stage} {errors}")
                } else {
                    stage.to_owned()
                },
            )
        })
        .collect()
}

fn render_diagnostics(report: &serde_json::Value) -> Option<RenderDiagnostics> {
    (report.get("stage")?.as_str()? == "interactive_image_render").then(|| {
        let gpu_attempt = report.get("gpu_attempt").filter(|value| !value.is_null());
        RenderDiagnostics {
            render_path: report["render_path"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
            pixel_format: report["pixel_format"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
            worker_classification: report["worker_classification"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
            gpu_fallback_used: report["gpu_fallback_used"].as_bool().unwrap_or(false),
            gpu_attempt_classification: gpu_attempt
                .and_then(|value| value["worker_classification"].as_str())
                .map(str::to_owned),
            gpu_failure_stage: gpu_attempt
                .and_then(|value| value["worker_diagnostics"]["failure_stage"].as_str())
                .map(str::to_owned),
            final_stages: completed_stages(report.get("worker_diagnostics")),
            gpu_stages: completed_stages(
                gpu_attempt.and_then(|value| value.get("worker_diagnostics")),
            ),
        }
    })
}

fn apply_dynamic_ui_report(
    parameters: &mut [aexcompat_broker::image_render::InteractiveParameter],
    report: &serde_json::Value,
) -> bool {
    if report.get("user_changed_param_requested") != Some(&serde_json::json!(true))
        || report.get("user_changed_param_error") != Some(&serde_json::json!(0))
    {
        return false;
    }
    let Some(rows) = report
        .get("parameters")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    let updates = parameters
        .iter()
        .map(|parameter| {
            rows.iter()
                .find(|row| {
                    row.get("index").and_then(serde_json::Value::as_u64)
                        == Some(parameter.slot as u64)
                })
                .and_then(|row| row.get("ui_flags").and_then(serde_json::Value::as_u64))
                .map(|flags| (flags & (1 << 5) == 0, flags & (1 << 9) == 0))
        })
        .collect::<Option<Vec<_>>>();
    let Some(updates) = updates else {
        return false;
    };
    for (parameter, (enabled, visible)) in parameters.iter_mut().zip(updates) {
        parameter.enabled = enabled;
        parameter.visible = visible;
    }
    true
}

struct HarnessApp {
    repository: PathBuf,
    selection: Option<Selection>,
    session_approved: bool,
    dependencies: Vec<SessionDependency>,
    approved_dependencies: Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>,
    approval_check: bool,
    trust_rebuilds: bool,
    selection_stale: bool,
    last_identity_check: Instant,
    input_image: Option<PathBuf>,
    audio_input: Option<PathBuf>,
    audio_effect_only: bool,
    input_preview: Option<egui::TextureHandle>,
    output_image: Option<PathBuf>,
    preview: Option<egui::TextureHandle>,
    reference_image: Option<PathBuf>,
    reference_preview: Option<egui::TextureHandle>,
    viewer_open: bool,
    viewer_mode: u8,
    pixel_comparison: Option<Result<PixelComparison, String>>,
    parameters: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    host_context: Option<aexcompat_broker::render_request::HostContext>,
    smart_render: bool,
    pixel_format: aexcompat_broker::image_render::RenderPixelFormat,
    gpu_backend: aexcompat_broker::image_render::RenderGpuBackend,
    frame: i32,
    frames_per_second: u32,
    frame_time_step: i32,
    duration_frames: i32,
    custom_ui_click_point: [u16; 2],
    custom_ui_click_color: [f32; 4],
    apply_custom_ui_click_to_render: bool,
    apply_custom_ui_draw_to_render: bool,
    custom_ui_drag_end: [u16; 2],
    custom_ui_drag_steps: u8,
    custom_ui_keycode: u32,
    custom_ui_key_modifiers: u16,
    busy: bool,
    status: String,
    report: String,
    render_diagnostics: Option<RenderDiagnostics>,
    failure_diagnostics: Option<FailureDiagnostics>,
    matrix_results: Vec<MatrixCase>,
    receiver: Option<Receiver<TaskResult>>,
    task_kind: TaskKind,
    inspect_after_refresh: bool,
}

impl HarnessApp {
    fn new(repository: PathBuf) -> Self {
        Self {
            repository,
            selection: None,
            session_approved: false,
            dependencies: Vec::new(),
            approved_dependencies: Vec::new(),
            approval_check: false,
            trust_rebuilds: false,
            selection_stale: false,
            last_identity_check: Instant::now(),
            input_image: None,
            audio_input: None,
            audio_effect_only: false,
            input_preview: None,
            output_image: None,
            preview: None,
            reference_image: None,
            reference_preview: None,
            viewer_open: false,
            viewer_mode: 0,
            pixel_comparison: None,
            parameters: Vec::new(),
            host_context: None,
            smart_render: false,
            pixel_format: aexcompat_broker::image_render::RenderPixelFormat::Argb8,
            gpu_backend: aexcompat_broker::image_render::RenderGpuBackend::Auto,
            frame: 0,
            frames_per_second: 30,
            frame_time_step: 1,
            duration_frames: 300,
            custom_ui_click_point: [20, 20],
            custom_ui_click_color: [0.125, 0.25, 0.75, 1.0],
            apply_custom_ui_click_to_render: false,
            apply_custom_ui_draw_to_render: false,
            custom_ui_drag_end: [20, 20],
            custom_ui_drag_steps: 4,
            custom_ui_keycode: 0x8000_0041,
            custom_ui_key_modifiers: 0,
            busy: false,
            status: "Select an AEX file. Selection does not execute native code.".into(),
            report: String::new(),
            render_diagnostics: None,
            failure_diagnostics: None,
            matrix_results: Vec::new(),
            receiver: None,
            task_kind: TaskKind::Generic,
            inspect_after_refresh: false,
        }
    }

    fn show_image_viewer(&mut self, ctx: &egui::Context) {
        if !self.viewer_open {
            return;
        }
        let mut open = self.viewer_open;
        egui::Window::new("FHD Image Viewer")
            .open(&mut open)
            .default_size(egui::vec2(1600.0, 900.0))
            .min_size(egui::vec2(640.0, 360.0))
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.viewer_mode, 0, "Input");
                    ui.selectable_value(&mut self.viewer_mode, 1, "AEX output");
                    ui.selectable_value(&mut self.viewer_mode, 2, "Compare");
                    ui.separator();
                    ui.label("FHD canvas / aspect-fit");
                });
                ui.separator();
                match self.viewer_mode {
                    0 => show_viewer_texture(ui, "Input", self.input_preview.as_ref()),
                    1 => show_viewer_texture(ui, "AEX output", self.preview.as_ref()),
                    _ => ui.columns(2, |columns| {
                        show_viewer_texture(&mut columns[0], "Input", self.input_preview.as_ref());
                        show_viewer_texture(&mut columns[1], "AEX output", self.preview.as_ref());
                    }),
                }
            });
        self.viewer_open = open;
    }

    fn spawn<F>(&mut self, work: F)
    where
        F: FnOnce() -> Result<(String, Option<PathBuf>), String> + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = work();
            let _ = sender.send(match result {
                Ok((body, output)) => TaskResult {
                    success: true,
                    body,
                    output,
                },
                Err(body) => TaskResult {
                    success: false,
                    body,
                    output: None,
                },
            });
        });
        self.receiver = Some(receiver);
        self.busy = true;
        self.task_kind = TaskKind::Generic;
    }

    fn choose_aex(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("After Effects plug-in", &["aex"])
            .pick_file()
        else {
            return;
        };
        self.status = "Computing AEX identity...".into();
        self.spawn(move || {
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            let hash = format!("{:X}", Sha256::digest(&bytes));
            Ok((
                format!("{}\n{}\n{}", path.display(), bytes.len(), hash),
                None,
            ))
        });
        self.task_kind = TaskKind::IdentifyAex;
    }

    fn add_dependency(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Windows dependency", &["dll"])
            .pick_file()
        else {
            return;
        };
        match fs::read(&path) {
            Ok(bytes) => {
                let key = path.to_string_lossy().to_lowercase();
                if self
                    .dependencies
                    .iter()
                    .any(|item| item.path.to_string_lossy().to_lowercase() == key)
                {
                    self.status = "That dependency DLL is already listed.".into();
                    return;
                }
                self.dependencies.push(SessionDependency {
                    path,
                    size: bytes.len() as u64,
                    sha256: format!("{:X}", Sha256::digest(&bytes)),
                });
                match self.approve_session() {
                    Ok(()) => self.status = "Dependency manifest refreshed.".into(),
                    Err(error) => {
                        self.invalidate_session_approval("Dependency manifest validation failed.");
                        self.report = error;
                    }
                }
            }
            Err(error) => {
                self.status = "Dependency DLL could not be read.".into();
                self.report = error.to_string();
            }
        }
    }

    fn invalidate_session_approval(&mut self, status: &str) {
        self.session_approved = false;
        self.approval_check = false;
        self.approved_dependencies.clear();
        self.status = status.into();
    }

    fn approve_session(&mut self) -> Result<(), String> {
        let selection = self.selection.as_ref().ok_or("No AEX is selected")?;
        let main = aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact {
            path: selection.path.clone(),
            expected_sha256: decode_sha256(&selection.sha256)?,
            expected_size: selection.size,
        };
        let dependencies = self
            .dependencies
            .iter()
            .map(|item| {
                serde_json::json!({
                    "path": item.path,
                    "basename": item.path.file_name().and_then(|name| name.to_str()).unwrap_or(""),
                    "sha256": item.sha256,
                    "size": item.size,
                })
            })
            .collect::<Vec<_>>();
        let json = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "dependencies": dependencies,
        }))
        .map_err(|error| error.to_string())?;
        let manifest =
            aexcompat_broker::session_dependency_manifest::parse_and_validate(&json, &main)
                .map_err(|error| error.to_string())?;
        self.approved_dependencies = manifest.into_approved_image_artifacts();
        self.session_approved = true;
        Ok(())
    }

    fn choose_input(&mut self, ctx: &egui::Context) {
        let selected = rfd::FileDialog::new()
            .add_filter(
                "Image",
                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
            )
            .pick_file();
        let Some(path) = selected else {
            return;
        };
        match load_preview(ctx, "input", &path) {
            Ok(preview) => {
                self.input_image = Some(path);
                self.input_preview = Some(preview);
                self.output_image = None;
                self.preview = None;
                self.pixel_comparison = None;
                self.status = "Input image loaded. Ready to render.".into();
            }
            Err(error) => {
                self.status = "Input image could not be decoded.".into();
                self.report = error;
            }
        }
    }

    fn choose_audio_input(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Mono float32 LE audio", &["f32"])
            .pick_file()
        else {
            return;
        };
        self.audio_input = Some(path);
        self.status = "Audio input selected. Ready to render.".into();
    }

    fn choose_reference(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "AE reference image",
                &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
            )
            .pick_file()
        else {
            return;
        };
        match load_preview(ctx, "reference", &path) {
            Ok(preview) => {
                self.reference_image = Some(path);
                self.reference_preview = Some(preview);
                self.refresh_pixel_comparison();
                self.status = "AE reference image loaded.".into();
            }
            Err(error) => {
                self.status = "AE reference image could not be decoded.".into();
                self.report = error;
            }
        }
    }

    fn load_debug_request(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AEXCompat debug request", &["json"])
            .pick_file()
        else {
            return;
        };
        let result = (|| {
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            if bytes.len() > 64 * 1024 {
                return Err("assignment document exceeds 64 KiB".to_owned());
            }
            let document: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            let timing = typed_request_timing(&document)?;
            let mut parameters = self.parameters.clone();
            apply_typed_assignments(&mut parameters, &document)?;
            let host_context = typed_request_host_context(&document)?;
            Ok((parameters, timing, host_context))
        })();
        match result {
            Ok((parameters, timing, host_context)) => {
                self.parameters = parameters;
                self.host_context = host_context;
                self.frame = timing.current_time / timing.time_step;
                self.frames_per_second = timing.time_scale;
                self.frame_time_step = timing.time_step;
                self.duration_frames = timing.total_time / timing.time_step;
                self.status = format!("Loaded debug request: {}", path.display());
            }
            Err(error) => {
                self.status = "Debug request was rejected without changing controls.".into();
                self.report = error;
            }
        }
    }

    fn save_debug_request(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("AEXCompat debug request", &["json"])
            .set_file_name("aex-debug-request.json")
            .save_file()
        else {
            return;
        };
        let document = typed_request_document(
            &self.parameters,
            self.frame,
            self.frames_per_second,
            self.frame_time_step,
            self.duration_frames,
            self.host_context.as_ref(),
        );
        match serde_json::to_vec_pretty(&document)
            .map_err(|error| error.to_string())
            .and_then(|bytes| fs::write(&path, bytes).map_err(|error| error.to_string()))
        {
            Ok(()) => self.status = format!("Saved debug request: {}", path.display()),
            Err(error) => {
                self.status = "Debug request could not be saved.".into();
                self.report = error;
            }
        }
    }

    fn refresh_pixel_comparison(&mut self) {
        self.pixel_comparison = match (&self.reference_image, &self.output_image) {
            (Some(reference), Some(output)) => Some(compare_images(reference, output)),
            _ => None,
        };
    }

    fn refresh_aex(&mut self) {
        let Some(selected) = &self.selection else {
            return;
        };
        let path = selected.path.clone();
        let previous_hash = selected.sha256.clone();
        match fs::read(&path) {
            Ok(bytes) => {
                let hash = format!("{:X}", Sha256::digest(&bytes));
                let metadata = fs::metadata(&path).ok();
                let profile = profile_for_hash(&hash);
                let identity_changed = hash != previous_hash;
                self.selection = Some(Selection {
                    path: path.clone(),
                    size: bytes.len() as u64,
                    sha256: hash,
                    profile,
                    modified: metadata.and_then(|value| value.modified().ok()),
                });
                self.selection_stale = false;
                self.parameters.clear();
                self.audio_input = None;
                self.audio_effect_only = false;
                self.host_context = None;
                self.output_image = None;
                self.preview = None;
                if identity_changed {
                    self.approved_dependencies.clear();
                    match discover_adjacent_imports(&path) {
                        Ok(dependencies) => {
                            self.dependencies = dependencies;
                            match self.approve_session() {
                                Ok(()) => {
                                    self.inspect_after_refresh = true;
                                    self.status =
                                        "Rebuilt AEX identity and dependencies refreshed.".into();
                                }
                                Err(error) => {
                                    self.invalidate_session_approval(
                                        "Rebuilt dependency manifest validation failed.",
                                    );
                                    self.report = error;
                                }
                            }
                        }
                        Err(error) => {
                            self.invalidate_session_approval(
                                "Rebuilt dependency discovery failed safely.",
                            );
                            self.report = error;
                        }
                    }
                } else {
                    self.status = "AEX identity is unchanged.".into();
                }
            }
            Err(error) => {
                self.status = "Could not reload the selected AEX.".into();
                self.report = error.to_string();
            }
        }
    }

    fn check_selected_identity(&mut self) {
        if self.last_identity_check.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_identity_check = Instant::now();
        let Some(selected) = &self.selection else {
            return;
        };
        let Ok(metadata) = fs::metadata(&selected.path) else {
            self.selection_stale = true;
            self.status = "Selected AEX is unavailable. Reload after the build finishes.".into();
            return;
        };
        let modified = metadata.modified().ok();
        let hash_changed = fs::read(&selected.path).map_or(true, |bytes| {
            !format!("{:X}", Sha256::digest(bytes)).eq_ignore_ascii_case(&selected.sha256)
        });
        if metadata.len() != selected.size || modified != selected.modified || hash_changed {
            if !self.selection_stale {
                self.status =
                    "AEX build changed. Reload its identity before native execution.".into();
            }
            self.selection_stale = true;
            self.session_approved = false;
            self.approved_dependencies.clear();
        }
        if self.dependencies.iter().any(|dependency| {
            fs::read(&dependency.path).map_or(true, |bytes| {
                bytes.len() as u64 != dependency.size
                    || !format!("{:X}", Sha256::digest(&bytes))
                        .eq_ignore_ascii_case(&dependency.sha256)
            })
        }) {
            self.invalidate_session_approval(
                "A dependency DLL changed. Re-add it and approve the session again.",
            );
        }
    }

    fn inspect_parameters_async(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Loading Effect Controls...".into();
        self.spawn(move || {
            let (parameters, diagnostics) =
                aexcompat_broker::image_render::inspect_experimental_with_diagnostics(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            let report = serde_json::json!({
                "stage": "parameter_inspection",
                "parameters": parameters,
                "worker_diagnostics": diagnostics,
            });
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
        self.task_kind = TaskKind::InspectParameters;
    }

    fn inspect_external_dependencies(&mut self, missing_only: bool) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = if missing_only {
            "Inspecting missing external dependencies..."
        } else {
            "Inspecting all external dependencies..."
        }
        .into();
        self.spawn(move || {
            let report =
                aexcompat_broker::image_render::inspect_experimental_external_dependencies(
                    &repository,
                    &plugin_path,
                    &hash,
                    missing_only,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_options_dialog(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing the advertised options dialog...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::probe_experimental_options_dialog(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_automatic_options_dialog(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing the sequence-requested options dialog...".into();
        self.spawn(move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_automatic_options_dialog(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_nop_render(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing NOP_RENDER source passthrough...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::probe_experimental_nop_render(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_smart_nop_render(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing SmartFX NOP_RENDER source passthrough...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::probe_experimental_smart_nop_render(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_input_buffer_write(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing advertised input-buffer write access...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::probe_experimental_input_buffer_write(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_smart_input_buffer_write(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing SmartFX input-buffer write access...".into();
        self.spawn(move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_smart_input_buffer_write(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_frame_resize(&mut self, expand: bool) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = if expand {
            "Probing advertised FRAME_SETUP expansion...".into()
        } else {
            "Probing advertised FRAME_SETUP shrink...".into()
        };
        self.spawn(move || {
            let report = if expand {
                aexcompat_broker::image_render::probe_experimental_expand_buffer(
                    &repository,
                    &plugin_path,
                    &hash,
                )
            } else {
                aexcompat_broker::image_render::probe_experimental_shrink_buffer(
                    &repository,
                    &plugin_path,
                    &hash,
                )
            }
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_persistent_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing two frames in one isolated sequence...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::probe_experimental_persistent_sequence(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_flattened_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing sequence save/reload ownership...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::probe_experimental_flattened_sequence(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_copied_flattened_sequence(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Probing non-destructive sequence save...".into();
        self.spawn(move || {
            let report =
                aexcompat_broker::image_render::probe_experimental_copied_flattened_sequence(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn probe_custom_ui_cursor(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI cursor event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_cursor(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI requested the eyedropper cursor.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI cursor event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_draw(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Recording an isolated custom UI draw event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_draw(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI draw commands were recorded safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
                self.apply_custom_ui_draw_to_render = true;
                self.apply_custom_ui_click_to_render = false;
            }
            Err(error) => {
                self.status = "Custom UI draw event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_lifecycle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI lifecycle...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_lifecycle(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI lifecycle failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_idle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching one custom UI idle event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_idle(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI idle lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI idle event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_keydown(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching one custom UI key event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_keydown(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_keycode,
            self.custom_ui_key_modifiers,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI key lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI key event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_mouse_exited(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching a Layer/Comp custom UI mouse-exited event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_mouse_exited(
            &self.repository,
            &selection.path,
            &selection.sha256,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI mouse-exited lifecycle completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI mouse-exited event failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_click(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching an isolated custom UI click event...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_click(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_click_color,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI click changed the effect value safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
                self.apply_custom_ui_click_to_render = true;
                self.apply_custom_ui_draw_to_render = false;
            }
            Err(error) => {
                self.status = "Custom UI click failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn probe_custom_ui_drag(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        self.status = "Dispatching a bounded custom UI drag sequence...".into();
        match aexcompat_broker::image_render::probe_experimental_custom_ui_drag(
            &self.repository,
            &selection.path,
            &selection.sha256,
            self.custom_ui_click_point,
            self.custom_ui_drag_end,
            self.custom_ui_drag_steps,
            &self.parameters,
        ) {
            Ok(report) => {
                self.status = "Custom UI drag sequence completed safely.".into();
                self.report = serde_json::to_string_pretty(&report).unwrap_or_default();
            }
            Err(error) => {
                self.status = "Custom UI drag failed safely.".into();
                self.report = error.to_string();
            }
        }
    }

    fn render_to(&mut self, output: PathBuf) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.input_image.clone() else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let registered = selection.profile == Some("scattermap");
        let parameters = self.parameters.clone();
        let host_context = self.host_context.clone();
        let smart = self.smart_render;
        let pixel_format = self.pixel_format;
        let gpu_backend = self.gpu_backend;
        let audio_sidecar = self.audio_input.clone();
        let dependencies = self.approved_dependencies.clone();
        let custom_ui_action = if self.apply_custom_ui_click_to_render {
            Some(aexcompat_broker::image_render::RenderUiAction::Click {
                point: self.custom_ui_click_point,
                color: self.custom_ui_click_color,
            })
        } else if self.apply_custom_ui_draw_to_render {
            Some(aexcompat_broker::image_render::RenderUiAction::Draw)
        } else {
            None
        };
        let timing = match render_timing(
            self.frame,
            self.duration_frames,
            self.frames_per_second,
            self.frame_time_step,
        ) {
            Ok(timing) => timing,
            Err(error) => {
                self.status = "Render timing is invalid.".into();
                self.report = error;
                return;
            }
        };
        if audio_sidecar.is_some()
            && (smart
                || pixel_format != aexcompat_broker::image_render::RenderPixelFormat::Argb8
                || host_context.is_some()
                || custom_ui_action.is_some())
        {
            self.status = "Audio sidecar requires plain classic ARGB8 rendering.".into();
            self.report = "Disable SmartFX, deep color, mask/spatial/render context, and custom UI actions before rendering with audio.".into();
            return;
        }
        if audio_sidecar.is_some() && !dependencies.is_empty() {
            self.status =
                "Dependency DLLs are not supported by the audio-sidecar render path.".into();
            self.report =
                "Remove dependencies or disable the audio sidecar before rendering.".into();
            return;
        }
        let use_registered_default = registered
            && parameters.is_empty()
            && self.frame == 0
            && custom_ui_action.is_none()
            && pixel_format == aexcompat_broker::image_render::RenderPixelFormat::Argb8
            && audio_sidecar.is_none();
        self.status = "Rendering in an isolated worker...".into();
        self.spawn(move || {
            let report = if let Some(audio) = audio_sidecar {
                aexcompat_broker::image_render::render_experimental_image_with_audio_sidecar(
                    &repository,
                    &plugin_path,
                    &hash,
                    &input,
                    &audio,
                    &output,
                    &parameters,
                    timing,
                )
            } else if !smart && use_registered_default && dependencies.is_empty() {
                aexcompat_broker::image_render::render_image(
                    &repository,
                    "scattermap",
                    &input,
                    &output,
                )
            } else {
                aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                    &repository,
                    &plugin_path,
                    &hash,
                    &input,
                    &output,
                    &parameters,
                    timing,
                    smart,
                    pixel_format,
                    host_context.as_ref(),
                    custom_ui_action,
                    gpu_backend,
                    dependencies,
                )
            }
            .map_err(|error| error.to_string())?;
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, Some(output)))
        });
    }

    fn render_and_save(&mut self) {
        let Some(output) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name("aex-output.png")
            .save_file()
        else {
            return;
        };
        self.render_to(output);
    }

    fn render_audio_and_save(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.audio_input.clone() else {
            return;
        };
        let Some(output) = rfd::FileDialog::new()
            .add_filter("Mono float32 LE audio", &["f32"])
            .set_file_name("aex-output.f32")
            .save_file()
        else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        self.status = "Rendering audio in an isolated worker...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::render_experimental_audio(
                &repository,
                &plugin_path,
                &hash,
                &input,
                &output,
                &parameters,
            )
            .map_err(|error| error.to_string())?;
            let mut report = report;
            report["published_output"] = serde_json::json!(output);
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, None))
        });
    }

    fn quick_render(&mut self) {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let output = self
            .repository
            .join("target/harness-output")
            .join(format!("render-{nonce}.png"));
        self.render_to(output);
    }

    fn run_compatibility_matrix(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let Some(input) = self.input_image.clone() else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        let host_context = self.host_context.clone();
        let timing = match render_timing(
            self.frame,
            self.duration_frames,
            self.frames_per_second,
            self.frame_time_step,
        ) {
            Ok(timing) => timing,
            Err(error) => {
                self.status = "Matrix timing is invalid.".into();
                self.report = error;
                return;
            }
        };
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let output_root = repository
            .join("target/harness-matrix")
            .join(nonce.to_string());
        self.status = "Running six isolated Effect compatibility cases...".into();
        self.matrix_results.clear();
        self.spawn(move || {
            let report = run_effect_matrix(
                &repository,
                &plugin_path,
                &hash,
                &input,
                &output_root,
                &parameters,
                timing,
                None,
                host_context.as_ref(),
            );
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            Ok((body, None))
        });
    }

    fn trigger_button(&mut self, slot: u32) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        let parameters = self.parameters.clone();
        self.status = format!("Dispatching PF_Cmd_USER_CHANGED_PARAM for slot {slot}...");
        self.spawn(move || {
            let report = aexcompat_broker::image_render::trigger_experimental_button(
                &repository,
                &plugin_path,
                &hash,
                slot,
                &parameters,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn initialize_aegp(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Initializing AEGP in an isolated worker...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::initialize_experimental_aegp(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn update_aegp_menu(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching an isolated AEGP update-menu event...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::dispatch_experimental_aegp_update_menu(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_idle(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching one isolated AEGP idle event...".into();
        self.spawn(move || {
            let report = aexcompat_broker::image_render::dispatch_experimental_aegp_idle(
                &repository,
                &plugin_path,
                &hash,
            )
            .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_command_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Dispatching an isolated AEGP command ON/OFF roundtrip...".into();
        self.spawn(move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_command_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_active_idle_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Running AEGP ON / active idle / OFF in isolation...".into();
        self.spawn(move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_active_idle_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn dispatch_aegp_comp_idle_roundtrip(&mut self) {
        let Some(selection) = &self.selection else {
            return;
        };
        let repository = self.repository.clone();
        let plugin_path = selection.path.clone();
        let hash = selection.sha256.clone();
        self.status = "Running AEGP ON / comp idle / OFF in isolation...".into();
        self.spawn(move || {
            let report =
                aexcompat_broker::image_render::dispatch_experimental_aegp_comp_idle_roundtrip(
                    &repository,
                    &plugin_path,
                    &hash,
                )
                .map_err(|error| error.to_string())?;
            Ok((
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?,
                None,
            ))
        });
    }

    fn show_effect_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Effect Controls").size(20.0));
        if let Some(selection) = &self.selection {
            ui.label(
                selection
                    .path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Selected AEX"),
            );
        } else {
            ui.label("Select an AEX to load its parameters.");
        }
        ui.separator();
        if self.busy && self.task_kind == TaskKind::InspectParameters {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading parameters...");
            });
        } else if self.selection.is_some() && self.parameters.is_empty() {
            ui.label("This effect exposed no editable parameters.");
            if ui.small_button("Reload controls").clicked() {
                self.inspect_parameters_async();
            }
        }

        let mut clicked_button = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for parameter in &mut self.parameters {
                    if !parameter.visible {
                        continue;
                    }
                    if parameter.kind == "group_start" {
                        ui.add_space(8.0);
                        ui.label(RichText::new(&parameter.name).strong());
                        continue;
                    }
                    if parameter.kind == "group_end" {
                        ui.separator();
                        continue;
                    }
                    if parameter.kind == "button" {
                        if ui
                            .add_enabled(
                                parameter.enabled && !self.busy,
                                egui::Button::new(&parameter.name),
                            )
                            .clicked()
                        {
                            clicked_button = Some(parameter.slot);
                        }
                        continue;
                    }
                    if matches!(parameter.kind.as_str(), "custom" | "no_data") {
                        ui.label(&parameter.name);
                        ui.small(format!("{} (read-only)", parameter.kind));
                        continue;
                    }

                    let previous_value = parameter.value;
                    let previous_color = parameter.color;
                    let previous_components = parameter.components;
                    let previous_layer = parameter.layer_path.clone();
                    let previous_summary = parameter.debug_summary.clone();
                    ui.add_enabled_ui(parameter.enabled && !self.busy, |ui| {
                        ui.label(RichText::new(&parameter.name).small());
                        if parameter.kind == "layer" {
                            ui.horizontal(|ui| {
                                if ui.small_button("Choose image").clicked() {
                                    parameter.layer_path = rfd::FileDialog::new()
                                        .add_filter(
                                            "Image",
                                            &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
                                        )
                                        .pick_file();
                                }
                                ui.label(
                                    parameter
                                        .layer_path
                                        .as_ref()
                                        .and_then(|path| path.file_name())
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("Not connected"),
                                );
                            });
                        } else if parameter.kind == "arbitrary_data" {
                            ui.add(
                                egui::TextEdit::singleline(
                                    parameter.debug_summary.get_or_insert_with(String::new),
                                )
                                .desired_width(f32::INFINITY),
                            );
                        } else if parameter.kind == "path" {
                            ui.add(
                                egui::DragValue::new(&mut parameter.value)
                                    .range(0.0..=parameter.maximum),
                            );
                        } else if matches!(parameter.kind.as_str(), "angle" | "point" | "point3d") {
                            ui.horizontal(|ui| {
                                for (index, label) in ["X", "Y", "Z"]
                                    .iter()
                                    .enumerate()
                                    .take(parameter.component_count)
                                {
                                    ui.label(*label);
                                    ui.add(
                                        egui::DragValue::new(&mut parameter.components[index])
                                            .speed(0.1),
                                    );
                                }
                            });
                        } else if parameter.kind == "color" {
                            let mut color = Color32::from_rgba_unmultiplied(
                                parameter.color[1],
                                parameter.color[2],
                                parameter.color[3],
                                parameter.color[0],
                            );
                            if ui.color_edit_button_srgba(&mut color).changed() {
                                parameter.color = [color.a(), color.r(), color.g(), color.b()];
                            }
                        } else if !parameter.choices.is_empty() {
                            let mut selected = parameter.value as usize;
                            egui::ComboBox::from_id_salt(("effect-control", parameter.slot))
                                .selected_text(
                                    parameter
                                        .choices
                                        .get(selected.saturating_sub(1))
                                        .map(String::as_str)
                                        .unwrap_or("Unknown"),
                                )
                                .show_ui(ui, |ui| {
                                    for (index, choice) in parameter.choices.iter().enumerate() {
                                        ui.selectable_value(&mut selected, index + 1, choice);
                                    }
                                });
                            parameter.value = selected as f64;
                        } else if parameter.kind == "integer"
                            && parameter.minimum == 0.0
                            && parameter.maximum == 1.0
                        {
                            let mut checked = parameter.value != 0.0;
                            if ui.checkbox(&mut checked, "Enabled").changed() {
                                parameter.value = f64::from(checked);
                            }
                        } else {
                            ui.add(
                                egui::Slider::new(
                                    &mut parameter.value,
                                    parameter.minimum..=parameter.maximum,
                                )
                                .show_value(true),
                            );
                        }
                    });
                    if parameter.supervised
                        && (parameter.value != previous_value
                            || parameter.color != previous_color
                            || parameter.components != previous_components
                            || parameter.layer_path != previous_layer
                            || parameter.debug_summary != previous_summary)
                    {
                        clicked_button = Some(parameter.slot);
                    }
                    ui.add_space(4.0);
                }
            });
        if let Some(slot) = clicked_button {
            self.trigger_button(slot);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let result = self
            .receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        let Some(result) = result else {
            if self.busy {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return;
        };
        let task_kind = self.task_kind;
        self.busy = false;
        self.failure_diagnostics = (!result.success)
            .then(|| failure_diagnostics(&result.body))
            .flatten();
        if !result.success {
            self.render_diagnostics = None;
        }
        self.matrix_results = serde_json::from_str(&result.body)
            .ok()
            .as_ref()
            .and_then(compatibility_matrix)
            .unwrap_or_default();
        self.status = if result.success {
            "Completed"
        } else {
            "Failed safely"
        }
        .into();
        let mut inspect_selected_aex = false;
        if task_kind == TaskKind::IdentifyAex && result.success {
            let mut lines = result.body.lines();
            if let (Some(path), Some(size), Some(hash)) = (lines.next(), lines.next(), lines.next())
            {
                let profile = profile_for_hash(hash);
                self.selection = Some(Selection {
                    path: path.into(),
                    size: size.parse().unwrap_or(0),
                    sha256: hash.into(),
                    profile,
                    modified: fs::metadata(path)
                        .ok()
                        .and_then(|value| value.modified().ok()),
                });
                self.session_approved = true;
                self.approved_dependencies.clear();
                self.approval_check = false;
                self.selection_stale = false;
                match discover_adjacent_imports(Path::new(path)) {
                    Ok(dependencies) => {
                        self.dependencies = dependencies;
                        match self.approve_session() {
                            Ok(()) => inspect_selected_aex = true,
                            Err(error) => {
                                self.invalidate_session_approval(
                                    "Automatic dependency manifest validation failed.",
                                );
                                self.report = error;
                            }
                        }
                    }
                    Err(error) => {
                        self.invalidate_session_approval(
                            "Automatic adjacent dependency discovery failed safely.",
                        );
                        self.report = error;
                    }
                }
            }
        }
        if task_kind == TaskKind::InspectParameters && result.success {
            if let Ok(report) = serde_json::from_str::<serde_json::Value>(&result.body) {
                if let Ok(parameters) = serde_json::from_value(report["parameters"].clone()) {
                    self.parameters = parameters;
                    self.audio_effect_only = report["worker_diagnostics"]["audio_effect_only"]
                        .as_bool()
                        .unwrap_or(false);
                    self.status = format!(
                        "Effect Controls ready: {} editable parameter(s).",
                        self.parameters.len()
                    );
                }
            }
        }
        if let Some(output) = result.output {
            self.render_diagnostics = serde_json::from_str(&result.body)
                .ok()
                .as_ref()
                .and_then(render_diagnostics);
            self.output_image = Some(output.clone());
            if let Ok(preview) = load_preview(ctx, "output", &output) {
                self.preview = Some(preview);
            }
            self.refresh_pixel_comparison();
        }
        if let Ok(report) = serde_json::from_str(&result.body) {
            apply_dynamic_ui_report(&mut self.parameters, &report);
        }
        self.report = result.body;
        self.receiver = None;
        self.task_kind = TaskKind::Generic;
        if inspect_selected_aex {
            self.inspect_parameters_async();
        }
    }
}

impl eframe::App for HarnessApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.check_selected_identity();
        if self.inspect_after_refresh && !self.busy {
            self.inspect_after_refresh = false;
            self.inspect_parameters_async();
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.heading(RichText::new("AEXCompat Effect Harness").size(25.0));
            ui.label("Isolated image and audio host for Effect AEX development");
            ui.add_space(8.0);
        });
        egui::SidePanel::left("effect_controls")
            .default_width(320.0)
            .min_width(240.0)
            .max_width(520.0)
            .resizable(true)
            .show(ctx, |ui| self.show_effect_controls(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
            if ui.add_enabled(!self.busy, egui::Button::new("1. Select AEX")).clicked() {
                self.selection = None;
                self.session_approved = false;
                self.approved_dependencies.clear();
                self.trust_rebuilds = false;
                self.selection_stale = false;
                self.parameters.clear();
                self.audio_input = None;
                self.audio_effect_only = false;
                self.host_context = None;
                self.choose_aex();
            }
            if let Some(selected) = &self.selection {
                let selected_path = selected.path.display().to_string();
                let selected_size = selected.size;
                let selected_hash = selected.sha256.clone();
                let selected_profile = selected.profile;
                ui.group(|ui| {
                    ui.label(RichText::new(selected_path).strong());
                    ui.collapsing("Binary details and dependency DLLs", |ui| {
                    ui.label(format!("{} bytes", selected_size));
                    ui.monospace(selected_hash);
                    if self.selection_stale {
                        ui.colored_label(Color32::from_rgb(210, 75, 55), "Build changed: native execution is paused until reload");
                    }
                    if let Some(profile) = selected_profile {
                        ui.colored_label(Color32::from_rgb(30, 150, 95), format!("Registered profile: {profile}"));
                    }
                    ui.separator();
                    ui.label(RichText::new("Session dependency DLLs").strong());
                    if self.dependencies.is_empty() {
                        ui.label("No additional DLLs selected.");
                    }
                    let mut remove = None;
                    for (index, dependency) in self.dependencies.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let basename = dependency.path.file_name().and_then(|name| name.to_str()).unwrap_or("<invalid>");
                            let short_hash = dependency.sha256.get(..12).unwrap_or(&dependency.sha256);
                            ui.monospace(format!("{basename}  {short_hash}...  {} bytes", dependency.size));
                            if ui.add_enabled(!self.busy, egui::Button::new("Remove")).clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Add DLL")).clicked() {
                            self.add_dependency();
                        }
                        if ui.add_enabled(!self.busy && !self.dependencies.is_empty(), egui::Button::new("Clear all")).clicked() {
                            self.dependencies.clear();
                            self.approved_dependencies.clear();
                            self.session_approved = true;
                            self.status = "Dependency list cleared.".into();
                        }
                    });
                    if let Some(index) = remove {
                        self.dependencies.remove(index);
                        if self.dependencies.is_empty() {
                            self.approved_dependencies.clear();
                            self.session_approved = true;
                        } else if let Err(error) = self.approve_session() {
                            self.invalidate_session_approval("Dependency manifest validation failed.");
                            self.report = error;
                        }
                    }
                    if selected_profile.is_none() {
                        ui.colored_label(Color32::from_rgb(215, 145, 40), "Unregistered AEX: direct isolated execution enabled");
                        ui.label("The selected binary is hashed automatically and runs in a timeout-limited restricted worker. This is not a complete security sandbox.");
                    }
                    if ui.add_enabled(!self.busy, egui::Button::new("Reload rebuilt AEX")).clicked() {
                        self.refresh_aex();
                    }
                    });
                });
                if self.session_approved && !self.selection_stale {
                    ui.add_space(10.0);
                    if ui.add_enabled(!self.busy, egui::Button::new("Reload Effect Controls")).clicked() { self.inspect_parameters_async(); }
                    ui.collapsing("Developer probes and diagnostics", |ui| {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Inspect all dependencies")).clicked() { self.inspect_external_dependencies(false); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Inspect missing dependencies")).clicked() { self.inspect_external_dependencies(true); }
                    });
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe 2-frame persistent sequence")).clicked() { self.probe_persistent_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe sequence save/reload")).clicked() { self.probe_flattened_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe non-destructive sequence save")).clicked() { self.probe_copied_flattened_sequence(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe options dialog")).clicked() { self.probe_options_dialog(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe automatic options dialog")).clicked() { self.probe_automatic_options_dialog(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe NOP_RENDER passthrough")).clicked() { self.probe_nop_render(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe SmartFX NOP_RENDER passthrough")).clicked() { self.probe_smart_nop_render(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe input-buffer write access")).clicked() { self.probe_input_buffer_write(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe SmartFX input-buffer write access")).clicked() { self.probe_smart_input_buffer_write(); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe FRAME_SETUP expansion")).clicked() { self.probe_frame_resize(true); }
                    if ui.add_enabled(!self.busy, egui::Button::new("Probe FRAME_SETUP shrink")).clicked() { self.probe_frame_resize(false); }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 4 != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Probe custom UI cursor")).clicked()
                    {
                        self.probe_custom_ui_cursor();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Record custom UI draw")).clicked()
                    {
                        self.probe_custom_ui_draw();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0) {
                        let changed = ui.checkbox(
                            &mut self.apply_custom_ui_draw_to_render,
                            "Draw custom UI before each render",
                        ).changed();
                        if changed && self.apply_custom_ui_draw_to_render {
                            self.apply_custom_ui_click_to_render = false;
                        }
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Test custom UI lifecycle")).clicked()
                    {
                        self.probe_custom_ui_lifecycle();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0)
                        && ui.add_enabled(!self.busy, egui::Button::new("Dispatch custom UI idle")).clicked()
                    {
                        self.probe_custom_ui_idle();
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Custom UI key event").strong());
                            ui.horizontal(|ui| {
                                ui.label("Keycode");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_keycode).range(0u32..=0xC000_FFFFu32));
                                ui.label(format!("0x{:08X}", self.custom_ui_keycode));
                                ui.label("Modifiers");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_key_modifiers));
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch key")).clicked() {
                                    self.probe_custom_ui_keydown();
                                }
                            });
                            ui.label("Default is printable A. The custom UI click X/Y values are used as the screen point.");
                        });
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 4 != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Custom UI click").strong());
                            ui.horizontal(|ui| {
                                ui.label("X");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_click_point[0]).range(0..=8192));
                                ui.label("Y");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_click_point[1]).range(0..=8192));
                                ui.color_edit_button_rgba_unmultiplied(&mut self.custom_ui_click_color);
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch click")).clicked() {
                                    self.probe_custom_ui_click();
                                }
                            });
                            let changed = ui.checkbox(
                                &mut self.apply_custom_ui_click_to_render,
                                "Apply this click before each render",
                            ).changed();
                            if changed && self.apply_custom_ui_click_to_render {
                                self.apply_custom_ui_draw_to_render = false;
                            }
                        });
                    }
                    if self.parameters.iter().any(|parameter| parameter.custom_ui_events & 3 != 0) {
                        ui.group(|ui| {
                            ui.label(RichText::new("Comp / Layer custom UI drag").strong());
                            ui.horizontal(|ui| {
                                ui.label("End X");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_end[0]).range(0..=8192));
                                ui.label("End Y");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_end[1]).range(0..=8192));
                                ui.label("Steps");
                                ui.add(egui::DragValue::new(&mut self.custom_ui_drag_steps).range(1..=32));
                                if ui.add_enabled(!self.busy, egui::Button::new("Dispatch drag")).clicked() {
                                    self.probe_custom_ui_drag();
                                }
                            });
                            ui.label("The custom UI click X/Y values above are used as the drag start.");
                            if ui.add_enabled(!self.busy, egui::Button::new("Dispatch mouse exited")).clicked() {
                                self.probe_custom_ui_mouse_exited();
                            }
                        });
                    }
                    ui.collapsing("AEGP diagnostics (advanced)", |ui| {
                        if ui.add_enabled(!self.busy, egui::Button::new("Initialize as AEGP")).clicked() { self.initialize_aegp(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Dispatch AEGP update-menu")).clicked() { self.update_aegp_menu(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Dispatch one AEGP idle tick")).clicked() { self.dispatch_aegp_idle(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP command ON/OFF roundtrip")).clicked() { self.dispatch_aegp_command_roundtrip(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP active-idle roundtrip")).clicked() { self.dispatch_aegp_active_idle_roundtrip(); }
                        if ui.add_enabled(!self.busy, egui::Button::new("Run AEGP comp-idle roundtrip")).clicked() { self.dispatch_aegp_comp_idle_roundtrip(); }
                    });
                    });
                    // Effect parameters live in the persistent left-side Effect Controls panel.
                    if false {
                    let mut clicked_button = None;
                    for parameter in &mut self.parameters {
                        if !parameter.visible {
                            continue;
                        }
                        if parameter.kind == "group_start" {
                            ui.add_space(6.0);
                            ui.label(RichText::new(&parameter.name).strong().size(16.0));
                            continue;
                        }
                        if parameter.kind == "group_end" {
                            ui.separator();
                            continue;
                        }
                        if parameter.kind == "button" {
                            if ui
                                .add_enabled(
                                    parameter.enabled && !self.busy,
                                    egui::Button::new(&parameter.name),
                                )
                                .clicked()
                            {
                                clicked_button = Some(parameter.slot);
                            }
                            continue;
                        }
                        if matches!(
                            parameter.kind.as_str(),
                            "custom" | "no_data"
                        ) {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(&parameter.name);
                                ui.monospace(format!("{} (read-only)", parameter.kind));
                                if parameter.custom_ui_events != 0 {
                                    ui.monospace(format!(
                                        "custom UI {}x{}, events=0x{:X}",
                                        parameter.control_size[0],
                                        parameter.control_size[1],
                                        parameter.custom_ui_events
                                    ));
                                }
                                if let Some(summary) = &parameter.debug_summary {
                                    ui.collapsing("Observed value", |ui| {
                                        ui.monospace(summary);
                                    });
                                } else {
                                    ui.label("No printable value exposed by the effect.");
                                }
                            });
                            continue;
                        }
                        let previous_value = parameter.value;
                        let previous_color = parameter.color;
                        let previous_components = parameter.components;
                        let previous_layer = parameter.layer_path.clone();
                        let previous_summary = parameter.debug_summary.clone();
                        ui.add_enabled_ui(parameter.enabled, |ui| ui.horizontal(|ui| {
                            ui.label(&parameter.name);
                            if parameter.kind == "layer" {
                                if ui.button("Select image").clicked() {
                                    parameter.layer_path = rfd::FileDialog::new()
                                        .add_filter("Image", &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"])
                                        .pick_file();
                                }
                                if let Some(path) = &parameter.layer_path {
                                    ui.monospace(path.display().to_string());
                                } else {
                                    ui.label("Not connected");
                                }
                            } else if parameter.kind == "arbitrary_data" {
                                let text = parameter.debug_summary.get_or_insert_with(String::new);
                                ui.add(egui::TextEdit::singleline(text).desired_width(320.0));
                                ui.label("PRINT/SCAN text");
                            } else if parameter.kind == "path" {
                                ui.add(egui::DragValue::new(&mut parameter.value).range(0.0..=parameter.maximum).speed(1.0));
                                ui.label("0=None, 1..N=mask index");
                            } else if matches!(parameter.kind.as_str(), "angle" | "point" | "point3d") {
                                let labels = ["X", "Y", "Z"];
                                for (index, label) in labels.iter().enumerate().take(parameter.component_count) {
                                    ui.label(*label);
                                    ui.add(egui::DragValue::new(&mut parameter.components[index]).speed(0.1).range(-32768.0..=32768.0));
                                }
                            } else if parameter.kind == "color" {
                                let mut color = Color32::from_rgba_unmultiplied(parameter.color[1], parameter.color[2], parameter.color[3], parameter.color[0]);
                                if ui.color_edit_button_srgba(&mut color).changed() {
                                    parameter.color = [color.a(), color.r(), color.g(), color.b()];
                                }
                            } else if !parameter.choices.is_empty() {
                                let mut selected = parameter.value as usize;
                                egui::ComboBox::from_id_salt(parameter.slot)
                                    .selected_text(parameter.choices.get(selected.saturating_sub(1)).map(String::as_str).unwrap_or("Unknown"))
                                    .show_ui(ui, |ui| {
                                        for (index, choice) in parameter.choices.iter().enumerate() {
                                            ui.selectable_value(&mut selected, index + 1, choice);
                                        }
                                    });
                                parameter.value = selected as f64;
                            } else if parameter.kind == "integer" && parameter.minimum == 0.0 && parameter.maximum == 1.0 {
                                let mut checked = parameter.value != 0.0;
                                if ui.checkbox(&mut checked, "").changed() { parameter.value = if checked { 1.0 } else { 0.0 }; }
                            } else {
                                ui.add(egui::Slider::new(&mut parameter.value, parameter.minimum..=parameter.maximum));
                            }
                        }));
                        if parameter.supervised
                            && (parameter.value != previous_value
                                || parameter.color != previous_color
                                || parameter.components != previous_components
                                || parameter.layer_path != previous_layer
                                || parameter.debug_summary != previous_summary)
                        {
                            clicked_button = Some(parameter.slot);
                        }
                    }
                    if let Some(slot) = clicked_button {
                        self.trigger_button(slot);
                    }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Render path:");
                        ui.selectable_value(&mut self.smart_render, false, "Classic");
                        ui.selectable_value(&mut self.smart_render, true, "SmartFX");
                    });
                    ui.horizontal(|ui| {
                        use aexcompat_broker::image_render::RenderPixelFormat;
                        ui.label("Pixel depth:");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb8, "8 bpc");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb16, "16 bpc");
                        ui.selectable_value(&mut self.pixel_format, RenderPixelFormat::Argb32f, "32 bpc float");
                    });
                    ui.horizontal(|ui| {
                        use aexcompat_broker::image_render::RenderGpuBackend;
                        ui.label("GPU backend:");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Auto, "Auto");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Cuda, "CUDA");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::OpenCl, "OpenCL");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::DirectX, "DirectX");
                        ui.selectable_value(&mut self.gpu_backend, RenderGpuBackend::Cpu, "CPU");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Frame:");
                        if ui.add(egui::DragValue::new(&mut self.frame).range(0..=10_000_000)).changed() {
                            self.duration_frames = self.duration_frames.max(self.frame.saturating_add(1));
                        }
                        ui.label("Duration frames:");
                        ui.add(egui::DragValue::new(&mut self.duration_frames).range(self.frame.saturating_add(1)..=10_000_001));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Time scale:");
                        ui.add(egui::DragValue::new(&mut self.frames_per_second).range(1..=1_000_000));
                        ui.label("Frame step:");
                        ui.add(egui::DragValue::new(&mut self.frame_time_step).range(1..=100_000));
                        ui.label(format!("{:.5} fps", self.frames_per_second as f64 / self.frame_time_step as f64));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Rate presets:");
                        for (label, scale, step) in [("23.976", 24_000, 1_001), ("29.97", 30_000, 1_001), ("59.94", 60_000, 1_001)] {
                            if ui.button(label).clicked() {
                                self.frames_per_second = scale;
                                self.frame_time_step = step;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        let enabled = self.host_context.as_ref().and_then(|context| context.spatial).is_some();
                        let mut requested = enabled;
                        if ui.checkbox(&mut requested, "Spatial context").changed() {
                            if requested {
                                let context = self.host_context.get_or_insert_with(|| aexcompat_broker::render_request::HostContext {
                                    mask_scene: aexcompat_broker::render_request::MaskScene { masks: Vec::new() },
                                    spatial: None,
                                    render_environment: None,
                                    aux_channels: Vec::new(),
                                    alpha_as_coverage_params: Vec::new(),
                                });
                                context.spatial = Some(aexcompat_broker::render_request::SpatialContext {
                                    downsample_x: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    downsample_y: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    pixel_aspect_ratio: aexcompat_broker::render_request::RationalScale { numerator: 1, denominator: 1 },
                                    full_resolution_width: None,
                                    full_resolution_height: None,
                                    pre_effect_source_origin_x: None,
                                    pre_effect_source_origin_y: None,
                                });
                            } else if let Some(context) = &mut self.host_context {
                                context.spatial = None;
                                if context.mask_scene.masks.is_empty() { self.host_context = None; }
                            }
                        }
                        if enabled {
                            for (label, x, y, par) in [
                                ("Full", (1, 1), (1, 1), (1, 1)),
                                ("Half", (1, 2), (1, 2), (1, 1)),
                                ("Quarter", (1, 4), (1, 4), (1, 1)),
                                ("D1/DV NTSC", (1, 1), (1, 1), (10, 11)),
                            ] {
                                if ui.button(label).clicked() {
                                    if let Some(spatial) = self.host_context.as_mut().and_then(|context| context.spatial.as_mut()) {
                                        spatial.downsample_x = aexcompat_broker::render_request::RationalScale { numerator: x.0, denominator: x.1 };
                                        spatial.downsample_y = aexcompat_broker::render_request::RationalScale { numerator: y.0, denominator: y.1 };
                                        spatial.pixel_aspect_ratio = aexcompat_broker::render_request::RationalScale { numerator: par.0, denominator: par.1 };
                                        if let Some((width, height)) = self.input_image.as_ref().and_then(|path| image::image_dimensions(path).ok()) {
                                            spatial.full_resolution_width = width.checked_mul(x.1 as u32).and_then(|value| value.checked_div(x.0 as u32));
                                            spatial.full_resolution_height = height.checked_mul(y.1 as u32).and_then(|value| value.checked_div(y.0 as u32));
                                        }
                                    }
                                }
                            }
                        }
                    });
                    if let Some(spatial) = self.host_context.as_mut().and_then(|context| context.spatial.as_mut()) {
                        ui.horizontal(|ui| {
                            for (label, ratio) in [
                                ("Downsample X", &mut spatial.downsample_x),
                                ("Downsample Y", &mut spatial.downsample_y),
                                ("Pixel aspect", &mut spatial.pixel_aspect_ratio),
                            ] {
                                ui.label(label);
                                ui.add(egui::DragValue::new(&mut ratio.numerator).range(1..=1_000_000));
                                ui.label("/");
                                ui.add(egui::DragValue::new(&mut ratio.denominator).range(1..=1_000_000));
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut explicit = spatial.full_resolution_width.is_some() && spatial.full_resolution_height.is_some();
                            if ui.checkbox(&mut explicit, "Explicit full-resolution size").changed() {
                                if explicit {
                                    let dimensions = self.input_image.as_ref().and_then(|path| image::image_dimensions(path).ok()).unwrap_or((1, 1));
                                    spatial.full_resolution_width = Some(dimensions.0);
                                    spatial.full_resolution_height = Some(dimensions.1);
                                } else {
                                    spatial.full_resolution_width = None;
                                    spatial.full_resolution_height = None;
                                }
                            }
                            if let (Some(width), Some(height)) = (&mut spatial.full_resolution_width, &mut spatial.full_resolution_height) {
                                ui.add(egui::DragValue::new(width).range(1..=32768));
                                ui.label("x");
                                ui.add(egui::DragValue::new(height).range(1..=32768));
                            }
                        });
                        ui.horizontal(|ui| {
                            let mut explicit = spatial.pre_effect_source_origin_x.is_some()
                                && spatial.pre_effect_source_origin_y.is_some();
                            if ui.checkbox(&mut explicit, "Pre-effect source origin").changed() {
                                if explicit {
                                    spatial.pre_effect_source_origin_x = Some(0);
                                    spatial.pre_effect_source_origin_y = Some(0);
                                } else {
                                    spatial.pre_effect_source_origin_x = None;
                                    spatial.pre_effect_source_origin_y = None;
                                }
                            }
                            if let (Some(x), Some(y)) = (
                                &mut spatial.pre_effect_source_origin_x,
                                &mut spatial.pre_effect_source_origin_y,
                            ) {
                                ui.label("X");
                                ui.add(egui::DragValue::new(x).range(-32768..=32768));
                                ui.label("Y");
                                ui.add(egui::DragValue::new(y).range(-32768..=32768));
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let enabled = self.host_context.as_ref().and_then(|context| context.render_environment).is_some();
                        let mut requested = enabled;
                        if ui.checkbox(&mut requested, "Render environment").changed() {
                            if requested {
                                let context = self.host_context.get_or_insert_with(|| aexcompat_broker::render_request::HostContext {
                                    mask_scene: aexcompat_broker::render_request::MaskScene { masks: Vec::new() },
                                    spatial: None,
                                    render_environment: None,
                                    aux_channels: Vec::new(),
                                    alpha_as_coverage_params: Vec::new(),
                                });
                                context.render_environment = Some(aexcompat_broker::render_request::RenderEnvironment {
                                    quality: aexcompat_broker::render_request::RenderQuality::High,
                                    field: aexcompat_broker::render_request::RenderField::Frame,
                                    shutter_angle: 0.0,
                                    shutter_phase: 0.0,
                                });
                            } else if let Some(context) = &mut self.host_context {
                                context.render_environment = None;
                                if context.mask_scene.masks.is_empty() && context.spatial.is_none() { self.host_context = None; }
                            }
                        }
                    });
                    if let Some(environment) = self.host_context.as_mut().and_then(|context| context.render_environment.as_mut()) {
                        ui.horizontal(|ui| {
                            use aexcompat_broker::render_request::{RenderField, RenderQuality};
                            ui.label("Quality:");
                            ui.selectable_value(&mut environment.quality, RenderQuality::Low, "Low");
                            ui.selectable_value(&mut environment.quality, RenderQuality::High, "High");
                            ui.label("Field:");
                            ui.selectable_value(&mut environment.field, RenderField::Frame, "Frame");
                            ui.selectable_value(&mut environment.field, RenderField::Upper, "Upper");
                            ui.selectable_value(&mut environment.field, RenderField::Lower, "Lower");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Shutter angle:");
                            ui.add(egui::DragValue::new(&mut environment.shutter_angle).speed(0.01).range(0.0..=1.0));
                            ui.label("Shutter phase:");
                            ui.add(egui::DragValue::new(&mut environment.shutter_phase).speed(0.01).range(-1.0..=1.0));
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.busy && !self.parameters.is_empty(), egui::Button::new("Load debug request...")).clicked() { self.load_debug_request(); }
                        if ui.add_enabled(!self.busy && !self.parameters.is_empty(), egui::Button::new("Save debug request...")).clicked() { self.save_debug_request(); }
                    });
                    if let Some(mask_count) = self.host_context.as_ref().map(|context| context.mask_scene.masks.len()) {
                        ui.horizontal(|ui| {
                            ui.label(format!("Host mask context: {mask_count} mask(s)"));
                            if ui.add_enabled(!self.busy, egui::Button::new("Clear masks")).clicked() {
                                if let Some(context) = &mut self.host_context {
                                    context.mask_scene.masks.clear();
                                    if context.spatial.is_none() && context.render_environment.is_none() { self.host_context = None; }
                                }
                            }
                        });
                    }
                    if self.audio_effect_only {
                        ui.colored_label(Color32::from_rgb(30, 120, 170), RichText::new("Audio-only Effect").strong());
                        ui.label("Transport: 44.1 kHz, mono, float32 little-endian raw samples");
                        if ui.add_enabled(!self.busy, egui::Button::new("3. Select input audio (.f32)")).clicked() { self.choose_audio_input(); }
                        if let Some(path) = &self.audio_input { ui.monospace(path.display().to_string()); }
                        if ui.add_enabled(!self.busy && self.audio_input.is_some(), egui::Button::new("4. Render and save audio (.f32)...")).clicked() { self.render_audio_and_save(); }
                    } else {
                        if ui.add_enabled(!self.busy, egui::Button::new("3. Select input image")).clicked() { self.choose_input(ctx); }
                        if let Some(path) = &self.input_image { ui.monospace(path.display().to_string()); }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.busy, egui::Button::new("Select visual audio sidecar (.f32, optional)")).clicked() { self.choose_audio_input(); }
                            if self.audio_input.is_some() && ui.add_enabled(!self.busy, egui::Button::new("Clear sidecar")).clicked() { self.audio_input = None; }
                        });
                        if let Some(path) = &self.audio_input { ui.monospace(format!("Audio sidecar: {}", path.display())); }
                        if self.audio_input.is_some() {
                            ui.label("Sidecar mode: classic ARGB8, mono float32 LE, 44.1 kHz");
                        }
                        if ui.add_enabled(!self.busy, egui::Button::new("Select AE reference output (optional)")).clicked() { self.choose_reference(ctx); }
                        if let Some(path) = &self.reference_image { ui.monospace(format!("Reference: {}", path.display())); }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("4. Quick render")).clicked() { self.quick_render(); }
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("Render and save PNG...")).clicked() { self.render_and_save(); }
                            if ui.add_enabled(!self.busy && self.input_image.is_some(), egui::Button::new("Run 6-case compatibility matrix")).clicked() { self.run_compatibility_matrix(); }
                        });
                    }
                }
            }
            ui.separator();
            ui.label(RichText::new(&self.status).strong());
            if let Some(path) = &self.output_image { ui.monospace(format!("Output: {}", path.display())); }
            if self.input_preview.is_some() || self.preview.is_some() || self.reference_preview.is_some() {
                if ui.button("Open FHD image viewer").clicked() {
                    self.viewer_open = true;
                    self.viewer_mode = if self.preview.is_some() { 2 } else { 0 };
                }
                ui.columns(3, |columns| {
                    show_preview(&mut columns[0], "Input", self.input_preview.as_ref());
                    show_preview(&mut columns[1], "AEX output", self.preview.as_ref());
                    show_preview(&mut columns[2], "AE reference", self.reference_preview.as_ref());
                });
            }
            if let Some(comparison) = &self.pixel_comparison {
                ui.group(|ui| match comparison {
                    Ok(comparison) => {
                        let color = if comparison.exact() {
                            Color32::from_rgb(30, 150, 95)
                        } else {
                            Color32::from_rgb(215, 145, 40)
                        };
                        ui.colored_label(
                            color,
                            RichText::new(if comparison.exact() {
                                "Pixel-exact match with AE reference"
                            } else {
                                "Pixel difference from AE reference"
                            })
                            .strong(),
                        );
                        ui.monospace(format!(
                            "{}x{} | differing pixels: {} / {} | max channel error: {} | MAE: {:.6}",
                            comparison.width,
                            comparison.height,
                            comparison.differing_pixels,
                            u64::from(comparison.width) * u64::from(comparison.height),
                            comparison.max_channel_error,
                            comparison.mean_absolute_error
                        ));
                    }
                    Err(error) => {
                        ui.colored_label(Color32::from_rgb(210, 75, 55), RichText::new("AE reference comparison unavailable").strong());
                        ui.label(error);
                    }
                });
            }
            if let Some(diagnostics) = &self.render_diagnostics {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Effect render diagnostics").strong());
                        ui.monospace(format!(
                            "{} / {} / {}",
                            diagnostics.render_path,
                            diagnostics.pixel_format,
                            diagnostics.worker_classification
                        ));
                    });
                    if diagnostics.gpu_fallback_used {
                        ui.colored_label(
                            Color32::from_rgb(215, 145, 40),
                            format!(
                                "GPU attempt {} at {}; output rejected, fresh CPU worker succeeded",
                                diagnostics
                                    .gpu_attempt_classification
                                    .as_deref()
                                    .unwrap_or("failed"),
                                diagnostics.gpu_failure_stage.as_deref().unwrap_or("unknown stage")
                            ),
                        );
                    }
                    ui.collapsing("Final selector timeline", |ui| {
                        for stage in &diagnostics.final_stages {
                            ui.monospace(stage);
                        }
                    });
                    if !diagnostics.gpu_stages.is_empty() {
                        ui.collapsing("Rejected GPU selector timeline", |ui| {
                            for stage in &diagnostics.gpu_stages {
                                ui.monospace(stage);
                            }
                        });
                    }
                });
            }
            if let Some(diagnostics) = &self.failure_diagnostics {
                ui.group(|ui| {
                    let worker_succeeded = diagnostics.classification == "ok";
                    ui.colored_label(
                        if worker_succeeded {
                            Color32::from_rgb(205, 135, 35)
                        } else {
                            Color32::from_rgb(210, 75, 55)
                        },
                        RichText::new(if worker_succeeded {
                            "Effect worker succeeded; host validation stopped the result"
                        } else {
                            "Effect worker failed safely"
                        })
                        .strong(),
                    );
                    ui.monospace(format!(
                        "classification={} stage={} selector_error={} exit={} elapsed={}ms",
                        diagnostics.classification,
                        diagnostics.failure_stage.as_deref().unwrap_or("unknown"),
                        diagnostics
                            .selector_error
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        diagnostics
                            .exit_code
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        diagnostics
                            .elapsed_ms
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                    ));
                    ui.collapsing("Completed selector timeline", |ui| {
                        for stage in &diagnostics.stages {
                            ui.monospace(stage);
                        }
                    });
                });
            }
            if !self.matrix_results.is_empty() {
                ui.group(|ui| {
                    ui.label(RichText::new("Effect compatibility matrix").strong());
                    egui::Grid::new("effect_compatibility_matrix")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Path");
                            ui.label("Depth");
                            ui.label("Result");
                            ui.label("Details");
                            ui.end_row();
                            for case in &self.matrix_results {
                                ui.monospace(&case.render_path);
                                ui.monospace(&case.pixel_format);
                                if case.passed {
                                    ui.colored_label(Color32::from_rgb(30, 150, 95), "PASS");
                                    let relation = match (
                                        case.output_relation.as_deref(),
                                        case.differing_input_pixels,
                                    ) {
                                        (Some("pixels_changed"), Some(count)) => {
                                            format!("pixels changed: {count}")
                                        }
                                        (Some(value), _) => value.replace('_', " "),
                                        _ => case.output_png.clone().unwrap_or_default(),
                                    };
                                    ui.monospace(relation);
                                } else if !case.applicable {
                                    ui.colored_label(Color32::from_rgb(215, 145, 40), "UNSUPPORTED");
                                    ui.monospace("AEX did not advertise this pixel depth");
                                } else {
                                    ui.colored_label(Color32::from_rgb(210, 75, 55), "FAIL");
                                    let mut details = format!(
                                        "{} / {} / error {}",
                                        case.classification,
                                        case.failure_stage.as_deref().unwrap_or("unknown"),
                                        case.selector_error
                                            .map(|value| value.to_string())
                                            .unwrap_or_else(|| "unknown".into())
                                    );
                                    if let Some(error) = &case.error {
                                        details.push_str(" / ");
                                        details.push_str(error);
                                    }
                                    ui.monospace(details);
                                }
                                ui.end_row();
                            }
                        });
                });
            }
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut self.report).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
            });
                });
        });
        self.show_image_viewer(ctx);
    }
}

fn load_preview(
    ctx: &egui::Context,
    texture_name: &str,
    path: &Path,
) -> Result<egui::TextureHandle, String> {
    let image = image::open(path).map_err(|error| error.to_string())?;
    let rgba = image.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(ctx.load_texture(
        texture_name,
        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    ))
}

fn show_preview(ui: &mut egui::Ui, label: &str, texture: Option<&egui::TextureHandle>) {
    ui.label(RichText::new(label).strong());
    let Some(texture) = texture else {
        ui.label("Not available");
        return;
    };
    let available = ui.available_width().max(1.0);
    let scale = (available / texture.size()[0] as f32).min(1.0);
    ui.image((
        texture.id(),
        egui::vec2(
            texture.size()[0] as f32 * scale,
            texture.size()[1] as f32 * scale,
        ),
    ));
}

fn show_viewer_texture(ui: &mut egui::Ui, label: &str, texture: Option<&egui::TextureHandle>) {
    let Some(texture) = texture else {
        ui.centered_and_justified(|ui| {
            ui.label(format!("{label} is not available"));
        });
        return;
    };
    let source = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.monospace(format!("{} x {}", texture.size()[0], texture.size()[1]));
    });
    let available = ui.available_size().max(egui::vec2(1.0, 1.0));
    let scale = (available.x / source.x).min(available.y / source.y);
    let display = source * scale;
    ui.allocate_ui_with_layout(
        available,
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.image((texture.id(), display));
        },
    );
}

fn repository_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()?
                .parent()?
                .parent()?
                .parent()
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .canonicalize()
                .unwrap()
        })
}

fn main() -> eframe::Result {
    let repository = repository_root();
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 4 && args[1] == "--compare-images" {
        match compare_images(Path::new(&args[2]), Path::new(&args[3])) {
            Ok(comparison) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&comparison.report()).unwrap()
                );
                if !comparison.exact() {
                    std::process::exit(2);
                }
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--render-experimental-matrix" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let report = run_effect_matrix(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            None,
            None,
        );
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-reference-matrix" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let report = run_effect_matrix(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[5]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            Some(Path::new(&args[4])),
            None,
        );
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        if report["failed_count"].as_u64().unwrap_or(6) != 0 {
            std::process::exit(2);
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--render-image" {
        let report = aexcompat_broker::image_render::render_image(
            &repository,
            "scattermap",
            Path::new(&args[2]),
            Path::new(&args[3]),
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5
        && matches!(
            args[1].to_string_lossy().as_ref(),
            "--render-experimental"
                | "--render-experimental-16"
                | "--render-experimental-32"
                | "--render-experimental-smart"
                | "--render-experimental-smart-16"
                | "--render-experimental-smart-32"
        )
    {
        use aexcompat_broker::image_render::RenderPixelFormat;
        let command = args[1].to_string_lossy();
        let smart = command.contains("smart");
        let pixel_format = if command.ends_with("-16") {
            RenderPixelFormat::Argb16
        } else if command.ends_with("-32") {
            RenderPixelFormat::Argb32f
        } else {
            RenderPixelFormat::Argb8
        };
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            smart,
            pixel_format,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6
        && (args[1] == "--render-experimental-request"
            || args[1] == "--render-experimental-smart-request")
    {
        let smart = args[1] == "--render-experimental-smart-request";
        let request_path = Path::new(&args[5]);
        let request_bytes = fs::read(request_path).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is not valid JSON: {error}");
                std::process::exit(1);
            });
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let timing = typed_request_timing(&document).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });
        let host_context = typed_request_host_context(&document).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });
        if let Err(error) = apply_typed_assignments(&mut parameters, &document) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format_and_context(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            timing,
            smart,
            aexcompat_broker::image_render::RenderPixelFormat::Argb8,
            host_context.as_ref(),
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-audio-request" {
        let request_bytes = fs::read(Path::new(&args[5])).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is not valid JSON: {error}");
                std::process::exit(1);
            });
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        if let Err(error) = apply_typed_assignments(&mut parameters, &document) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        match aexcompat_broker::image_render::render_experimental_audio(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-image-audio-sidecar" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        match aexcompat_broker::image_render::render_experimental_image_with_audio_sidecar(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            Path::new(&args[5]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() >= 7
        && args.len() % 2 == 1
        && (args[1] == "--render-experimental-layer-slots"
            || args[1] == "--render-experimental-smart-layer-slots")
    {
        let smart = args[1] == "--render-experimental-smart-layer-slots";
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        if let Err(error) = assign_layer_paths(&mut parameters, &args[5..]) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            smart,
            aexcompat_broker::image_render::RenderPixelFormat::Argb8,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() >= 7
        && args.len() % 2 == 1
        && (args[1] == "--render-experimental-param"
            || args[1] == "--render-experimental-smart-param")
    {
        let smart = args[1] == "--render-experimental-smart-param";
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let mut assigned_slots = Vec::new();
        for assignment in args[5..].chunks_exact(2) {
            let slot = assignment[0].to_string_lossy().parse::<u32>().unwrap_or(0);
            let value = assignment[1]
                .to_string_lossy()
                .parse::<f64>()
                .unwrap_or(f64::NAN);
            if assigned_slots.contains(&slot) {
                eprintln!("parameter slot {slot} was assigned more than once");
                std::process::exit(1);
            }
            let Some(parameter) = parameters.iter_mut().find(|item| item.slot == slot) else {
                eprintln!("AEX exposes no parameter at slot {slot}");
                std::process::exit(1);
            };
            if !value.is_finite() || value < parameter.minimum || value > parameter.maximum {
                eprintln!(
                    "parameter value must be within {}..={}",
                    parameter.minimum, parameter.maximum
                );
                std::process::exit(1);
            }
            parameter.value = value;
            assigned_slots.push(slot);
        }
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            smart,
            aexcompat_broker::image_render::RenderPixelFormat::Argb8,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 7
        && (args[1] == "--render-experimental-time"
            || args[1] == "--render-experimental-smart-time")
    {
        let smart = args[1] == "--render-experimental-smart-time";
        let plugin = Path::new(&args[2]);
        let frame = args[5].to_string_lossy().parse::<i32>().unwrap_or(-1);
        let fps = args[6].to_string_lossy().parse::<u32>().unwrap_or(0);
        let timing = aexcompat_broker::image_render::RenderTiming {
            current_time: frame,
            time_step: 1,
            total_time: frame.saturating_add(1),
            time_scale: fps,
        };
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image_at_time(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image_at_time(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6
        && (args[1] == "--render-experimental-layer"
            || args[1] == "--render-experimental-smart-layer")
    {
        let smart = args[1] == "--render-experimental-smart-layer";
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let Some(layer) = parameters.iter_mut().find(|item| item.kind == "layer") else {
            eprintln!("AEX exposes no secondary layer parameter");
            std::process::exit(1);
        };
        layer.layer_path = Some(PathBuf::from(&args[4]));
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[5]),
                &parameters,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[5]),
                &parameters,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if (6..=13).contains(&args.len())
        && (args[1] == "--render-experimental-layers"
            || args[1] == "--render-experimental-smart-layers")
    {
        let smart = args[1] == "--render-experimental-smart-layers";
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_default();
        let layers = parameters
            .iter_mut()
            .filter(|item| item.kind == "layer")
            .collect::<Vec<_>>();
        if args.len() - 5 > layers.len() {
            eprintln!("more secondary images were supplied than observed layer parameters");
            std::process::exit(1);
        }
        for (layer, path) in layers.into_iter().zip(args[5..].iter()) {
            layer.layer_path = Some(PathBuf::from(path));
        }
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--inspect-experimental" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--inspect-experimental-dependencies" {
        let plugin = Path::new(&args[2]);
        let mode = args[3].to_string_lossy();
        if mode != "all" && mode != "missing" {
            eprintln!("dependency mode must be all or missing");
            std::process::exit(1);
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::inspect_experimental_external_dependencies(
            &repository,
            plugin,
            &hash,
            mode == "missing",
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-options-dialog" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_options_dialog(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-automatic-options-dialog" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_automatic_options_dialog(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-nop-render" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_nop_render(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-smart-nop-render" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_smart_nop_render(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-input-buffer-write" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_input_buffer_write(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-smart-input-buffer-write" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_smart_input_buffer_write(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3
        && (args[1] == "--probe-experimental-expand-buffer"
            || args[1] == "--probe-experimental-shrink-buffer")
    {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let result = if args[1] == "--probe-experimental-expand-buffer" {
            aexcompat_broker::image_render::probe_experimental_expand_buffer(
                &repository,
                plugin,
                &hash,
            )
        } else {
            aexcompat_broker::image_render::probe_experimental_shrink_buffer(
                &repository,
                plugin,
                &hash,
            )
        };
        match result {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-persistent-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_persistent_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-flattened-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_flattened_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-copied-flattened-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::probe_experimental_copied_flattened_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--trigger-experimental-button" {
        let plugin = Path::new(&args[2]);
        let slot = args[3].to_string_lossy().parse::<u32>().unwrap_or(0);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let parameters = match aexcompat_broker::image_render::inspect_experimental(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(parameters) => parameters,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        };
        match aexcompat_broker::image_render::trigger_experimental_button(
            &repository,
            plugin,
            &hash,
            slot,
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--trigger-experimental-request" {
        let plugin = Path::new(&args[2]);
        let slot = args[3].to_string_lossy().parse::<u32>().unwrap_or(0);
        let request_path = Path::new(&args[4]);
        let request_bytes = fs::read(request_path).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is invalid JSON: {error}");
                std::process::exit(1);
            });
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_else(|error| {
                    eprintln!("{error}");
                    std::process::exit(1);
                });
        if let Err(error) = apply_typed_assignments(&mut parameters, &document) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        match aexcompat_broker::image_render::trigger_experimental_button(
            &repository,
            plugin,
            &hash,
            slot,
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--initialize-experimental-aegp" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::initialize_experimental_aegp(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-update-menu" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_update_menu(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-idle" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_idle(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-command-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_command_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-active-idle-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_active_idle_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-comp-idle-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_comp_idle_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-keyframe-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_keyframe_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-seek-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_seek_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-trim-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_trim_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-switch-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = format!("{:X}", Sha256::digest(fs::read(plugin).unwrap()));
        match aexcompat_broker::image_render::dispatch_experimental_aegp_switch_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([920.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AEXCompat Image Harness",
        options,
        Box::new(move |_cc| Ok(Box::new(HarnessApp::new(repository)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_aex(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aexcompat-harness-{name}-{}-{}.{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            "aex"
        ))
    }

    fn temporary_png(name: &str) -> PathBuf {
        temporary_aex(name).with_extension("png")
    }

    fn parameter(slot: u32, kind: &str) -> aexcompat_broker::image_render::InteractiveParameter {
        aexcompat_broker::image_render::InteractiveParameter {
            slot,
            name: format!("Parameter {slot}"),
            kind: kind.into(),
            minimum: 0.0,
            maximum: 1.0,
            value: 0.0,
            choices: Vec::new(),
            color: [255, 0, 0, 0],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: kind == "button",
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }
    }

    #[test]
    fn layer_cli_assignments_are_multi_slot_and_fail_closed() {
        let assignments = ["2", "map.png", "9", "background.png"].map(std::ffi::OsString::from);
        let mut parameters = vec![
            parameter(1, "float"),
            parameter(2, "layer"),
            parameter(9, "layer"),
        ];
        assign_layer_paths(&mut parameters, &assignments).unwrap();
        assert_eq!(
            parameters[1].layer_path.as_deref(),
            Some(Path::new("map.png"))
        );
        assert_eq!(
            parameters[2].layer_path.as_deref(),
            Some(Path::new("background.png"))
        );

        let duplicate = ["2", "a.png", "2", "b.png"].map(std::ffi::OsString::from);
        let mut rejected = vec![parameter(2, "layer")];
        assert!(assign_layer_paths(&mut rejected, &duplicate)
            .unwrap_err()
            .contains("more than once"));
        assert!(rejected[0].layer_path.is_none());
        let wrong_type = ["1", "a.png"].map(std::ffi::OsString::from);
        assert!(assign_layer_paths(&mut parameters, &wrong_type)
            .unwrap_err()
            .contains("not a Layer input"));
        let unknown = ["77", "a.png"].map(std::ffi::OsString::from);
        assert!(assign_layer_paths(&mut parameters, &unknown)
            .unwrap_err()
            .contains("no parameter"));
    }

    #[test]
    fn typed_assignment_document_is_strict_typed_and_atomic() {
        let mut parameters = vec![
            parameter(1, "integer"),
            parameter(2, "color"),
            parameter(3, "point"),
            parameter(4, "layer"),
            parameter(5, "arbitrary_data"),
        ];
        parameters[0].maximum = 10.0;
        parameters[2].component_count = 2;
        let document = serde_json::json!({
            "schema_version": 1,
            "assignments": [
                {"slot": 1, "value": 7},
                {"slot": 2, "color": [255, 20, 40, 60]},
                {"slot": 3, "components": [320.0, 180.0]},
                {"slot": 4, "layer": "map.png"},
                {"slot": 5, "text": "value=7"}
            ]
        });
        apply_typed_assignments(&mut parameters, &document).unwrap();
        let default_timing = typed_request_timing(&document).unwrap();
        assert_eq!(default_timing.current_time, 0);
        assert_eq!(default_timing.time_scale, 1);
        assert_eq!(parameters[0].value, 7.0);
        assert_eq!(parameters[1].color, [255, 20, 40, 60]);
        assert_eq!(parameters[2].components[..2], [320.0, 180.0]);
        assert_eq!(
            parameters[3].layer_path.as_deref(),
            Some(Path::new("map.png"))
        );
        assert_eq!(parameters[4].debug_summary.as_deref(), Some("value=7"));
        let saved = typed_request_document(&parameters, 12, 60, 1, 600, None);
        let saved_timing = typed_request_timing(&saved).unwrap();
        assert_eq!(saved_timing.current_time, 12);
        assert_eq!(saved_timing.time_scale, 60);
        assert_eq!(saved_timing.total_time, 600);
        let mut roundtripped = vec![
            parameter(1, "integer"),
            parameter(2, "color"),
            parameter(3, "point"),
            parameter(4, "layer"),
            parameter(5, "arbitrary_data"),
        ];
        roundtripped[0].maximum = 10.0;
        roundtripped[2].component_count = 2;
        apply_typed_assignments(&mut roundtripped, &saved).unwrap();
        assert_eq!(
            serde_json::to_value(&roundtripped).unwrap(),
            serde_json::to_value(&parameters).unwrap()
        );

        let before = parameters.clone();
        for invalid in [
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":3},{"slot":1,"value":4}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":3,"unknown":true}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":2,"value":3}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":1,"value":11}
            ]}),
            serde_json::json!({"schema_version":1,"assignments":[
                {"slot":5,"text":""}
            ]}),
        ] {
            assert!(apply_typed_assignments(&mut parameters, &invalid).is_err());
            assert_eq!(
                serde_json::to_value(&parameters).unwrap(),
                serde_json::to_value(&before).unwrap()
            );
        }

        let timed = serde_json::json!({
            "schema_version": 1,
            "timing": {"frame": 30, "fps": 24},
            "assignments": []
        });
        let timing = typed_request_timing(&timed).unwrap();
        assert_eq!(timing.current_time, 30);
        assert_eq!(timing.time_step, 1);
        assert_eq!(timing.total_time, 31);
        assert_eq!(timing.time_scale, 24);
        let duration_timing = typed_request_timing(&serde_json::json!({
            "timing":{"frame":30,"fps":24,"duration_frames":240}
        }))
        .unwrap();
        assert_eq!(duration_timing.total_time, 240);
        let fractional_timing = typed_request_timing(&serde_json::json!({
            "timing":{
                "frame":30,"time_scale":30000,"time_step":1001,"duration_frames":300
            }
        }))
        .unwrap();
        assert_eq!(fractional_timing.current_time, 30_030);
        assert_eq!(fractional_timing.time_step, 1_001);
        assert_eq!(fractional_timing.total_time, 300_300);
        assert_eq!(fractional_timing.time_scale, 30_000);
        for invalid_timing in [
            serde_json::json!({"timing":{"frame":-1,"fps":30}}),
            serde_json::json!({"timing":{"frame":1,"fps":0}}),
            serde_json::json!({"timing":{"frame":1,"fps":30,"extra":true}}),
            serde_json::json!({"timing":{"frame":30,"fps":30,"duration_frames":30}}),
            serde_json::json!({"timing":{"frame":1,"time_scale":30000}}),
            serde_json::json!({"timing":{"frame":1,"fps":30,"time_scale":30000,"time_step":1001}}),
            serde_json::json!({"timing":{"frame":10000000,"time_scale":1000000,"time_step":100000,"duration_frames":10000001}}),
        ] {
            assert!(typed_request_timing(&invalid_timing).is_err());
        }
    }

    #[test]
    fn supervised_change_applies_dynamic_ui_flags_atomically() {
        let mut parameters = vec![parameter(1, "integer"), parameter(2, "float")];
        parameters[0].supervised = true;
        let report = serde_json::json!({
            "user_changed_param_requested": true,
            "user_changed_param_error": 0,
            "parameters": [
                {"index": 1, "ui_flags": 1 << 5},
                {"index": 2, "ui_flags": 1 << 9}
            ]
        });
        assert!(apply_dynamic_ui_report(&mut parameters, &report));
        assert!(!parameters[0].enabled);
        assert!(parameters[0].visible);
        assert!(parameters[1].enabled);
        assert!(!parameters[1].visible);

        let before = serde_json::to_value(&parameters).unwrap();
        let incomplete = serde_json::json!({
            "user_changed_param_requested": true,
            "user_changed_param_error": 0,
            "parameters": [{"index": 1, "ui_flags": 0}]
        });
        assert!(!apply_dynamic_ui_report(&mut parameters, &incomplete));
        assert_eq!(serde_json::to_value(&parameters).unwrap(), before);
    }

    #[test]
    fn ae_reference_comparison_reports_exact_and_bounded_pixel_error() {
        let reference_path = temporary_png("reference");
        let exact_path = temporary_png("exact");
        let changed_path = temporary_png("changed");
        let reference =
            image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 128]).unwrap();
        reference.save(&reference_path).unwrap();
        reference.save(&exact_path).unwrap();
        let changed =
            image::RgbaImage::from_raw(2, 1, vec![10, 20, 30, 255, 40, 54, 60, 128]).unwrap();
        changed.save(&changed_path).unwrap();

        let exact = compare_images(&reference_path, &exact_path).unwrap();
        assert!(exact.exact());
        assert_eq!(exact.max_channel_error, 0);
        assert_eq!(exact.mean_absolute_error, 0.0);

        let changed = compare_images(&reference_path, &changed_path).unwrap();
        assert!(!changed.exact());
        assert_eq!(changed.differing_pixels, 1);
        assert_eq!(changed.max_channel_error, 4);
        assert_eq!(changed.mean_absolute_error, 0.5);

        for path in [reference_path, exact_path, changed_path] {
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn ae_reference_comparison_rejects_dimension_mismatch() {
        let reference_path = temporary_png("reference-size");
        let output_path = temporary_png("output-size");
        image::RgbaImage::new(2, 2).save(&reference_path).unwrap();
        image::RgbaImage::new(3, 2).save(&output_path).unwrap();
        let error = compare_images(&reference_path, &output_path).unwrap_err();
        assert!(error.contains("AE reference is 2x2"));
        assert!(error.contains("AEX output is 3x2"));
        fs::remove_file(reference_path).unwrap();
        fs::remove_file(output_path).unwrap();
    }

    #[test]
    fn only_exact_registered_hashes_are_recognized() {
        assert_ne!(SCATTERMAP_HASH, MASKOFFSET_HASH);
        assert_eq!(SCATTERMAP_HASH.len(), 64);
    }

    #[test]
    fn effect_diagnostics_separate_rejected_gpu_and_final_cpu_timelines() {
        let report = serde_json::json!({
            "stage": "interactive_image_render",
            "render_path": "smartfx",
            "pixel_format": "argb32f",
            "worker_classification": "ok",
            "gpu_fallback_used": true,
            "worker_diagnostics": { "stage_events": [
                { "stage": "smart_pre_render", "state": "end", "errors": { "error": 0 } },
                { "stage": "smart_render_cpu", "state": "end", "errors": { "error": 0 } }
            ]},
            "gpu_attempt": {
                "worker_classification": "nonzero_exit",
                "worker_diagnostics": {
                    "failure_stage": "gpu_device_setdown",
                    "stage_events": [
                        { "stage": "smart_render_gpu", "state": "end", "errors": { "error": 0 } },
                        { "stage": "gpu_device_setdown", "state": "end", "errors": { "error": 512 } }
                    ]
                }
            }
        });
        let diagnostics = render_diagnostics(&report).unwrap();
        assert_eq!(diagnostics.render_path, "smartfx");
        assert_eq!(diagnostics.pixel_format, "argb32f");
        assert!(diagnostics.gpu_fallback_used);
        assert_eq!(
            diagnostics.gpu_attempt_classification.as_deref(),
            Some("nonzero_exit")
        );
        assert_eq!(
            diagnostics.gpu_failure_stage.as_deref(),
            Some("gpu_device_setdown")
        );
        assert!(diagnostics.final_stages[1].starts_with("smart_render_cpu"));
        assert!(diagnostics.gpu_stages[0].starts_with("smart_render_gpu"));
        assert!(diagnostics.gpu_stages[1].contains("512"));
    }

    #[test]
    fn failed_worker_diagnostics_are_extracted_from_bounded_error_text() {
        let message = concat!(
            "isolated AEX image render failed validation: diagnostics=",
            r#"{"classification":"nonzero_exit","failure_stage":"smart_render_cpu","exit_code":22,"elapsed_ms":19,"stage_events":[{"stage":"smart_pre_render","state":"end","errors":{"error":0}},{"stage":"smart_render_cpu","state":"end","errors":{"error":25}}]}"#,
            r#", report={"smart_render_error":25}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "nonzero_exit");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("smart_render_cpu")
        );
        assert_eq!(diagnostics.exit_code, Some(22));
        assert_eq!(diagnostics.elapsed_ms, Some(19));
        assert_eq!(diagnostics.selector_error, Some(25));
        assert_eq!(diagnostics.stages.len(), 2);
        assert!(diagnostics.stages[1].contains("25"));
    }

    #[test]
    fn malformed_failure_diagnostics_do_not_escape_the_ui_boundary() {
        assert!(failure_diagnostics("worker report unavailable: not-json").is_none());
        assert!(failure_diagnostics("unrelated error").is_none());
    }

    #[test]
    fn invalid_smartfx_rect_is_reported_without_copying_the_full_worker_report() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":null}, report="#,
            r#"{"result_rects_valid":false,"width":0,"height":0,"large":"payload"}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("result_rect_validation")
        );
        assert_eq!(
            matrix_error_summary(message),
            "SmartFX did not return a valid result rectangle"
        );
    }

    #[test]
    fn unsupported_depth_is_not_misreported_as_a_selector_or_rect_failure() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"render"}, report="#,
            r#"{"depth_supported":false,"smart_render_error":-1,"result_rects_valid":false}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "unsupported_pixel_depth");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("pixel_depth_negotiation")
        );
        assert_eq!(diagnostics.selector_error, None);
        assert_eq!(
            matrix_error_summary(message),
            "AEX did not advertise support for the requested pixel depth"
        );
    }

    #[test]
    fn unsupported_smart_render_path_precedes_depth_and_selector_failures() {
        let message = concat!(
            r#"failed: diagnostics={"classification":"nonzero_exit","failure_stage":"smart_render"}, report="#,
            r#"{"smart_render_supported":false,"depth_supported":false,"smart_render_error":-1,"result_rects_valid":false}"#,
        );
        let diagnostics = failure_diagnostics(message).unwrap();
        assert_eq!(diagnostics.classification, "unsupported_render_path");
        assert_eq!(
            diagnostics.failure_stage.as_deref(),
            Some("render_path_negotiation")
        );
        assert_eq!(diagnostics.selector_error, None);
        assert_eq!(
            matrix_error_summary(message),
            "AEX did not advertise SmartFX render support"
        );
    }

    #[test]
    fn matrix_rows_preserve_success_and_failure_diagnostics() {
        let report = serde_json::json!({
            "stage": "effect_compatibility_matrix",
            "cases": [
                {"render_path":"classic","pixel_format":"argb8","passed":true,
                 "classification":"ok","output_png":"classic.png",
                 "output_relation":"pixels_changed","differing_input_pixels":42},
                {"render_path":"smartfx","pixel_format":"argb32f","passed":false,
                 "classification":"crashed","failure_stage":"smart_render_gpu",
                 "selector_error":512,"error":"GPU selector crashed"},
                {"render_path":"classic","pixel_format":"argb16","passed":false,
                 "applicable":false,"classification":"unsupported_pixel_depth",
                 "failure_stage":"pixel_depth_negotiation"}
            ]
        });
        let cases = compatibility_matrix(&report).unwrap();
        assert_eq!(cases.len(), 3);
        assert!(cases[0].passed);
        assert!(cases[0].applicable);
        assert_eq!(cases[0].output_png.as_deref(), Some("classic.png"));
        assert_eq!(cases[0].output_relation.as_deref(), Some("pixels_changed"));
        assert_eq!(cases[0].differing_input_pixels, Some(42));
        assert!(!cases[1].passed);
        assert_eq!(cases[1].classification, "crashed");
        assert_eq!(cases[1].failure_stage.as_deref(), Some("smart_render_gpu"));
        assert_eq!(cases[1].selector_error, Some(512));
        assert_eq!(cases[1].error.as_deref(), Some("GPU selector crashed"));
        assert!(!cases[2].passed);
        assert!(!cases[2].applicable);
        assert_eq!(cases[2].classification, "unsupported_pixel_depth");
    }

    #[test]
    fn rebuilt_dev_binary_is_rehashed_and_automatically_enabled() {
        let path = temporary_aex("reload");
        let mut first_build = fs::read(std::env::current_exe().unwrap()).unwrap();
        fs::write(&path, &first_build).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let first_hash = format!("{:X}", Sha256::digest(&first_build));
        let mut app = HarnessApp::new(std::env::temp_dir());
        app.selection = Some(Selection {
            path: path.clone(),
            size: metadata.len(),
            sha256: first_hash.clone(),
            profile: None,
            modified: metadata.modified().ok(),
        });
        app.session_approved = true;
        app.trust_rebuilds = true;
        app.last_identity_check = Instant::now() - Duration::from_secs(1);

        first_build.extend_from_slice(b"second build");
        fs::write(&path, &first_build).unwrap();
        app.check_selected_identity();
        assert!(app.selection_stale);

        app.refresh_aex();
        let refreshed = app.selection.as_ref().unwrap();
        assert_ne!(refreshed.sha256, first_hash);
        assert!(!app.selection_stale);
        assert!(app.session_approved);
        assert!(app.inspect_after_refresh);
        assert!(app.parameters.is_empty());
        assert!(app.preview.is_none());

        app.trust_rebuilds = false;
        first_build.extend_from_slice(b"third build");
        fs::write(&path, &first_build).unwrap();
        app.refresh_aex();
        assert!(app.session_approved);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn adjacent_import_discovery_accepts_a_valid_pe_and_rejects_malformed_input() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-import-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let valid = root.join("valid.aex");
        fs::copy(std::env::current_exe().unwrap(), &valid).unwrap();
        assert!(discover_adjacent_imports(&valid).unwrap().is_empty());

        let malformed = root.join("malformed.aex");
        fs::write(&malformed, b"not a PE image").unwrap();
        assert!(discover_adjacent_imports(&malformed)
            .unwrap_err()
            .contains("Could not inspect PE imports"));
        fs::remove_dir_all(root).unwrap();
    }
}
