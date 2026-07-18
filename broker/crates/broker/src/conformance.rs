use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;

const MAX_MISSING_SUITES: usize = 16;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suite_timeline: Vec<SuiteEvent>,
}

#[derive(Clone, Debug)]
pub struct RuntimeFailure {
    pub classification: Classification,
    pub selector_error: Option<i64>,
    pub missing_suites: Vec<MissingSuite>,
}

impl RuntimeFailure {
    pub fn new(classification: Classification) -> Self {
        Self {
            classification,
            selector_error: None,
            missing_suites: Vec::new(),
        }
    }
}

pub trait RuntimeCollectorBackend {
    fn inspect(&mut self) -> Result<Value, RuntimeFailure>;
    fn input_world(&self, depth: PixelDepth) -> Option<WorldMetadata>;
    fn render(&mut self, path: RenderPath, depth: PixelDepth) -> Result<Value, RuntimeFailure>;
}

pub fn collect_runtime_results<B: RuntimeCollectorBackend>(
    backend: &mut B,
    paths: &[RenderPath],
    depths: &[PixelDepth],
) -> Vec<DepthResult> {
    let inspection = backend.inspect();
    let inspection_failure = inspection.as_ref().err().cloned();
    let mut results = Vec::with_capacity(depths.len());
    let first_path = paths.first().copied().unwrap_or(RenderPath::Classic);
    for &depth in depths.iter().take(3) {
        let input_world = backend.input_world(depth);
        if let Some(failure) = &inspection_failure {
            results.push(failed_result_with_input(
                first_path,
                depth,
                input_world,
                failure.clone(),
            ));
            continue;
        }
        let mut selected = failed_result_with_input(
            first_path,
            depth,
            input_world.clone(),
            RuntimeFailure::new(Classification::Unsupported),
        );
        for &path in paths {
            let outcome = backend.render(path, depth);
            selected = normalize_outcome(path, depth, input_world.clone(), outcome);
            if selected.classification != Classification::Unsupported {
                break;
            }
        }
        results.push(selected);
    }
    results
}

fn normalize_outcome(
    path: RenderPath,
    depth: PixelDepth,
    input_world: Option<WorldMetadata>,
    outcome: Result<Value, RuntimeFailure>,
) -> DepthResult {
    match outcome {
        Ok(report) => normalize_success(path, depth, &report)
            .unwrap_or_else(|failure| failed_result_with_input(path, depth, input_world, failure)),
        Err(failure) => failed_result_with_input(path, depth, input_world, failure),
    }
}

