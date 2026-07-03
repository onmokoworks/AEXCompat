//! Contract-first AEX image probe broker.
//!
//! This broker validates JSON requests and writes machine-readable reports. It
//! deliberately does not load `.aex` files or start host applications. The
//! optional worker-launch path may start only an explicit local worker stub, and
//! that stub keeps `.aex` loading disabled.

use anyhow::{bail, Context};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ALLOWED_OPERATIONS: &[&str] = &["catalog", "describe", "render_png", "identity_transport"];
const ALLOWED_STATUSES: &[&str] = &[
    "ok",
    "catalog_ok",
    "allowlist_denied",
    "invalid_request",
    "unsupported_plugin_class",
    "unsupported_selector",
    "unsupported_suite",
    "timeout",
    "plugin_exception",
    "worker_crash",
    "worker_protocol_error",
    "internal_error",
];
const WORKER_LOG_PREVIEW_CHARS: usize = 2048;
const WORKER_LOG_TRUNCATED_MARKER: &str = "...[truncated]";
const REDACTED_LOCAL_PATH: &str = "<redacted-local-path>";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeRequest {
    pub schema_version: u32,
    pub operation: String,
    #[serde(default)]
    pub plugin_path: Option<String>,
    #[serde(default)]
    pub allowlist: Option<String>,
    #[serde(default)]
    pub input_png: Option<String>,
    #[serde(default)]
    pub output_png: Option<String>,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub pixel_format: Option<String>,
    #[serde(default)]
    pub frame: Option<FrameSpec>,
    #[serde(default)]
    pub limits: Option<ProbeLimits>,
    #[serde(default)]
    pub timeouts_ms: Option<ProbeTimeouts>,
    #[serde(default)]
    pub worker_exe: Option<String>,
    #[serde(default)]
    pub loader_preflight: Option<String>,
    #[serde(default)]
    pub loader_intent: Option<LoaderIntent>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameSpec {
    #[serde(default)]
    pub time_seconds: Option<f64>,
    #[serde(default)]
    pub frame_index: Option<u32>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeLimits {
    #[serde(default)]
    pub max_width: Option<u32>,
    #[serde(default)]
    pub max_height: Option<u32>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeTimeouts {
    #[serde(default)]
    pub launch: Option<u64>,
    #[serde(default)]
    pub setup: Option<u64>,
    #[serde(default)]
    pub render: Option<u64>,
    #[serde(default)]
    pub teardown: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoaderIntent {
    #[serde(default)]
    pub request_real_aex_load: bool,
    #[serde(default)]
    pub approval_status: Option<String>,
    #[serde(default)]
    pub sandbox_profile: Option<String>,
    #[serde(default)]
    pub worker_revalidation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeAllowlist {
    schema_version: u32,
    #[serde(default)]
    publication_status: Option<String>,
    #[serde(default)]
    allowlist_publication_status: Option<String>,
    #[serde(default)]
    default_max_plugin_bytes: Option<u64>,
    #[serde(default)]
    entries: Vec<AllowlistEntry>,
    #[serde(default)]
    blocked_classes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AllowlistEntry {
    id: String,
    plugin_path: String,
    expected_class: String,
    allowed_operations: Vec<String>,
    #[serde(default)]
    max_width: Option<u32>,
    #[serde(default)]
    max_height: Option<u32>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    fixture_status: Option<String>,
    #[serde(default)]
    publication_status: Option<String>,
    #[serde(default)]
    path_publication_status: Option<String>,
    #[serde(default)]
    binary_publication_status: Option<String>,
    #[serde(default)]
    license_status: Option<String>,
    #[serde(default)]
    max_plugin_bytes: Option<u64>,
    #[serde(default)]
    canonical_plugin_path: Option<String>,
    #[serde(default)]
    observed_size_bytes: Option<u64>,
    #[serde(default)]
    observed_modified_unix_ms: Option<u64>,
    #[serde(default)]
    classifier_status: Option<String>,
    #[serde(default)]
    classifier_inferred_class: Option<String>,
    #[serde(default)]
    loader_approval_status: Option<String>,
    #[serde(default)]
    sandbox_profile_status: Option<String>,
    #[serde(default)]
    worker_revalidation_status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeReport {
    pub schema_version: u32,
    pub status: String,
    pub plugin_path: Option<String>,
    pub plugin_class: String,
    pub entrypoint: Option<String>,
    pub selectors: Vec<Value>,
    pub stage: Option<String>,
    pub identity_preflight: Option<IdentityPreflightReport>,
    pub loader_approval: Option<LoaderApprovalReport>,
    pub worker_identity_revalidation: Option<WorkerIdentityRevalidationReport>,
    pub worker_loader_ticket: Option<WorkerLoaderTicketReport>,
    pub sandbox_preflight: Option<SandboxPreflightReport>,
    pub output_png: Option<String>,
    pub warnings: Vec<String>,
    pub unsupported: Vec<String>,
    pub crash: Option<Value>,
    pub elapsed_ms: u128,
}

type ProbeReportResult<T> = Result<T, Box<ProbeReport>>;

#[derive(Debug, Clone, Serialize)]
pub struct IdentityPreflightReport {
    pub canonical_plugin_path: Option<String>,
    pub exists: bool,
    pub is_file: bool,
    pub extension: Option<String>,
    pub observed_size_bytes: Option<u64>,
    pub observed_modified_unix_ms: Option<u64>,
    pub allowlist_id: String,
    pub status: String,
    pub denied_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoaderApprovalReport {
    pub sandbox_policy_version: u32,
    pub requested: bool,
    pub approved: bool,
    pub loader_enabled: bool,
    pub real_aex_load_enabled: bool,
    pub approval_status: String,
    pub allowlist_approval_status: String,
    pub sandbox_profile: String,
    pub sandbox_profile_status: String,
    pub worker_revalidation: String,
    pub worker_revalidation_status: String,
    pub job_object: bool,
    pub handle_inheritance_disabled: bool,
    pub environment_sanitized: bool,
    pub controlled_working_directory: bool,
    pub bounded_stdio: bool,
    pub worker_side_revalidation: bool,
    pub status: String,
    pub denied_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerIdentityRevalidationReport {
    pub performed: bool,
    pub status: String,
    pub canonical_plugin_path: Option<String>,
    pub allowlist_id: Option<String>,
    pub observed_size_bytes: Option<u64>,
    pub observed_modified_unix_ms: Option<u64>,
    pub classifier_status: Option<String>,
    pub publication_status: Option<String>,
    pub license_status: Option<String>,
    pub denied_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerLoaderTicketReport {
    pub performed: bool,
    pub status: String,
    pub allowlist_id: Option<String>,
    pub native_load_performed: bool,
    pub worker_may_load_plugin: bool,
    pub denied_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SandboxPreflightReport {
    pub performed: bool,
    pub profile: String,
    pub status: String,
    pub job_object_status: String,
    pub checks: Vec<SandboxCheckReport>,
    pub worker_attestation: Option<WorkerSandboxAttestationReport>,
    pub explicit_worker_path: bool,
    pub no_shell: bool,
    pub environment_sanitized: bool,
    pub bounded_stdio: bool,
    pub controlled_working_directory: bool,
    pub generated_root_confined: bool,
    pub handle_inheritance_disabled: bool,
    pub handle_inheritance_status: String,
    pub inheritance_sentinel_provided: bool,
    pub inheritance_sentinel_inherited: Option<bool>,
    pub job_object_attempted: bool,
    pub job_object_assigned: bool,
    pub kill_on_job_close: bool,
    pub network_required: bool,
    pub platform: String,
    pub platform_error_code: Option<i32>,
    pub platform_error_name: Option<String>,
    pub denied_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerSandboxAttestationReport {
    pub schema_version: u32,
    pub status: String,
    pub current_dir: Option<String>,
    pub current_dir_matches_broker: bool,
    pub generated_root_confined: bool,
    pub env_path_absent: bool,
    pub env_comspec_absent: bool,
    pub env_count: Option<u64>,
    pub stdin_contract: Option<String>,
    pub worker_exe_name: Option<String>,
    pub inheritance_sentinel_provided: bool,
    pub inheritance_sentinel_inherited: Option<bool>,
    pub handle_inheritance_disabled: bool,
    pub denied_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SandboxCheckReport {
    pub name: String,
    pub status: String,
    pub evidence: String,
    pub denied_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkerTransportManifest {
    schema_version: u32,
    transport_protocol_version: u32,
    pixel_format: String,
    raw_rgba_path: String,
    generated_root: String,
    width: u32,
    height: u32,
    row_stride_bytes: u64,
    decoded_bytes: u64,
}

#[derive(Debug, Serialize)]
struct WorkerIdentityManifest {
    schema_version: u32,
    identity_protocol_version: u32,
    generated_by: String,
    generated_unix_ms: u64,
    max_manifest_age_ms: u64,
    allowlist_id: String,
    operation: String,
    canonical_plugin_path: String,
    expected_extension: String,
    expected_class: String,
    fixture_status: String,
    publication_status: String,
    license_status: String,
    classifier_status: String,
    classifier_inferred_class: String,
    observed_size_bytes: u64,
    observed_modified_unix_ms: Option<u64>,
    max_plugin_bytes: u64,
    loader_approval_status: String,
    sandbox_profile: String,
    sandbox_profile_status: String,
    worker_revalidation_status: String,
    binary_evidence_mode: String,
}

#[derive(Debug, Serialize)]
struct WorkerLoaderTicketManifest {
    schema_version: u32,
    ticket_protocol_version: u32,
    generated_by: String,
    generated_unix_ms: u64,
    max_ticket_age_ms: u64,
    publication_status: String,
    status: String,
    native_load_performed: bool,
    worker_may_load_plugin: bool,
    broker_may_load_plugin: bool,
    allowlist_id: String,
    operation: String,
    selected_loader_entry: WorkerLoaderTicketEntry,
    required_runtime_evidence: WorkerLoaderTicketRuntimeEvidence,
    planned_stages: Vec<WorkerLoaderTicketStage>,
    denied_surfaces: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct WorkerLoaderTicketEntry {
    effect_id: String,
    normalized_plugin_path: String,
    path_match_status: String,
    allowlist_operation_status: String,
    entry_ready: bool,
}

#[derive(Debug, Serialize)]
struct WorkerLoaderTicketRuntimeEvidence {
    worker_identity_revalidation_required: String,
    worker_attestation_required: String,
    sandbox_preflight_required: String,
    job_object_required: String,
    handle_inheritance_required: String,
}

#[derive(Debug, Serialize)]
struct WorkerLoaderTicketStage {
    stage: String,
    status: String,
}

impl ProbeReport {
    fn new(status: &str, elapsed_ms: u128) -> Self {
        debug_assert!(ALLOWED_STATUSES.contains(&status));
        Self {
            schema_version: 1,
            status: status.to_owned(),
            plugin_path: None,
            plugin_class: "unknown".to_owned(),
            entrypoint: None,
            selectors: Vec::new(),
            stage: None,
            identity_preflight: None,
            loader_approval: None,
            worker_identity_revalidation: None,
            worker_loader_ticket: None,
            sandbox_preflight: None,
            output_png: None,
            warnings: Vec::new(),
            unsupported: Vec::new(),
            crash: None,
            elapsed_ms,
        }
    }

    fn invalid(message: impl Into<String>, elapsed_ms: u128) -> Self {
        let mut report = Self::new("invalid_request", elapsed_ms);
        report.warnings.push(message.into());
        report
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cli = BrokerCli::parse(&args)?;
    let request_text = std::fs::read_to_string(&cli.request)
        .with_context(|| format!("failed to read request {}", cli.request.display()))?;
    let report = run_probe_request_text(&request_text, Some(&cli.request))?;
    write_report(&cli.report, &report)?;
    Ok(())
}

#[derive(Debug)]
struct BrokerCli {
    request: PathBuf,
    report: PathBuf,
}

impl BrokerCli {
    fn parse(args: &[String]) -> anyhow::Result<Self> {
        let mut request = None;
        let mut report = None;
        let mut i = 1usize;
        while i < args.len() {
            match args[i].as_str() {
                "--request" => {
                    i += 1;
                    request = args.get(i).map(PathBuf::from);
                }
                "--report" => {
                    i += 1;
                    report = args.get(i).map(PathBuf::from);
                }
                "--help" | "-h" => {
                    print_usage_and_exit();
                }
                value => bail!("unknown argument {value}"),
            }
            i += 1;
        }
        let request = request.context("--request <path> is required")?;
        let report = report.context("--report <path> is required")?;
        Ok(Self { request, report })
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!("usage: aex_image_probe --request <request.json> --report <report.json>");
    std::process::exit(2);
}

pub fn run_probe_request_text(
    request_text: &str,
    request_path: Option<&Path>,
) -> anyhow::Result<ProbeReport> {
    let started = Instant::now();
    let request: ProbeRequest = match serde_json::from_str(request_text) {
        Ok(request) => request,
        Err(err) => {
            return Ok(ProbeReport::invalid(
                format!("request JSON should parse: {err}"),
                started.elapsed().as_millis(),
            ));
        }
    };
    Ok(run_probe_request(
        &request,
        request_path,
        started.elapsed().as_millis(),
    ))
}

pub fn run_probe_request(
    request: &ProbeRequest,
    request_path: Option<&Path>,
    elapsed_ms: u128,
) -> ProbeReport {
    if request.schema_version != 1 {
        return ProbeReport::invalid("schema_version must be 1", elapsed_ms);
    }
    if !ALLOWED_OPERATIONS.contains(&request.operation.as_str()) {
        return ProbeReport::invalid(
            format!("unsupported operation {}", request.operation),
            elapsed_ms,
        );
    }
    if !request.params.is_null() && !request.params.is_object() {
        return ProbeReport::invalid("params must be a JSON object", elapsed_ms);
    }
    if request.pixel_format.as_deref().unwrap_or("rgba8") != "rgba8" {
        return ProbeReport::invalid("pixel_format must be rgba8", elapsed_ms);
    }
    if let Some(timeouts) = &request.timeouts_ms {
        for (name, value) in [
            ("launch", timeouts.launch),
            ("setup", timeouts.setup),
            ("render", timeouts.render),
            ("teardown", timeouts.teardown),
        ] {
            if matches!(value, Some(0 | 30_001..)) {
                return ProbeReport::invalid(
                    format!("{name} timeout must be between 1 and 30000 ms"),
                    elapsed_ms,
                );
            }
        }
    }

    match request.operation.as_str() {
        "catalog" => catalog_report(elapsed_ms),
        "identity_transport" => identity_transport_report(request, elapsed_ms),
        "describe" | "render_png" => describe_or_render_report(request, request_path, elapsed_ms),
        _ => ProbeReport::invalid("unsupported operation", elapsed_ms),
    }
}

fn catalog_report(elapsed_ms: u128) -> ProbeReport {
    let mut report = ProbeReport::new("catalog_ok", elapsed_ms);
    report
        .warnings
        .push("catalog operation is metadata-only; no .aex loaded".to_owned());
    report
}

fn identity_transport_report(request: &ProbeRequest, elapsed_ms: u128) -> ProbeReport {
    let Some(input_png) = request.input_png.as_deref().filter(|path| !path.is_empty()) else {
        return ProbeReport::invalid("input_png is required for identity_transport", elapsed_ms);
    };
    let Some(output_png) = request
        .output_png
        .as_deref()
        .filter(|path| !path.is_empty())
    else {
        return ProbeReport::invalid("output_png is required for identity_transport", elapsed_ms);
    };

    let input_png = resolve_cwd_relative_path(input_png);
    if !input_png.is_file() {
        return ProbeReport::invalid(
            "input_png must be an existing file for identity_transport",
            elapsed_ms,
        );
    }
    let output_png = resolve_cwd_relative_path(output_png);
    if output_png.extension().and_then(|ext| ext.to_str()) != Some("png") {
        return ProbeReport::invalid("output_png must end with .png", elapsed_ms);
    }
    if !is_generated_output_path(&output_png) {
        return ProbeReport::invalid(
            "output_png must be under target/aex-image-probe generated root",
            elapsed_ms,
        );
    }
    if output_png.exists() {
        return ProbeReport::invalid("output_png must not already exist", elapsed_ms);
    }

    let reader = match image::ImageReader::open(&input_png) {
        Ok(reader) => reader,
        Err(err) => {
            return ProbeReport::invalid(format!("input_png should open: {err}"), elapsed_ms);
        }
    };
    let reader = match reader.with_guessed_format() {
        Ok(reader) => reader,
        Err(err) => {
            return ProbeReport::invalid(
                format!("input_png format should be detected: {err}"),
                elapsed_ms,
            );
        }
    };
    let decoded = match reader.decode() {
        Ok(decoded) => decoded,
        Err(err) => {
            return ProbeReport::invalid(format!("input_png should decode: {err}"), elapsed_ms);
        }
    };
    let rgba = decoded.to_rgba8();
    let (actual_width, actual_height) = rgba.dimensions();
    if let Some(frame) = &request.frame {
        if frame.width.is_some_and(|width| width != actual_width)
            || frame.height.is_some_and(|height| height != actual_height)
        {
            return ProbeReport::invalid(
                "input_png dimensions must match supplied frame width and height",
                elapsed_ms,
            );
        }
    }
    let limits = request.limits.as_ref();
    let max_width = limits.and_then(|limits| limits.max_width).unwrap_or(4096);
    let max_height = limits.and_then(|limits| limits.max_height).unwrap_or(4096);
    if actual_width > max_width || actual_height > max_height {
        return ProbeReport::invalid("identity_transport dimensions exceed limits", elapsed_ms);
    }
    let decoded_bytes = u64::from(actual_width)
        .saturating_mul(u64::from(actual_height))
        .saturating_mul(4);
    let max_bytes = limits
        .and_then(|limits| limits.max_bytes)
        .unwrap_or(64 * 1024 * 1024);
    if decoded_bytes > max_bytes {
        return ProbeReport::invalid(
            "identity_transport decoded bytes exceed max_bytes",
            elapsed_ms,
        );
    }

    if let Some(parent) = output_png.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            return ProbeReport::invalid(
                format!("generated output root should be created: {err}"),
                elapsed_ms,
            );
        }
    }
    let file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_png)
    {
        Ok(file) => file,
        Err(err) => {
            return ProbeReport::invalid(
                format!("identity_transport output should be created: {err}"),
                elapsed_ms,
            );
        }
    };
    let encoder = image::codecs::png::PngEncoder::new(file);
    if let Err(err) = encoder.write_image(
        rgba.as_raw(),
        actual_width,
        actual_height,
        image::ColorType::Rgba8.into(),
    ) {
        let _ = std::fs::remove_file(&output_png);
        return ProbeReport::invalid(
            format!("identity_transport output should be written: {err}"),
            elapsed_ms,
        );
    }

    let mut report = ProbeReport::new("ok", elapsed_ms);
    report.plugin_class = "identity-transport".to_owned();
    report.output_png = Some(output_png.to_string_lossy().replace('\\', "/"));
    report.warnings.push(
        "identity_transport decoded and re-encoded RGBA8 PNG only; no .aex loaded or rendered"
            .to_owned(),
    );
    report
        .unsupported
        .push("identity_transport is not AEX render correctness evidence".to_owned());
    report
}

fn describe_or_render_report(
    request: &ProbeRequest,
    request_path: Option<&Path>,
    elapsed_ms: u128,
) -> ProbeReport {
    let Some(plugin_path) = request.plugin_path.as_deref() else {
        return ProbeReport::invalid("plugin_path is required", elapsed_ms);
    };
    if !Path::new(plugin_path).is_absolute() {
        return ProbeReport::invalid("plugin_path must be absolute", elapsed_ms);
    }
    if !plugin_path.to_ascii_lowercase().ends_with(".aex") {
        return ProbeReport::invalid("plugin_path must end with .aex", elapsed_ms);
    }
    let Some(allowlist_path) = request.allowlist.as_deref() else {
        return ProbeReport::invalid("allowlist is required", elapsed_ms);
    };
    if request.operation == "render_png" {
        if request.input_png.as_deref().unwrap_or_default().is_empty() {
            return ProbeReport::invalid("input_png is required for render_png", elapsed_ms);
        }
        if request.output_png.as_deref().unwrap_or_default().is_empty() {
            return ProbeReport::invalid("output_png is required for render_png", elapsed_ms);
        }
        if let Some(frame) = &request.frame {
            if frame.width.unwrap_or(0) == 0 || frame.height.unwrap_or(0) == 0 {
                return ProbeReport::invalid(
                    "frame.width and frame.height are required for render_png",
                    elapsed_ms,
                );
            }
        } else {
            return ProbeReport::invalid("frame is required for render_png", elapsed_ms);
        }
    }

    let allowlist_path = resolve_request_relative_path(allowlist_path, request_path);
    let allowlist = match load_allowlist(&allowlist_path) {
        Ok(allowlist) => allowlist,
        Err(err) => {
            return ProbeReport::invalid(format!("allowlist should load: {err:#}"), elapsed_ms);
        }
    };
    if allowlist.schema_version != 1 {
        return ProbeReport::invalid("allowlist schema_version must be 1", elapsed_ms);
    }
    if allowlist
        .allowlist_publication_status
        .as_deref()
        .or(allowlist.publication_status.as_deref())
        .unwrap_or("unknown")
        == "unknown"
    {
        return ProbeReport::invalid(
            "allowlist publication_status unknown is fail-closed",
            elapsed_ms,
        );
    }
    let Some(entry) = allowlist
        .entries
        .iter()
        .find(|entry| same_path_text(&entry.plugin_path, plugin_path))
    else {
        let mut report = ProbeReport::new("allowlist_denied", elapsed_ms);
        report.plugin_path = Some(plugin_path.to_owned());
        report
            .warnings
            .push("plugin_path is not present in allowlist".to_owned());
        return report;
    };

    if !entry
        .allowed_operations
        .iter()
        .any(|operation| operation == &request.operation)
    {
        let mut report = ProbeReport::new("allowlist_denied", elapsed_ms);
        report.plugin_path = Some(plugin_path.to_owned());
        report.plugin_class = entry.expected_class.clone();
        report
            .warnings
            .push("operation is not allowed for plugin_path".to_owned());
        return report;
    }
    if !matches!(entry.expected_class.as_str(), "classic-effect") {
        let mut report = ProbeReport::new("unsupported_plugin_class", elapsed_ms);
        report.plugin_path = Some(plugin_path.to_owned());
        report.plugin_class = entry.expected_class.clone();
        report
            .unsupported
            .push(format!("unsupported plugin class {}", entry.expected_class));
        return report;
    }
    if let Some(fixture_status) = entry.fixture_status.as_deref() {
        if !matches!(
            fixture_status,
            "local-build-candidate" | "possible-local-build-candidate"
        ) {
            let mut report = ProbeReport::new("unsupported_plugin_class", elapsed_ms);
            report.plugin_path = Some(plugin_path.to_owned());
            report.plugin_class = entry.expected_class.clone();
            report
                .unsupported
                .push(format!("unsupported fixture status {fixture_status}"));
            return report;
        }
    }
    if allowlist
        .blocked_classes
        .iter()
        .any(|class| class == &entry.expected_class)
    {
        let mut report = ProbeReport::new("unsupported_plugin_class", elapsed_ms);
        report.plugin_path = Some(plugin_path.to_owned());
        report.plugin_class = entry.expected_class.clone();
        report
            .unsupported
            .push(format!("blocked plugin class {}", entry.expected_class));
        return report;
    }
    if request.operation == "render_png" {
        if let Some(limits) = &request.limits {
            if let Some(frame) = &request.frame {
                let max_width = limits.max_width.unwrap_or(4096);
                let max_height = limits.max_height.unwrap_or(4096);
                if frame.width.unwrap_or(0) > max_width || frame.height.unwrap_or(0) > max_height {
                    return ProbeReport::invalid("frame exceeds request limits", elapsed_ms);
                }
                if let Some(max_bytes) = limits.max_bytes {
                    let required_bytes = u64::from(frame.width.unwrap_or(0))
                        .saturating_mul(u64::from(frame.height.unwrap_or(0)))
                        .saturating_mul(4);
                    if required_bytes > max_bytes {
                        return ProbeReport::invalid("frame exceeds request max_bytes", elapsed_ms);
                    }
                }
            }
        }
        if let Some(max_width) = entry.max_width {
            if request
                .frame
                .as_ref()
                .and_then(|frame| frame.width)
                .unwrap_or(0)
                > max_width
            {
                return ProbeReport::invalid("frame exceeds allowlist max_width", elapsed_ms);
            }
        }
        if let Some(max_height) = entry.max_height {
            if request
                .frame
                .as_ref()
                .and_then(|frame| frame.height)
                .unwrap_or(0)
                > max_height
            {
                return ProbeReport::invalid("frame exceeds allowlist max_height", elapsed_ms);
            }
        }
    }
    if let Err(report) =
        validate_loader_intent_metadata(request, request_path, plugin_path, entry, elapsed_ms)
    {
        return *report;
    }

    worker_protocol_stub_report(
        request,
        request_path,
        plugin_path,
        entry,
        allowlist.default_max_plugin_bytes,
        elapsed_ms,
    )
}

fn validate_loader_intent_metadata(
    request: &ProbeRequest,
    request_path: Option<&Path>,
    plugin_path: &str,
    entry: &AllowlistEntry,
    elapsed_ms: u128,
) -> ProbeReportResult<()> {
    let Some(intent) = request.loader_intent.as_ref() else {
        return Ok(());
    };
    if !intent.request_real_aex_load {
        return Ok(());
    }

    let denied_reason = if intent.approval_status.as_deref() != Some("approved-local-only") {
        Some("loader approval is missing or not approved-local-only".to_owned())
    } else if entry.loader_approval_status.as_deref() != Some("approved-local-only") {
        Some("allowlist loader approval is missing or not approved-local-only".to_owned())
    } else if intent.sandbox_profile.as_deref() != Some("windows-job-object-v0") {
        Some("sandbox profile is missing or not windows-job-object-v0".to_owned())
    } else if entry.sandbox_profile_status.as_deref() != Some("implemented-v0") {
        Some("allowlist sandbox profile is missing or not implemented-v0".to_owned())
    } else if intent.worker_revalidation.as_deref() != Some("required") {
        Some("worker revalidation request is missing or not required".to_owned())
    } else if entry.worker_revalidation_status.as_deref() != Some("required") {
        Some("allowlist worker revalidation is missing or not required".to_owned())
    } else {
        validate_loader_preflight_evidence(request, request_path, plugin_path, entry).err()
    };

    let Some(denied_reason) = denied_reason else {
        return Ok(());
    };

    let mut report = worker_error_report(
        "worker_protocol_error",
        plugin_path,
        entry,
        elapsed_ms,
        "handshake",
    );
    report.loader_approval = Some(loader_approval_report(
        request,
        entry,
        Some(denied_reason.clone()),
        false,
        None,
    ));
    report
        .unsupported
        .push("native AEX loading is gated off in this build".to_owned());
    report.warnings.push(denied_reason.to_owned());
    Err(Box::new(report))
}

fn validate_loader_preflight_evidence(
    request: &ProbeRequest,
    request_path: Option<&Path>,
    plugin_path: &str,
    entry: &AllowlistEntry,
) -> Result<(), String> {
    let Some(preflight_path) = request.loader_preflight.as_deref() else {
        return Err("loader_preflight evidence is required before worker revalidation".to_owned());
    };
    let preflight_path = resolve_request_relative_path(preflight_path, request_path);
    let text = std::fs::read_to_string(&preflight_path)
        .map_err(|err| format!("loader_preflight evidence should load: {err}"))?;
    if text.len() > 256 * 1024 {
        return Err("loader_preflight evidence exceeds 256 KiB".to_owned());
    }
    let lowered = text.to_ascii_lowercase();
    for token in forbidden_loader_preflight_tokens() {
        if lowered.contains(&token) {
            return Err(format!(
                "loader_preflight evidence contains forbidden token {token}"
            ));
        }
    }
    let evidence: Value = serde_json::from_str(&text)
        .map_err(|err| format!("loader_preflight evidence should parse: {err}"))?;
    if evidence["schema_version"].as_u64() != Some(1) {
        return Err("loader_preflight schema_version must be 1".to_owned());
    }
    if evidence["publication_status"].as_str() != Some("local-only") {
        return Err("loader_preflight publication_status must be local-only".to_owned());
    }
    if evidence["status"].as_str() != Some("preflight_passed_no_load") {
        return Err("loader_preflight status must be preflight_passed_no_load".to_owned());
    }
    if evidence["preflight_passed"].as_bool() != Some(true) {
        return Err("loader_preflight preflight_passed must be true".to_owned());
    }
    if evidence["native_load_performed"].as_bool() != Some(false) {
        return Err("loader_preflight native_load_performed must be false".to_owned());
    }
    if evidence["broker_may_load_plugin"].as_bool() != Some(false) {
        return Err("loader_preflight broker_may_load_plugin must be false".to_owned());
    }
    let selected_candidate = &evidence["selected_candidate"];
    if selected_candidate.is_null() {
        return Err("loader_preflight selected_candidate is required".to_owned());
    }
    let selected_candidate_id = selected_candidate["id"]
        .as_str()
        .ok_or_else(|| "loader_preflight selected_candidate.id is required".to_owned())?;
    if selected_candidate_id.trim().is_empty() {
        return Err("loader_preflight selected_candidate.id must not be empty".to_owned());
    }
    let selected_fixture = evidence["selected_fixture"]
        .as_str()
        .ok_or_else(|| "loader_preflight selected_fixture is required".to_owned())?;
    if selected_fixture != selected_candidate_id {
        return Err(
            "loader_preflight selected_fixture does not match selected_candidate.id".to_owned(),
        );
    }
    if selected_candidate["loader_gate_effect_id"].as_str() != Some(entry.id.as_str()) {
        return Err("loader_preflight selected_candidate does not match allowlist id".to_owned());
    }
    let selected_path = selected_candidate["plugin_path"]
        .as_str()
        .ok_or_else(|| "loader_preflight selected_candidate.plugin_path is required".to_owned())?;
    if !same_path_text(selected_path, plugin_path) {
        return Err(
            "loader_preflight selected_candidate.plugin_path does not match request plugin_path"
                .to_owned(),
        );
    }
    if !same_path_text(selected_path, &entry.plugin_path) {
        return Err(
            "loader_preflight selected_candidate.plugin_path does not match allowlist plugin_path"
                .to_owned(),
        );
    }
    if let Some(loader_gate_path_value) = selected_candidate.get("loader_gate_plugin_path") {
        if !loader_gate_path_value.is_null() {
            let loader_gate_path = loader_gate_path_value.as_str().ok_or_else(|| {
                "loader_preflight selected_candidate.loader_gate_plugin_path must be a string"
                    .to_owned()
            })?;
            if loader_gate_path.trim().is_empty() {
                return Err(
                    "loader_preflight selected_candidate.loader_gate_plugin_path must not be empty"
                        .to_owned(),
                );
            }
            if !same_path_text(loader_gate_path, plugin_path) {
                return Err(
                    "loader_preflight selected_candidate.loader_gate_plugin_path does not match request plugin_path"
                        .to_owned(),
                );
            }
            if !same_path_text(loader_gate_path, &entry.plugin_path) {
                return Err(
                    "loader_preflight selected_candidate.loader_gate_plugin_path does not match allowlist plugin_path"
                        .to_owned(),
                );
            }
            if !same_path_text(loader_gate_path, selected_path) {
                return Err(
                    "loader_preflight selected_candidate.loader_gate_plugin_path does not match selected_candidate.plugin_path"
                        .to_owned(),
                );
            }
        }
    }
    let selected_loader_entry = &evidence["selected_loader_entry"];
    if selected_loader_entry.is_null() {
        return Err("loader_preflight selected_loader_entry is required".to_owned());
    }
    if selected_loader_entry["effect_id"].as_str() != Some(entry.id.as_str()) {
        return Err(
            "loader_preflight selected_loader_entry.effect_id does not match allowlist id"
                .to_owned(),
        );
    }
    let selected_loader_entry_path =
        selected_loader_entry["plugin_path"]
            .as_str()
            .ok_or_else(|| {
                "loader_preflight selected_loader_entry.plugin_path is required".to_owned()
            })?;
    if !same_path_text(selected_loader_entry_path, plugin_path) {
        return Err(
            "loader_preflight selected_loader_entry.plugin_path does not match request plugin_path"
                .to_owned(),
        );
    }
    if !same_path_text(selected_loader_entry_path, &entry.plugin_path) {
        return Err(
            "loader_preflight selected_loader_entry.plugin_path does not match allowlist plugin_path"
                .to_owned(),
        );
    }
    if !same_path_text(selected_loader_entry_path, selected_path) {
        return Err(
            "loader_preflight selected_loader_entry.plugin_path does not match selected_candidate.plugin_path"
                .to_owned(),
        );
    }
    let selected_loader_entry_normalized = selected_loader_entry["normalized_plugin_path"]
        .as_str()
        .ok_or_else(|| {
            "loader_preflight selected_loader_entry.normalized_plugin_path is required".to_owned()
        })?;
    let selected_loader_entry_key = loader_preflight_path_key(selected_loader_entry_path);
    let request_plugin_key = loader_preflight_path_key(plugin_path);
    let allowlist_plugin_key = loader_preflight_path_key(&entry.plugin_path);
    if selected_loader_entry_normalized != selected_loader_entry_key.as_str()
        || selected_loader_entry_normalized != request_plugin_key.as_str()
        || selected_loader_entry_normalized != allowlist_plugin_key.as_str()
    {
        return Err(
            "loader_preflight selected_loader_entry.normalized_plugin_path is inconsistent"
                .to_owned(),
        );
    }
    if selected_loader_entry["path_match_status"].as_str() != Some("matched_normalized_path") {
        return Err(
            "loader_preflight selected_loader_entry.path_match_status must be matched_normalized_path"
                .to_owned(),
        );
    }
    for (field, expected) in [
        ("pre_loader_status", "approved-local-only"),
        ("loader_approval_status", "approved-local-only"),
        ("allowlist_operation_status", "render_png"),
        (
            "handle_inheritance_required",
            "sentinel_not_inherited-with-explicit-handle-list",
        ),
        ("worker_identity_revalidation_required", "passed"),
        ("worker_attestation_required", "passed"),
        ("sandbox_preflight_required", "passed"),
        ("job_object_required", "assigned-with-kill-on-close"),
    ] {
        if selected_loader_entry[field].as_str() != Some(expected) {
            return Err(format!(
                "loader_preflight selected_loader_entry.{field} must be {expected}"
            ));
        }
    }
    if selected_loader_entry["entry_ready"].as_bool() != Some(true) {
        return Err("loader_preflight selected_loader_entry.entry_ready must be true".to_owned());
    }
    let loader_gate = &evidence["loader_gate"];
    let fixture_gate = &evidence["fixture_gate"];
    if fixture_gate["selected_fixture"].as_str() != Some(selected_candidate_id) {
        return Err(
            "loader_preflight fixture_gate.selected_fixture does not match selected_candidate.id"
                .to_owned(),
        );
    }
    if fixture_gate["approval_approved"].as_bool() != Some(true)
        || fixture_gate["approval_loader_enabled"].as_bool() != Some(true)
        || fixture_gate["approval_real_aex_load_enabled"].as_bool() != Some(true)
        || fixture_gate["approval_render_png_enabled"].as_bool() != Some(true)
    {
        return Err("loader_preflight fixture_gate approval is not open".to_owned());
    }
    if fixture_gate["candidate_count"].as_u64().unwrap_or(0) == 0 {
        return Err("loader_preflight fixture_gate candidate_count must be nonzero".to_owned());
    }
    if loader_gate["approved"].as_bool() != Some(true)
        || loader_gate["loader_enabled"].as_bool() != Some(true)
        || loader_gate["real_aex_load_enabled"].as_bool() != Some(true)
        || loader_gate["open_candidate_count"].as_u64() != Some(1)
    {
        return Err(
            "loader_preflight loader_gate is not open for exactly one candidate".to_owned(),
        );
    }
    if loader_gate["entry_count"].as_u64().unwrap_or(0) == 0 {
        return Err("loader_preflight loader_gate entry_count must be nonzero".to_owned());
    }
    for name in [
        "fixture_gate_schema_version",
        "loader_gate_schema_version",
        "fixture_gate_unique_candidates",
        "loader_gate_unique_entries",
        "selected_fixture",
        "selected_fixture_metadata",
        "fixture_gate_approval",
        "loader_gate_single_ready_entry",
        "selected_fixture_in_loader_gate",
        "loader_gate_open",
        "selected_loader_entry",
    ] {
        if !loader_preflight_check_passed(&evidence, name) {
            return Err(format!(
                "loader_preflight required check {name} did not pass"
            ));
        }
    }
    Ok(())
}

fn loader_preflight_check_passed(evidence: &Value, name: &str) -> bool {
    evidence["checks"]
        .as_array()
        .map(|checks| {
            checks.iter().any(|check| {
                check["name"].as_str() == Some(name) && check["status"].as_str() == Some("passed")
            })
        })
        .unwrap_or(false)
}

fn forbidden_loader_preflight_tokens() -> Vec<String> {
    vec![
        "sha256".to_owned(),
        "base64".to_owned(),
        ["load", "library"].concat(),
        ["lib", "loading"].concat(),
        ["effect", "main"].concat(),
        "output_png".to_owned(),
        "rendered_pixels".to_owned(),
    ]
}

fn loader_approval_report(
    request: &ProbeRequest,
    entry: &AllowlistEntry,
    denied_reason: Option<String>,
    worker_side_revalidation: bool,
    sandbox_preflight: Option<&SandboxPreflightReport>,
) -> LoaderApprovalReport {
    let intent = request.loader_intent.as_ref();
    LoaderApprovalReport {
        sandbox_policy_version: 0,
        requested: intent
            .map(|intent| intent.request_real_aex_load)
            .unwrap_or(false),
        approved: false,
        loader_enabled: false,
        real_aex_load_enabled: false,
        approval_status: intent
            .and_then(|intent| intent.approval_status.clone())
            .unwrap_or_else(|| "missing".to_owned()),
        allowlist_approval_status: entry
            .loader_approval_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        sandbox_profile: intent
            .and_then(|intent| intent.sandbox_profile.clone())
            .unwrap_or_else(|| "missing".to_owned()),
        sandbox_profile_status: entry
            .sandbox_profile_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        worker_revalidation: intent
            .and_then(|intent| intent.worker_revalidation.clone())
            .unwrap_or_else(|| "missing".to_owned()),
        worker_revalidation_status: entry
            .worker_revalidation_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        job_object: sandbox_preflight
            .map(|preflight| preflight.job_object_assigned)
            .unwrap_or(false),
        handle_inheritance_disabled: sandbox_preflight
            .map(|preflight| preflight.handle_inheritance_disabled)
            .unwrap_or(false),
        environment_sanitized: sandbox_preflight
            .map(|preflight| preflight.environment_sanitized)
            .unwrap_or(true),
        controlled_working_directory: sandbox_preflight
            .map(|preflight| preflight.controlled_working_directory)
            .unwrap_or(false),
        bounded_stdio: sandbox_preflight
            .map(|preflight| preflight.bounded_stdio)
            .unwrap_or(true),
        worker_side_revalidation,
        status: "denied".to_owned(),
        denied_reason,
    }
}

fn worker_identity_revalidation_report(
    handshake: &Value,
    identity: &IdentityPreflightReport,
    entry: &AllowlistEntry,
) -> WorkerIdentityRevalidationReport {
    let status = handshake["worker_revalidation"]["status"]
        .as_str()
        .unwrap_or("not_run");
    let performed = status != "not_run";
    let publication_status = entry
        .binary_publication_status
        .as_deref()
        .or(entry.publication_status.as_deref())
        .or(entry.path_publication_status.as_deref())
        .map(str::to_owned);
    WorkerIdentityRevalidationReport {
        performed,
        status: status.to_owned(),
        canonical_plugin_path: identity.canonical_plugin_path.clone(),
        allowlist_id: handshake["worker_revalidation"]["allowlist_id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| Some(entry.id.clone()).filter(|_| performed)),
        observed_size_bytes: identity.observed_size_bytes.filter(|_| performed),
        observed_modified_unix_ms: identity.observed_modified_unix_ms.filter(|_| performed),
        classifier_status: entry.classifier_status.clone().filter(|_| performed),
        publication_status: publication_status.filter(|_| performed),
        license_status: entry.license_status.clone().filter(|_| performed),
        denied_reason: handshake["worker_revalidation"]["denied_reason"]
            .as_str()
            .map(str::to_owned),
    }
}

fn worker_loader_ticket_report(handshake: &Value) -> Option<WorkerLoaderTicketReport> {
    let ticket = handshake.get("loader_ticket")?;
    let status = ticket["status"].as_str().unwrap_or("missing").to_owned();
    let native_load_performed = ticket["native_load_performed"].as_bool().unwrap_or(true);
    let worker_may_load_plugin = ticket["worker_may_load_plugin"].as_bool().unwrap_or(true);
    let denied_reason =
        if status == "accepted_no_load" && !native_load_performed && !worker_may_load_plugin {
            None
        } else {
            ticket["denied_reason"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| Some("worker loader ticket was not accepted as no-load".to_owned()))
        };
    Some(WorkerLoaderTicketReport {
        performed: true,
        status,
        allowlist_id: ticket["allowlist_id"].as_str().map(str::to_owned),
        native_load_performed,
        worker_may_load_plugin,
        denied_reason,
    })
}

impl SandboxPreflightReport {
    fn new(profile: &str, generated_workdir: Option<&Path>) -> Self {
        let generated_root_confined = generated_workdir
            .map(|dir| is_generated_output_path(&dir.join("sandbox-cwd-placeholder.png")))
            .unwrap_or(false);
        Self {
            performed: true,
            profile: profile.to_owned(),
            status: "not_run".to_owned(),
            job_object_status: "not_attempted".to_owned(),
            checks: Vec::new(),
            worker_attestation: None,
            explicit_worker_path: true,
            no_shell: true,
            environment_sanitized: true,
            bounded_stdio: true,
            controlled_working_directory: generated_workdir.is_some(),
            generated_root_confined,
            handle_inheritance_disabled: false,
            handle_inheritance_status: "not_measured".to_owned(),
            inheritance_sentinel_provided: false,
            inheritance_sentinel_inherited: None,
            job_object_attempted: false,
            job_object_assigned: false,
            kill_on_job_close: false,
            network_required: false,
            platform: std::env::consts::OS.to_owned(),
            platform_error_code: None,
            platform_error_name: None,
            denied_reason: None,
        }
    }

    fn apply_worker_attestation(
        &mut self,
        handshake: &Value,
        expected_workdir: Option<&Path>,
        expected_inheritance_sentinel: bool,
    ) -> Result<(), String> {
        let attestation = handshake
            .get("sandbox_attestation")
            .ok_or_else(|| "worker sandbox_attestation is missing".to_owned())?;
        let schema_version = attestation["schema_version"]
            .as_u64()
            .ok_or_else(|| "worker sandbox_attestation.schema_version is missing".to_owned())?;
        if schema_version != 1 {
            return Err("worker sandbox_attestation schema_version is unsupported".to_owned());
        }

        let current_dir = attestation["current_dir"].as_str().map(str::to_owned);
        let current_dir_matches_broker = match (current_dir.as_deref(), expected_workdir) {
            (Some(observed), Some(expected)) => {
                same_path_text(observed, &expected.to_string_lossy())
            }
            _ => false,
        };
        let observed_generated_root = current_dir
            .as_deref()
            .map(|dir| is_generated_output_path(Path::new(dir)))
            .unwrap_or(false);
        let worker_generated_root = attestation["generated_root_confined"]
            .as_bool()
            .unwrap_or(false);
        let generated_root_confined = observed_generated_root && worker_generated_root;
        let env_path_absent = attestation["env_path_absent"].as_bool().unwrap_or(false);
        let env_comspec_absent = attestation["env_comspec_absent"].as_bool().unwrap_or(false);
        let stdin_contract = attestation["stdin_contract"].as_str().map(str::to_owned);
        let stdin_is_null = stdin_contract.as_deref() == Some("null");
        let inheritance_sentinel_provided = attestation["inheritance_sentinel_provided"]
            .as_bool()
            .unwrap_or(false);
        let inheritance_sentinel_inherited =
            attestation["inheritance_sentinel_inherited"].as_bool();
        let handle_inheritance_disabled = attestation["handle_inheritance_disabled"]
            .as_bool()
            .unwrap_or(false);
        let handle_inheritance_passed = if expected_inheritance_sentinel {
            inheritance_sentinel_provided && handle_inheritance_disabled
        } else {
            true
        };
        let passed = current_dir_matches_broker
            && generated_root_confined
            && env_path_absent
            && env_comspec_absent
            && stdin_is_null
            && handle_inheritance_passed;

        self.environment_sanitized =
            self.environment_sanitized && env_path_absent && env_comspec_absent;
        self.controlled_working_directory =
            self.controlled_working_directory && current_dir_matches_broker;
        self.generated_root_confined = self.generated_root_confined && generated_root_confined;
        self.bounded_stdio = self.bounded_stdio && stdin_is_null;
        if expected_inheritance_sentinel {
            self.inheritance_sentinel_provided = inheritance_sentinel_provided;
            self.inheritance_sentinel_inherited = inheritance_sentinel_inherited;
            self.handle_inheritance_disabled = handle_inheritance_disabled;
            self.handle_inheritance_status = if handle_inheritance_disabled {
                "sentinel_not_inherited".to_owned()
            } else if inheritance_sentinel_inherited == Some(true) {
                "sentinel_inherited".to_owned()
            } else if inheritance_sentinel_provided {
                "sentinel_unverified".to_owned()
            } else {
                "sentinel_missing".to_owned()
            };
            if !handle_inheritance_disabled {
                self.status = "failed".to_owned();
                if self.denied_reason.is_none() {
                    self.denied_reason =
                        Some("inheritance sentinel was visible or unverified in worker".to_owned());
                }
            }
        }

        let denied_reason = if passed {
            None
        } else {
            Some("worker-observed sandbox attestation did not match broker policy".to_owned())
        };
        self.worker_attestation = Some(WorkerSandboxAttestationReport {
            schema_version: schema_version as u32,
            status: if passed { "passed" } else { "failed" }.to_owned(),
            current_dir,
            current_dir_matches_broker,
            generated_root_confined,
            env_path_absent,
            env_comspec_absent,
            env_count: attestation["env_count"].as_u64(),
            stdin_contract,
            worker_exe_name: attestation["worker_exe_name"].as_str().map(str::to_owned),
            inheritance_sentinel_provided,
            inheritance_sentinel_inherited,
            handle_inheritance_disabled,
            denied_reason,
        });
        Ok(())
    }

    fn record_handle_inheritance_probe_created(&mut self) {
        self.handle_inheritance_status = "sentinel_created".to_owned();
    }

    fn record_handle_inheritance_probe_unavailable(&mut self) {
        self.handle_inheritance_status = "not_applicable".to_owned();
    }

    fn record_handle_inheritance_probe_error(&mut self, err: SandboxPlatformError) {
        self.handle_inheritance_status = "sentinel_create_failed".to_owned();
        self.status = "failed".to_owned();
        self.platform_error_code = err.code;
        self.platform_error_name = err.name.map(str::to_owned);
        self.denied_reason = Some(format!(
            "inheritance sentinel could not be created: {}",
            err.message
        ));
    }

    fn record_job_created(&mut self) {
        self.job_object_attempted = true;
        self.kill_on_job_close = true;
        self.job_object_status = "created".to_owned();
    }

    fn record_job_assigned(&mut self) {
        self.job_object_attempted = true;
        self.job_object_assigned = true;
        self.kill_on_job_close = true;
        if self.denied_reason.is_none() {
            self.status = "passed".to_owned();
        }
        self.job_object_status = "assigned".to_owned();
    }

    fn record_job_error(&mut self, status: &str, reason: &str, err: Option<SandboxPlatformError>) {
        self.job_object_attempted = status != "unsupported_platform";
        self.status = "failed".to_owned();
        self.job_object_status = status.to_owned();
        self.denied_reason = Some(reason.to_owned());
        if let Some(err) = err {
            self.platform_error_code = err.code;
            self.platform_error_name = err.name.map(str::to_owned);
            self.denied_reason = Some(format!("{reason}: {}", err.message));
        }
    }

    fn finalize(&mut self) {
        if self.status == "not_run" {
            self.status = if self.job_object_assigned {
                "passed".to_owned()
            } else {
                if self.denied_reason.is_none() {
                    self.denied_reason =
                        Some("worker job object attachment was not confirmed".to_owned());
                }
                "failed".to_owned()
            };
        }

        let mut checks = Vec::new();
        push_bool_check(
            &mut checks,
            "explicit_worker_path",
            self.explicit_worker_path,
            "absolute allowlisted worker_exe filename was used",
        );
        push_bool_check(
            &mut checks,
            "no_shell",
            self.no_shell,
            "worker was launched directly with std::process::Command",
        );
        let worker_attestation_passed = self
            .worker_attestation
            .as_ref()
            .map(|attestation| attestation.status == "passed")
            .unwrap_or(false);
        let environment_evidence = if worker_attestation_passed {
            "broker used env_clear and worker attested ambient command environment absence"
        } else {
            "broker used env_clear; child-observed environment is reported by worker_sandbox_attestation"
        };
        let cwd_evidence = if worker_attestation_passed {
            "worker-attested current_dir matches generated target/aex-image-probe folder"
        } else {
            "broker set generated target/aex-image-probe current_dir; child-observed cwd is reported by worker_sandbox_attestation"
        };
        push_bool_check(
            &mut checks,
            "environment_sanitized",
            self.environment_sanitized,
            environment_evidence,
        );
        push_bool_check(
            &mut checks,
            "bounded_stdio",
            self.bounded_stdio,
            "stdin is null and stdout/stderr are captured for bounded report previews",
        );
        push_bool_check(
            &mut checks,
            "controlled_working_directory",
            self.controlled_working_directory && self.generated_root_confined,
            cwd_evidence,
        );
        let (attestation_status, attestation_evidence, attestation_reason) =
            match self.worker_attestation.as_ref() {
                Some(attestation) if attestation.status == "passed" => (
                    "measured_pass",
                    format!(
                        "current_dir={}, env_count={}",
                        attestation.current_dir.as_deref().unwrap_or("<missing>"),
                        attestation
                            .env_count
                            .map(|count| count.to_string())
                            .unwrap_or_else(|| "<missing>".to_owned())
                    ),
                    None,
                ),
                Some(attestation) => (
                    "measured_fail",
                    format!(
                        "current_dir_matches_broker={}, generated_root_confined={}, env_path_absent={}, env_comspec_absent={}, stdin_contract={}",
                        attestation.current_dir_matches_broker,
                        attestation.generated_root_confined,
                        attestation.env_path_absent,
                        attestation.env_comspec_absent,
                        attestation.stdin_contract.as_deref().unwrap_or("<missing>")
                    ),
                    attestation.denied_reason.clone(),
                ),
                None => (
                    "not_measured",
                    "worker did not return sandbox_attestation".to_owned(),
                    None,
                ),
            };
        checks.push(SandboxCheckReport {
            name: "worker_sandbox_attestation".to_owned(),
            status: attestation_status.to_owned(),
            evidence: attestation_evidence,
            denied_reason: attestation_reason,
        });
        let handle_status = match self.handle_inheritance_status.as_str() {
            "sentinel_not_inherited" => "measured_pass",
            "not_applicable" => "not_applicable",
            "not_measured" | "sentinel_created" => "not_measured",
            _ => "measured_fail",
        };
        let handle_evidence = match self.handle_inheritance_status.as_str() {
            "sentinel_not_inherited" => "inheritable sentinel handle was not visible in the worker",
            "sentinel_inherited" => "inheritable sentinel handle was visible in the worker",
            "sentinel_missing" => "worker did not report the inheritance sentinel",
            "sentinel_unverified" => "worker could not verify the inheritance sentinel handle",
            "sentinel_create_failed" => "broker could not create inheritance sentinel handle",
            "not_applicable" => "handle inheritance sentinel is Windows-only",
            _ => "inheritance sentinel was not measured before worker attestation",
        };
        checks.push(SandboxCheckReport {
            name: "handle_inheritance".to_owned(),
            status: handle_status.to_owned(),
            evidence: handle_evidence.to_owned(),
            denied_reason: if handle_status == "measured_fail" {
                self.denied_reason.clone()
            } else {
                None
            },
        });
        let job_status = if self.job_object_assigned && self.kill_on_job_close {
            "measured_pass"
        } else if self.job_object_status == "unsupported_platform" {
            "not_applicable"
        } else if self.job_object_attempted {
            "measured_fail"
        } else {
            "not_measured"
        };
        checks.push(SandboxCheckReport {
            name: "job_object_kill_on_close".to_owned(),
            status: job_status.to_owned(),
            evidence: self.job_object_status.clone(),
            denied_reason: self.denied_reason.clone(),
        });
        checks.push(SandboxCheckReport {
            name: "network_required".to_owned(),
            status: if self.network_required {
                "measured_fail"
            } else {
                "not_applicable"
            }
            .to_owned(),
            evidence: "worker handshake path does not require network access".to_owned(),
            denied_reason: None,
        });
        self.checks = checks;
    }
}

fn push_bool_check(checks: &mut Vec<SandboxCheckReport>, name: &str, passed: bool, evidence: &str) {
    checks.push(SandboxCheckReport {
        name: name.to_owned(),
        status: if passed {
            "measured_pass"
        } else {
            "measured_fail"
        }
        .to_owned(),
        evidence: evidence.to_owned(),
        denied_reason: None,
    });
}

#[derive(Debug)]
struct SandboxPlatformError {
    stage: &'static str,
    code: Option<i32>,
    name: Option<&'static str>,
    message: String,
}

#[cfg(windows)]
impl SandboxPlatformError {
    fn from_windows(stage: &'static str, err: windows::core::Error) -> Self {
        let code = err.code().0;
        Self {
            stage,
            code: Some(code),
            name: platform_error_name(code),
            message: err.message(),
        }
    }
}

fn platform_error_name(code: i32) -> Option<&'static str> {
    match code as u32 {
        0x80070005 => Some("HRESULT_FROM_WIN32_ERROR_ACCESS_DENIED"),
        0x80070006 => Some("HRESULT_FROM_WIN32_ERROR_INVALID_HANDLE"),
        0x80070057 => Some("HRESULT_FROM_WIN32_ERROR_INVALID_PARAMETER"),
        _ => None,
    }
}

fn sandbox_workdir(
    transport_manifest: Option<&Path>,
    identity_manifest: Option<&Path>,
    loader_ticket_manifest: Option<&Path>,
) -> Option<PathBuf> {
    transport_manifest
        .or(identity_manifest)
        .or(loader_ticket_manifest)
        .and_then(Path::parent)
        .filter(|dir| is_generated_output_path(&dir.join("sandbox-cwd-placeholder.png")))
        .map(Path::to_path_buf)
}

#[derive(Debug, Clone, Copy)]
struct WorkerExitStatus {
    code: Option<i32>,
}

impl WorkerExitStatus {
    fn success(self) -> bool {
        self.code == Some(0)
    }

    fn code(self) -> Option<i32> {
        self.code
    }
}

struct WorkerOutput {
    status: WorkerExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(windows)]
struct WorkerChild {
    process: windows::Win32::Foundation::HANDLE,
    thread: windows::Win32::Foundation::HANDLE,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

#[cfg(windows)]
impl WorkerChild {
    fn resume(&mut self) -> std::io::Result<()> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::ResumeThread;

        if self.thread.is_invalid() {
            return Ok(());
        }
        let previous_suspend_count = unsafe { ResumeThread(self.thread) };
        if previous_suspend_count == u32::MAX {
            return Err(std::io::Error::last_os_error());
        }
        let _ = unsafe { CloseHandle(self.thread) };
        self.thread = windows::Win32::Foundation::HANDLE::default();
        Ok(())
    }

    fn try_wait(&mut self) -> std::io::Result<Option<WorkerExitStatus>> {
        use windows::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::WaitForSingleObject;

        match unsafe { WaitForSingleObject(self.process, 0) } {
            WAIT_OBJECT_0 => self.exit_status().map(Some),
            WAIT_TIMEOUT => Ok(None),
            other => Err(std::io::Error::other(format!(
                "WaitForSingleObject returned {other:?}"
            ))),
        }
    }

    fn kill(&mut self) -> std::io::Result<()> {
        use windows::Win32::System::Threading::TerminateProcess;

        unsafe { TerminateProcess(self.process, 1) }.map_err(windows_io_error)
    }

    fn wait_with_output(
        mut self,
        observed_exit: Option<WorkerExitStatus>,
    ) -> std::io::Result<WorkerOutput> {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::System::Threading::WaitForSingleObject;

        let status = match observed_exit {
            Some(status) => status,
            None => {
                let wait = unsafe { WaitForSingleObject(self.process, u32::MAX) };
                if wait != WAIT_OBJECT_0 {
                    return Err(std::io::Error::other(format!(
                        "WaitForSingleObject returned {wait:?}"
                    )));
                }
                self.exit_status()?
            }
        };
        let stdout = std::fs::read(&self.stdout_path).unwrap_or_default();
        let stderr = std::fs::read(&self.stderr_path).unwrap_or_default();
        if !self.thread.is_invalid() {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.thread) };
            self.thread = windows::Win32::Foundation::HANDLE::default();
        }
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.process) };
        self.process = windows::Win32::Foundation::HANDLE::default();
        Ok(WorkerOutput {
            status,
            stdout,
            stderr,
        })
    }

    fn exit_status(&self) -> std::io::Result<WorkerExitStatus> {
        use windows::Win32::System::Threading::GetExitCodeProcess;

        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.process, &mut code) }.map_err(windows_io_error)?;
        Ok(WorkerExitStatus {
            code: Some(code as i32),
        })
    }
}

#[cfg(windows)]
impl Drop for WorkerChild {
    fn drop(&mut self) {
        if !self.thread.is_invalid() {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.thread) };
        }
        if !self.process.is_invalid() {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.process) };
        }
    }
}

#[cfg(windows)]
fn spawn_worker_child(
    worker_path: &Path,
    worker_args: &[String],
    sandbox_workdir: Option<&Path>,
) -> std::io::Result<WorkerChild> {
    use std::ffi::OsStr;
    use std::fs::OpenOptions;
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        SetHandleInformation, BOOL, FALSE, HANDLE, HANDLE_FLAG_INHERIT, TRUE,
    };
    use windows::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
        UpdateProcThreadAttribute, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
        EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    };

    struct AttributeListGuard {
        raw: LPPROC_THREAD_ATTRIBUTE_LIST,
        _buffer: Vec<u8>,
    }

    impl Drop for AttributeListGuard {
        fn drop(&mut self) {
            unsafe { DeleteProcThreadAttributeList(self.raw) };
        }
    }

    fn set_inherit(handle: HANDLE, enabled: BOOL) -> std::io::Result<()> {
        unsafe {
            SetHandleInformation(
                handle,
                HANDLE_FLAG_INHERIT.0,
                if enabled.as_bool() {
                    HANDLE_FLAG_INHERIT
                } else {
                    Default::default()
                },
            )
        }
        .map_err(windows_io_error)
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn quote_arg(value: &OsStr) -> String {
        let text = value.to_string_lossy();
        if !text.is_empty()
            && !text
                .chars()
                .any(|ch| ch == '"' || ch == '\\' || ch.is_ascii_whitespace())
        {
            return text.into_owned();
        }
        let mut quoted = String::from("\"");
        let mut backslashes = 0usize;
        for ch in text.chars() {
            match ch {
                '\\' => backslashes += 1,
                '"' => {
                    quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                    quoted.push('"');
                    backslashes = 0;
                }
                _ => {
                    quoted.extend(std::iter::repeat_n('\\', backslashes));
                    backslashes = 0;
                    quoted.push(ch);
                }
            }
        }
        quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
        quoted.push('"');
        quoted
    }

    let log_dir = sandbox_workdir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("aviutlas-aex-image-probe"));
    std::fs::create_dir_all(&log_dir)?;
    let unique = format!(
        "worker-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let stdout_path = log_dir.join(format!("{unique}.stdout.log"));
    let stderr_path = log_dir.join(format!("{unique}.stderr.log"));
    let stdin_file = std::fs::File::open("NUL")?;
    let stdout_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&stdout_path)?;
    let stderr_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&stderr_path)?;

    let stdin_handle = HANDLE(stdin_file.as_raw_handle());
    let stdout_handle = HANDLE(stdout_file.as_raw_handle());
    let stderr_handle = HANDLE(stderr_file.as_raw_handle());
    let inherited_handles = [stdin_handle, stdout_handle, stderr_handle];
    for handle in inherited_handles {
        set_inherit(handle, TRUE)?;
    }

    let mut attr_size = 0usize;
    let _ = unsafe {
        InitializeProcThreadAttributeList(
            LPPROC_THREAD_ATTRIBUTE_LIST(std::ptr::null_mut()),
            1,
            0,
            &mut attr_size,
        )
    };
    if attr_size == 0 {
        for handle in inherited_handles {
            let _ = set_inherit(handle, FALSE);
        }
        return Err(std::io::Error::other(
            "InitializeProcThreadAttributeList did not report a buffer size",
        ));
    }
    let mut attr_buffer = vec![0u8; attr_size];
    let attr_raw = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buffer.as_mut_ptr() as *mut _);
    unsafe { InitializeProcThreadAttributeList(attr_raw, 1, 0, &mut attr_size) }
        .map_err(windows_io_error)?;
    unsafe {
        UpdateProcThreadAttribute(
            attr_raw,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(inherited_handles.as_ptr() as *const _),
            inherited_handles.len() * size_of::<HANDLE>(),
            None,
            None,
        )
    }
    .map_err(windows_io_error)?;
    let attr_guard = AttributeListGuard {
        raw: attr_raw,
        _buffer: attr_buffer,
    };

    let mut command_parts = Vec::with_capacity(worker_args.len() + 1);
    command_parts.push(quote_arg(worker_path.as_os_str()));
    command_parts.extend(worker_args.iter().map(|arg| quote_arg(OsStr::new(arg))));
    let command_line = command_parts.join(" ");
    let mut command_line_wide = wide_null(OsStr::new(&command_line));
    let application_wide = wide_null(worker_path.as_os_str());
    let current_dir_wide = sandbox_workdir
        .map(|path| wide_null(path.as_os_str()))
        .unwrap_or_default();
    let environment = [0u16, 0u16];

    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = stdin_handle;
    startup.StartupInfo.hStdOutput = stdout_handle;
    startup.StartupInfo.hStdError = stderr_handle;
    startup.lpAttributeList = attr_guard.raw;
    let mut process_info: PROCESS_INFORMATION = unsafe { zeroed() };
    let create_result = unsafe {
        CreateProcessW(
            PCWSTR(application_wide.as_ptr()),
            PWSTR(command_line_wide.as_mut_ptr()),
            None,
            None,
            TRUE,
            CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED,
            Some(environment.as_ptr() as *const _),
            if current_dir_wide.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(current_dir_wide.as_ptr())
            },
            &startup.StartupInfo as *const _,
            &mut process_info,
        )
    };
    for handle in inherited_handles {
        let _ = set_inherit(handle, FALSE);
    }
    create_result.map_err(windows_io_error)?;

    Ok(WorkerChild {
        process: process_info.hProcess,
        thread: process_info.hThread,
        stdout_path,
        stderr_path,
    })
}

