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
    missing_suites: Vec<MissingSuite>,
    last_seh_selector: Option<String>,
    last_seh_error: Option<i64>,
    last_seh_exception_code: Option<u64>,
    stages: Vec<String>,
}

#[derive(Debug, PartialEq)]
struct MissingSuite {
    name: String,
    version: i32,
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
    request_path: Option<&Path>,
) -> Result<(), String> {
    let root = document
        .as_object()
        .ok_or_else(|| "assignment document must be an object".to_owned())?;
    if root.keys().any(|key| {
        !matches!(
            key.as_str(),
            "schema_version"
                | "assignments"
                | "timing"
                | "host_context"
                | "render_settings"
                | "dependencies"
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
                let layer = Path::new(path);
                // A bundle-relative layer path (written by the conformance runner)
                // is resolved against the request's own bundle root with the same
                // traversal and containment guards as pinned dependencies, so a
                // moved or replayed bundle keeps resolving and no absolute host
                // path is trusted. Absolute paths, or requests with no bundle root
                // context, stay verbatim for backwards compatibility.
                let resolved = match (layer.is_absolute(), request_path) {
                    (false, Some(request)) => {
                        let bundle_root = request
                            .parent()
                            .and_then(Path::parent)
                            .ok_or_else(|| "request path has no bundle root".to_owned())?
                            .canonicalize()
                            .map_err(|error| {
                                format!("bundle root could not be resolved: {error}")
                            })?;
                        if layer
                            .components()
                            .any(|component| !matches!(component, std::path::Component::Normal(_)))
                        {
                            return Err(format!(
                                "parameter slot {slot} layer path must be bundle-relative without traversal"
                            ));
                        }
                        let candidate =
                            bundle_root.join(layer).canonicalize().map_err(|error| {
                                format!("layer path could not be resolved: {error}")
                            })?;
                        if !candidate.starts_with(&bundle_root) || !candidate.is_file() {
                            return Err(format!(
                                "parameter slot {slot} layer path escapes the bundle or is not a file"
                            ));
                        }
                        candidate
                    }
                    _ => PathBuf::from(path),
                };
                parameter.layer_path = Some(resolved);
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

const CONFORMANCE_RENDER_SETTINGS_ENV: &str = "AEXCOMPAT_CONFORMANCE_RENDER_SETTINGS";

fn typed_request_render_settings(document: &serde_json::Value) -> Result<Option<String>, String> {
    let Some(settings) = document.get("render_settings") else {
        return Ok(None);
    };
    let settings = settings
        .as_object()
        .ok_or_else(|| "render_settings must be an object".to_owned())?;
    if settings.keys().any(|key| {
        !matches!(
            key.as_str(),
            "premultiplication" | "color_management" | "linear_light" | "renderer"
        )
    }) {
        return Err("render_settings contains an unknown field".into());
    }
    let premultiplication = settings
        .get("premultiplication")
        .and_then(serde_json::Value::as_str)
        .filter(|value| matches!(*value, "straight" | "premultiplied" | "opaque"))
        .ok_or_else(|| "render_settings premultiplication is unsupported".to_owned())?;
    let color_management = settings
        .get("color_management")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "render_settings color_management must be an object".to_owned())?;
    if color_management
        .keys()
        .any(|key| !matches!(key.as_str(), "enabled" | "working_space"))
    {
        return Err("render_settings color_management contains an unknown field".into());
    }
    let color_enabled = color_management
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "render_settings color_management.enabled must be boolean".to_owned())?;
    let working_space = color_management
        .get("working_space")
        .ok_or_else(|| "render_settings color_management.working_space is required".to_owned())?;
    if color_enabled || !working_space.is_null() {
        return Err("unsupported render setting: color management".into());
    }
    let linear_light = settings
        .get("linear_light")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "render_settings linear_light must be boolean".to_owned())?;
    if linear_light {
        return Err("unsupported render setting: linear light".into());
    }
    let renderer = settings
        .get("renderer")
        .and_then(serde_json::Value::as_str)
        .filter(|value| matches!(*value, "AEXCompat CPU" | "software"))
        .ok_or_else(|| "unsupported render setting: renderer".to_owned())?;
    if [premultiplication, renderer]
        .iter()
        .any(|value| value.bytes().any(|byte| byte < 0x20 || byte == b'|'))
    {
        return Err("render_settings contains an unsafe transport value".into());
    }
    Ok(Some(format!("v1|{premultiplication}|0|-|0|{renderer}")))
}

struct ConformanceRenderSettingsGuard {
    previous: Option<std::ffi::OsString>,
}

impl ConformanceRenderSettingsGuard {
    fn install(encoded: Option<&str>) -> Result<Self, String> {
        let previous = std::env::var_os(CONFORMANCE_RENDER_SETTINGS_ENV);
        // SAFETY: this guard is installed only by the synchronous CLI path before
        // any render worker thread is spawned, and remains alive until that work joins.
        unsafe {
            match encoded {
                Some(value) => std::env::set_var(CONFORMANCE_RENDER_SETTINGS_ENV, value),
                None => std::env::remove_var(CONFORMANCE_RENDER_SETTINGS_ENV),
            }
        }
        Ok(Self { previous })
    }
}

impl Drop for ConformanceRenderSettingsGuard {
    fn drop(&mut self) {
        // SAFETY: the synchronous CLI render has completed before this guard drops,
        // so no worker thread can concurrently access or mutate the process environment.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(CONFORMANCE_RENDER_SETTINGS_ENV, value),
                None => std::env::remove_var(CONFORMANCE_RENDER_SETTINGS_ENV),
            }
        }
    }
}