fn normalize_success(
    path: RenderPath,
    depth: PixelDepth,
    report: &Value,
) -> Result<DepthResult, RuntimeFailure> {
    let selector_error = selector_error(report, path).unwrap_or(0);
    let suites = missing_suites(report);
    if !suites.is_empty() {
        return Err(RuntimeFailure {
            classification: Classification::MissingSuite,
            selector_error: Some(selector_error),
            missing_suites: suites,
        });
    }
    if selector_error != 0 {
        return Err(RuntimeFailure {
            classification: Classification::SelectorError,
            selector_error: Some(selector_error),
            missing_suites: Vec::new(),
        });
    }

    let width = report.get("width").and_then(Value::as_u64);
    let height = report.get("height").and_then(Value::as_u64);
    let output_sha256 = report.get("output_sha256").and_then(Value::as_str);
    let (Some(width), Some(height), Some(output_sha256)) = (width, height, output_sha256) else {
        return Err(RuntimeFailure::new(Classification::InvalidOutput));
    };
    if width == 0 || height == 0 || width > u32::MAX as u64 || height > u32::MAX as u64 {
        return Err(RuntimeFailure::new(Classification::InvalidOutput));
    }
    if output_sha256.len() != 64 || !output_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RuntimeFailure::new(Classification::InvalidOutput));
    }
    let bytes_per_pixel = match depth {
        PixelDepth::Argb8 => 4,
        PixelDepth::Argb16 => 8,
        PixelDepth::Argb32f => 16,
    };
    let row_bytes = report
        .get("row_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(width.saturating_mul(bytes_per_pixel));
    let extent = report.get("extent_hint");
    let rect_value = |name: &str, fallback: i64| {
        extent
            .and_then(|item| item.get(name))
            .and_then(Value::as_i64)
            .unwrap_or(fallback)
    };
    let premultiplication = parse_premultiplication(report);
    let input_width = report
        .get("input_width")
        .and_then(Value::as_u64)
        .unwrap_or(width);
    let input_height = report
        .get("input_height")
        .and_then(Value::as_u64)
        .unwrap_or(height);
    if input_width == 0
        || input_height == 0
        || input_width > u32::MAX as u64
        || input_height > u32::MAX as u64
    {
        return Err(RuntimeFailure::new(Classification::HostValidationError));
    }
    let input_world = WorldMetadata {
        width: input_width as u32,
        height: input_height as u32,
        row_bytes: input_width.saturating_mul(bytes_per_pixel),
        pixel_format: depth,
        premultiplication,
        extent_hint: Rect {
            left: 0,
            top: 0,
            right: input_width as i64,
            bottom: input_height as i64,
        },
    };
    Ok(DepthResult {
        depth,
        classification: Classification::Ok,
        selector: SelectorResult {
            render_path: path,
            completed: true,
            error_code: Some(0),
        },
        input_world: Some(input_world),
        world: Some(WorldMetadata {
            width: width as u32,
            height: height as u32,
            row_bytes,
            pixel_format: depth,
            premultiplication,
            extent_hint: Rect {
                left: rect_value("left", 0),
                top: rect_value("top", 0),
                right: rect_value("right", width as i64),
                bottom: rect_value("bottom", height as i64),
            },
        }),
        output_sha256: Some(output_sha256.to_ascii_lowercase()),
        missing_suites: Vec::new(),
        suite_timeline: suite_timeline(report),
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
        suite_timeline: Vec::new(),
    }
}

fn parse_premultiplication(report: &Value) -> Premultiplication {
    match report.get("premultiplication").and_then(Value::as_str) {
        Some("straight") => Premultiplication::Straight,
        Some("opaque") => Premultiplication::Opaque,
        _ => Premultiplication::Premultiplied,
    }
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

pub fn runtime_failure_from_io(error: &io::Error) -> RuntimeFailure {
    let message = error.to_string();
    let diagnostics = message.find('{').and_then(|start| {
        serde_json::Deserializer::from_str(&message[start..])
            .into_iter::<Value>()
            .next()
            .and_then(Result::ok)
    });
    let worker_classification = diagnostics
        .as_ref()
        .and_then(|value| value.get("classification"))
        .and_then(Value::as_str);
    let mut failure = RuntimeFailure::new(match worker_classification {
        Some("timeout_killed") => Classification::TimeoutKilled,
        Some("crashed") => Classification::Crashed,
        Some("nonzero_exit") => Classification::NonzeroExit,
        _ if message.contains("unsupported") || message.contains("unavailable") => {
            Classification::Unsupported
        }
        _ if message.contains("selector") || message.contains("rejected PF_") => {
            Classification::SelectorError
        }
        _ => Classification::NonzeroExit,
    });
    if let Some(value) = diagnostics {
        failure.selector_error = value.get("selector_error").and_then(Value::as_i64);
        failure.missing_suites = missing_suites(&value);
        if !failure.missing_suites.is_empty() {
            failure.classification = Classification::MissingSuite;
        }
    }
    failure
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
            .map_err(|error| runtime_failure_from_io(&error))?;
            self.parameters = parameters;
            Ok(diagnostics)
        }

        fn input_world(&self, depth: PixelDepth) -> Option<WorldMetadata> {
            let image = image::image_dimensions(self.input_path).ok()?;
            let bytes_per_pixel = match depth {
                PixelDepth::Argb8 => 4,
                PixelDepth::Argb16 => 8,
                PixelDepth::Argb32f => 16,
            };
            Some(WorldMetadata {
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
                .map_err(|error| runtime_failure_from_io(&error))?;
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
            .map_err(|error| runtime_failure_from_io(&error))
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
    use std::collections::VecDeque;

    struct FakeBackend {
        inspect: Result<Value, RuntimeFailure>,
        renders: VecDeque<Result<Value, RuntimeFailure>>,
    }

    impl RuntimeCollectorBackend for FakeBackend {
        fn inspect(&mut self) -> Result<Value, RuntimeFailure> {
            self.inspect.clone()
        }

        fn input_world(&self, depth: PixelDepth) -> Option<WorldMetadata> {
            Some(WorldMetadata {
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

    fn successful_report() -> Value {
        json!({
            "width": 2, "height": 3, "row_bytes": 16,
            "output_sha256": "ab".repeat(32), "render_error": 0,
            "extent_hint": {"left": 1, "top": 2, "right": 3, "bottom": 5}
        })
    }

    #[test]
    fn normalizes_success_and_world_metadata() {
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(successful_report())]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb16]);
        assert_eq!(result[0].classification, Classification::Ok);
        assert_eq!(result[0].selector.error_code, Some(0));
        assert_eq!(result[0].world.as_ref().unwrap().row_bytes, 16);
        assert_eq!(result[0].world.as_ref().unwrap().extent_hint.left, 1);
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
            },
            RuntimeFailure {
                classification: Classification::SelectorError,
                selector_error: Some(25),
                missing_suites: vec![],
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
                collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8]);
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
        );
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
        let failure = runtime_failure_from_io(&error);
        assert_eq!(failure.classification, Classification::MissingSuite);
        assert_eq!(failure.selector_error, Some(9));
        assert_eq!(failure.missing_suites.len(), 1);

        let error = io::Error::other(format!("worker failed: {diagnostics}, report=ignored"));
        let failure = runtime_failure_from_io(&error);
        assert_eq!(failure.classification, Classification::MissingSuite);
    }

    #[test]
    fn invalid_success_report_is_classified() {
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(json!({"width": 2, "height": 2}))]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Smartfx], &[PixelDepth::Argb32f]);
        assert_eq!(result[0].classification, Classification::InvalidOutput);
        assert!(!result[0].selector.completed);
    }

    #[test]
    fn invalid_output_digest_is_rejected() {
        let mut report = successful_report();
        report["output_sha256"] = json!("not-a-digest");
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(report)]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8]);
        assert_eq!(result[0].classification, Classification::InvalidOutput);
    }

    #[test]
    fn preserves_bounded_valid_suite_timeline() {
        let mut report = successful_report();
        report["suite_timeline"] = json!([
            {"sequence": 0, "action": "acquire", "name": "PF World Suite", "version": 2, "selector": "PF Render", "result": 0},
            {"sequence": 1, "action": "release", "name": "C:\\private", "version": 2, "selector": "PF Render", "result": 0}
        ]);
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([Ok(report)]),
        };
        let result =
            collect_runtime_results(&mut backend, &[RenderPath::Classic], &[PixelDepth::Argb8]);
        assert_eq!(result[0].suite_timeline.len(), 1);
        assert_eq!(result[0].suite_timeline[0].action, SuiteAction::Acquire);
    }

    #[test]
    fn falls_back_from_unsupported_smartfx_to_classic_once_per_depth() {
        let mut backend = FakeBackend {
            inspect: Ok(json!({})),
            renders: VecDeque::from([
                Err(RuntimeFailure::new(Classification::Unsupported)),
                Ok(successful_report()),
            ]),
        };
        let result = collect_runtime_results(
            &mut backend,
            &[RenderPath::Smartfx, RenderPath::Classic],
            &[PixelDepth::Argb8],
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].classification, Classification::Ok);
        assert_eq!(result[0].selector.render_path, RenderPath::Classic);
    }
}