#[cfg(windows)]
fn windows_io_error(err: windows::core::Error) -> std::io::Error {
    std::io::Error::other(err.message())
}

#[cfg(not(windows))]
struct WorkerChild {
    inner: std::process::Child,
}

#[cfg(not(windows))]
impl WorkerChild {
    fn resume(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn try_wait(&mut self) -> std::io::Result<Option<WorkerExitStatus>> {
        self.inner.try_wait().map(|status| {
            status.map(|status| WorkerExitStatus {
                code: status.code(),
            })
        })
    }

    fn kill(&mut self) -> std::io::Result<()> {
        self.inner.kill()
    }

    fn wait_with_output(
        self,
        _observed_exit: Option<WorkerExitStatus>,
    ) -> std::io::Result<WorkerOutput> {
        self.inner.wait_with_output().map(|output| WorkerOutput {
            status: WorkerExitStatus {
                code: output.status.code(),
            },
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[cfg(not(windows))]
fn spawn_worker_child(
    worker_path: &Path,
    worker_args: &[String],
    sandbox_workdir: Option<&Path>,
) -> std::io::Result<WorkerChild> {
    let mut command = std::process::Command::new(worker_path);
    command
        .args(worker_args)
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(workdir) = sandbox_workdir {
        command.current_dir(workdir);
    }
    command.spawn().map(|inner| WorkerChild { inner })
}

#[cfg(windows)]
struct WorkerJobObjectGuard {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl WorkerJobObjectGuard {
    fn new_kill_on_close() -> Result<Self, SandboxPlatformError> {
        use std::mem::size_of;
        use windows::core::PCWSTR;
        use windows::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|err| SandboxPlatformError::from_windows("create_job_object", err))?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set_result = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if let Err(err) = set_result {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(handle) };
            return Err(SandboxPlatformError::from_windows("set_job_limits", err));
        }
        Ok(Self { handle })
    }

    fn assign_child(&self, child: &WorkerChild) -> Result<(), SandboxPlatformError> {
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;

        unsafe { AssignProcessToJobObject(self.handle, child.process) }
            .map_err(|err| SandboxPlatformError::from_windows("assign_process", err))
    }
}

#[cfg(windows)]
impl Drop for WorkerJobObjectGuard {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(not(windows))]
struct WorkerJobObjectGuard;

#[cfg(not(windows))]
impl WorkerJobObjectGuard {
    fn new_kill_on_close() -> Result<Self, SandboxPlatformError> {
        Err(SandboxPlatformError {
            stage: "unsupported_platform",
            code: None,
            name: Some("UNSUPPORTED_PLATFORM"),
            message: "Windows Job Objects are unavailable on this platform".to_owned(),
        })
    }

    fn assign_child(&self, _child: &WorkerChild) -> Result<(), SandboxPlatformError> {
        Ok(())
    }
}

#[cfg(windows)]
struct InheritanceSentinelGuard {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl InheritanceSentinelGuard {
    fn new() -> Result<Self, SandboxPlatformError> {
        use std::mem::size_of;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{FALSE, TRUE};
        use windows::Win32::Security::SECURITY_ATTRIBUTES;
        use windows::Win32::System::Threading::CreateEventW;

        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: TRUE,
        };
        let handle = unsafe { CreateEventW(Some(&attributes), TRUE, FALSE, PCWSTR::null()) }
            .map_err(|err| {
                SandboxPlatformError::from_windows("create_inheritance_sentinel", err)
            })?;
        Ok(Self { handle })
    }

    fn arg_value(&self) -> String {
        (self.handle.0 as usize).to_string()
    }
}

#[cfg(windows)]
impl Drop for InheritanceSentinelGuard {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(not(windows))]
struct InheritanceSentinelGuard;

#[cfg(not(windows))]
impl InheritanceSentinelGuard {
    fn new() -> Result<Self, SandboxPlatformError> {
        Err(SandboxPlatformError {
            stage: "unsupported_platform",
            code: None,
            name: Some("UNSUPPORTED_PLATFORM"),
            message: "Windows inheritable handle sentinel is unavailable on this platform"
                .to_owned(),
        })
    }

    fn arg_value(&self) -> String {
        String::new()
    }
}

fn worker_protocol_stub_report(
    request: &ProbeRequest,
    request_path: Option<&Path>,
    plugin_path: &str,
    entry: &AllowlistEntry,
    default_max_plugin_bytes: Option<u64>,
    elapsed_ms: u128,
) -> ProbeReport {
    if let Some(worker_exe) = request.worker_exe.as_deref() {
        return launch_worker_stub_report(
            request,
            request_path,
            plugin_path,
            entry,
            default_max_plugin_bytes,
            worker_exe,
            elapsed_ms,
        );
    }

    let mut report = ProbeReport::invalid(
        "worker_exe is required after allowlist validation",
        elapsed_ms,
    );
    report.plugin_path = Some(plugin_path.to_owned());
    report.plugin_class = entry.expected_class.clone();
    report.stage = Some("handshake".to_owned());
    if let Some(timeout_ms) = entry.timeout_ms {
        report
            .warnings
            .push(format!("allowlist timeout budget {timeout_ms} ms"));
    }
    report
        .unsupported
        .push("first contract slice does not spawn or load AEX workers".to_owned());
    report
}

fn launch_worker_stub_report(
    request: &ProbeRequest,
    request_path: Option<&Path>,
    plugin_path: &str,
    entry: &AllowlistEntry,
    default_max_plugin_bytes: Option<u64>,
    worker_exe: &str,
    elapsed_ms: u128,
) -> ProbeReport {
    let worker_path = Path::new(worker_exe);
    if !worker_path.is_absolute() {
        return ProbeReport::invalid("worker_exe must be absolute", elapsed_ms);
    }
    if !worker_path.is_file() {
        return ProbeReport::invalid("worker_exe must be an existing file", elapsed_ms);
    }
    if !is_allowed_worker_executable(worker_path) {
        return ProbeReport::invalid(
            "worker_exe must use the allowlisted AEX worker stub filename",
            elapsed_ms,
        );
    }
    let identity =
        match validate_plugin_identity(plugin_path, entry, default_max_plugin_bytes, elapsed_ms) {
            Ok(identity) => identity,
            Err(report) => return *report,
        };
    let transport_manifest = if request.operation == "render_png" {
        match prepare_rgba_transport(request, request_path, elapsed_ms) {
            Ok(manifest) => Some(manifest),
            Err(report) => return *report,
        }
    } else {
        None
    };
    let identity_manifest = if request
        .loader_intent
        .as_ref()
        .map(|intent| intent.request_real_aex_load)
        .unwrap_or(false)
    {
        match prepare_identity_revalidation_manifest(
            request,
            &identity,
            entry,
            default_max_plugin_bytes,
            elapsed_ms,
        ) {
            Ok(manifest) => Some(manifest),
            Err(report) => return *report,
        }
    } else {
        None
    };
    let loader_ticket_manifest = if request
        .loader_intent
        .as_ref()
        .map(|intent| intent.request_real_aex_load)
        .unwrap_or(false)
    {
        match prepare_worker_loader_ticket_manifest(request, request_path, entry, elapsed_ms) {
            Ok(manifest) => Some(manifest),
            Err(report) => return *report,
        }
    } else {
        None
    };

    let launch_timeout_ms = request
        .timeouts_ms
        .as_ref()
        .and_then(|timeouts| timeouts.launch)
        .unwrap_or(3000);
    let mut worker_args = vec![
        "--handshake".to_owned(),
        "--protocol-version".to_owned(),
        "1".to_owned(),
    ];
    if let Some(path) = &transport_manifest {
        worker_args.push("--transport-manifest".to_owned());
        worker_args.push(path.to_string_lossy().into_owned());
    }
    if let Some(path) = &identity_manifest {
        worker_args.push("--identity-manifest".to_owned());
        worker_args.push(path.to_string_lossy().into_owned());
    }
    if let Some(path) = &loader_ticket_manifest {
        worker_args.push("--loader-ticket".to_owned());
        worker_args.push(path.to_string_lossy().into_owned());
    }

    let sandbox_workdir = sandbox_workdir(
        transport_manifest.as_deref(),
        identity_manifest.as_deref(),
        loader_ticket_manifest.as_deref(),
    );
    let mut sandbox_preflight =
        SandboxPreflightReport::new("windows-job-object-v0", sandbox_workdir.as_deref());
    let inheritance_sentinel = match InheritanceSentinelGuard::new() {
        Ok(sentinel) => {
            sandbox_preflight.record_handle_inheritance_probe_created();
            worker_args.push("--inheritance-sentinel".to_owned());
            worker_args.push(sentinel.arg_value());
            Some(sentinel)
        }
        Err(err) if err.name == Some("UNSUPPORTED_PLATFORM") => {
            sandbox_preflight.record_handle_inheritance_probe_unavailable();
            None
        }
        Err(err) => {
            sandbox_preflight.record_handle_inheritance_probe_error(err);
            None
        }
    };
    let sandbox_job = match WorkerJobObjectGuard::new_kill_on_close() {
        Ok(job) => {
            sandbox_preflight.record_job_created();
            Some(job)
        }
        Err(err) if err.name == Some("UNSUPPORTED_PLATFORM") => {
            sandbox_preflight.record_job_error(
                "unsupported_platform",
                "worker job object is unavailable on this platform",
                Some(err),
            );
            None
        }
        Err(err) => {
            let (status, reason) = if err.stage == "set_job_limits" {
                (
                    "set_limit_failed",
                    "worker job object kill-on-close limit could not be set",
                )
            } else {
                ("create_failed", "worker job object could not be created")
            };
            sandbox_preflight.record_job_error(status, reason, Some(err));
            None
        }
    };
    let mut child = match spawn_worker_child(
        Path::new(worker_path),
        &worker_args,
        sandbox_workdir.as_deref(),
    ) {
        Ok(child) => child,
        Err(err) => {
            let mut report = worker_error_report(
                "worker_protocol_error",
                plugin_path,
                entry,
                elapsed_ms,
                "handshake",
            );
            report
                .warnings
                .push(format!("failed to spawn worker: {err}"));
            report.identity_preflight = Some(identity);
            sandbox_preflight.finalize();
            report.sandbox_preflight = Some(sandbox_preflight);
            return report;
        }
    };
    let _inheritance_sentinel = inheritance_sentinel;
    let _sandbox_job = match sandbox_job {
        Some(job) => match job.assign_child(&child) {
            Ok(()) => {
                sandbox_preflight.record_job_assigned();
                Some(job)
            }
            Err(err) => {
                sandbox_preflight.record_job_error(
                    "assign_failed",
                    "worker process could not be assigned to the job object",
                    Some(err),
                );
                None
            }
        },
        None => None,
    };
    if let Err(err) = child.resume() {
        let _ = child.kill();
        let mut report = worker_error_report(
            "worker_protocol_error",
            plugin_path,
            entry,
            elapsed_ms,
            "handshake",
        );
        report.warnings.push(format!(
            "failed to resume worker after sandbox assignment: {err}"
        ));
        report.identity_preflight = Some(identity);
        sandbox_preflight.finalize();
        report.sandbox_preflight = Some(sandbox_preflight);
        return report;
    }
    sandbox_preflight.finalize();

    let deadline = Instant::now() + Duration::from_millis(launch_timeout_ms);
    let observed_exit = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break status;
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child.wait_with_output(None).ok();
                let mut report =
                    worker_error_report("timeout", plugin_path, entry, elapsed_ms, "handshake");
                report.crash = Some(json!({
                    "stage": "handshake",
                    "stdout_preview": output
                        .as_ref()
                        .map(|output| worker_log_preview(&output.stdout))
                        .unwrap_or_default(),
                    "stderr_preview": output
                        .as_ref()
                        .map(|output| worker_log_preview(&output.stderr))
                        .unwrap_or_default()
                }));
                report
                    .warnings
                    .push("worker handshake timed out; child was killed".to_owned());
                report.identity_preflight = Some(identity);
                report.sandbox_preflight = Some(sandbox_preflight);
                return report;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(err) => {
                let mut report = worker_error_report(
                    "worker_protocol_error",
                    plugin_path,
                    entry,
                    elapsed_ms,
                    "handshake",
                );
                report
                    .warnings
                    .push(format!("failed to poll worker: {err}"));
                report.identity_preflight = Some(identity);
                report.sandbox_preflight = Some(sandbox_preflight);
                return report;
            }
        }
    };

    let output = match child.wait_with_output(Some(observed_exit)) {
        Ok(output) => output,
        Err(err) => {
            let mut report = worker_error_report(
                "worker_protocol_error",
                plugin_path,
                entry,
                elapsed_ms,
                "handshake",
            );
            report
                .warnings
                .push(format!("failed to collect worker output: {err}"));
            report.identity_preflight = Some(identity);
            report.sandbox_preflight = Some(sandbox_preflight);
            return report;
        }
    };

    if !output.status.success() {
        let mut report =
            worker_error_report("worker_crash", plugin_path, entry, elapsed_ms, "handshake");
        report.crash = Some(json!({
            "stage": "handshake",
            "exit_code": output.status.code(),
            "stdout_preview": worker_log_preview(&output.stdout),
            "stderr_preview": worker_log_preview(&output.stderr)
        }));
        report.identity_preflight = Some(identity);
        report.sandbox_preflight = Some(sandbox_preflight);
        return report;
    }

    let stdout = bounded_text(&output.stdout);
    let handshake: Value = match serde_json::from_str(&stdout) {
        Ok(handshake) => handshake,
        Err(err) => {
            let mut report = worker_error_report(
                "worker_protocol_error",
                plugin_path,
                entry,
                elapsed_ms,
                "handshake",
            );
            report
                .warnings
                .push(format!("worker handshake JSON should parse: {err}"));
            report.crash = Some(json!({
                "stage": "handshake",
                "stdout_preview": worker_log_preview(&output.stdout),
                "stderr_preview": worker_log_preview(&output.stderr)
            }));
            report.identity_preflight = Some(identity);
            report.sandbox_preflight = Some(sandbox_preflight);
            return report;
        }
    };

    if handshake["worker_protocol_version"].as_u64() != Some(1)
        || handshake["status"].as_str() != Some("worker_ready")
        || handshake["aex_loading"].as_str() != Some("disabled")
    {
        let mut report = worker_error_report(
            "worker_protocol_error",
            plugin_path,
            entry,
            elapsed_ms,
            "handshake",
        );
        report
            .warnings
            .push("worker handshake did not match v0 disabled-loading contract".to_owned());
        report.identity_preflight = Some(identity);
        report.sandbox_preflight = Some(sandbox_preflight);
        return report;
    }
    if let Err(reason) = sandbox_preflight.apply_worker_attestation(
        &handshake,
        sandbox_workdir.as_deref(),
        _inheritance_sentinel.is_some(),
    ) {
        let mut report = worker_error_report(
            "worker_protocol_error",
            plugin_path,
            entry,
            elapsed_ms,
            "handshake",
        );
        report.warnings.push(reason);
        report.identity_preflight = Some(identity);
        sandbox_preflight.finalize();
        report.sandbox_preflight = Some(sandbox_preflight);
        return report;
    }
    sandbox_preflight.finalize();
    if request.operation == "render_png"
        && handshake["transport_status"].as_str() != Some("validated")
    {
        let mut report = worker_error_report(
            "worker_protocol_error",
            plugin_path,
            entry,
            elapsed_ms,
            "handshake",
        );
        report
            .warnings
            .push("worker transport validation was not confirmed".to_owned());
        report.identity_preflight = Some(identity);
        report.sandbox_preflight = Some(sandbox_preflight);
        return report;
    }
    if request
        .loader_intent
        .as_ref()
        .map(|intent| intent.request_real_aex_load)
        .unwrap_or(false)
    {
        let worker_side_revalidation =
            handshake["worker_revalidation"]["status"].as_str() == Some("passed");
        let worker_loader_ticket = worker_loader_ticket_report(&handshake);
        let worker_loader_ticket_accepted = worker_loader_ticket
            .as_ref()
            .map(|ticket| ticket.status == "accepted_no_load")
            .unwrap_or(false);
        let denied_reason = if worker_side_revalidation && worker_loader_ticket_accepted {
            "native AEX loading is disabled pending a separate measured loader slice"
        } else if worker_side_revalidation {
            "worker loader ticket was not accepted"
        } else {
            "worker revalidation was not confirmed"
        };
        let mut report = worker_error_report(
            "worker_protocol_error",
            plugin_path,
            entry,
            elapsed_ms,
            "handshake",
        );
        report.loader_approval = Some(loader_approval_report(
            request,
            entry,
            Some(denied_reason.to_owned()),
            worker_side_revalidation,
            Some(&sandbox_preflight),
        ));
        report.worker_identity_revalidation = Some(worker_identity_revalidation_report(
            &handshake, &identity, entry,
        ));
        report.worker_loader_ticket = worker_loader_ticket;
        report
            .warnings
            .push("worker handshake ok; native AEX loading remains disabled".to_owned());
        report.warnings.push(denied_reason.to_owned());
        report
            .unsupported
            .push("native AEX loading is gated off in this build".to_owned());
        report.identity_preflight = Some(identity);
        report.sandbox_preflight = Some(sandbox_preflight);
        return report;
    }

    let mut report = worker_error_report(
        "worker_protocol_error",
        plugin_path,
        entry,
        elapsed_ms,
        "handshake",
    );
    report.entrypoint = Some("aex_worker_stub_handshake".to_owned());
    report
        .warnings
        .push("worker handshake ok; real .aex loading remains disabled".to_owned());
    if handshake["transport_status"].as_str() == Some("validated") {
        report
            .warnings
            .push("worker transport manifest validated".to_owned());
    }
    report
        .unsupported
        .push("worker-launch slice stops before native entrypoint".to_owned());
    report.loader_approval = Some(loader_approval_report(
        request,
        entry,
        Some("real_aex_loading_disabled".to_owned()),
        false,
        Some(&sandbox_preflight),
    ));
    report.identity_preflight = Some(identity);
    report.sandbox_preflight = Some(sandbox_preflight);
    report
}

fn worker_error_report(
    status: &str,
    plugin_path: &str,
    entry: &AllowlistEntry,
    elapsed_ms: u128,
    stage: &str,
) -> ProbeReport {
    let mut report = ProbeReport::new(status, elapsed_ms);
    report.plugin_path = Some(plugin_path.to_owned());
    report.plugin_class = entry.expected_class.clone();
    report.stage = Some(stage.to_owned());
    report.warnings.push(format!("allowlist id: {}", entry.id));
    report.warnings.push(format!("worker stage: {stage}"));
    report
}

fn validate_plugin_identity(
    plugin_path: &str,
    entry: &AllowlistEntry,
    default_max_plugin_bytes: Option<u64>,
    elapsed_ms: u128,
) -> ProbeReportResult<IdentityPreflightReport> {
    let path = Path::new(plugin_path);
    let mut preflight = IdentityPreflightReport {
        canonical_plugin_path: None,
        exists: path.exists(),
        is_file: path.is_file(),
        extension: path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!(".{}", ext.to_ascii_lowercase())),
        observed_size_bytes: None,
        observed_modified_unix_ms: None,
        allowlist_id: entry.id.clone(),
        status: "denied".to_owned(),
        denied_reason: None,
    };

    if !path.is_file() {
        return Err(Box::new(identity_denied_report(
            preflight,
            "plugin_path must be an existing file before worker launch",
            elapsed_ms,
        )));
    }
    if !plugin_path.to_ascii_lowercase().ends_with(".aex") {
        return Err(Box::new(identity_denied_report(
            preflight,
            "plugin_path must end with .aex",
            elapsed_ms,
        )));
    }
    let canonical = match std::fs::canonicalize(path) {
        Ok(canonical) => canonical,
        Err(err) => {
            return Err(Box::new(identity_denied_report(
                preflight,
                format!("plugin_path should canonicalize: {err}"),
                elapsed_ms,
            )));
        }
    };
    preflight.canonical_plugin_path = Some(canonical.to_string_lossy().replace('\\', "/"));
    if !canonical
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("aex"))
        .unwrap_or(false)
    {
        return Err(Box::new(identity_denied_report(
            preflight,
            "canonical plugin path must end with .aex",
            elapsed_ms,
        )));
    }
    if let Some(expected_canonical) = entry.canonical_plugin_path.as_deref() {
        if !same_path_text(expected_canonical, &canonical.to_string_lossy()) {
            return Err(Box::new(identity_denied_report(
                preflight,
                "canonical_plugin_path does not match allowlist identity",
                elapsed_ms,
            )));
        }
    }
    let publication_status = entry
        .binary_publication_status
        .as_deref()
        .or(entry.publication_status.as_deref())
        .or(entry.path_publication_status.as_deref())
        .unwrap_or("unknown");
    if !matches!(
        publication_status,
        "local-only" | "public-candidate-reviewed" | "public-candidate"
    ) {
        return Err(Box::new(identity_denied_report(
            preflight,
            "allowlist publication_status unknown is fail-closed",
            elapsed_ms,
        )));
    }
    if !matches!(
        entry.license_status.as_deref().unwrap_or("unknown"),
        "local-only-reviewed" | "local-only-unpublished" | "reviewed"
    ) {
        return Err(Box::new(identity_denied_report(
            preflight,
            "allowlist license_status unknown is fail-closed",
            elapsed_ms,
        )));
    }
    if entry.fixture_status.as_deref() != Some("local-build-candidate") {
        return Err(Box::new(identity_denied_report(
            preflight,
            "fixture_status must be local-build-candidate before worker launch",
            elapsed_ms,
        )));
    }
    if !matches!(
        entry.classifier_status.as_deref().unwrap_or("unknown"),
        "classified_from_inventory"
            | "classified_from_adjacent_source"
            | "candidate_for_contract_probe"
    ) {
        return Err(Box::new(identity_denied_report(
            preflight,
            "classifier_status is required before worker launch",
            elapsed_ms,
        )));
    }
    if !matches!(
        entry
            .classifier_inferred_class
            .as_deref()
            .unwrap_or(entry.expected_class.as_str()),
        "classic-effect" | "classic-effect-candidate"
    ) {
        return Err(Box::new(identity_denied_report(
            preflight,
            "classifier_inferred_class must be classic effect compatible",
            elapsed_ms,
        )));
    }
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(err) => {
            return Err(Box::new(identity_denied_report(
                preflight,
                format!("plugin_path metadata should load: {err}"),
                elapsed_ms,
            )));
        }
    };
    preflight.observed_size_bytes = Some(metadata.len());
    preflight.observed_modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64);
    if matches!(entry.observed_size_bytes, Some(size) if size != metadata.len()) {
        return Err(Box::new(identity_denied_report(
            preflight,
            "observed_size_bytes does not match current plugin metadata",
            elapsed_ms,
        )));
    }
    if let (Some(expected), Some(observed)) = (
        entry.observed_modified_unix_ms,
        preflight.observed_modified_unix_ms,
    ) {
        if expected != observed {
            return Err(Box::new(identity_denied_report(
                preflight,
                "observed_modified_unix_ms does not match current plugin metadata",
                elapsed_ms,
            )));
        }
    }
    let max_plugin_bytes = entry
        .max_plugin_bytes
        .or(default_max_plugin_bytes)
        .unwrap_or(64 * 1024 * 1024);
    if metadata.len() == 0 {
        return Err(Box::new(identity_denied_report(
            preflight,
            "plugin_path must not be empty before worker launch",
            elapsed_ms,
        )));
    }
    if metadata.len() > max_plugin_bytes {
        return Err(Box::new(identity_denied_report(
            preflight,
            "plugin_path exceeds allowlist max_plugin_bytes",
            elapsed_ms,
        )));
    }
    preflight.status = "allowed".to_owned();
    Ok(preflight)
}