fn typed_request_dependencies(
    document: &serde_json::Value,
    request_path: &Path,
) -> Result<Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>, String> {
    let Some(items) = document.get("dependencies") else {
        return Ok(Vec::new());
    };
    let items = items
        .as_array()
        .filter(|items| items.len() <= 64)
        .ok_or_else(|| "dependencies must be an array of at most 64 artifacts".to_owned())?;
    let bundle_root = request_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "request path has no bundle root".to_owned())?
        .canonicalize()
        .map_err(|error| format!("bundle root could not be resolved: {error}"))?;
    let mut dependencies = Vec::with_capacity(items.len());
    let mut basenames = std::collections::HashSet::new();
    for item in items {
        let item = item
            .as_object()
            .ok_or_else(|| "dependency identity must be an object".to_owned())?;
        if item
            .keys()
            .any(|key| !matches!(key.as_str(), "path" | "sha256" | "size_bytes"))
        {
            return Err("dependency identity contains an unknown field".into());
        }
        let relative = item
            .get("path")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 240 && !value.contains('\\'))
            .ok_or_else(|| "dependency path is invalid".to_owned())?;
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err("dependency path must be bundle-relative without traversal".into());
        }
        let path = bundle_root
            .join(relative_path)
            .canonicalize()
            .map_err(|error| format!("dependency path could not be resolved: {error}"))?;
        if !path.starts_with(&bundle_root) || !path.is_file() {
            return Err("dependency path escapes the bundle or is not a file".into());
        }
        let basename = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "dependency basename is not Unicode".to_owned())?
            .to_ascii_lowercase();
        if !basenames.insert(basename) {
            return Err("dependency basenames must be case-insensitively unique".into());
        }
        let expected_size = item
            .get("size_bytes")
            .and_then(serde_json::Value::as_u64)
            .filter(|size| *size <= 1024 * 1024 * 1024 * 1024)
            .ok_or_else(|| "dependency size_bytes is invalid".to_owned())?;
        let actual_size = fs::metadata(&path)
            .map_err(|error| format!("dependency metadata failed: {error}"))?
            .len();
        if actual_size != expected_size {
            return Err("dependency size changed after bundle verification".into());
        }
        let expected_sha256 = item
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "dependency sha256 is missing".to_owned())
            .and_then(decode_sha256)?;
        dependencies.push(
            aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact {
                path,
                expected_sha256,
                expected_size,
            },
        );
    }
    Ok(dependencies)
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
            missing_suites: Vec::new(),
            last_seh_selector: None,
            last_seh_error: None,
            last_seh_exception_code: None,
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
    // Plug-in hash is provenance, not a private path, so it is safe to trace.
    tracing::debug!(
        plugin_hash = %hash,
        parameters = parameters.len(),
        "harness effect matrix render start"
    );
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

