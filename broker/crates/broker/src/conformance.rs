use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;

const MAX_MISSING_SUITES: usize = 16;
// A native report can contain 65,536 Suite events with 96-byte safely copied
// names. Keep failure parsing bounded, but large enough to retain even the
// escaped worker-bounded report instead of degrading failures to `nonzero_exit`.
const MAX_ERROR_TEXT_BYTES: usize = 32 * 1024 * 1024;
const MAX_ERROR_JSON_BYTES: usize = 24 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PixelDepth {
    Argb8,
    Argb16,
    Argb32f,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderPath {
    Classic,
    Smartfx,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Ok,
    Unsupported,
    SelectorError,
    MissingSuite,
    NonzeroExit,
    Crashed,
    TimeoutKilled,
    InvalidOutput,
    HostValidationError,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Premultiplication {
    Straight,
    Premultiplied,
    Opaque,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MissingSuite {
    pub name: String,
    pub version: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SelectorResult {
    pub render_path: RenderPath,
    pub completed: bool,
    pub error_code: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Rect {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorldMetadata {
    pub width: u32,
    pub height: u32,
    pub row_bytes: u64,
    pub pixel_format: PixelDepth,
    pub premultiplication: Premultiplication,
    pub extent_hint: Rect,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuiteAction {
    Acquire,
    Release,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SuiteEvent {
    pub sequence: u32,
    pub action: SuiteAction,
    pub name: String,
    pub version: u16,
    pub selector: String,
    pub result: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DepthResult {
    pub depth: PixelDepth,
    pub classification: Classification,
    pub selector: SelectorResult,
    pub input_world: Option<WorldMetadata>,
    pub world: Option<WorldMetadata>,
    pub output_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_suites: Vec<MissingSuite>,
    #[serde(default)]
    pub suite_timeline: Vec<SuiteEvent>,
}

#[derive(Clone, Debug)]
pub struct RuntimeFailure {
    pub classification: Classification,
    pub selector_error: Option<i64>,
    pub missing_suites: Vec<MissingSuite>,
    pub suite_timeline: Vec<SuiteEvent>,
    pub retry_classic: bool,
}

impl RuntimeFailure {
    pub fn new(classification: Classification) -> Self {
        Self {
            classification,
            selector_error: None,
            missing_suites: Vec::new(),
            suite_timeline: Vec::new(),
            retry_classic: false,
        }
    }
}

pub trait RuntimeCollectorBackend {
    fn inspect(&mut self) -> Result<Value, RuntimeFailure>;
    fn input_world(&self, depth: PixelDepth) -> Result<WorldMetadata, RuntimeFailure>;
    fn render(&mut self, path: RenderPath, depth: PixelDepth) -> Result<Value, RuntimeFailure>;
}

pub fn collect_runtime_results<B: RuntimeCollectorBackend>(
    backend: &mut B,
    paths: &[RenderPath],
    depths: &[PixelDepth],
) -> Result<Vec<DepthResult>, RuntimeFailure> {
    // input_world is required by every depth_result in the report schema.
    // Decode every requested depth before inspection or rendering so a bad
    // input fails the run without producing schema-invalid partial evidence.
    let requested_depths = depths.iter().take(3).copied().collect::<Vec<_>>();
    let input_worlds = requested_depths
        .iter()
        .map(|&depth| backend.input_world(depth))
        .collect::<Result<Vec<_>, _>>()?;
    let inspection = backend.inspect();
    let inspection_failure = inspection.as_ref().err().cloned();
    let mut results = Vec::with_capacity(depths.len());
    let first_path = paths.first().copied().unwrap_or(RenderPath::Classic);
    for (depth, input_world) in requested_depths.into_iter().zip(input_worlds) {
        if let Some(failure) = &inspection_failure {
            results.push(failed_result_with_input(
                first_path,
                depth,
                Some(input_world),
                failure.clone(),
            ));
            continue;
        }
        let mut selected = failed_result_with_input(
            first_path,
            depth,
            Some(input_world.clone()),
            RuntimeFailure::new(Classification::Unsupported),
        );
        for &path in paths {
            let outcome = backend.render(path, depth);
            let (result, retry_classic) =
                normalize_outcome(path, depth, Some(input_world.clone()), outcome);
            selected = result;
            if !retry_classic {
                break;
            }
        }
        results.push(selected);
    }
    Ok(results)
}

fn normalize_outcome(
    path: RenderPath,
    depth: PixelDepth,
    input_world: Option<WorldMetadata>,
    outcome: Result<Value, RuntimeFailure>,
) -> (DepthResult, bool) {
    match outcome {
        Ok(report) => normalize_success(path, depth, input_world.as_ref(), &report)
            .map(|result| (result, false))
            .unwrap_or_else(|failure| {
                let retry_classic = failure.retry_classic;
                (
                    failed_result_with_input(path, depth, input_world, failure),
                    retry_classic,
                )
            }),
        Err(failure) => {
            let retry_classic = failure.retry_classic;
            (
                failed_result_with_input(path, depth, input_world, failure),
                retry_classic,
            )
        }
    }
}

fn normalize_success(
    path: RenderPath,
    depth: PixelDepth,
    expected_input_world: Option<&WorldMetadata>,
    report: &Value,
) -> Result<DepthResult, RuntimeFailure> {
    let timeline = suite_timeline(report);
    if let Some(failure) = classify_report(path, report) {
        return Err(with_suite_timeline(failure, &timeline));
    }

    let width = report.get("width").and_then(Value::as_u64);
    let height = report.get("height").and_then(Value::as_u64);
    let output_sha256 = report.get("output_sha256").and_then(Value::as_str);
    let (Some(width), Some(height), Some(output_sha256)) = (width, height, output_sha256) else {
        return Err(with_suite_timeline(
            RuntimeFailure::new(Classification::InvalidOutput),
            &timeline,
        ));
    };
    if width == 0 || height == 0 || width > u32::MAX as u64 || height > u32::MAX as u64 {
        return Err(with_suite_timeline(
            RuntimeFailure::new(Classification::InvalidOutput),
            &timeline,
        ));
    }
    if output_sha256.len() != 64 || !output_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(with_suite_timeline(
            RuntimeFailure::new(Classification::InvalidOutput),
            &timeline,
        ));
    }
    let input_world = parse_world(report.get("input_world"))
        .map_err(|failure| with_suite_timeline(failure, &timeline))?;
    let output_world = parse_world(report.get("output_world"))
        .map_err(|failure| with_suite_timeline(failure, &timeline))?;
    if expected_input_world != Some(&input_world)
        || input_world.pixel_format != depth
        || output_world.pixel_format != depth
        || u64::from(output_world.width) != width
        || u64::from(output_world.height) != height
    {
        return Err(with_suite_timeline(
            RuntimeFailure::new(Classification::HostValidationError),
            &timeline,
        ));
    }
    Ok(DepthResult {
        depth,
        classification: Classification::Ok,
        selector: SelectorResult {
            render_path: path,
            completed: true,
            error_code: Some(0),
        },
        input_world: Some(input_world),
        world: Some(output_world),
        output_sha256: Some(output_sha256.to_ascii_lowercase()),
        missing_suites: Vec::new(),
        suite_timeline: timeline,
    })
}

fn failed_result_with_input(
    path: RenderPath,
    depth: PixelDepth,
    input_world: Option<WorldMetadata>,
    mut failure: RuntimeFailure,
) -> DepthResult {
    sanitize_missing_suites(&mut failure.missing_suites);
    DepthResult {
        depth,
        classification: failure.classification,
        selector: SelectorResult {
            render_path: path,
            completed: false,
            error_code: failure.selector_error,
        },
        input_world,
        world: None,
        output_sha256: None,
        missing_suites: failure.missing_suites,
        suite_timeline: failure.suite_timeline,
    }
}

fn parse_pixel_depth(value: Option<&Value>) -> Option<PixelDepth> {
    match value.and_then(Value::as_str) {
        Some("argb8") => Some(PixelDepth::Argb8),
        Some("argb16") => Some(PixelDepth::Argb16),
        Some("argb32f") => Some(PixelDepth::Argb32f),
        Some(_) => None,
        None => None,
    }
}

fn parse_premultiplication(value: Option<&Value>) -> Option<Premultiplication> {
    match value.and_then(Value::as_str) {
        Some("straight") => Some(Premultiplication::Straight),
        Some("premultiplied") => Some(Premultiplication::Premultiplied),
        Some("opaque") => Some(Premultiplication::Opaque),
        Some(_) | None => None,
    }
}

fn parse_rect(value: Option<&Value>) -> Option<Rect> {
    let object = value?.as_object()?;
    Some(Rect {
        left: object.get("left")?.as_i64()?,
        top: object.get("top")?.as_i64()?,
        right: object.get("right")?.as_i64()?,
        bottom: object.get("bottom")?.as_i64()?,
    })
}

fn parse_world(value: Option<&Value>) -> Result<WorldMetadata, RuntimeFailure> {
    let Some(object) = value.and_then(Value::as_object) else {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    };
    let Some(width) = object.get("width").and_then(Value::as_u64) else {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    };
    let Some(height) = object.get("height").and_then(Value::as_u64) else {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    };
    let Some(row_bytes) = object.get("row_bytes").and_then(Value::as_u64) else {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    };
    let Some(pixel_format) = parse_pixel_depth(object.get("pixel_format")) else {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    };
    let Some(premultiplication) = parse_premultiplication(object.get("premultiplication")) else {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    };
    let Some(extent_hint) = parse_rect(object.get("extent_hint")) else {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    };
    let bytes_per_pixel = match pixel_format {
        PixelDepth::Argb8 => 4,
        PixelDepth::Argb16 => 8,
        PixelDepth::Argb32f => 16,
    };
    if width == 0
        || height == 0
        || width > u32::MAX as u64
        || height > u32::MAX as u64
        || row_bytes < width.saturating_mul(bytes_per_pixel)
        || extent_hint.left < 0
        || extent_hint.top < 0
        || extent_hint.right <= extent_hint.left
        || extent_hint.bottom <= extent_hint.top
        || extent_hint.right > i64::try_from(width).unwrap_or(i64::MAX)
        || extent_hint.bottom > i64::try_from(height).unwrap_or(i64::MAX)
    {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    }
    Ok(WorldMetadata {
        width: width as u32,
        height: height as u32,
        row_bytes,
        pixel_format,
        premultiplication,
        extent_hint,
    })
}

fn suite_timeline(report: &Value) -> Vec<SuiteEvent> {
    report
        .get("suite_timeline")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(65_536)
        .filter_map(|item| {
            let event = SuiteEvent {
                sequence: u32::try_from(item.get("sequence")?.as_u64()?).ok()?,
                action: match item.get("action")?.as_str()? {
                    "acquire" => SuiteAction::Acquire,
                    "release" => SuiteAction::Release,
                    _ => return None,
                },
                name: item.get("name")?.as_str()?.to_owned(),
                version: u16::try_from(item.get("version")?.as_u64()?).ok()?,
                selector: item.get("selector")?.as_str()?.to_owned(),
                result: item.get("result")?.as_i64()?,
            };
            valid_suite_event(&event).then_some(event)
        })
        .collect()
}

fn with_suite_timeline(mut failure: RuntimeFailure, timeline: &[SuiteEvent]) -> RuntimeFailure {
    failure.suite_timeline = timeline.to_vec();
    failure
}

fn valid_suite_event(event: &SuiteEvent) -> bool {
    let selector = event.selector.as_bytes();
    let valid_selector = (1..=64).contains(&selector.len())
        && selector
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'_' | b'-'));
    let mut suite = vec![MissingSuite {
        name: event.name.clone(),
        version: event.version,
    }];
    sanitize_missing_suites(&mut suite);
    valid_selector && suite.len() == 1
}

fn selector_error(report: &Value, path: RenderPath) -> Option<i64> {
    let key = match path {
        RenderPath::Classic => "render_error",
        RenderPath::Smartfx => "smart_render_selector_error",
    };
    report
        .get(key)
        .or_else(|| report.get("selector_error"))
        .and_then(Value::as_i64)
}

fn classify_report(path: RenderPath, report: &Value) -> Option<RuntimeFailure> {
    let suites = missing_suites(report);
    if !suites.is_empty() {
        return Some(RuntimeFailure {
            classification: Classification::MissingSuite,
            selector_error: selector_error(report, path),
            missing_suites: suites,
            suite_timeline: Vec::new(),
            retry_classic: false,
        });
    }
    let image_supported = report
        .get("image_render_supported")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let depth_supported = report
        .get("depth_supported")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let smart_supported = report
        .get("smart_render_supported")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !image_supported || !depth_supported {
        return Some(RuntimeFailure {
            classification: Classification::Unsupported,
            selector_error: None,
            missing_suites: Vec::new(),
            suite_timeline: Vec::new(),
            retry_classic: false,
        });
    }
    if path == RenderPath::Smartfx && !smart_supported {
        return Some(RuntimeFailure {
            classification: Classification::Unsupported,
            selector_error: None,
            missing_suites: Vec::new(),
            suite_timeline: Vec::new(),
            retry_classic: true,
        });
    }
    // The native SmartResult initializes undispatched selector stages to -1.
    // Preserve other negative host errors (for example transport error -6),
    // but never let that sentinel mask a later stage's reported error.
    let reported_error = |key| {
        report
            .get(key)
            .and_then(Value::as_i64)
            .filter(|error| *error != 0 && *error != -1)
    };
    let selector_error = selector_error(report, path)
        .filter(|error| *error != 0 && *error != -1)
        .or_else(|| reported_error("pre_render_error"))
        .or_else(|| reported_error("smart_render_error"));
    if selector_error.is_some() {
        return Some(RuntimeFailure {
            classification: Classification::SelectorError,
            selector_error,
            missing_suites: Vec::new(),
            suite_timeline: Vec::new(),
            retry_classic: false,
        });
    }
    match report.get("output_pixels_valid").and_then(Value::as_bool) {
        Some(false) => {
            return Some(RuntimeFailure::new(Classification::InvalidOutput));
        }
        _ => {}
    }
    if report.get("result_rects_valid").and_then(Value::as_bool) == Some(false) {
        return Some(RuntimeFailure::new(Classification::HostValidationError));
    }
    match report
        .get("worker_classification")
        .or_else(|| report.get("classification"))
        .and_then(Value::as_str)
    {
        Some("crashed") => Some(RuntimeFailure::new(Classification::Crashed)),
        Some("timeout") | Some("timeout_killed") => {
            Some(RuntimeFailure::new(Classification::TimeoutKilled))
        }
        Some("nonzero_exit") => Some(RuntimeFailure::new(Classification::NonzeroExit)),
        _ => None,
    }
}

fn missing_suites(report: &Value) -> Vec<MissingSuite> {
    let source = report.get("missing_suites").or_else(|| {
        report
            .get("worker_diagnostics")
            .and_then(|value| value.get("missing_suites"))
    });
    let mut suites = source
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(MissingSuite {
                name: item.get("name")?.as_str()?.to_owned(),
                version: u16::try_from(item.get("version")?.as_u64()?).ok()?,
            })
        })
        .collect::<Vec<_>>();
    sanitize_missing_suites(&mut suites);
    suites
}

fn sanitize_missing_suites(suites: &mut Vec<MissingSuite>) {
    suites.retain(|suite| {
        let bytes = suite.name.as_bytes();
        (2..=64).contains(&bytes.len())
            && bytes[0].is_ascii_alphabetic()
            && bytes[bytes.len() - 1].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'_' | b'-'))
            && suite.version > 0
    });
    suites.sort_by(|left, right| (&left.name, left.version).cmp(&(&right.name, right.version)));
    suites.dedup();
    suites.truncate(MAX_MISSING_SUITES);
}

fn parse_bounded_json_after(message: &str, marker: &[u8]) -> Option<Value> {
    let bytes = message.as_bytes();
    let bounded = &bytes[..bytes.len().min(MAX_ERROR_TEXT_BYTES)];
    let start = bounded
        .windows(marker.len())
        .position(|window| window == marker)?
        + marker.len();
    let tail = &bounded[start..start + (bounded.len() - start).min(MAX_ERROR_JSON_BYTES)];
    serde_json::Deserializer::from_slice(tail)
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
}

fn parse_bounded_json_object(message: &str) -> Option<Value> {
    let bytes = message.as_bytes();
    let bounded = &bytes[..bytes.len().min(MAX_ERROR_TEXT_BYTES)];
    let start = bounded.iter().position(|byte| *byte == b'{')?;
    let tail = &bounded[start..start + (bounded.len() - start).min(MAX_ERROR_JSON_BYTES)];
    serde_json::Deserializer::from_slice(tail)
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
}

fn runtime_failure_from_values(
    path: RenderPath,
    diagnostics: Option<&Value>,
    report: Option<&Value>,
) -> RuntimeFailure {
    let mut failure = report
        .and_then(|value| classify_report(path, value))
        .or_else(|| diagnostics.and_then(|value| classify_report(path, value)))
        .unwrap_or_else(|| RuntimeFailure::new(Classification::NonzeroExit));
    let report_timeline = report.map(suite_timeline).unwrap_or_default();
    let diagnostics_timeline = diagnostics.map(suite_timeline).unwrap_or_default();
    failure.suite_timeline = if report_timeline.is_empty() {
        diagnostics_timeline
    } else {
        report_timeline
    };
    let mut suites = diagnostics.map(missing_suites).unwrap_or_default();
    if let Some(report) = report {
        suites.extend(missing_suites(report));
    }
    sanitize_missing_suites(&mut suites);
    if !suites.is_empty() {
        failure.classification = Classification::MissingSuite;
        failure.missing_suites = suites;
        failure.retry_classic = false;
    }
    failure
}

pub fn runtime_failure_from_io(path: RenderPath, error: &io::Error) -> RuntimeFailure {
    let message = error.to_string();
    let diagnostics = parse_bounded_json_after(&message, b"diagnostics=")
        .or_else(|| parse_bounded_json_object(&message));
    let report = parse_bounded_json_after(&message, b"report=");
    let mut failure = runtime_failure_from_values(path, diagnostics.as_ref(), report.as_ref());
    let diagnostics_is_worker_success = diagnostics.as_ref().is_some_and(|value| {
        value
            .get("worker_classification")
            .or_else(|| value.get("classification"))
            .and_then(Value::as_str)
            == Some("ok")
    });
    let report_is_worker_success = report.as_ref().is_some_and(|value| {
        value
            .get("worker_classification")
            .or_else(|| value.get("classification"))
            .and_then(Value::as_str)
            == Some("ok")
    });
    let broker_io_marker = [
        "failed validation",
        "validated worker",
        "worker report unavailable",
        "worker output",
        "input image",
        "image decode",
        "image format",
    ]
    .iter()
    .any(|needle| message.to_ascii_lowercase().contains(needle));
    if ((!diagnostics.is_some() && !report.is_some())
        || diagnostics_is_worker_success
        || report_is_worker_success
        || (error.kind() == io::ErrorKind::InvalidData && broker_io_marker))
        && failure.classification == Classification::NonzeroExit
    {
        failure.classification = classify_broker_io_error(&message);
        failure.selector_error = None;
        failure.retry_classic = false;
    }
    failure
}

fn classify_broker_io_error(message: &str) -> Classification {
    let message = message.to_ascii_lowercase();
    if [
        "worker output",
        "output raw",
        "output png",
        "output file",
        "output size",
        "output unavailable",
        "output dimensions",
        "output path",
    ]
    .iter()
    .any(|needle| message.contains(needle))
    {
        Classification::InvalidOutput
    } else {
        Classification::HostValidationError
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;
    use crate::image_render::{self, InteractiveParameter, RenderPixelFormat, RenderTiming};
    use std::path::{Path, PathBuf};

    pub struct ImageRenderBackend<'a> {
        pub repository: &'a Path,
        pub plugin_path: &'a Path,
        pub approved_sha256: &'a str,
        pub input_path: &'a Path,
        pub output_directory: &'a Path,
        pub parameters: Vec<InteractiveParameter>,
        pub timing: RenderTiming,
    }

    impl RuntimeCollectorBackend for ImageRenderBackend<'_> {
        fn inspect(&mut self) -> Result<Value, RuntimeFailure> {
            let (parameters, diagnostics) = image_render::inspect_experimental_with_diagnostics(
                self.repository,
                self.plugin_path,
                self.approved_sha256,
            )
            .map_err(|error| runtime_failure_from_io(RenderPath::Classic, &error))?;
            self.parameters = parameters;
            Ok(diagnostics)
        }

        fn input_world(&self, depth: PixelDepth) -> Result<WorldMetadata, RuntimeFailure> {
            let image = image_render::decode_bounded_image(self.input_path, "input preflight")
                .map_err(|_| RuntimeFailure::new(Classification::HostValidationError))?;
            let image = (image.width(), image.height());
            let bytes_per_pixel = match depth {
                PixelDepth::Argb8 => 4,
                PixelDepth::Argb16 => 8,
                PixelDepth::Argb32f => 16,
            };
            Ok(WorldMetadata {
                width: image.0,
                height: image.1,
                row_bytes: u64::from(image.0).saturating_mul(bytes_per_pixel),
                pixel_format: depth,
                premultiplication: Premultiplication::Premultiplied,
                extent_hint: Rect {
                    left: 0,
                    top: 0,
                    right: i64::from(image.0),
                    bottom: i64::from(image.1),
                },
            })
        }

        fn render(&mut self, path: RenderPath, depth: PixelDepth) -> Result<Value, RuntimeFailure> {
            std::fs::create_dir_all(self.output_directory)
                .map_err(|error| runtime_failure_from_io(path, &error))?;
            let output = output_path(self.output_directory, path, depth);
            let format = match depth {
                PixelDepth::Argb8 => RenderPixelFormat::Argb8,
                PixelDepth::Argb16 => RenderPixelFormat::Argb16,
                PixelDepth::Argb32f => RenderPixelFormat::Argb32f,
            };
            image_render::render_experimental_image_at_time_with_format(
                self.repository,
                self.plugin_path,
                self.approved_sha256,
                self.input_path,
                &output,
                &self.parameters,
                self.timing,
                path == RenderPath::Smartfx,
                format,
            )
            .map_err(|error| runtime_failure_from_io(path, &error))
        }
    }

    fn output_path(directory: &Path, path: RenderPath, depth: PixelDepth) -> PathBuf {
        let path = match path {
            RenderPath::Classic => "classic",
            RenderPath::Smartfx => "smartfx",
        };
        let depth = match depth {
            PixelDepth::Argb8 => "argb8",
            PixelDepth::Argb16 => "argb16",
            PixelDepth::Argb32f => "argb32f",
        };
        directory.join(format!("{path}-{depth}.png"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;
    use std::collections::VecDeque;

    struct FakeBackend {
        inspect: Result<Value, RuntimeFailure>,
        renders: VecDeque<Result<Value, RuntimeFailure>>,
    }

    impl RuntimeCollectorBackend for FakeBackend {
        fn inspect(&mut self) -> Result<Value, RuntimeFailure> {
            self.inspect.clone()
        }

        fn input_world(&self, depth: PixelDepth) -> Result<WorldMetadata, RuntimeFailure> {
            Ok(WorldMetadata {
                width: 2,
                height: 3,
                row_bytes: match depth {
                    PixelDepth::Argb8 => 8,
                    PixelDepth::Argb16 => 16,
                    PixelDepth::Argb32f => 32,
                },
                pixel_format: depth,
                premultiplication: Premultiplication::Premultiplied,
                extent_hint: Rect {
                    left: 0,
                    top: 0,
                    right: 2,
                    bottom: 3,
                },
            })
        }

        fn render(
            &mut self,
            _path: RenderPath,
            _depth: PixelDepth,
        ) -> Result<Value, RuntimeFailure> {
            self.renders.pop_front().expect("render outcome")
        }
    }

    struct UndecodableInputBackend {
        inspect_called: Cell<bool>,
        render_called: Cell<bool>,
    }

    impl RuntimeCollectorBackend for UndecodableInputBackend {
        fn inspect(&mut self) -> Result<Value, RuntimeFailure> {
            self.inspect_called.set(true);
            Ok(json!({}))
        }

        fn input_world(&self, _depth: PixelDepth) -> Result<WorldMetadata, RuntimeFailure> {
            Err(RuntimeFailure::new(Classification::HostValidationError))
        }

        fn render(
            &mut self,
            _path: RenderPath,
            _depth: PixelDepth,
        ) -> Result<Value, RuntimeFailure> {
            self.render_called.set(true);
            unreachable!("bad input must fail before native rendering")
        }
    }

    fn successful_report(depth: PixelDepth) -> Value {
        let (pixel_format, row_bytes) = match depth {
            PixelDepth::Argb8 => ("argb8", 8),
            PixelDepth::Argb16 => ("argb16", 16),
            PixelDepth::Argb32f => ("argb32f", 32),
        };
        json!({
            "width": 2, "height": 3, "row_bytes": 16,
            "output_sha256": "ab".repeat(32), "render_error": 0,
            "input_world": {
                "width": 2, "height": 3, "row_bytes": row_bytes,
                "pixel_format": pixel_format, "premultiplication": "premultiplied",
                "extent_hint": {"left": 0, "top": 0, "right": 2, "bottom": 3}
            },
            "output_world": {
                "width": 2, "height": 3, "row_bytes": row_bytes,
                "pixel_format": pixel_format, "premultiplication": "premultiplied",
                "extent_hint": {"left": 0, "top": 1, "right": 2, "bottom": 3}
            }
        })
    }

    #[test]
    fn normalizes_success_and_world_metadata() {
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(successful_report(PixelDepth::Argb16))]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb16])
                .unwrap();
        assert_eq!(result[0].classification, Classification::Ok);
        assert_eq!(result[0].selector.error_code, Some(0));
        assert_eq!(result[0].world.as_ref().unwrap().row_bytes, 16);
        assert_eq!(result[0].world.as_ref().unwrap().extent_hint.left, 0);
    }

    #[test]
    fn undecodable_input_fails_before_inspection_or_depth_results() {
        let mut backend = UndecodableInputBackend {
            inspect_called: Cell::new(false),
            render_called: Cell::new(false),
        };

        let failure = collect_runtime_results(
            &mut backend,
            &[RenderPath::Classic],
            &[PixelDepth::Argb8, PixelDepth::Argb16],
        )
        .unwrap_err();

        assert_eq!(failure.classification, Classification::HostValidationError);
        assert!(!backend.inspect_called.get());
        assert!(!backend.render_called.get());
    }

    #[test]
    fn successful_report_must_match_preflight_input_world() {
        let mut reports = Vec::new();
        for (field, value) in [
            ("width", json!(1)),
            ("row_bytes", json!(12)),
            ("premultiplication", json!("straight")),
            (
                "extent_hint",
                json!({"left": 0, "top": 0, "right": 1, "bottom": 3}),
            ),
        ] {
            let mut report = successful_report(PixelDepth::Argb8);
            report["input_world"][field] = value;
            reports.push(report);
        }

        for report in reports {
            let mut backend = FakeBackend {
                inspect: Ok(json!({})),
                renders: VecDeque::from([Ok(report)]),
            };
            let results = collect_runtime_results(
                &mut backend,
                &[RenderPath::Classic],
                &[PixelDepth::Argb8],
            )
            .unwrap();
            assert_eq!(
                results[0].classification,
                Classification::HostValidationError
            );
        }
    }

    #[test]
    fn distinguishes_missing_suite_selector_crash_and_timeout() {
        let cases = [
            RuntimeFailure {
                classification: Classification::MissingSuite,
                selector_error: Some(2),
                missing_suites: vec![MissingSuite {
                    name: "PF World Suite".into(),
                    version: 2,
                }],
                suite_timeline: Vec::new(),
                retry_classic: false,
            },
            RuntimeFailure {
                classification: Classification::SelectorError,
                selector_error: Some(25),
                missing_suites: vec![],
                suite_timeline: Vec::new(),
                retry_classic: false,
            },
            RuntimeFailure::new(Classification::Crashed),
            RuntimeFailure::new(Classification::TimeoutKilled),
        ];
        for failure in cases {
            let expected = failure.classification;
            let expected_selector = failure.selector_error;
            let expected_suites = failure.missing_suites.len();
            let mut backend = FakeBackend {
                inspect: Ok(json!({})),
                renders: VecDeque::from([Err(failure)]),
            };
            let results =
                collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8])
                    .unwrap();
            assert_eq!(results[0].classification, expected);
            assert_eq!(results[0].selector.error_code, expected_selector);
            assert_eq!(results[0].missing_suites.len(), expected_suites);
            assert!(results[0].input_world.is_some());
        }
    }

    #[test]
    fn inspect_failure_prevents_render_and_preserves_classification() {
        let mut backend = FakeBackend {
            inspect: Err(RuntimeFailure::new(Classification::Crashed)),
            renders: VecDeque::new(),
        };
        let results = collect_runtime_results(
            &mut backend,
            &[RenderPath::Classic, RenderPath::Smartfx],
            &[PixelDepth::Argb8],
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results
            .iter()
            .all(|item| item.classification == Classification::Crashed));
    }

    #[test]
    fn extracts_structured_worker_failure_from_io_error() {
        let diagnostics = json!({
            "classification": "timeout_killed",
            "selector_error": 9,
            "missing_suites": [{"name": "PF World Suite", "version": 2}]
        });
        let error = io::Error::other(format!("worker failed safely: {diagnostics}"));
        let failure = runtime_failure_from_io(RenderPath::Classic, &error);
        assert_eq!(failure.classification, Classification::MissingSuite);
        assert_eq!(failure.selector_error, Some(9));
        assert_eq!(failure.missing_suites.len(), 1);

        let error = io::Error::other(format!("worker failed: {diagnostics}, report=ignored"));
        let failure = runtime_failure_from_io(RenderPath::Classic, &error);
        assert_eq!(failure.classification, Classification::MissingSuite);
    }

    #[test]
    fn invalid_success_report_is_classified() {
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(json!({"width": 2, "height": 2}))]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Smartfx], &[PixelDepth::Argb32f])
                .unwrap();
        assert_eq!(result[0].classification, Classification::InvalidOutput);
        assert!(!result[0].selector.completed);
    }

    #[test]
    fn invalid_output_digest_is_rejected() {
        let mut report = successful_report(PixelDepth::Argb8);
        report["output_sha256"] = json!("not-a-digest");
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(report)]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8])
                .unwrap();
        assert_eq!(result[0].classification, Classification::InvalidOutput);
    }

    #[test]
    fn rejects_extent_outside_world_or_with_signed_overflow_inputs() {
        for extent in [
            json!({"left": -1, "top": 0, "right": 2, "bottom": 3}),
            json!({"left": 0, "top": 0, "right": 3, "bottom": 3}),
            json!({"left": 1, "top": 1, "right": 1, "bottom": 2}),
            json!({"left": 0, "top": 0, "right": i64::MAX, "bottom": 3}),
        ] {
            let mut report = successful_report(PixelDepth::Argb8);
            report["output_world"]["extent_hint"] = extent;
            let mut backend = FakeBackend {
                inspect: Ok(json!({})),
                renders: VecDeque::from([Ok(report)]),
            };
            let result =
                collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8])
                    .unwrap();
            assert_eq!(
                result[0].classification,
                Classification::HostValidationError
            );
        }
    }

    #[test]
    fn preserves_bounded_valid_suite_timeline() {
        let mut report = successful_report(PixelDepth::Argb8);
        report["suite_timeline"] = json!([
            {"sequence": 0, "action": "acquire", "name": "PF World Suite", "version": 2, "selector": "PF Render", "result": 0},
            {"sequence": 1, "action": "release", "name": "C:\\private", "version": 2, "selector": "PF Render", "result": 0}
        ]);
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(report)]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8])
                .unwrap();
        assert_eq!(result[0].suite_timeline.len(), 1);
        assert_eq!(result[0].suite_timeline[0].action, SuiteAction::Acquire);
    }

    #[test]
    fn falls_back_from_unsupported_smartfx_to_classic_once_per_depth() {
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([
                Err(RuntimeFailure {
                    classification: Classification::Unsupported,
                    selector_error: None,
                    missing_suites: Vec::new(),
                    suite_timeline: Vec::new(),
                    retry_classic: true,
                }),
                Ok(successful_report(PixelDepth::Argb8)),
            ]),
        };
        let result = collect_runtime_results(
            &mut backend,
            &[RenderPath::Smartfx, RenderPath::Classic],
            &[PixelDepth::Argb8],
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].classification, Classification::Ok);
        assert_eq!(result[0].selector.render_path, RenderPath::Classic);
    }

    #[test]
    fn parses_diagnostics_and_report_with_bounded_exclusive_classification() {
        let error = io::Error::other(
            "worker failed safely: diagnostics={\"classification\":\"nonzero_exit\"}, report={\"smart_render_supported\":false,\"depth_supported\":true}",
        );
        let failure = runtime_failure_from_io(RenderPath::Smartfx, &error);
        assert_eq!(failure.classification, Classification::Unsupported);
        assert!(failure.retry_classic);

        let error = io::Error::other(
            "worker failed safely: diagnostics={\"classification\":\"nonzero_exit\"}, report={\"render_error\":25}",
        );
        let failure = runtime_failure_from_io(RenderPath::Classic, &error);
        assert_eq!(failure.classification, Classification::SelectorError);
        assert_eq!(failure.selector_error, Some(25));
        assert!(!failure.retry_classic);
    }

    #[test]
    fn redacted_path_does_not_hide_compact_worker_report() {
        let message = r#"worker failed at C:\private\worker.exe,diagnostics={"classification":"nonzero_exit"},report={"render_error":25}"#;
        let (redacted, truncated) = crate::redact_windows_paths(message, 4096);
        assert!(!truncated);
        let failure = runtime_failure_from_io(RenderPath::Classic, &io::Error::other(redacted));
        assert_eq!(failure.classification, Classification::SelectorError);
        assert_eq!(failure.selector_error, Some(25));
    }

    #[test]
    fn smartfx_uses_first_nonzero_selector_error() {
        for (field, code) in [("pre_render_error", 25), ("smart_render_error", 516)] {
            let mut report = json!({
                "smart_render_selector_error": 0,
                "pre_render_error": 0,
                "smart_render_error": 0,
                "output_pixels_valid": false
            });
            report[field] = json!(code);

            let failure = classify_report(RenderPath::Smartfx, &report).expect("classification");
            assert_eq!(failure.classification, Classification::SelectorError);
            assert_eq!(failure.selector_error, Some(code));
        }
    }

    #[test]
    fn smartfx_skips_native_not_dispatched_sentinels() {
        for (pre_render_error, smart_render_error, expected) in
            [(25, -1, 25), (-1, -6, -6)]
        {
            let report = json!({
                "smart_render_selector_error": -1,
                "pre_render_error": pre_render_error,
                "smart_render_error": smart_render_error,
                "output_pixels_valid": false
            });

            let failure = classify_report(RenderPath::Smartfx, &report).expect("classification");
            assert_eq!(failure.classification, Classification::SelectorError);
            assert_eq!(failure.selector_error, Some(expected));
        }
    }

    #[test]
    fn preserves_broker_io_and_validation_classifications() {
        let output = io::Error::new(
            io::ErrorKind::InvalidData,
            "validated worker output size mismatch",
        );
        assert_eq!(
            runtime_failure_from_io(RenderPath::Classic, &output).classification,
            Classification::InvalidOutput
        );

        let input = io::Error::new(io::ErrorKind::InvalidData, "input image decode failed");
        assert_eq!(
            runtime_failure_from_io(RenderPath::Classic, &input).classification,
            Classification::HostValidationError
        );

        let report = io::Error::other(
            "validated worker output unavailable: error=missing, diagnostics={\"classification\":\"ok\"}, report={\"worker_classification\":\"ok\"}",
        );
        assert_eq!(
            runtime_failure_from_io(RenderPath::Classic, &report).classification,
            Classification::InvalidOutput
        );

        let diagnostics_only = io::Error::other(
            "validated worker output unavailable: error=missing, diagnostics={\"classification\":\"ok\"}",
        );
        assert_eq!(
            runtime_failure_from_io(RenderPath::Classic, &diagnostics_only).classification,
            Classification::InvalidOutput
        );

        let validation_with_worker_failure = io::Error::new(
            io::ErrorKind::InvalidData,
            "isolated AEX image render failed validation: diagnostics={\"classification\":\"nonzero_exit\"}, report={\"worker_classification\":\"nonzero_exit\"}",
        );
        assert_eq!(
            runtime_failure_from_io(RenderPath::Classic, &validation_with_worker_failure)
                .classification,
            Classification::HostValidationError
        );

        let output_with_worker_failure = io::Error::new(
            io::ErrorKind::InvalidData,
            "validated worker output size mismatch: diagnostics={\"classification\":\"nonzero_exit\"}, report={\"worker_classification\":\"nonzero_exit\"}",
        );
        assert_eq!(
            runtime_failure_from_io(RenderPath::Classic, &output_with_worker_failure)
                .classification,
            Classification::InvalidOutput
        );
    }

    #[test]
    fn preserves_suite_timeline_for_failed_depth_reports() {
        let timeline = json!([
            {"sequence": 0, "action": "acquire", "name": "PF World Suite", "version": 2, "selector": "PF Render", "result": 0}
        ]);
        for (field, value, expected) in [
            ("render_error", json!(25), Classification::SelectorError),
            (
                "output_pixels_valid",
                json!(false),
                Classification::InvalidOutput,
            ),
            (
                "result_rects_valid",
                json!(false),
                Classification::HostValidationError,
            ),
        ] {
            let mut report = successful_report(PixelDepth::Argb8);
            report[field] = value;
            report["suite_timeline"] = timeline.clone();
            let mut backend = FakeBackend {
                inspect: Ok(json!({})),
                renders: VecDeque::from([Ok(report)]),
            };
            let result =
                collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8])
                    .unwrap();
            assert_eq!(result[0].classification, expected);
            assert_eq!(result[0].suite_timeline.len(), 1);
            assert_eq!(result[0].suite_timeline[0].action, SuiteAction::Acquire);
        }

        let mut report = successful_report(PixelDepth::Argb8);
        report["missing_suites"] = json!([
            {"name": "PF World Suite", "version": 2}
        ]);
        report["suite_timeline"] = timeline;
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(report)]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8])
                .unwrap();
        assert_eq!(result[0].classification, Classification::MissingSuite);
        assert_eq!(result[0].suite_timeline.len(), 1);
    }

    #[test]
    fn preserves_failure_reports_larger_than_32_kib() {
        let timeline = (0..600)
            .map(|sequence| {
                json!({
                    "sequence": sequence,
                    "action": "acquire",
                    "name": "PF Very Long Synthetic Conformance Suite Name For Failure Report",
                    "version": 2,
                    "selector": "PF Very Long Synthetic Selector Name For Failure Report",
                    "result": 0
                })
            })
            .collect::<Vec<_>>();
        let report = json!({
            "render_error": 25,
            "suite_timeline": timeline
        });
        let message = format!(
            "worker failed safely: diagnostics={{\"classification\":\"nonzero_exit\"}}, report={report}"
        );
        assert!(message.len() > 32 * 1024);

        let failure = runtime_failure_from_io(RenderPath::Classic, &io::Error::other(message));
        assert_eq!(failure.classification, Classification::SelectorError);
        assert_eq!(failure.selector_error, Some(25));
        assert_eq!(failure.suite_timeline.len(), 600);
    }

    #[test]
    fn preserves_worker_bounded_failure_reports_larger_than_16_mib() {
        let report = format!(
            "{{\"render_error\":25,\"bounded_worker_payload\":\"{}\"}}",
            "x".repeat(17 * 1024 * 1024)
        );
        let message = format!(
            "worker failed safely: diagnostics={{\"classification\":\"nonzero_exit\"}}, report={report}"
        );
        assert!(report.len() > 16 * 1024 * 1024);
        assert!(report.len() < MAX_ERROR_JSON_BYTES);

        let failure = runtime_failure_from_io(RenderPath::Classic, &io::Error::other(message));
        assert_eq!(failure.classification, Classification::SelectorError);
        assert_eq!(failure.selector_error, Some(25));
    }

    #[test]
    fn only_smartfx_path_unsupported_can_trigger_classic_fallback() {
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([
                Err(RuntimeFailure {
                    classification: Classification::Unsupported,
                    selector_error: None,
                    missing_suites: Vec::new(),
                    suite_timeline: Vec::new(),
                    retry_classic: false,
                }),
                Ok(successful_report(PixelDepth::Argb8)),
            ]),
        };
        let result = collect_runtime_results(
            &mut backend,
            &[RenderPath::Classic, RenderPath::Smartfx],
            &[PixelDepth::Argb8],
        )
        .unwrap();
        assert_eq!(result[0].classification, Classification::Unsupported);
        assert_eq!(backend.renders.len(), 1);
    }

    #[test]
    fn worker_report_fixture_covers_all_depths_and_preserves_world_metadata() {
        for depth in [PixelDepth::Argb8, PixelDepth::Argb16, PixelDepth::Argb32f] {
            let mut backend = FakeBackend {
                inspect: Ok(json!({})),
                renders: VecDeque::from([Ok(successful_report(depth))]),
            };
            let result = collect_runtime_results(
                &mut backend,
                &[RenderPath::Classic],
                std::slice::from_ref(&depth),
            )
            .unwrap();
            let result = &result[0];
            assert_eq!(result.classification, Classification::Ok);
            assert_eq!(result.world.as_ref().unwrap().pixel_format, depth);
            assert_eq!(result.input_world.as_ref().unwrap().pixel_format, depth);
            let bytes_per_pixel = match depth {
                PixelDepth::Argb8 => 4,
                PixelDepth::Argb16 => 8,
                PixelDepth::Argb32f => 16,
            };
            assert!(result.world.as_ref().unwrap().row_bytes >= 2 * bytes_per_pixel);
        }
    }

    #[test]
    fn missing_world_metadata_is_validation_error_not_a_default_world() {
        let mut report = successful_report(PixelDepth::Argb8);
        report.as_object_mut().unwrap().remove("input_world");
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(report)]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8])
                .unwrap();
        assert_eq!(
            result[0].classification,
            Classification::HostValidationError
        );
    }

    #[test]
    fn serializes_empty_suite_timeline_for_failures() {
        let result = failed_result_with_input(
            RenderPath::Classic,
            PixelDepth::Argb8,
            None,
            RuntimeFailure::new(Classification::Crashed),
        );
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["suite_timeline"], json!([]));
    }
}