fn identity_denied_report(
    mut preflight: IdentityPreflightReport,
    reason: impl Into<String>,
    elapsed_ms: u128,
) -> ProbeReport {
    let reason = reason.into();
    preflight.denied_reason = Some(reason.clone());
    let mut report = ProbeReport::invalid(reason, elapsed_ms);
    report.identity_preflight = Some(preflight);
    report
}

fn prepare_rgba_transport(
    request: &ProbeRequest,
    request_path: Option<&Path>,
    elapsed_ms: u128,
) -> ProbeReportResult<PathBuf> {
    let input_png = resolve_cwd_relative_path(request.input_png.as_deref().unwrap_or_default());
    if !input_png.is_file() {
        return Err(Box::new(ProbeReport::invalid(
            "input_png must be an existing file before worker launch",
            elapsed_ms,
        )));
    }
    let output_png = resolve_cwd_relative_path(request.output_png.as_deref().unwrap_or_default());
    if output_png.extension().and_then(|ext| ext.to_str()) != Some("png") {
        return Err(Box::new(ProbeReport::invalid(
            "output_png must end with .png",
            elapsed_ms,
        )));
    }
    if !is_generated_output_path(&output_png) {
        return Err(Box::new(ProbeReport::invalid(
            "output_png must be under target/aex-image-probe generated root",
            elapsed_ms,
        )));
    }
    if output_png.exists() {
        return Err(Box::new(ProbeReport::invalid(
            "output_png must not already exist",
            elapsed_ms,
        )));
    }

    let reader = match image::ImageReader::open(&input_png) {
        Ok(reader) => reader,
        Err(err) => {
            return Err(Box::new(ProbeReport::invalid(
                format!("input_png should open: {err}"),
                elapsed_ms,
            )));
        }
    };
    let reader = match reader.with_guessed_format() {
        Ok(reader) => reader,
        Err(err) => {
            return Err(Box::new(ProbeReport::invalid(
                format!("input_png format should be detected: {err}"),
                elapsed_ms,
            )));
        }
    };
    let image = match reader.decode() {
        Ok(image) => image,
        Err(err) => {
            return Err(Box::new(ProbeReport::invalid(
                format!("input_png should decode: {err}"),
                elapsed_ms,
            )));
        }
    };
    let rgba = image.to_rgba8();
    let (actual_width, actual_height) = rgba.dimensions();
    let frame = request
        .frame
        .as_ref()
        .expect("frame is validated before transport");
    if frame.width != Some(actual_width) || frame.height != Some(actual_height) {
        return Err(Box::new(ProbeReport::invalid(
            "input_png dimensions must match frame.width and frame.height",
            elapsed_ms,
        )));
    }

    let raw_bytes = u64::from(actual_width)
        .saturating_mul(u64::from(actual_height))
        .saturating_mul(4);
    if let Some(max_bytes) = request.limits.as_ref().and_then(|limits| limits.max_bytes) {
        if raw_bytes > max_bytes {
            return Err(Box::new(ProbeReport::invalid(
                "decoded RGBA transport exceeds request max_bytes",
                elapsed_ms,
            )));
        }
    }

    let output_parent = output_png.parent().ok_or_else(|| {
        Box::new(ProbeReport::invalid(
            "output_png parent is required",
            elapsed_ms,
        ))
    })?;
    std::fs::create_dir_all(output_parent).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("generated output root should be created: {err}"),
            elapsed_ms,
        ))
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let raw_path = output_parent.join(format!("worker-input-{stamp}.rgba8"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&raw_path)
        .map_err(|err| {
            Box::new(ProbeReport::invalid(
                format!("raw RGBA transport should be created: {err}"),
                elapsed_ms,
            ))
        })?;
    file.write_all(rgba.as_raw()).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("raw RGBA transport should be written: {err}"),
            elapsed_ms,
        ))
    })?;

    let _ = request_path;
    let manifest_path = output_parent.join(format!("worker-transport-{stamp}.json"));
    let manifest = WorkerTransportManifest {
        schema_version: 1,
        transport_protocol_version: 1,
        pixel_format: "rgba8".to_owned(),
        raw_rgba_path: raw_path.to_string_lossy().replace('\\', "/"),
        generated_root: output_parent.to_string_lossy().replace('\\', "/"),
        width: actual_width,
        height: actual_height,
        row_stride_bytes: u64::from(actual_width).saturating_mul(4),
        decoded_bytes: raw_bytes,
    };
    let manifest_text = serde_json::to_string_pretty(&manifest).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("worker transport manifest should serialize: {err}"),
            elapsed_ms,
        ))
    })?;
    let mut manifest_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path)
        .map_err(|err| {
            Box::new(ProbeReport::invalid(
                format!("worker transport manifest should be created: {err}"),
                elapsed_ms,
            ))
        })?;
    manifest_file
        .write_all(manifest_text.as_bytes())
        .map_err(|err| {
            Box::new(ProbeReport::invalid(
                format!("worker transport manifest should be written: {err}"),
                elapsed_ms,
            ))
        })?;
    Ok(manifest_path)
}