fn typed_failure_document(message: &str) -> Option<serde_json::Value> {
    let diagnostics = json_after_marker(message, "diagnostics=")
        .or_else(|| json_after_marker(message, "worker report unavailable: "))
        .or_else(|| {
            json_after_marker(message, "AEX parameter inspection worker failed safely: ")
        })?;
    let report = json_after_marker(message, "report=");
    let mut document = report
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    for field in [
        "classification",
        "failure_stage",
        "exit_code",
        "elapsed_ms",
        "plugin_kind",
        "missing_suites",
        "suite_timeline",
    ] {
        if let Some(value) = diagnostics.get(field) {
            document.insert(field.to_owned(), value.clone());
        }
    }
    Some(serde_json::Value::Object(document))
}

fn emit_typed_failure(message: &str) {
    if let Some(document) = typed_failure_document(message)
        && let Ok(encoded) = serde_json::to_string(&document)
    {
        println!("{encoded}");
    }
    eprintln!("{message}");
}

fn emit_typed_failure_with_parameter_metadata(
    message: &str,
    parameter_metadata: &serde_json::Value,
) {
    let mut document = typed_failure_document(message).unwrap_or_else(|| {
        serde_json::json!({
            "classification": "nonzero_exit",
            "failure_stage": "render",
        })
    });
    document["parameter_metadata"] = parameter_metadata.clone();
    if let Ok(encoded) = serde_json::to_string(&document) {
        println!("{encoded}");
    }
    eprintln!("{message}");
}

fn host_request_validation_failure(parameter_metadata: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "classification": "host_validation_error",
        "failure_stage": "request_validation",
        "parameter_metadata": parameter_metadata,
    })
}

fn emit_host_request_validation_failure(message: &str, parameter_metadata: &serde_json::Value) {
    let document = host_request_validation_failure(parameter_metadata);
    if let Ok(encoded) = serde_json::to_string(&document) {
        println!("{encoded}");
    }
    eprintln!("{message}");
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
    let missing_suites = diagnostics["missing_suites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|suite| {
            let name = suite["name"].as_str()?;
            let version = i32::try_from(suite["version"].as_i64()?).ok()?;
            valid_suite_name(name)
                .then(|| MissingSuite {
                    name: name.to_owned(),
                    version,
                })
                .filter(|suite| suite.version > 0)
        })
        .fold(Vec::new(), |mut suites, suite| {
            if suites.len() < 16 && !suites.contains(&suite) {
                suites.push(suite);
            }
            suites
        });
    let last_seh_selector = report
        .as_ref()
        .and_then(|value| value["last_seh_selector"].as_str())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 32
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
        })
        .map(str::to_owned);
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
        missing_suites,
        last_seh_selector,
        last_seh_error: report
            .as_ref()
            .and_then(|value| value["last_seh_error"].as_i64())
            .filter(|value| i32::try_from(*value).is_ok()),
        last_seh_exception_code: report
            .as_ref()
            .and_then(|value| value["last_seh_exception_code"].as_u64())
            .filter(|value| u32::try_from(*value).is_ok()),
        stages: completed_stages(Some(&diagnostics)),
    })
}

fn valid_suite_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'_' | b'-'))
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

/// The AEX's own SUPPORTS_SMART_RENDER declaration from the parameter
/// inspection diagnostics; None until an inspection has completed (issue #105).
fn advertised_smart_render(report: &serde_json::Value) -> Option<bool> {
    report["worker_diagnostics"]["smart_render_advertised"].as_bool()
}