fn prepare_identity_revalidation_manifest(
    request: &ProbeRequest,
    identity: &IdentityPreflightReport,
    entry: &AllowlistEntry,
    default_max_plugin_bytes: Option<u64>,
    elapsed_ms: u128,
) -> ProbeReportResult<PathBuf> {
    let Some(canonical_plugin_path) = identity.canonical_plugin_path.as_deref() else {
        return Err(Box::new(ProbeReport::invalid(
            "identity_preflight canonical path is required for worker revalidation",
            elapsed_ms,
        )));
    };
    let Some(observed_size_bytes) = identity.observed_size_bytes else {
        return Err(Box::new(ProbeReport::invalid(
            "identity_preflight observed size is required for worker revalidation",
            elapsed_ms,
        )));
    };

    let generated_root = request
        .output_png
        .as_deref()
        .map(resolve_cwd_relative_path)
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("target")
                .join("aex-image-probe")
                .join("identity")
        });
    if !is_generated_output_path(&generated_root.join("manifest-placeholder.png")) {
        return Err(Box::new(ProbeReport::invalid(
            "identity manifest must be under target/aex-image-probe generated root",
            elapsed_ms,
        )));
    }
    std::fs::create_dir_all(&generated_root).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("identity manifest root should be created: {err}"),
            elapsed_ms,
        ))
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let manifest_path = generated_root.join(format!("worker-identity-{stamp}.json"));
    let publication_status = entry
        .binary_publication_status
        .as_deref()
        .or(entry.publication_status.as_deref())
        .or(entry.path_publication_status.as_deref())
        .unwrap_or("unknown");
    let max_plugin_bytes = entry
        .max_plugin_bytes
        .or(default_max_plugin_bytes)
        .unwrap_or(64 * 1024 * 1024);
    let manifest = WorkerIdentityManifest {
        schema_version: 1,
        identity_protocol_version: 1,
        generated_by: "aex_image_probe".to_owned(),
        generated_unix_ms: current_unix_ms(),
        max_manifest_age_ms: 30_000,
        allowlist_id: entry.id.clone(),
        operation: request.operation.clone(),
        canonical_plugin_path: canonical_plugin_path.to_owned(),
        expected_extension: ".aex".to_owned(),
        expected_class: entry.expected_class.clone(),
        fixture_status: entry
            .fixture_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        publication_status: publication_status.to_owned(),
        license_status: entry
            .license_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        classifier_status: entry
            .classifier_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        classifier_inferred_class: entry
            .classifier_inferred_class
            .clone()
            .unwrap_or_else(|| entry.expected_class.clone()),
        observed_size_bytes,
        observed_modified_unix_ms: identity.observed_modified_unix_ms,
        max_plugin_bytes,
        loader_approval_status: entry
            .loader_approval_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        sandbox_profile: request
            .loader_intent
            .as_ref()
            .and_then(|intent| intent.sandbox_profile.clone())
            .unwrap_or_else(|| "missing".to_owned()),
        sandbox_profile_status: entry
            .sandbox_profile_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        worker_revalidation_status: entry
            .worker_revalidation_status
            .clone()
            .unwrap_or_else(|| "missing".to_owned()),
        binary_evidence_mode: "metadata-only".to_owned(),
    };
    let manifest_text = serde_json::to_string_pretty(&manifest).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("identity manifest should serialize: {err}"),
            elapsed_ms,
        ))
    })?;
    let mut manifest_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path)
        .map_err(|err| {
            Box::new(ProbeReport::invalid(
                format!("identity manifest should be created: {err}"),
                elapsed_ms,
            ))
        })?;
    manifest_file
        .write_all(manifest_text.as_bytes())
        .map_err(|err| {
            Box::new(ProbeReport::invalid(
                format!("identity manifest should be written: {err}"),
                elapsed_ms,
            ))
        })?;
    Ok(manifest_path)
}

fn prepare_worker_loader_ticket_manifest(
    request: &ProbeRequest,
    request_path: Option<&Path>,
    entry: &AllowlistEntry,
    elapsed_ms: u128,
) -> ProbeReportResult<PathBuf> {
    let Some(preflight_path) = request.loader_preflight.as_deref() else {
        return Err(Box::new(ProbeReport::invalid(
            "loader_preflight is required before worker loader ticket",
            elapsed_ms,
        )));
    };
    let preflight_path = resolve_request_relative_path(preflight_path, request_path);
    let preflight_text = std::fs::read_to_string(&preflight_path).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("loader_preflight should read before worker loader ticket: {err}"),
            elapsed_ms,
        ))
    })?;
    if preflight_text.len() > 256 * 1024 {
        return Err(Box::new(ProbeReport::invalid(
            "loader_preflight is too large for worker loader ticket",
            elapsed_ms,
        )));
    }
    let preflight: Value = serde_json::from_str(&preflight_text).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("loader_preflight should parse before worker loader ticket: {err}"),
            elapsed_ms,
        ))
    })?;
    let selected_loader_entry = &preflight["selected_loader_entry"];
    let normalized_plugin_path = selected_loader_entry["normalized_plugin_path"]
        .as_str()
        .ok_or_else(|| {
            Box::new(ProbeReport::invalid(
                "selected_loader_entry.normalized_plugin_path is required for worker loader ticket",
                elapsed_ms,
            ))
        })?;
    let path_match_status = selected_loader_entry["path_match_status"]
        .as_str()
        .ok_or_else(|| {
            Box::new(ProbeReport::invalid(
                "selected_loader_entry.path_match_status is required for worker loader ticket",
                elapsed_ms,
            ))
        })?;
    let allowlist_operation_status = selected_loader_entry["allowlist_operation_status"]
        .as_str()
        .ok_or_else(|| {
            Box::new(ProbeReport::invalid(
                "selected_loader_entry.allowlist_operation_status is required for worker loader ticket",
                elapsed_ms,
            ))
        })?;

    let generated_root = request
        .output_png
        .as_deref()
        .map(resolve_cwd_relative_path)
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("target")
                .join("aex-image-probe")
                .join("loader-ticket")
        });
    if !is_generated_output_path(&generated_root.join("ticket-placeholder.png")) {
        return Err(Box::new(ProbeReport::invalid(
            "worker loader ticket must be under target/aex-image-probe generated root",
            elapsed_ms,
        )));
    }
    std::fs::create_dir_all(&generated_root).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("worker loader ticket root should be created: {err}"),
            elapsed_ms,
        ))
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let manifest_path = generated_root.join(format!("worker-loader-ticket-{stamp}.json"));
    let ticket = WorkerLoaderTicketManifest {
        schema_version: 1,
        ticket_protocol_version: 1,
        generated_by: "aex_image_probe".to_owned(),
        generated_unix_ms: current_unix_ms(),
        max_ticket_age_ms: 30_000,
        publication_status: "local-only".to_owned(),
        status: "accepted_no_load".to_owned(),
        native_load_performed: false,
        worker_may_load_plugin: false,
        broker_may_load_plugin: false,
        allowlist_id: entry.id.clone(),
        operation: request.operation.clone(),
        selected_loader_entry: WorkerLoaderTicketEntry {
            effect_id: entry.id.clone(),
            normalized_plugin_path: normalized_plugin_path.to_owned(),
            path_match_status: path_match_status.to_owned(),
            allowlist_operation_status: allowlist_operation_status.to_owned(),
            entry_ready: selected_loader_entry["entry_ready"]
                .as_bool()
                .unwrap_or(false),
        },
        required_runtime_evidence: WorkerLoaderTicketRuntimeEvidence {
            worker_identity_revalidation_required: selected_loader_entry
                ["worker_identity_revalidation_required"]
                .as_str()
                .unwrap_or("missing")
                .to_owned(),
            worker_attestation_required: selected_loader_entry["worker_attestation_required"]
                .as_str()
                .unwrap_or("missing")
                .to_owned(),
            sandbox_preflight_required: selected_loader_entry["sandbox_preflight_required"]
                .as_str()
                .unwrap_or("missing")
                .to_owned(),
            job_object_required: selected_loader_entry["job_object_required"]
                .as_str()
                .unwrap_or("missing")
                .to_owned(),
            handle_inheritance_required: selected_loader_entry["handle_inheritance_required"]
                .as_str()
                .unwrap_or("missing")
                .to_owned(),
        },
        planned_stages: worker_loader_ticket_planned_stages(),
        denied_surfaces: vec![
            "AEGP".to_owned(),
            "AEIO".to_owned(),
            "SmartFX-only".to_owned(),
            "GPU".to_owned(),
            "custom UI".to_owned(),
            "audio".to_owned(),
            "layer checkout".to_owned(),
            "file/network APIs".to_owned(),
        ],
        notes: vec![
            "Worker validated loader ticket metadata only; no native AEX load was performed."
                .to_owned(),
        ],
    };
    let manifest_text = serde_json::to_string_pretty(&ticket).map_err(|err| {
        Box::new(ProbeReport::invalid(
            format!("worker loader ticket should serialize: {err}"),
            elapsed_ms,
        ))
    })?;
    let mut manifest_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path)
        .map_err(|err| {
            Box::new(ProbeReport::invalid(
                format!("worker loader ticket should be created: {err}"),
                elapsed_ms,
            ))
        })?;
    manifest_file
        .write_all(manifest_text.as_bytes())
        .map_err(|err| {
            Box::new(ProbeReport::invalid(
                format!("worker loader ticket should be written: {err}"),
                elapsed_ms,
            ))
        })?;
    Ok(manifest_path)
}

fn worker_loader_ticket_planned_stages() -> Vec<WorkerLoaderTicketStage> {
    [
        "load",
        "global_setup",
        "params_setup",
        "sequence_setup",
        "render",
        "sequence_teardown",
        "global_teardown",
    ]
    .iter()
    .map(|stage| WorkerLoaderTicketStage {
        stage: (*stage).to_owned(),
        status: "planned_not_run".to_owned(),
    })
    .collect()
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn is_allowed_worker_executable(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let file_stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_exe = file_name.ends_with(".exe") || !cfg!(windows);
    if !is_exe {
        return false;
    }
    if file_stem == "aex_effect_worker_stub" {
        return is_reviewed_cargo_example_worker(path);
    }
    is_reviewed_dynamic_test_worker(path, &file_stem)
}

fn resolve_cwd_relative_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(path)
}

fn is_generated_output_path(path: &Path) -> bool {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return false;
    }
    let text = normalize_path_text(&path.to_string_lossy());
    let root = generated_output_root();
    let root_text = normalize_path_text(&root.to_string_lossy());
    (text == root_text || text.starts_with(&format!("{root_text}/")))
        && existing_ancestors_are_plain_directories(&root, path)
}

fn generated_output_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-image-probe")
}

fn is_reviewed_cargo_example_worker(path: &Path) -> bool {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return false;
    }
    let target_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    if !path_text_is_under_root(path, &target_root) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(profile_dir) = parent.parent() else {
        return false;
    };
    let Some(target_dir) = profile_dir.parent() else {
        return false;
    };
    parent.file_name().and_then(|name| name.to_str()) == Some("examples")
        && matches!(
            profile_dir.file_name().and_then(|name| name.to_str()),
            Some("debug" | "release")
        )
        && target_dir.file_name().and_then(|name| name.to_str()) == Some("target")
        && existing_ancestors_are_plain_directories(&target_root, path)
}

fn is_reviewed_dynamic_test_worker(path: &Path, file_stem: &str) -> bool {
    const PREFIX: &str = "aex_effect_worker_stub_";
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return false;
    }
    let Some(suffix) = file_stem.strip_prefix(PREFIX) else {
        return false;
    };
    let Some((mode_and_pid, stamp)) = suffix.rsplit_once('_') else {
        return false;
    };
    let Some((mode, pid)) = mode_and_pid.rsplit_once('_') else {
        return false;
    };
    if pid != std::process::id().to_string() || stamp.parse::<u128>().is_err() {
        return false;
    }
    if !allowed_dynamic_worker_stub_mode(mode) {
        return false;
    }
    let worker_root = generated_output_root().join("test-workers");
    path_text_is_under_root(path, &worker_root)
        && existing_ancestors_are_plain_directories(&worker_root, path)
}

fn allowed_dynamic_worker_stub_mode(mode: &str) -> bool {
    matches!(
        mode,
        "bad_protocol"
            | "crash"
            | "enabled_without_revalidation"
            | "malformed"
            | "no_transport"
            | "noisy_crash"
            | "ready"
            | "revalidation_absent"
            | "revalidation_passed"
            | "spawn_descendant_timeout"
            | "timeout"
    )
}

fn path_text_is_under_root(path: &Path, root: &Path) -> bool {
    let text = normalize_path_text(&path.to_string_lossy());
    let root_text = normalize_path_text(&root.to_string_lossy());
    text == root_text || text.starts_with(&format!("{root_text}/"))
}

fn existing_ancestors_are_plain_directories(root: &Path, path: &Path) -> bool {
    let mut current = root.to_path_buf();
    if existing_path_is_reparse_or_symlink(&current) {
        return false;
    }
    let relative = match path.strip_prefix(root) {
        Ok(relative) => relative,
        Err(_) => return false,
    };
    let parent_relative = relative.parent().unwrap_or_else(|| Path::new(""));
    for component in parent_relative.components() {
        let Component::Normal(part) = component else {
            return false;
        };
        current.push(part);
        if current.exists() && existing_path_is_reparse_or_symlink(&current) {
            return false;
        }
    }
    true
}

fn existing_path_is_reparse_or_symlink(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn normalize_path_text(path: &str) -> String {
    let replaced = path.replace('\\', "/").to_ascii_lowercase();
    let mut prefix = String::new();
    let mut parts = Vec::new();
    for (index, part) in replaced.split('/').enumerate() {
        if index == 0 && part.ends_with(':') {
            prefix = part.to_owned();
            continue;
        }
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    if prefix.is_empty() {
        format!("/{}", parts.join("/"))
    } else {
        format!("{prefix}/{}", parts.join("/"))
    }
}

fn bounded_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.chars().take(WORKER_LOG_PREVIEW_CHARS).collect()
}

fn worker_log_preview(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let sanitized = sanitize_worker_log_text(&text);
    bounded_preview(&sanitized)
}

fn sanitize_worker_log_text(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut output = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        if looks_like_windows_abs_path(&chars, index) || looks_like_unix_abs_path(&chars, index) {
            output.push_str(REDACTED_LOCAL_PATH);
            index = consume_path_like_token(&chars, index);
            continue;
        }
        let ch = chars[index];
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            output.push(' ');
        } else {
            output.push(ch);
        }
        index += 1;
    }
    output
}

fn looks_like_windows_abs_path(chars: &[char], index: usize) -> bool {
    index + 2 < chars.len()
        && chars[index].is_ascii_alphabetic()
        && chars[index + 1] == ':'
        && matches!(chars[index + 2], '/' | '\\')
}

fn looks_like_unix_abs_path(chars: &[char], index: usize) -> bool {
    chars.get(index) == Some(&'/')
        && chars
            .get(index + 1)
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn consume_path_like_token(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len()
        && !matches!(
            chars[index],
            '\n' | '\r' | '\t' | '"' | '\'' | '`' | '<' | '>' | '{' | '}' | '[' | ']'
        )
    {
        index += 1;
    }
    index
}

fn bounded_preview(text: &str) -> String {
    let char_count = text.chars().count();
    if char_count <= WORKER_LOG_PREVIEW_CHARS {
        return text.to_owned();
    }
    let take = WORKER_LOG_PREVIEW_CHARS.saturating_sub(WORKER_LOG_TRUNCATED_MARKER.len());
    let mut preview: String = text.chars().take(take).collect();
    preview.push_str(WORKER_LOG_TRUNCATED_MARKER);
    preview
}

fn load_allowlist(path: &Path) -> anyhow::Result<ProbeAllowlist> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read allowlist {}", path.display()))?;
    serde_json::from_str(&text).context("allowlist JSON should parse")
}

fn write_report(path: &Path, report: &ProbeReport) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report dir {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&json!(report)).context("report should serialize")?;
    std::fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))
}

fn resolve_request_relative_path(path: &str, request_path: Option<&Path>) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    request_path
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .join(path)
}

fn same_path_text(left: &str, right: &str) -> bool {
    left.replace('\\', "/")
        .eq_ignore_ascii_case(&right.replace('\\', "/"))
}

fn loader_preflight_path_key(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for part in path.trim().replace('/', "\\").split('\\') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            match parts.last() {
                Some(previous) if !previous.ends_with(':') && previous != ".." => {
                    parts.pop();
                }
                _ => parts.push(part.to_string()),
            }
            continue;
        }
        parts.push(part.to_string());
    }
    parts.join("\\").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_is_metadata_only() {
        let request = ProbeRequest {
            schema_version: 1,
            operation: "catalog".to_owned(),
            plugin_path: None,
            allowlist: None,
            input_png: None,
            output_png: None,
            params: Value::Null,
            pixel_format: Some("rgba8".to_owned()),
            frame: None,
            limits: None,
            timeouts_ms: None,
            worker_exe: None,
            loader_preflight: None,
            loader_intent: None,
        };

        let report = run_probe_request(&request, None, 0);
        assert_eq!(report.status, "catalog_ok");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("no .aex loaded")));
    }

    #[test]
    fn invalid_operation_is_reported() {
        let report =
            run_probe_request_text(r#"{"schema_version":1,"operation":"load_now"}"#, None).unwrap();
        assert_eq!(report.status, "invalid_request");
        assert!(report.warnings[0].contains("unsupported operation"));
    }

    #[test]
    fn render_requires_frame_and_png_paths() {
        let report = run_probe_request_text(
            r#"{
                "schema_version": 1,
                "operation": "render_png",
                "plugin_path": "D:/example/Allowed.aex",
                "allowlist": "allowlist.json",
                "pixel_format": "rgba8"
            }"#,
            None,
        )
        .unwrap();
        assert_eq!(report.status, "invalid_request");
        assert!(report.warnings[0].contains("input_png"));
    }

    #[test]
    fn same_path_text_is_separator_and_case_tolerant() {
        assert!(same_path_text(
            r"D:\Projects\Example\Plugin.aex",
            "d:/projects/example/plugin.aex"
        ));
    }

    #[test]
    fn report_json_uses_allowed_status_shape() {
        let report = worker_protocol_stub_report(
            &ProbeRequest {
                schema_version: 1,
                operation: "describe".to_owned(),
                plugin_path: Some("D:/example/Allowed.aex".to_owned()),
                allowlist: Some("allowlist.json".to_owned()),
                input_png: None,
                output_png: None,
                params: json!({}),
                pixel_format: Some("rgba8".to_owned()),
                frame: None,
                limits: None,
                timeouts_ms: None,
                worker_exe: None,
                loader_preflight: None,
                loader_intent: None,
            },
            None,
            "D:/example/Allowed.aex",
            &AllowlistEntry {
                id: "allowed".to_owned(),
                plugin_path: "D:/example/Allowed.aex".to_owned(),
                expected_class: "classic-effect".to_owned(),
                allowed_operations: vec!["describe".to_owned()],
                max_width: None,
                max_height: None,
                timeout_ms: None,
                fixture_status: Some("local-build-candidate".to_owned()),
                publication_status: Some("local-only".to_owned()),
                path_publication_status: None,
                binary_publication_status: None,
                license_status: Some("local-only-reviewed".to_owned()),
                max_plugin_bytes: None,
                canonical_plugin_path: None,
                observed_size_bytes: None,
                observed_modified_unix_ms: None,
                classifier_status: Some("candidate_for_contract_probe".to_owned()),
                classifier_inferred_class: Some("classic-effect".to_owned()),
                loader_approval_status: None,
                sandbox_profile_status: None,
                worker_revalidation_status: None,
            },
            None,
            0,
        );
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["status"], "invalid_request");
        assert_eq!(json["plugin_class"], "classic-effect");
        assert_eq!(json["stage"], "handshake");
        assert!(json["crash"].is_null());
    }
}
