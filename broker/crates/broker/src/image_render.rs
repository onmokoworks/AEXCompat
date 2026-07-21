use crate::host_core::descriptor_manifest::load as load_manifest;
use crate::host_core::parameter::{apply_defaults, encode_worker_payload, ValidatedAssignments};
use crate::runtime_module_authorization::{
    encode_runtime_module_authorization, RuntimeModulePurpose,
};
use crate::runtime_module_policy::{
    authenticate_gpu_worker_report, ApprovedClassifiedModule, RuntimeBackend, RuntimeModulePolicy,
    WorkerModuleValidation,
};
use crate::secure_image_dispatch::{
    dispatch_secure_gpu_image, dispatch_secure_image, ApprovedImageArtifact,
    GpuRuntimeAuthorization, SecureImageDispatch, WorkerKind,
};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const MAX_DIMENSION: u32 = 4096;
pub(crate) const MAX_PIXELS: u64 = 16_777_216;
pub(crate) const MAX_RGBA_TRANSPORT_BYTES: u64 = MAX_PIXELS * 4;
pub(crate) const MAX_INTERNAL_IMAGE_BYTES: u64 = MAX_PIXELS * 16;
const MAX_PARAMETERS: u32 = 1024;
pub(crate) const INTERACTIVE_RENDER_TIMEOUT_MS: u64 = 30_000;
const MAX_STAGE_EVENTS: usize = 32;
const MAX_MISSING_SUITES: usize = 16;
const MAX_SUITE_NAME_LEN: usize = 96;
const STALE_IMAGE_TRANSPORT_AGE: Duration = Duration::from_secs(15 * 60);
const CONFORMANCE_RENDER_SETTINGS_ENV: &str = "AEXCOMPAT_CONFORMANCE_RENDER_SETTINGS";

fn conformance_render_settings_transport() -> io::Result<Option<String>> {
    let Ok(encoded) = std::env::var(CONFORMANCE_RENDER_SETTINGS_ENV) else {
        return Ok(None);
    };
    let fields = encoded.split('|').collect::<Vec<_>>();
    if fields.len() != 6
        || fields[0] != "v1"
        || !matches!(fields[1], "straight" | "premultiplied" | "opaque")
        || fields[2] != "0"
        || fields[3] != "-"
        || fields[4] != "0"
        || !matches!(fields[5], "AEXCompat CPU" | "software")
        || encoded.bytes().any(|byte| byte < 0x20)
    {
        return Err(invalid(
            "unsupported or malformed conformance render settings",
        ));
    }
    Ok(Some(encoded))
}

fn apply_conformance_premultiplication(rgba: &mut [u8], mode: &str) {
    if mode == "straight" {
        return;
    }
    for pixel in rgba.chunks_exact_mut(4) {
        if mode == "premultiplied" {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        } else if mode == "opaque" {
            pixel[3] = 255;
        }
    }
}

/// PF_OutFlag2_SUPPORTS_SMART_RENDER in the out_flags2 word the plug-in
/// advertises from PF_Cmd_GLOBAL_SETUP.
pub const PF_OUTFLAG2_SUPPORTS_SMART_RENDER: u64 = 1 << 10;

/// A plug-in that advertises SUPPORTS_SMART_RENDER expects the SmartFX
/// selector sequence; After Effects always prefers it over Classic RENDER,
/// so hosts can use this as the default render path (issue #105).
pub fn smart_render_advertised(out_flags2: u64) -> bool {
    out_flags2 & PF_OUTFLAG2_SUPPORTS_SMART_RENDER != 0
}

fn dispatch_approved_image(
    repository: &Path,
    worker_kind: WorkerKind,
    plugin_path: &Path,
    approved_sha256: &str,
    args_before_plugin: &[String],
    args_after_plugin: &[String],
    timeout: Duration,
) -> io::Result<crate::secure_launch::SecureLaunchResult> {
    dispatch_secure_image(SecureImageDispatch {
        repository,
        worker_kind,
        plugin: ApprovedImageArtifact {
            path: plugin_path.to_path_buf(),
            expected_sha256: decode_sha256_hex(approved_sha256)?,
            expected_size: fs::metadata(plugin_path)?.len(),
        },
        // Session approval currently covers only the selected plugin image.
        dependencies: vec![],
        args_before_plugin,
        args_after_plugin,
        timeout,
    })
}

fn dispatch_approved_image_with_dependencies(
    repository: &Path,
    worker_kind: WorkerKind,
    plugin_path: &Path,
    approved_sha256: &str,
    dependencies: Vec<ApprovedImageArtifact>,
    args_before_plugin: &[String],
    args_after_plugin: &[String],
    timeout: Duration,
) -> io::Result<crate::secure_launch::SecureLaunchResult> {
    dispatch_secure_image(SecureImageDispatch {
        repository,
        worker_kind,
        plugin: ApprovedImageArtifact {
            path: plugin_path.to_path_buf(),
            expected_sha256: decode_sha256_hex(approved_sha256)?,
            expected_size: fs::metadata(plugin_path)?.len(),
        },
        dependencies,
        args_before_plugin,
        args_after_plugin,
        timeout,
    })
}

struct RuntimeAuthorizationTransport {
    path: PathBuf,
    artifact: ApprovedImageArtifact,
    basename: String,
}

impl Drop for RuntimeAuthorizationTransport {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn prepare_runtime_authorization_transport(
    repository: &Path,
    policy: &RuntimeModulePolicy,
    backend: RuntimeBackend,
) -> io::Result<RuntimeAuthorizationTransport> {
    let mut session_identity = rand::random::<[u8; 32]>();
    if session_identity.iter().all(|byte| *byte == 0) {
        session_identity[0] = 1;
    }
    let manifest = encode_runtime_module_authorization(
        policy,
        RuntimeModulePurpose::PfParameterInspect,
        backend,
        session_identity,
    )?;
    let root = repository.join("target/runtime-module-authorization");
    fs::create_dir_all(&root)?;
    let basename = format!("authorization-{:032x}.bin", rand::random::<u128>());
    let path = root.join(&basename);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&manifest.bytes)?;
    file.sync_all()?;
    drop(file);
    Ok(RuntimeAuthorizationTransport {
        path: path.clone(),
        artifact: ApprovedImageArtifact {
            path,
            expected_sha256: manifest.sha256,
            expected_size: manifest.size,
        },
        basename,
    })
}

pub(crate) fn decode_sha256_hex(value: &str) -> io::Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(
            "plugin SHA-256 must be exactly 64 hexadecimal characters",
        ));
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).expect("ASCII hex was validated");
        decoded[index] = u8::from_str_radix(text, 16)
            .map_err(|_| invalid("plugin SHA-256 contains invalid hexadecimal"))?;
    }
    Ok(decoded)
}

fn decimal_component(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

fn is_owned_image_transport_name(name: &str) -> bool {
    let simple = [
        ("input-", ".rgba"),
        ("output-", ".rgba"),
        ("audio-", ".f32"),
        ("report-", ".json"),
        ("parameter-animation-", ".json"),
        ("aux-manifest-", ".json"),
    ];
    if simple.iter().any(|(prefix, suffix)| {
        name.strip_prefix(prefix)
            .and_then(|body| body.strip_suffix(suffix))
            .is_some_and(decimal_component)
    }) {
        return true;
    }

    // `layer-<nonce>-<index>.rgba` is the one-shot layered transport; the
    // resident session streams its own per-layer files as
    // `layer-session-<nonce>-<index>.rgba` (#268), a distinct prefix so the two
    // routes never collide on a nonce. Both are broker-owned and must be
    // reclaimable by the stale sweep when a crash skips their normal deletion.
    [
        ("layer-", ".rgba"),
        ("layer-session-", ".rgba"),
        ("aux-", ".f32le"),
    ]
    .iter()
    .any(|(prefix, suffix)| {
        name.strip_prefix(prefix)
            .and_then(|body| body.strip_suffix(suffix))
            .and_then(|body| body.split_once('-'))
            .is_some_and(|(nonce, index)| decimal_component(nonce) && decimal_component(index))
    })
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn cleanup_stale_image_transport_before(
    root: &Path,
    now: SystemTime,
    stale_age: Duration,
) -> io::Result<()> {
    let canonical_root = root.canonicalize()?;
    for entry in fs::read_dir(root)? {
        let Ok(entry) = entry else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_owned_image_transport_name(&name) {
            continue;
        }
        let path = root.join(&name);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() || is_reparse_point(&metadata) {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age <= stale_age {
            continue;
        }

        // Delete only the canonical regular file observed directly under this owned root.
        let Ok(canonical_path) = path.canonicalize() else {
            continue;
        };
        if canonical_path.parent() != Some(canonical_root.as_path())
            || canonical_path.file_name() != Some(entry.file_name().as_os_str())
        {
            continue;
        }
        let Ok(current) = fs::symlink_metadata(&canonical_path) else {
            continue;
        };
        if !current.file_type().is_file()
            || is_reparse_point(&current)
            || current.modified().ok() != Some(modified)
            || current.len() != metadata.len()
        {
            continue;
        }
        let _ = fs::remove_file(canonical_path);
    }
    Ok(())
}

fn cleanup_stale_image_transport(root: &Path, now: SystemTime) -> io::Result<()> {
    cleanup_stale_image_transport_before(root, now, STALE_IMAGE_TRANSPORT_AGE)
}

/// Decodes an input image while enforcing the transport bounds. Public so the
/// harness's resident-session adapter can decode its cached input through the
/// same fail-closed limits the one-shot entries apply.
pub fn decode_bounded_image(path: &Path, role: &str) -> io::Result<image::DynamicImage> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|error| invalid(format!("{role} image open failed: {error}")))?
        .with_guessed_format()
        .map_err(|error| invalid(format!("{role} image format failed: {error}")))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_RGBA_TRANSPORT_BYTES);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|error| invalid(format!("{role} image decode failed: {error}")))
}

/// Diagnostics for a dispatched worker run, including the kill evidence from
/// Job Object accounting (issue #21): why a dead worker died (timeout versus
/// allocations failing at the memory cap) and how much memory it peaked at.
pub(crate) fn isolated_worker_diagnostics(
    isolated: &crate::secure_launch::SecureLaunchResult,
    elapsed_ms: u128,
) -> Value {
    let mut diagnostics = worker_diagnostics(
        &isolated.stderr,
        isolated.stderr_truncated,
        isolated.classification.as_str(),
        isolated.exit_code,
        elapsed_ms,
    );
    let object = diagnostics
        .as_object_mut()
        .expect("worker_diagnostics returns an object");
    object.insert("kill_reason".into(), json!(isolated.kill_reason));
    object.insert(
        "memory_limit_reached".into(),
        json!(isolated.memory_limit_reached),
    );
    object.insert(
        "worker_peak_commit_bytes".into(),
        json!(isolated.worker_peak_commit_bytes),
    );
    object.insert(
        "peak_process_memory_bytes".into(),
        json!(isolated.peak_process_memory_bytes),
    );
    object.insert(
        "peak_job_memory_bytes".into(),
        json!(isolated.peak_job_memory_bytes),
    );
    object.insert(
        "process_memory_limit_bytes".into(),
        json!(isolated.process_memory_limit_bytes),
    );
    diagnostics
}

fn worker_diagnostics(
    stderr: &str,
    stderr_truncated: bool,
    classification: &str,
    exit_code: u32,
    elapsed_ms: u128,
) -> Value {
    let allowed = [
        "global_setup",
        "params_setup",
        "sequence_setup",
        "sequence_flatten",
        "get_flattened_sequence_data",
        "get_external_dependencies",
        "do_dialog",
        "automatic_dialog",
        "sequence_resetup",
        "frame_setup",
        "frame_setdown",
        "sequence_setdown",
        "render",
        "smart_render",
        "smart_pre_render",
        "smart_render_cpu",
        "smart_render_gpu",
        "gpu_device_setup",
        "gpu_device_info",
        "gpu_device_setdown",
        "audio_setup",
        "audio_render",
        "audio_setdown",
        "global_setdown",
    ];
    let mut events = Vec::new();
    let mut active_stages: Vec<String> = Vec::new();
    let mut first_failure_stage: Option<String> = None;
    let mut failure_stage: Option<String> = None;
    let mut last_completed_stage: Option<String> = None;
    let mut plugin_kind: Option<&str> = None;
    let mut minidump: Option<String> = None;

    for line in stderr.lines() {
        plugin_kind = plugin_kind.or_else(|| match line.trim() {
            "plugin_kind:aegp_candidate" => Some("aegp_candidate"),
            "plugin_kind:unknown_no_effect_entrypoint" => Some("unknown_no_effect_entrypoint"),
            _ => None,
        });
        if minidump.is_none() {
            if let Some(marker) = minidump_marker(line.trim()) {
                minidump = Some(marker);
            }
        }
        let Some(body) = line.trim().strip_prefix("stage:") else {
            continue;
        };
        let (token, detail) = body.split_once(' ').unwrap_or((body, ""));
        let (stage, state) = if let Some(stage) = token.strip_suffix("_begin") {
            (stage, "begin")
        } else if let Some(stage) = token.strip_suffix("_end") {
            (stage, "end")
        } else {
            continue;
        };
        if !allowed.contains(&stage) || events.len() >= MAX_STAGE_EVENTS {
            continue;
        }
        let errors = detail
            .split_whitespace()
            .filter_map(|item| {
                let (name, value) = item.split_once('=')?;
                if !matches!(name, "error" | "pre_error" | "render_error") {
                    return None;
                }
                value.parse::<i64>().ok().map(|value| (name, value))
            })
            .map(|(name, value)| (name.to_owned(), json!(value)))
            .collect::<serde_json::Map<_, _>>();
        if state == "begin" {
            active_stages.push(stage.to_owned());
        } else {
            if let Some(index) = active_stages.iter().rposition(|active| active == stage) {
                active_stages.remove(index);
            }
            last_completed_stage = Some(stage.to_owned());
            if errors
                .values()
                .any(|value| value.as_i64().unwrap_or(0) != 0)
            {
                if first_failure_stage.is_none() {
                    first_failure_stage = Some(stage.to_owned());
                }
                failure_stage = Some(stage.to_owned());
            }
        }
        events.push(json!({"stage": stage, "state": state, "errors": errors}));
    }
    let active_stage = active_stages.last().cloned();
    if failure_stage.is_none() && classification != "ok" {
        failure_stage = active_stage.clone();
        first_failure_stage = active_stage.clone();
    }

    json!({
        "classification": classification,
        "exit_code": exit_code,
        "elapsed_ms": elapsed_ms,
        "stderr_truncated": stderr_truncated,
        "stage_events": events,
        "active_stage": active_stage,
        "failure_stage": failure_stage,
        "first_failure_stage": first_failure_stage,
        "last_completed_stage": last_completed_stage,
        "missing_suites": [],
        "plugin_kind": plugin_kind,
        "minidump": minidump,
    })
}

/// Validates a `stage:minidump_*` stderr line against the exact shapes the
/// worker emits and returns a normalized, path-free marker. stderr is mixed
/// worker/plug-in output, so a plug-in could otherwise spoof
/// `stage:minidump_written name=C:\...` and smuggle a private path into
/// shareable diagnostics; anything not matching a worker-owned shape is
/// dropped. The broker-owned file name is deliberately never serialized.
fn minidump_marker(line: &str) -> Option<String> {
    let body = line.strip_prefix("stage:minidump_")?;
    if let Some(bytes) = body.strip_prefix("written bytes=") {
        if bytes.bytes().all(|b| b.is_ascii_digit()) && !bytes.is_empty() && bytes.len() <= 20 {
            return Some(format!("written bytes={bytes}"));
        }
        return None;
    }
    let reason = body.strip_prefix("failed reason=")?;
    let reason = reason.split_once(" code=").map_or(reason, |(head, _)| head);
    matches!(
        reason,
        "dbghelp_unavailable"
            | "entry_unavailable"
            | "create_failed"
            | "write_failed"
            | "handle_invalid"
            | "writer_unavailable"
            | "writer_timeout"
            | "capacity_exceeded"
    )
    .then(|| format!("failed reason={reason}"))
}

fn propagate_missing_suites(diagnostics: &mut Value, worker_report: &Value) {
    let mut suites = Vec::new();
    let mut seen = BTreeSet::new();
    for suite in worker_report["missing_suites"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if suites.len() >= MAX_MISSING_SUITES {
            break;
        }
        let Some(name) = suite["name"].as_str().filter(|name| {
            !name.is_empty()
                && name.len() <= MAX_SUITE_NAME_LEN
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'_' | b'-')
                })
        }) else {
            continue;
        };
        let Some(version) = suite["version"]
            .as_i64()
            .filter(|version| *version > 0 && *version <= i32::MAX as i64)
        else {
            continue;
        };
        if seen.insert((name.to_owned(), version)) {
            suites.push(json!({"name": name, "version": version}));
        }
    }
    diagnostics["missing_suites"] = Value::Array(suites);
}

fn module_audit_summary(audit: &Value) -> Option<Value> {
    let union = audit.get("observed_union")?;
    let policy = union
        .get("policy")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 260
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b' ')
                })
        })
        .take(MAX_MISSING_SUITES)
        .collect::<Vec<_>>();
    Some(json!({
        "status": audit.get("status").and_then(Value::as_str),
        "unknown_count": audit.get("unknown_count").and_then(Value::as_u64),
        "phase_count": audit.get("phase_count").and_then(Value::as_u64),
        "authorized_policy_modules": policy,
    }))
}

fn failed_module_audit_summary(stdout: &str) -> Option<Value> {
    let report: Value = serde_json::from_str(stdout.trim()).ok()?;
    if report.get("stage")? != "module_audit" {
        return None;
    }
    module_audit_summary(report.get("module_audit")?)
}

fn diagnostics_contains_gpu_stage(diagnostics: &Value) -> bool {
    diagnostics["failure_stage"]
        .as_str()
        .is_some_and(|stage| stage.starts_with("gpu_device_") || stage == "smart_render_gpu")
        || diagnostics["stage_events"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|event| {
                event["stage"].as_str().is_some_and(|stage| {
                    stage.starts_with("gpu_device_") || stage == "smart_render_gpu"
                })
            })
}

#[derive(Clone, Copy, Debug)]
pub struct RenderTiming {
    pub current_time: i32,
    pub time_step: i32,
    pub total_time: i32,
    pub time_scale: u32,
}

impl Default for RenderTiming {
    fn default() -> Self {
        Self {
            current_time: 0,
            time_step: 1,
            total_time: 1,
            time_scale: 1,
        }
    }
}

impl RenderTiming {
    fn is_valid(self) -> bool {
        self.current_time >= 0
            && self.time_step > 0
            && self.total_time >= self.current_time
            && self.time_scale > 0
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderPixelFormat {
    #[default]
    Argb8,
    Argb16,
    Argb32f,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderGpuBackend {
    #[default]
    Auto,
    Cuda,
    OpenCl,
    DirectX,
    Cpu,
}

/// Inputs produced by a GPU module-audit preflight for one render session.
/// The raw report is authenticated again immediately before worker dispatch.
pub struct GpuRuntimePolicyInput<'a> {
    pub policy: &'a RuntimeModulePolicy,
    pub module_report_json: &'a [u8],
    pub session_identity: [u8; 32],
    pub sealed_modules: &'a [ApprovedClassifiedModule],
    pub trusted_modules: &'a [ApprovedClassifiedModule],
    pub system32: &'a Path,
}

pub(crate) fn runtime_backend(backend: RenderGpuBackend) -> Option<RuntimeBackend> {
    match backend {
        RenderGpuBackend::Auto | RenderGpuBackend::Cuda => Some(RuntimeBackend::Cuda),
        RenderGpuBackend::OpenCl => Some(RuntimeBackend::Opencl),
        RenderGpuBackend::DirectX => Some(RuntimeBackend::Directx),
        RenderGpuBackend::Cpu => None,
    }
}

fn is_auto_gpu_preflight_error(error: &io::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    [
        "gpu render requires",
        "gpu infrastructure",
        "gpu backend unavailable",
        "gpu unavailable",
        "host policy",
        "runtime module",
        "gpu dispatch",
        "restricted token",
        "sealed tree acl",
        "trusted worker staging",
        "restricted process launch",
        "worker module audit validation",
        "local worker binary",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

impl RenderPixelFormat {
    pub(crate) fn report_name(self) -> &'static str {
        match self {
            Self::Argb8 => "argb8",
            Self::Argb16 => "argb16",
            Self::Argb32f => "argb32f",
        }
    }

    pub(crate) fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::Argb8 => 4,
            Self::Argb16 => 8,
            Self::Argb32f => 16,
        }
    }

    pub(crate) fn raw_extension(self) -> Option<&'static str> {
        match self {
            Self::Argb8 => None,
            Self::Argb16 => Some("rgba16le"),
            Self::Argb32f => Some("rgba32f-le"),
        }
    }
}

pub(crate) fn native_rgba_to_preview(bytes: &[u8], format: RenderPixelFormat) -> io::Result<Vec<u8>> {
    match format {
        RenderPixelFormat::Argb8 => Ok(bytes.to_vec()),
        RenderPixelFormat::Argb16 => {
            if bytes.len() % 8 != 0 {
                return Err(invalid("RGBA16 output is misaligned"));
            }
            Ok(bytes
                .chunks_exact(2)
                .map(|sample| {
                    let value = u16::from_le_bytes([sample[0], sample[1]]);
                    ((u32::from(value.min(32768)) * 255 + 16384) / 32768) as u8
                })
                .collect())
        }
        RenderPixelFormat::Argb32f => {
            if bytes.len() % 16 != 0 {
                return Err(invalid("RGBA32f output is misaligned"));
            }
            Ok(bytes
                .chunks_exact(4)
                .map(|sample| {
                    let value = f32::from_le_bytes(sample.try_into().expect("four-byte sample"));
                    if value.is_finite() {
                        (value.clamp(0.0, 1.0) * 255.0).round() as u8
                    } else {
                        0
                    }
                })
                .collect())
        }
    }
}

/// Opt-in world snapshot dumps and output checksum detail (issue #19). Both
/// default off and only take effect for broker-dispatched image renders.
const WORLD_DUMP_DIR_ENV: &str = "AEXCOMPAT_DUMP_WORLDS_DIR";
const OUTPUT_CHECKSUM_DETAIL_ENV: &str = "AEXCOMPAT_CHECKSUM_DETAIL";
const WORLD_DUMP_EXTENSIONS: [&str; 3] = [".rgba8", ".rgba16le", ".rgba32f-le"];

pub(crate) struct WorldDumpDir {
    pub(crate) path: PathBuf,
    display: String,
}

/// Opt-in crash minidump directory (issue #18), resolved for the report only.
/// The worker never receives this path or a dump-file handle: the broker
/// creates the dump file at the Windows launch boundary and hands the worker an
/// inherited pipe (see `minidump_policy`). Dumps contain plug-in memory, so
/// they stay local and the broker-owned file name is never serialized.
fn requested_minidump_directory(repository: &Path) -> io::Result<Option<String>> {
    crate::minidump_policy::configured_directory_display(repository)
}

fn requested_world_dump_dir(repository: &Path) -> io::Result<Option<WorldDumpDir>> {
    match std::env::var_os(WORLD_DUMP_DIR_ENV) {
        Some(value) => resolve_world_dump_dir(repository, Path::new(&value)).map(Some),
        None => Ok(None),
    }
}

/// Fail-closed resolution of the requested dump directory: it must resolve to
/// a broker-managed location under `<repository>/target/`, must not use
/// traversal components, and must start empty so stale snapshots can never be
/// mistaken for this run's output.
fn resolve_world_dump_dir(repository: &Path, requested: &Path) -> io::Result<WorldDumpDir> {
    resolve_managed_dump_dir(repository, requested, true)
}

pub(crate) fn resolve_managed_dump_dir(
    repository: &Path,
    requested: &Path,
    require_empty: bool,
) -> io::Result<WorldDumpDir> {
    if requested.as_os_str().is_empty() {
        return Err(invalid("world dump directory must not be empty"));
    }
    if requested.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        return Err(invalid(
            "world dump directory must not contain traversal components",
        ));
    }
    let resolved = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        repository.join(requested)
    };
    let target_root = repository.join("target");
    // Lexical pre-check before creating anything, so a rejected request never
    // leaves a directory outside the broker-managed target tree behind.
    if !resolved.starts_with(&target_root) {
        return Err(invalid(
            "world dump directory must stay under the repository target tree",
        ));
    }
    fs::create_dir_all(&resolved)?;
    let canonical = resolved.canonicalize()?;
    let canonical_target = target_root.canonicalize()?;
    if !canonical.starts_with(&canonical_target) {
        return Err(invalid(
            "world dump directory must stay under the repository target tree",
        ));
    }
    if require_empty && fs::read_dir(&canonical)?.next().is_some() {
        return Err(invalid("world dump directory must start empty"));
    }
    let display = canonical
        .strip_prefix(canonical_target.parent().unwrap_or(&canonical_target))
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "target/(world-dumps)".into());
    Ok(WorldDumpDir {
        path: canonical,
        display,
    })
}

fn output_checksum_detail_requested() -> bool {
    matches!(
        std::env::var(OUTPUT_CHECKSUM_DETAIL_ENV),
        Ok(value) if value == "1" || value.eq_ignore_ascii_case("true")
    )
}

/// Delete only the snapshot files this feature owns (NNN-<stage>-WxH.<ext>)
/// before a retry dispatch, so a fallback run cannot inherit stale dumps.
fn clear_world_dump_files(directory: &Path) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let owned = name.len() > 4
            && name.as_bytes()[..3].iter().all(u8::is_ascii_digit)
            && name.as_bytes()[3] == b'-'
            && WORLD_DUMP_EXTENSIONS
                .iter()
                .any(|extension| name.ends_with(extension));
        if owned && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// AE 16-bpc white point: ARGB16 transport samples are 0..=32768, not 0..=65535.
const AE_ARGB16_WHITE: u32 = 32768;

/// Expand AE-range RGBA16 transport bytes to full-range PNG16 samples.
/// Over-white samples are clamped exactly like the 8-bit preview path; the
/// count is returned so reports can surface them instead of hiding the clamp.
fn rgba16_transport_to_png16(bytes: &[u8]) -> io::Result<(Vec<u16>, u64)> {
    if bytes.len() % 8 != 0 {
        return Err(invalid("RGBA16 output is misaligned"));
    }
    let mut overrange_samples = 0u64;
    let samples = bytes
        .chunks_exact(2)
        .map(|sample| {
            let value = u32::from(u16::from_le_bytes([sample[0], sample[1]]));
            if value > AE_ARGB16_WHITE {
                overrange_samples += 1;
            }
            ((value.min(AE_ARGB16_WHITE) * 65535 + 16384) / 32768) as u16
        })
        .collect();
    Ok((samples, overrange_samples))
}

#[derive(Clone, Copy, Debug)]
pub enum RenderUiAction {
    Click { point: [u16; 2], color: [f32; 4] },
    Draw,
}

impl RenderUiAction {
    /// Encodes the action in the custom-UI trailer grammar shared by the
    /// one-shot argv path and the v:2 session `ui_action` field
    /// (`click:v1|x|y|r|g|b|a` / `draw:v1`). The click color is validated here
    /// (finite, in 0..=1) so both callers reject the same set before it reaches
    /// a worker.
    pub fn encode_ui_field(&self) -> io::Result<String> {
        match self {
            RenderUiAction::Click { point, color } => {
                // The worker (argv parser and the session ui_action decoder)
                // rejects x/y above 8192. Validate here so an out-of-range point
                // is a plain caller error before any transport mutation, not a
                // protocol violation that invalidates a resident session.
                if point[0] > 8192 || point[1] > 8192 {
                    return Err(invalid("custom UI render click point is out of range"));
                }
                if color
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                {
                    return Err(invalid("custom UI render click color is invalid"));
                }
                Ok(format!(
                    "click:v1|{}|{}|{}|{}|{}|{}",
                    point[0], point[1], color[0], color[1], color[2], color[3]
                ))
            }
            RenderUiAction::Draw => Ok("draw:v1".into()),
        }
    }
}

fn image_worker_command(
    smart: bool,
    pixel_format: RenderPixelFormat,
    layered: bool,
    backend: RenderGpuBackend,
) -> io::Result<&'static str> {
    let command = match (smart, pixel_format, layered, backend) {
        (true, RenderPixelFormat::Argb32f, false, RenderGpuBackend::Cuda)
        | (true, RenderPixelFormat::Argb32f, false, RenderGpuBackend::Auto) => "--smart-image32",
        (true, RenderPixelFormat::Argb32f, false, RenderGpuBackend::OpenCl) => {
            "--smart-image32-opencl"
        }
        (true, RenderPixelFormat::Argb32f, false, RenderGpuBackend::DirectX) => {
            "--smart-image32-directx"
        }
        (true, RenderPixelFormat::Argb32f, false, RenderGpuBackend::Cpu) => "--smart-image32-cpu",
        (true, RenderPixelFormat::Argb8, true, RenderGpuBackend::Auto) => "--smart-image-layer",
        (true, RenderPixelFormat::Argb16, true, RenderGpuBackend::Auto) => "--smart-image16-layer",
        (true, RenderPixelFormat::Argb32f, true, RenderGpuBackend::Auto) => "--smart-image32-layer",
        (true, RenderPixelFormat::Argb8, false, RenderGpuBackend::Auto) => "--smart-image",
        (true, RenderPixelFormat::Argb16, false, RenderGpuBackend::Auto) => "--smart-image16",
        (false, RenderPixelFormat::Argb8, true, RenderGpuBackend::Auto) => "--render-image-layer",
        (false, RenderPixelFormat::Argb16, true, RenderGpuBackend::Auto) => {
            "--render-image16-layer"
        }
        (false, RenderPixelFormat::Argb32f, true, RenderGpuBackend::Auto) => {
            "--render-image32-layer"
        }
        (false, RenderPixelFormat::Argb8, false, RenderGpuBackend::Auto) => "--render-image",
        (false, RenderPixelFormat::Argb16, false, RenderGpuBackend::Auto) => "--render-image16",
        (false, RenderPixelFormat::Argb32f, false, RenderGpuBackend::Auto) => "--render-image32",
        _ => {
            return Err(invalid(
                "explicit GPU backend requires non-layered SmartFX ARGB32f rendering",
            ))
        }
    };
    Ok(command)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InteractiveParameter {
    pub slot: u32,
    pub name: String,
    pub kind: String,
    pub minimum: f64,
    pub maximum: f64,
    pub value: f64,
    pub choices: Vec<String>,
    pub color: [u8; 4],
    pub components: [f64; 3],
    pub component_count: usize,
    pub layer_path: Option<PathBuf>,
    pub enabled: bool,
    pub visible: bool,
    pub supervised: bool,
    #[serde(default)]
    pub debug_summary: Option<String>,
    #[serde(default)]
    pub custom_ui_events: u32,
    #[serde(default)]
    pub control_size: [u16; 2],
}

#[derive(Clone, Debug)]
pub struct TimedLayerImage {
    pub slot: u32,
    pub time: AnimationTime,
    pub image_path: PathBuf,
}

fn validate_timed_layer_identities(
    timed_layers: &[TimedLayerImage],
    layer_slots: &HashSet<u32>,
) -> io::Result<()> {
    if timed_layers.len() > 64 {
        return Err(invalid(
            "timed secondary layer image count exceeds the transport limit",
        ));
    }
    for (index, layer) in timed_layers.iter().enumerate() {
        let duplicate = timed_layers[..index].iter().any(|prior| {
            prior.slot == layer.slot
                && i64::from(prior.time.value) * i64::from(layer.time.scale)
                    == i64::from(layer.time.value) * i64::from(prior.time.scale)
        });
        if layer.slot == 0
            || layer.slot > MAX_PARAMETERS
            || layer.time.scale == 0
            || !layer_slots.contains(&layer.slot)
            || duplicate
        {
            return Err(invalid(
                "timed secondary layers require a known layer slot, valid rational time, and unique slot/time",
            ));
        }
    }
    Ok(())
}

const MAX_ANIMATION_KEYS_PER_PARAMETER: usize = 256;
const MAX_ANIMATION_KEYS_TOTAL: usize = 4096;
const MAX_ANIMATION_JSON_BYTES: usize = 1024 * 1024;
const MAX_ARBITRARY_KEY_BYTES: usize = 64 * 1024;
const MAX_ARBITRARY_TOTAL_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationTime {
    pub value: i32,
    pub scale: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationInterpolation {
    Hold,
    Linear,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimationValue {
    Scalar { value: f64 },
    Color { value: [u8; 4] },
    Components { value: Vec<f64> },
    Arbitrary { value: Vec<u8> },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterAnimationKey {
    pub time: AnimationTime,
    pub interpolation: AnimationInterpolation,
    pub value: AnimationValue,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterAnimation {
    pub slot: u32,
    pub keys: Vec<ParameterAnimationKey>,
}

#[derive(Serialize)]
struct ParameterAnimationSidecar<'a> {
    schema_version: u32,
    parameters: &'a [ParameterAnimation],
}

pub fn parameter_animation_sidecar_json(animations: &[ParameterAnimation]) -> io::Result<Vec<u8>> {
    let mut slots = HashSet::new();
    let mut total_keys = 0usize;
    let mut total_arbitrary_bytes = 0usize;
    for animation in animations {
        if animation.slot == 0 || animation.slot > MAX_PARAMETERS || !slots.insert(animation.slot) {
            return Err(invalid(
                "parameter animation slots must be unique and in range",
            ));
        }
        if animation.keys.is_empty() || animation.keys.len() > MAX_ANIMATION_KEYS_PER_PARAMETER {
            return Err(invalid("parameter animation key count is out of range"));
        }
        total_keys = total_keys
            .checked_add(animation.keys.len())
            .ok_or_else(|| invalid("parameter animation key count overflow"))?;
        if total_keys > MAX_ANIMATION_KEYS_TOTAL {
            return Err(invalid("total parameter animation key count exceeds limit"));
        }
        let mut previous: Option<AnimationTime> = None;
        for key in &animation.keys {
            if key.time.scale == 0 {
                return Err(invalid("parameter animation time scale must be nonzero"));
            }
            if let Some(prior) = previous {
                let left = i64::from(prior.value)
                    .checked_mul(i64::from(key.time.scale))
                    .ok_or_else(|| invalid("parameter animation time comparison overflow"))?;
                let right = i64::from(key.time.value)
                    .checked_mul(i64::from(prior.scale))
                    .ok_or_else(|| invalid("parameter animation time comparison overflow"))?;
                if left >= right {
                    return Err(invalid(
                        "parameter animation times must be strictly ascending",
                    ));
                }
            }
            match &key.value {
                AnimationValue::Scalar { value } if !value.is_finite() => {
                    return Err(invalid("parameter animation scalar must be finite"));
                }
                AnimationValue::Components { value }
                    if value.is_empty()
                        || value.len() > 3
                        || value.iter().any(|component| !component.is_finite()) =>
                {
                    return Err(invalid("parameter animation components are invalid"));
                }
                AnimationValue::Arbitrary { value }
                    if value.is_empty() || value.len() > MAX_ARBITRARY_KEY_BYTES =>
                {
                    return Err(invalid("parameter animation arbitrary key is invalid"));
                }
                AnimationValue::Arbitrary { value } => {
                    total_arbitrary_bytes = total_arbitrary_bytes
                        .checked_add(value.len())
                        .ok_or_else(|| {
                            invalid("parameter animation arbitrary byte count overflow")
                        })?;
                    if total_arbitrary_bytes > MAX_ARBITRARY_TOTAL_BYTES {
                        return Err(invalid(
                            "parameter animation arbitrary bytes exceed total limit",
                        ));
                    }
                }
                _ => {}
            }
            previous = Some(key.time);
        }
        let has_arbitrary = animation
            .keys
            .iter()
            .any(|key| matches!(key.value, AnimationValue::Arbitrary { .. }));
        if has_arbitrary
            && animation
                .keys
                .iter()
                .any(|key| !matches!(key.value, AnimationValue::Arbitrary { .. }))
        {
            return Err(invalid(
                "arbitrary parameter animation keys cannot mix value types",
            ));
        }
    }
    let bytes = serde_json::to_vec(&ParameterAnimationSidecar {
        schema_version: 1,
        parameters: animations,
    })
    .map_err(|error| invalid(format!("parameter animation JSON failed: {error}")))?;
    if bytes.len() > MAX_ANIMATION_JSON_BYTES {
        return Err(invalid("parameter animation JSON exceeds byte limit"));
    }
    Ok(bytes)
}

pub(crate) fn validate_animation_bindings(
    parameters: &[InteractiveParameter],
    animations: &[ParameterAnimation],
) -> io::Result<()> {
    let mut parameter_slots = HashSet::new();
    for parameter in parameters {
        if !parameter_slots.insert(parameter.slot) {
            return Err(invalid("interactive parameter slots must be unique"));
        }
    }
    for animation in animations {
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.slot == animation.slot)
            .ok_or_else(|| invalid("parameter animation references an unknown slot"))?;
        for key in &animation.keys {
            let compatible = match (&key.value, parameter.kind.as_str()) {
                (AnimationValue::Scalar { value }, "integer" | "path") => {
                    value.fract() == 0.0
                        && *value >= parameter.minimum
                        && *value <= parameter.maximum
                }
                (AnimationValue::Scalar { value }, "float") => {
                    *value >= parameter.minimum && *value <= parameter.maximum
                }
                (AnimationValue::Color { .. }, "color") => true,
                (AnimationValue::Components { value }, "angle") => value.len() == 1,
                (AnimationValue::Components { value }, "point") => value.len() == 2,
                (AnimationValue::Components { value }, "point3d") => value.len() == 3,
                // Parameter discovery reports PF arbitrary parameters as
                // "arbitrary_data" (see interactive parameter kinds); there is
                // no "arbitrary" kind anywhere in the transport.
                (AnimationValue::Arbitrary { .. }, "arbitrary_data") => true,
                _ => false,
            };
            if !compatible {
                return Err(invalid(
                    "parameter animation value does not match its parameter slot",
                ));
            }
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

const MAX_AUX_CHANNELS: usize = 16;
const MAX_AUX_CHANNELS_PER_PARAM: usize = 8;
const MAX_AUX_SAMPLES_PER_CHANNEL: usize = 64;
const MAX_AUX_SAMPLES: usize = 256;
const MAX_AUX_SAMPLE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_AUX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

struct AuxTransport {
    manifest_path: PathBuf,
    _cleanup: Cleanup,
}

const AUX_CHANNEL_DEPTH: i32 = i32::from_be_bytes(*b"DPTH");
const AUX_CHANNEL_NORMALS: i32 = i32::from_be_bytes(*b"NRML");
const AUX_CHANNEL_MOTION_VECTORS: i32 = i32::from_be_bytes(*b"MTVR");

fn aux_type_dimension_matches(
    channel_type: i32,
    dimension: u8,
    interpretation: crate::render_request::AuxInterpretation,
) -> bool {
    use crate::render_request::AuxInterpretation;
    match interpretation {
        AuxInterpretation::Depth => channel_type == AUX_CHANNEL_DEPTH && dimension == 1,
        AuxInterpretation::Normals => channel_type == AUX_CHANNEL_NORMALS && dimension == 3,
        AuxInterpretation::MotionVectors => {
            channel_type == AUX_CHANNEL_MOTION_VECTORS && dimension == 2
        }
        AuxInterpretation::Generic => {
            !matches!(
                channel_type,
                AUX_CHANNEL_DEPTH | AUX_CHANNEL_NORMALS | AUX_CHANNEL_MOTION_VECTORS
            ) && (1..=4).contains(&dimension)
        }
    }
}

/// Strip the Windows `\\?\` (or `\\?\UNC\`) extended-length prefix from a path.
///
/// `Path::canonicalize()` — which every repository/transport root passes through
/// — returns verbatim `\\?\C:\...` paths on Windows. The worker's aux-manifest
/// loader rejects such paths: it gates each declared path on
/// `std::filesystem::absolute(p).lexically_normal() == std::filesystem::canonical(p)`,
/// and MSVC's `canonical` drops the `\\?\` prefix while `absolute` keeps it, so a
/// verbatim path never matches its own canonical form and the render exits 3
/// (issue #231). The broker already de-verbatims paths handed to the worker for
/// minidump and trace targets (`strip_extended_prefix` in minidump_policy.rs /
/// trace_policy.rs); the aux manifest and its sidecars must follow suit. The
/// string form is identity on any path without the prefix, so this is a no-op on
/// non-Windows and on already-plain paths.
fn strip_extended_prefix(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn prepare_aux_transport(
    repository: &Path,
    channels: &[crate::render_request::AuxChannel],
    root: &Path,
    nonce: u128,
) -> io::Result<Option<AuxTransport>> {
    if channels.is_empty() {
        return Ok(None);
    }
    if channels.len() > MAX_AUX_CHANNELS {
        return Err(invalid("aux channel count exceeds 16"));
    }
    // The worker's aux loader requires each declared path to equal its own
    // canonical form; a `\\?\` verbatim root (from `Path::canonicalize()`) fails
    // that gate, so hand the manifest and every sidecar plain absolute paths.
    let root = strip_extended_prefix(root);
    let root = root.as_path();
    let allowed_root = repository.canonicalize()?;
    let mut per_param = std::collections::BTreeMap::<u32, usize>::new();
    let mut channel_keys = BTreeSet::new();
    let mut source_paths = HashSet::new();
    let mut sample_count = 0usize;
    let mut total_bytes = 0u64;
    let manifest_path = root.join(format!("aux-manifest-{nonce}.json"));
    let mut cleanup = Cleanup(vec![manifest_path.clone()]);
    let mut manifest_channels = Vec::with_capacity(channels.len());

    for (channel_index, item) in channels.iter().enumerate() {
        let count = per_param.entry(item.param_index).or_default();
        *count += 1;
        if *count > MAX_AUX_CHANNELS_PER_PARAM {
            return Err(invalid("aux channels per parameter exceed 8"));
        }
        let channel = &item.channel;
        if !(1..=4).contains(&channel.dimension)
            || channel.width == 0
            || channel.height == 0
            || channel.width > MAX_DIMENSION
            || channel.height > MAX_DIMENSION
            || channel.name.as_bytes().len() > 63
            || channel.name.as_bytes().contains(&0)
        {
            return Err(invalid("aux channel descriptor is invalid"));
        }
        if !channel_keys.insert((item.param_index, channel.channel_type)) {
            return Err(invalid("duplicate aux channel key"));
        }
        if channel.samples.is_empty() || channel.samples.len() > MAX_AUX_SAMPLES_PER_CHANNEL {
            return Err(invalid("aux sample count per channel is outside 1..64"));
        }
        let pixels = u64::from(channel.width)
            .checked_mul(u64::from(channel.height))
            .ok_or_else(|| invalid("aux pixel count overflows"))?;
        if pixels > MAX_PIXELS {
            return Err(invalid("aux pixel count exceeds 16777216"));
        }
        let packed_row_bytes = u64::from(channel.width)
            .checked_mul(u64::from(channel.dimension))
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| invalid("aux row size overflows"))?;
        let native_row_bytes = channel.row_bytes.unwrap_or(
            i32::try_from(packed_row_bytes).map_err(|_| invalid("aux row size exceeds i32"))?,
        );
        let absolute_row_bytes = native_row_bytes
            .checked_abs()
            .ok_or_else(|| invalid("aux row stride overflows"))?
            as u64;
        if absolute_row_bytes < packed_row_bytes
            || channel.downsample_x.numerator <= 0
            || channel.downsample_x.denominator == 0
            || channel.downsample_y.numerator <= 0
            || channel.downsample_y.denominator == 0
            || channel.coordinate_space.is_empty()
            || channel.coordinate_space.len() > 64
            || channel.units.is_empty()
            || channel.units.len() > 64
        {
            return Err(invalid("aux native plane descriptor is invalid"));
        }
        let sample_bytes = absolute_row_bytes
            .checked_mul(u64::from(channel.height))
            .ok_or_else(|| invalid("aux sample size overflows"))?;
        if sample_bytes > MAX_AUX_SAMPLE_BYTES {
            return Err(invalid("aux sample exceeds 256 MiB"));
        }
        let mut times = BTreeSet::new();
        let mut manifest_samples = Vec::with_capacity(channel.samples.len());
        for (sample_index, sample) in channel.samples.iter().enumerate() {
            sample_count = sample_count
                .checked_add(1)
                .ok_or_else(|| invalid("aux sample count overflows"))?;
            if sample_count > MAX_AUX_SAMPLES
                || sample.time_scale == 0
                || !times.insert((sample.time, sample.time_scale))
                || !aux_type_dimension_matches(
                    channel.channel_type,
                    channel.dimension,
                    sample.interpretation,
                )
            {
                return Err(invalid("aux sample metadata is invalid or duplicated"));
            }
            total_bytes = total_bytes
                .checked_add(sample_bytes)
                .ok_or_else(|| invalid("aux total size overflows"))?;
            if total_bytes > MAX_AUX_TOTAL_BYTES {
                return Err(invalid("aux transport exceeds 512 MiB"));
            }
            if sample.path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            }) {
                return Err(invalid("aux sample path traversal forbidden"));
            }
            let requested = if sample.path.is_absolute() {
                sample.path.clone()
            } else {
                repository.join(&sample.path)
            };
            let canonical = requested
                .canonicalize()
                .map_err(|_| invalid("aux sample path is unavailable"))?;
            if !canonical.starts_with(&allowed_root) || !source_paths.insert(canonical.clone()) {
                return Err(invalid(
                    "aux sample path escapes its root or aliases another sample",
                ));
            }
            let bytes = fs::read(&canonical)?;
            if bytes.len() as u64 != sample_bytes {
                return Err(invalid(
                    "aux sample byte length does not match its dimensions",
                ));
            }
            if bytes.chunks_exact(4).any(|value| {
                !f32::from_le_bytes(value.try_into().expect("four-byte float")).is_finite()
            }) {
                return Err(invalid("aux sample contains a non-finite float"));
            }
            let raw_path = root.join(format!("aux-{nonce}-{channel_index}-{sample_index}.f32le"));
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&raw_path)?
                .write_all(&bytes)?;
            cleanup.0.push(raw_path.clone());
            let expected_sha256 = format!("{:x}", Sha256::digest(&bytes));
            let written = fs::read(&raw_path)?;
            if written.len() as u64 != sample_bytes
                || format!("{:x}", Sha256::digest(&written)) != expected_sha256
            {
                return Err(invalid("aux sidecar verification failed after write"));
            }
            manifest_samples.push(json!({
                "time": sample.time, "time_scale": sample.time_scale,
                "path": raw_path, "sampling": sample.sampling,
                "interpretation": sample.interpretation,
                "expected_byte_length": sample_bytes,
                "sha256": expected_sha256,
            }));
        }
        manifest_channels.push(json!({
            "param_index": item.param_index, "type": channel.channel_type,
            "name": channel.name, "data_type": "f32le", "dimension": channel.dimension,
            "width": channel.width, "height": channel.height, "samples": manifest_samples,
            "row_bytes": native_row_bytes, "origin_x": channel.origin_x,
            "origin_y": channel.origin_y, "downsample_x_num": channel.downsample_x.numerator,
            "downsample_x_den": channel.downsample_x.denominator,
            "downsample_y_num": channel.downsample_y.numerator,
            "downsample_y_den": channel.downsample_y.denominator,
            "coordinate_space": channel.coordinate_space, "units": channel.units,
        }));
    }
    let manifest = json!({
        "schema": "aux-manifest-v1",
        "nonce": nonce.to_string(),
        "channels": manifest_channels,
    });
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path)?;
    serde_json::to_writer_pretty(&mut output, &manifest)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    Ok(Some(AuxTransport {
        manifest_path,
        _cleanup: cleanup,
    }))
}

pub fn render_image(
    repository: &Path,
    plugin_id: &str,
    input_path: &Path,
    output_path: &Path,
) -> io::Result<Value> {
    if plugin_id != "scattermap" {
        return Err(invalid(
            "image rendering currently supports only scattermap",
        ));
    }
    let profile =
        crate::fixture_profiles::find(plugin_id).ok_or_else(|| invalid("unknown profile"))?;
    profile
        .classic_worker
        .ok_or_else(|| invalid("classic render is unavailable"))?;
    let approved = crate::render::entry(repository, plugin_id)?;
    let manifest = load_manifest(repository, plugin_id, profile.descriptor_manifest)?;
    if !manifest
        .plugin_sha256
        .eq_ignore_ascii_case(&approved.sha256)
    {
        return Err(invalid("descriptor and approved artifact digests differ"));
    }
    let payload = encode_worker_payload(
        &manifest.profile,
        &apply_defaults(&manifest.profile, &ValidatedAssignments::new()),
    )
    .map_err(invalid)?;
    render_with_artifact(
        repository,
        plugin_id,
        &approved.plugin_path,
        &approved.sha256,
        approved.timeout_ms,
        input_path,
        output_path,
        Some(payload),
        None,
        None,
        RenderTiming::default(),
        false,
        RenderPixelFormat::Argb8,
        RenderGpuBackend::Auto,
        None,
        None,
        None,
        None,
        Vec::new(),
        None,
        false,
    )
}

pub fn render_experimental_image(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    render_experimental_image_at_time(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        RenderTiming::default(),
    )
}

enum AudioWrapperOutcome {
    /// The length-1 audio session rendered and closed clean; this is the public
    /// report, satisfying the same contract the one-shot path asserts.
    Report(Value),
    /// The session carried the render but the render itself failed the way the
    /// one-shot path also fails it (a per-span compatibility error, or a
    /// post-render output/write failure). The one-shot `--render-audio` transport
    /// returns Err for the same input, so this is final, not a fallback.
    Failure(io::Error),
    /// The session infrastructure could not carry the render (open failure,
    /// worker crash or invalidation, malformed close). The string is the reason;
    /// the automatic one-shot fallback is removed (#98 W4, #264), so the caller
    /// turns this into an explicit fail-closed error rather than silently
    /// rerunning the one-shot transport. The one-shot transport stays reachable
    /// only via the AEXCOMPAT_DISABLE_RENDER_SESSION_WRAPPER override.
    Fallback(String),
}

/// Renders a single audio buffer through a length-1 AudioRenderSession (§10),
/// so the one-shot `--render-audio` argv transport is a fallback rather than
/// the only path (issue #98 W4 / #239). The session's own validation (guards,
/// checksum, generation, and the clean-close gate on the worker's audio
/// report) enforces the same contract the one-shot report is checked against,
/// so the synthesized report carries the asserted fields.
#[cfg(windows)]
fn render_audio_via_length_one_session(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input: &[u8],
    output_path: &Path,
    parameters: &[InteractiveParameter],
) -> AudioWrapperOutcome {
    use crate::render_session::{AudioRenderSession, AudioSessionOpenRequest, AudioSpanStatus};
    // Test-only fault injection (debug builds only); a no-op in release.
    if let Some(outcome) = forced_audio_session_fallback() {
        return outcome;
    }
    const SAMPLE_RATE: u32 = 44_100;
    let samples: Vec<f32> = input
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    let mut session = match AudioRenderSession::open(AudioSessionOpenRequest {
        repository,
        plugin_path,
        plugin_sha256: approved_sha256,
        parameters: Some(parameters),
        dependencies: Vec::new(),
        max_samples: samples.len() as u32,
        channels: 1,
        time_scale: SAMPLE_RATE,
        frame_deadline: Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    }) {
        Ok(session) => session,
        Err(error) => {
            return AudioWrapperOutcome::Fallback(format!("audio session open failed: {error}"));
        }
    };
    let outcome = match session.render_span(0, &samples) {
        Ok(outcome) => outcome,
        // Invalidation (worker crash, deadline, or a host-protection invariant).
        Err(error) => {
            let reason = format!("the audio session was invalidated: {error}");
            let _ = session.close();
            return AudioWrapperOutcome::Fallback(reason);
        }
    };
    let (output, output_start) = match outcome.status {
        AudioSpanStatus::Rendered {
            samples,
            output_start,
            ..
        } => (samples, output_start),
        // A per-span compatibility error: the one-shot `--render-audio` path
        // returns Err for the same input (its worker exits non-zero and the
        // report gate rejects a non-"render_completed" status), so this is a
        // final compatibility failure, not a session-infrastructure fallback.
        // Fail closed with the error the effect reported (#98 W4, #264).
        AudioSpanStatus::SpanError { render_error } => {
            let _ = session.close();
            return AudioWrapperOutcome::Failure(invalid(format!(
                "the audio render reported a compatibility error (audio_render_error {render_error})"
            )));
        }
    };
    let close = session.close();
    if close.get("session_clean") != Some(&Value::Bool(true))
        || close.get("invalidated") != Some(&Value::Bool(false))
    {
        return AudioWrapperOutcome::Fallback(
            "the audio session did not close cleanly".into(),
        );
    }
    if output_path.exists() {
        return AudioWrapperOutcome::Failure(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "output audio already exists",
        ));
    }
    let mut destination = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
    {
        Ok(file) => file,
        Err(error) => return AudioWrapperOutcome::Failure(error),
    };
    if let Err(error) = destination
        .write_all(&output)
        .and_then(|_| destination.sync_all())
    {
        drop(destination);
        let _ = fs::remove_file(output_path);
        return AudioWrapperOutcome::Failure(error);
    }
    // Preserve the one-shot audio report schema: start from the worker's
    // aggregate audio report (which carries the audio_* telemetry the SDK audio
    // contract consumes, at parity with emit_audio_render_report) and add or
    // override the fields render_experimental_audio's one-shot path exposes, so
    // a Windows caller on the default session path sees the same public shape.
    let Some(mut report) = close
        .get("final_report")
        .and_then(Value::as_object)
        .cloned()
    else {
        return AudioWrapperOutcome::Fallback(
            "the audio session close carried no final report".into(),
        );
    };
    // Override/add the fields the one-shot emit_audio_render_report exposes so
    // the default session path returns the same public JSON shape as the
    // `--render-audio` fallback. The wrapper only reaches here on a clean close
    // (every span succeeded), so the selector errors are 0, the ranges valid,
    // and the output was created.
    for (key, value) in [
        ("stage", json!("audio_render")),
        ("status", json!("render_completed")),
        ("sample_rate", json!(SAMPLE_RATE)),
        ("channels", json!(1)),
        ("sample_format", json!("float32")),
        ("audio_setup_error", json!(0)),
        ("audio_render_error", json!(0)),
        ("audio_setdown_error", json!(0)),
        ("setup_range_valid", json!(true)),
        ("guard_bytes_intact", json!(true)),
        ("samples_finite", json!(true)),
        ("input_samples", json!(input.len() / 4)),
        ("output_start_sample", json!(output_start)),
        ("output_samples", json!(output.len() / 4)),
        ("output_created", json!(true)),
        ("input_sha256", json!(format!("{:x}", Sha256::digest(input)))),
        ("output_sha256", json!(format!("{:x}", Sha256::digest(&output)))),
        ("output_transport", json!("mono_f32le_44100")),
        ("render_path", json!("audio_session")),
    ] {
        report.insert(key.to_owned(), value);
    }
    // Preserve the one-shot path's top-level `worker_diagnostics`
    // (classification / stage events) rather than only nesting it under
    // session_close, so a caller on the default session path keeps that field.
    let worker_diagnostics = close
        .get("worker")
        .and_then(|worker| worker.get("diagnostics"))
        .cloned()
        .unwrap_or(Value::Null);
    report.insert("worker_diagnostics".to_owned(), worker_diagnostics);
    report.insert("session_close".to_owned(), close);
    AudioWrapperOutcome::Report(Value::Object(report))
}

pub fn render_experimental_audio(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    const SAMPLE_RATE: u32 = 44_100;
    const MAX_SAMPLES: usize = 10_000_000;
    let plugin_bytes = fs::read(plugin_path)?;
    let actual = format!("{:X}", Sha256::digest(&plugin_bytes));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    if output_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "output audio already exists",
        ));
    }
    let input = fs::read(input_path)?;
    if input.is_empty() || input.len() % 4 != 0 || input.len() / 4 > MAX_SAMPLES {
        return Err(invalid(
            "audio input must contain 1..10000000 float32 samples",
        ));
    }
    if input
        .chunks_exact(4)
        .any(|bytes| !f32::from_le_bytes(bytes.try_into().unwrap()).is_finite())
    {
        return Err(invalid("audio input contains a non-finite sample"));
    }

    // Route the render through a length-1 audio session by default (protocol
    // §10); the one-shot `--render-audio` argv transport below is the fallback
    // when the session infrastructure cannot carry it. The escape hatch
    // (AEXCOMPAT_DISABLE_RENDER_SESSION_WRAPPER=1) forces the one-shot path.
    #[cfg(windows)]
    if std::env::var_os(DISABLE_SESSION_WRAPPER_ENV).is_none() {
        match render_audio_via_length_one_session(
            repository,
            plugin_path,
            approved_sha256,
            &input,
            output_path,
            parameters,
        ) {
            AudioWrapperOutcome::Report(report) => return Ok(report),
            AudioWrapperOutcome::Failure(error) => return Err(error),
            // Fail closed (#98 W4, #264): an audio-session infrastructure failure
            // no longer silently falls back to the one-shot transport. Surface it
            // as an explicit diagnostic. The one-shot transport stays reachable
            // via the AEXCOMPAT_DISABLE_RENDER_SESSION_WRAPPER override, which
            // bypasses the session attempt entirely.
            AudioWrapperOutcome::Fallback(reason) => {
                return Err(invalid(format!(
                    "the resident audio render session could not carry this render ({reason}); \
                     the automatic one-shot fallback is disabled. Diagnose the session failure, \
                     or set {DISABLE_SESSION_WRAPPER_ENV}=1 to force the one-shot transport for \
                     this render."
                )));
            }
        }
    }

    let transport = repository.join("target/audio-transport");
    fs::create_dir_all(&transport)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("system clock is before UNIX epoch"))?
        .as_nanos();
    let worker_input = transport.join(format!("input-{nonce}.f32"));
    let worker_output = transport.join(format!("output-{nonce}.f32"));
    fs::write(&worker_input, &input)?;
    let _cleanup = Cleanup(vec![worker_input.clone(), worker_output.clone()]);
    let args_before_plugin = vec!["--render-audio".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
        worker_input.to_string_lossy().into_owned(),
        worker_output.to_string_lossy().into_owned(),
        (input.len() / 4).to_string(),
        SAMPLE_RATE.to_string(),
    ];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Render,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "audio render worker failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("audio render worker report is invalid"))?;
    let output_samples = report
        .get("output_samples")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("audio report has no output sample count"))?
        as usize;
    let output = fs::read(&worker_output)?;
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("sample_rate") != Some(&json!(SAMPLE_RATE))
        || report.get("channels") != Some(&json!(1))
        || report.get("sample_format") != Some(&json!("float32"))
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("samples_finite") != Some(&json!(true))
        || report.get("audio_lifetimes_balanced") != Some(&json!(true))
        || report.get("invalid_audio_operations") != Some(&json!(0))
        || output_samples > input.len() / 4
        || output.len() != output_samples * 4
    {
        return Err(invalid("audio worker contract failed"));
    }
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    if let Err(error) = destination
        .write_all(&output)
        .and_then(|_| destination.sync_all())
    {
        drop(destination);
        let _ = fs::remove_file(output_path);
        return Err(error);
    }
    report["input_sha256"] = json!(format!("{:x}", Sha256::digest(&input)));
    report["output_sha256"] = json!(format!("{:x}", Sha256::digest(&output)));
    report["output_transport"] = json!("mono_f32le_44100");
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn render_experimental_image_at_time(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
) -> io::Result<Value> {
    render_experimental_image_at_time_with_format(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        false,
        RenderPixelFormat::Argb8,
    )
}

pub fn render_experimental_image_at_time_with_format(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
) -> io::Result<Value> {
    render_experimental_image_at_time_with_format_and_context(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        smart,
        pixel_format,
        None,
    )
}

pub fn render_experimental_image_at_time_with_format_and_context(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    host_context: Option<&crate::render_request::HostContext>,
) -> io::Result<Value> {
    render_experimental_image_at_time_with_format_context_and_click(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        smart,
        pixel_format,
        host_context,
        None,
    )
}

pub fn render_experimental_image_at_time_with_format_context_and_click(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    host_context: Option<&crate::render_request::HostContext>,
    custom_ui_click: Option<([u16; 2], [f32; 4])>,
) -> io::Result<Value> {
    render_experimental_image_at_time_with_format_context_and_ui_action(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        smart,
        pixel_format,
        host_context,
        custom_ui_click.map(|(point, color)| RenderUiAction::Click { point, color }),
    )
}

pub fn render_experimental_image_at_time_with_format_context_and_ui_action(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    host_context: Option<&crate::render_request::HostContext>,
    custom_ui_action: Option<RenderUiAction>,
) -> io::Result<Value> {
    render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        smart,
        pixel_format,
        host_context,
        custom_ui_action,
        RenderGpuBackend::Auto,
    )
}

pub fn render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    host_context: Option<&crate::render_request::HostContext>,
    custom_ui_action: Option<RenderUiAction>,
    gpu_backend: RenderGpuBackend,
) -> io::Result<Value> {
    render_experimental_image_with_approved_dependencies(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        smart,
        pixel_format,
        host_context,
        custom_ui_action,
        gpu_backend,
        Vec::new(),
    )
}

pub fn render_experimental_image_with_approved_dependencies(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    host_context: Option<&crate::render_request::HostContext>,
    custom_ui_action: Option<RenderUiAction>,
    gpu_backend: RenderGpuBackend,
    dependencies: Vec<ApprovedImageArtifact>,
) -> io::Result<Value> {
    render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        smart,
        pixel_format,
        host_context,
        custom_ui_action,
        gpu_backend,
        dependencies,
        None,
    )
}

pub fn render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    host_context: Option<&crate::render_request::HostContext>,
    custom_ui_action: Option<RenderUiAction>,
    gpu_backend: RenderGpuBackend,
    dependencies: Vec<ApprovedImageArtifact>,
    gpu_runtime_policy: Option<GpuRuntimePolicyInput<'_>>,
) -> io::Result<Value> {
    let bytes = fs::read(plugin_path)?;
    let actual = format!("{:X}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    render_with_artifact(
        repository,
        "experimental",
        plugin_path,
        &actual,
        INTERACTIVE_RENDER_TIMEOUT_MS,
        input_path,
        output_path,
        Some(encode_interactive_payload(parameters)?),
        Some(parameters),
        host_context,
        timing,
        smart,
        pixel_format,
        gpu_backend,
        custom_ui_action,
        None,
        None,
        None,
        dependencies,
        gpu_runtime_policy,
        false,
    )
}

/// Argb16 render whose output PNG keeps 16-bit depth: the worker's AE-range
/// RGBA16 transport is expanded to full-range RGBA16 PNG samples instead of
/// being quantized to the 8-bit preview. The depth-preserving raw sidecar is
/// written exactly as in the preview path.
pub fn render_experimental_image_at_time_with_deep16_png(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
) -> io::Result<Value> {
    let bytes = fs::read(plugin_path)?;
    let actual = format!("{:X}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    render_with_artifact(
        repository,
        "experimental",
        plugin_path,
        &actual,
        INTERACTIVE_RENDER_TIMEOUT_MS,
        input_path,
        output_path,
        Some(encode_interactive_payload(parameters)?),
        Some(parameters),
        None,
        timing,
        smart,
        RenderPixelFormat::Argb16,
        RenderGpuBackend::Auto,
        None,
        None,
        None,
        None,
        Vec::new(),
        None,
        true,
    )
}

pub fn render_experimental_image_with_timed_layers(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timed_layers: &[TimedLayerImage],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
) -> io::Result<Value> {
    let bytes = fs::read(plugin_path)?;
    let actual = format!("{:X}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    render_with_artifact(
        repository,
        "experimental-timed-layers",
        plugin_path,
        &actual,
        INTERACTIVE_RENDER_TIMEOUT_MS,
        input_path,
        output_path,
        Some(encode_interactive_payload(parameters)?),
        Some(parameters),
        None,
        timing,
        smart,
        pixel_format,
        RenderGpuBackend::Auto,
        None,
        None,
        None,
        Some(timed_layers),
        Vec::new(),
        None,
        false,
    )
}

pub fn render_experimental_image_with_parameter_animation(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    animations: &[ParameterAnimation],
    timing: RenderTiming,
) -> io::Result<Value> {
    parameter_animation_sidecar_json(animations)?;
    validate_animation_bindings(parameters, animations)?;
    let bytes = fs::read(plugin_path)?;
    let actual = format!("{:X}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    // Issue #227: encode the base payload from the same parameters the session
    // wrapper reconstructs (`render_session.rs` builds `encode_interactive_payload`
    // from the parameter set). A fixed `"v5|"` placeholder never matched that
    // reconstruction, so the length-1 session gate in `render_with_artifact`
    // (`payload == encode_interactive_payload(interactive_parameters)`) always
    // failed and parameter animation was forced onto the one-shot argv path.
    // Deriving the payload here lets the gate hold so animation rides the session
    // transport, and keeps the one-shot fallback byte-identical: the animation
    // sidecar overwrites every animated slot's value per frame, and an empty
    // payload is version-agnostic to the worker, so the only observable change
    // is that a non-animated parameter's declared value is now honored instead
    // of dropped (matching every other interactive entrypoint).
    render_with_artifact(
        repository,
        "experimental-parameter-animation",
        plugin_path,
        &actual,
        INTERACTIVE_RENDER_TIMEOUT_MS,
        input_path,
        output_path,
        Some(encode_interactive_payload(parameters)?),
        Some(parameters),
        None,
        timing,
        false,
        RenderPixelFormat::Argb8,
        RenderGpuBackend::Auto,
        None,
        None,
        Some(animations),
        None,
        Vec::new(),
        None,
        false,
    )
}

pub fn render_experimental_image_with_audio_sidecar(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    audio_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
) -> io::Result<Value> {
    let bytes = fs::read(plugin_path)?;
    let actual = format!("{:X}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    render_with_artifact(
        repository,
        "experimental-audio-sidecar",
        plugin_path,
        &actual,
        INTERACTIVE_RENDER_TIMEOUT_MS,
        input_path,
        output_path,
        Some(encode_interactive_payload(parameters)?),
        Some(parameters),
        None,
        timing,
        false,
        RenderPixelFormat::Argb8,
        RenderGpuBackend::Auto,
        None,
        Some(audio_path),
        None,
        None,
        Vec::new(),
        None,
        false,
    )
}

pub fn render_experimental_smart_image(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    render_experimental_smart_image_at_time(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        RenderTiming::default(),
    )
}

pub fn render_experimental_smart_image_at_time(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
) -> io::Result<Value> {
    render_experimental_image_at_time_with_format(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        true,
        RenderPixelFormat::Argb8,
    )
}

pub fn inspect_experimental(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Vec<InteractiveParameter>> {
    inspect_experimental_with_diagnostics(repository, plugin_path, approved_sha256)
        .map(|(parameters, _)| parameters)
}

pub fn inspect_experimental_external_dependencies(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    missing_only: bool,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let check_type = if missing_only { 2 } else { 1 };
    let args_before_plugin = vec!["--l2-external-dependencies".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), check_type.to_string()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "external dependency inspection failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("external dependency worker report is invalid"))?;
    let handle_returned = report
        .get("handle_returned")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if report.get("status") != Some(&json!("dependencies_inspected"))
        || report.get("check_type") != Some(&json!(check_type))
        || report.get("selector_error") != Some(&json!(0))
        || report.get("exception_code") != Some(&json!(0))
        || report.get("handle_valid") != Some(&json!(true))
        || report.get("nul_terminated") != Some(&json!(true))
        || report.get("handle_host_disposed") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || (!missing_only && !handle_returned)
    {
        return Err(invalid("external dependency worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_options_dialog(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-do-dialog".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "options dialog probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("options dialog worker report is invalid"))?;
    if report.get("status") != Some(&json!("dialog_completed"))
        || report.get("dialog_advertised") != Some(&json!(true))
        || report.get("selector_dispatched") != Some(&json!(true))
        || report.get("selector_error") != Some(&json!(0))
        || report.get("exception_code") != Some(&json!(0))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("global_setdown_error") != Some(&json!(0))
    {
        return Err(invalid("options dialog worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_automatic_options_dialog(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-auto-dialog".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "automatic options dialog probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("automatic options dialog worker report is invalid"))?;
    if report.get("status") != Some(&json!("automatic_dialog_completed"))
        || report.get("dialog_capability_advertised") != Some(&json!(true))
        || report.get("automatic_dialog_requested") != Some(&json!(true))
        || report.get("selector_dispatched") != Some(&json!(true))
        || report.get("sequence_setup_error") != Some(&json!(0))
        || report.get("sequence_setup_exception_code") != Some(&json!(0))
        || report.get("dialog_error") != Some(&json!(0))
        || report.get("dialog_exception_code") != Some(&json!(0))
        || report.get("sequence_setdown_error") != Some(&json!(0))
        || report.get("sequence_setdown_exception_code") != Some(&json!(0))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("global_setdown_error") != Some(&json!(0))
    {
        return Err(invalid("automatic options dialog worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_nop_render(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "default".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Render,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "NOP_RENDER probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("NOP_RENDER worker report is invalid"))?;
    let input_hash = report.get("input_sha256").and_then(Value::as_str);
    let output_hash = report.get("output_sha256").and_then(Value::as_str);
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("nop_render_advertised") != Some(&json!(true))
        || report.get("render_selector_dispatched") != Some(&json!(false))
        || report.get("render_performed") != Some(&json!(false))
        || report.get("render_error") != Some(&json!(0))
        || input_hash.is_none()
        || input_hash != output_hash
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("world_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
        || report.get("global_setdown_error") != Some(&json!(0))
    {
        return Err(invalid("NOP_RENDER worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_smart_nop_render(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--smart".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "default".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Smart,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "SmartFX NOP_RENDER probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("SmartFX NOP_RENDER worker report is invalid"))?;
    let input_hash = report.get("input_sha256").and_then(Value::as_str);
    let output_hash = report.get("output_sha256").and_then(Value::as_str);
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("smart_render_supported") != Some(&json!(true))
        || report.get("nop_render_advertised") != Some(&json!(true))
        || report.get("smart_pre_render_dispatched") != Some(&json!(false))
        || report.get("smart_render_selector_dispatched") != Some(&json!(false))
        || report.get("render_performed") != Some(&json!(false))
        || report.get("pre_render_error") != Some(&json!(0))
        || report.get("smart_render_error") != Some(&json!(0))
        || input_hash.is_none()
        || input_hash != output_hash
        || report.get("result_rects_valid") != Some(&json!(true))
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("world_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
        || report.get("global_setdown_error") != Some(&json!(0))
    {
        return Err(invalid("SmartFX NOP_RENDER worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_input_buffer_write(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "default".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Render,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "input-buffer write probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("input-buffer write worker report is invalid"))?;
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("input_write_advertised") != Some(&json!(true))
        || report.get("input_buffer_writable") != Some(&json!(true))
        || report.get("render_selector_dispatched") != Some(&json!(true))
        || report.get("render_error") != Some(&json!(0))
        || report.get("input_sha256") == report.get("output_sha256")
        || report.get("output_sha256")
            != Some(&json!(
                "6232b2fc98e93754b771a0bf22a5b3c81bc0bce580499dbc4fb32ad0490e2b5a"
            ))
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("world_lifetimes_balanced") != Some(&json!(true))
        || report.get("global_setdown_error") != Some(&json!(0))
    {
        return Err(invalid("input-buffer write worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_smart_input_buffer_write(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--smart".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "default".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Smart,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "SmartFX input-buffer write probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("SmartFX input-buffer write worker report is invalid"))?;
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("smart_render_supported") != Some(&json!(true))
        || report.get("input_write_advertised") != Some(&json!(true))
        || report.get("input_buffer_writable") != Some(&json!(true))
        || report.get("smart_pre_render_dispatched") != Some(&json!(true))
        || report.get("smart_render_selector_dispatched") != Some(&json!(true))
        || report.get("pre_render_error") != Some(&json!(0))
        || report.get("smart_render_error") != Some(&json!(0))
        || report.get("input_sha256") != report.get("output_sha256")
        || report.get("output_sha256")
            != Some(&json!(
                "6232b2fc98e93754b771a0bf22a5b3c81bc0bce580499dbc4fb32ad0490e2b5a"
            ))
        || report.get("result_rects_valid") != Some(&json!(true))
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("world_lifetimes_balanced") != Some(&json!(true))
        || report.get("global_setdown_error") != Some(&json!(0))
    {
        return Err(invalid("SmartFX input-buffer write worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

#[derive(Clone, Copy)]
enum FrameResizeDirection {
    Expand,
    Shrink,
}

fn probe_experimental_frame_resize(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    direction: FrameResizeDirection,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "default".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Render,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "FRAME_SETUP resize probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("FRAME_SETUP resize worker report is invalid"))?;
    let width = report
        .get("width")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let height = report
        .get("height")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let direction_valid = match direction {
        FrameResizeDirection::Expand => {
            report.get("expand_buffer_advertised") == Some(&json!(true))
                && (width > 16 || height > 12)
        }
        FrameResizeDirection::Shrink => {
            report.get("shrink_buffer_advertised") == Some(&json!(true))
                && width > 0
                && height > 0
                && (width < 16 || height < 12)
        }
    };
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("render_error") != Some(&json!(0))
        || report.get("render_selector_dispatched") != Some(&json!(true))
        || !direction_valid
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("world_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
        || report.get("global_setdown_error") != Some(&json!(0))
    {
        return Err(invalid("FRAME_SETUP resize worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_expand_buffer(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    probe_experimental_frame_resize(
        repository,
        plugin_path,
        approved_sha256,
        FrameResizeDirection::Expand,
    )
}

pub fn probe_experimental_shrink_buffer(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    probe_experimental_frame_resize(
        repository,
        plugin_path,
        approved_sha256,
        FrameResizeDirection::Shrink,
    )
}

pub fn probe_experimental_persistent_sequence(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "persistent_sequence".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Render,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "persistent sequence probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("persistent sequence worker report is invalid"))?;
    let frame_errors = report
        .get("persistent_frame_errors")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("persistent sequence report has no frame results"))?;
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("persistent_sequence") != Some(&json!(true))
        || report.get("persistent_sequence_setup_error") != Some(&json!(0))
        || report.get("persistent_sequence_setdown_error") != Some(&json!(0))
        || frame_errors.len() != 2
        || frame_errors.iter().any(|error| error != &json!(0))
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("world_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
        || report.get("param_checkouts_balanced") != Some(&json!(true))
    {
        return Err(invalid("persistent sequence worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_flattened_sequence(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "flattened_sequence".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Render,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "flattened sequence probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("flattened sequence worker report is invalid"))?;
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("flattened_sequence") != Some(&json!(true))
        || report.get("sequence_flatten_error") != Some(&json!(0))
        || report.get("sequence_resetup_error") != Some(&json!(0))
        || report.get("flattened_handle_replaced") != Some(&json!(true))
        || report.get("resetup_handle_replaced") != Some(&json!(true))
        || report.get("flattened_handle_host_disposed") != Some(&json!(true))
        || report.get("persistent_sequence_setdown_error") != Some(&json!(0))
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("world_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
        || report.get("pf_path_lifetimes_balanced") != Some(&json!(true))
    {
        return Err(invalid("flattened sequence worker contract failed"));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn probe_experimental_copied_flattened_sequence(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        "copied_flattened_sequence".into(),
    ];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Render,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "non-destructive sequence save probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("non-destructive sequence save report is invalid"))?;
    if report.get("status") != Some(&json!("render_completed"))
        || report.get("copied_flattened_sequence") != Some(&json!(true))
        || report.get("get_flattened_sequence_data_error") != Some(&json!(0))
        || report.get("original_sequence_preserved") != Some(&json!(true))
        || report.get("flattened_handle_host_disposed") != Some(&json!(true))
        || report.get("persistent_sequence_setdown_error") != Some(&json!(0))
        || report.get("guard_bytes_intact") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
        || report.get("pf_path_lifetimes_balanced") != Some(&json!(true))
    {
        return Err(invalid(
            "non-destructive sequence save worker contract failed",
        ));
    }
    report["worker_diagnostics"] = diagnostics;
    Ok(report)
}

pub fn inspect_experimental_with_diagnostics(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    inspect_experimental_with_diagnostics_and_runtime_policy(
        repository,
        plugin_path,
        approved_sha256,
        Vec::new(),
        None,
    )
}

pub fn inspect_experimental_with_approved_dependencies(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    dependencies: Vec<ApprovedImageArtifact>,
) -> io::Result<Vec<InteractiveParameter>> {
    inspect_experimental_with_diagnostics_and_runtime_policy(
        repository,
        plugin_path,
        approved_sha256,
        dependencies,
        None,
    )
    .map(|(parameters, _)| parameters)
}

pub fn inspect_experimental_with_approved_dependencies_and_diagnostics(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    dependencies: Vec<ApprovedImageArtifact>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    inspect_experimental_with_diagnostics_and_runtime_policy(
        repository,
        plugin_path,
        approved_sha256,
        dependencies,
        None,
    )
}

pub fn inspect_experimental_with_runtime_policy(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    policy: &RuntimeModulePolicy,
    backend: RuntimeBackend,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    if backend == RuntimeBackend::Cpu {
        return Err(invalid(
            "runtime-authorized inspection requires a GPU backend",
        ));
    }
    inspect_experimental_with_diagnostics_and_runtime_policy(
        repository,
        plugin_path,
        approved_sha256,
        Vec::new(),
        Some((policy, backend)),
    )
}

fn inspect_experimental_with_diagnostics_and_runtime_policy(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    mut dependencies: Vec<ApprovedImageArtifact>,
    runtime_policy: Option<(&RuntimeModulePolicy, RuntimeBackend)>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-params-only".into()];
    let mut args_after_plugin = vec![actual.to_ascii_lowercase()];
    let authorization = runtime_policy
        .map(|(policy, backend)| {
            prepare_runtime_authorization_transport(repository, policy, backend)
        })
        .transpose()?;
    if let Some(authorization) = &authorization {
        args_after_plugin.push("--runtime-module-authorization-v1".into());
        args_after_plugin.push(authorization.basename.clone());
    }
    let started = Instant::now();
    if let Some(authorization) = &authorization {
        dependencies.push(authorization.artifact.clone());
    }
    let isolated = if !dependencies.is_empty() {
        dispatch_approved_image_with_dependencies(
            repository,
            WorkerKind::L2,
            plugin_path,
            approved_sha256,
            dependencies,
            &args_before_plugin,
            &args_after_plugin,
            Duration::from_millis(5_000),
        )?
    } else {
        dispatch_approved_image(
            repository,
            WorkerKind::L2,
            plugin_path,
            approved_sha256,
            &args_before_plugin,
            &args_after_plugin,
            Duration::from_millis(5_000),
        )?
    };
    let mut diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    let worker_report: Option<Value> = serde_json::from_str(isolated.stdout.trim()).ok();
    if let Some(report) = &worker_report {
        propagate_missing_suites(&mut diagnostics, report);
    }
    if isolated.classification.as_str() != "ok" {
        if let Some(summary) = failed_module_audit_summary(&isolated.stdout) {
            diagnostics["module_audit_failure"] = summary;
        }
        return Err(invalid(format!(
            "AEX parameter inspection worker failed safely: {diagnostics}"
        )));
    }
    let report = worker_report.ok_or_else(|| invalid("inspection worker report is invalid"))?;
    if let Some(summary) = report.get("module_audit").and_then(module_audit_summary) {
        diagnostics["module_audit"] = summary;
    }
    let advertised_out_flags = report.get("out_flags").and_then(Value::as_u64).unwrap_or(0);
    let advertised_out_flags2 = report
        .get("out_flags2")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let audio_effect_only = advertised_out_flags & (1_u64 << 31) != 0;
    diagnostics["advertised_out_flags"] = json!(advertised_out_flags);
    diagnostics["advertised_out_flags2"] = json!(advertised_out_flags2);
    diagnostics["smart_render_advertised"] = json!(smart_render_advertised(advertised_out_flags2));
    diagnostics["audio_effect_only"] = json!(audio_effect_only);
    diagnostics["image_render_supported"] = json!(!audio_effect_only);
    diagnostics["runtime_module_policy_applied"] = json!(runtime_policy.is_some());
    if report.get("params_setup_error") != Some(&json!(0)) {
        return Err(invalid("AEX rejected PF_PARAMS_SETUP"));
    }
    let rows = report
        .get("parameters")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("inspection report has no parameters"))?;
    let mut parameters = Vec::new();
    let mut parameter_metadata = Vec::new();
    let custom_ui_events = report
        .get("custom_ui")
        .and_then(|value| value.get("events"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    for row in rows {
        let observed_type = row
            .get("type")
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid("inspection parameter has no numeric type"))?;
        let observed_index = row
            .get("index")
            .and_then(Value::as_u64)
            .filter(|value| *value <= u64::from(u16::MAX))
            .ok_or_else(|| invalid("inspection parameter has no bounded index"))?;
        let default = row.get("default").and_then(Value::as_f64).unwrap_or(0.0);
        let ui_flags = row.get("ui_flags").and_then(Value::as_u64).unwrap_or(0);
        let default_color = row.get("default_color");
        let channel = |name: &str| {
            default_color
                .and_then(|value| value.get(name))
                .and_then(Value::as_u64)
                .unwrap_or(if name == "alpha" { 255 } else { 0 }) as u8
        };
        let known_metadata_kind = match observed_type {
            0 => "layer",
            1 => "slider",
            2 => "fixed_slider",
            3 => "angle",
            4 => "checkbox",
            5 => "color",
            6 => "point",
            7 => "popup",
            8 => "custom",
            9 => "no_data",
            10 => "float_slider",
            11 => "arbitrary_data",
            12 => "path",
            13 => "group_start",
            14 => "group_end",
            15 => "button",
            18 => "point3d",
            16 => "reserved16",
            17 => "reserved17",
            _ => "",
        };
        let metadata_kind = if known_metadata_kind.is_empty() {
            format!("unknown_{observed_type}")
        } else {
            known_metadata_kind.to_owned()
        };
        let runtime_kind = match observed_type {
            0 => "layer",
            3 => "angle",
            5 => "color",
            6 => "point",
            2 | 10 => "float",
            8 => "custom",
            9 => "no_data",
            11 => "arbitrary_data",
            12 => "path",
            13 => "group_start",
            14 => "group_end",
            15 => "button",
            18 => "point3d",
            _ => "integer",
        };
        let host_minimum = if observed_type == 12 {
            0.0
        } else {
            row.get("valid_min")
                .and_then(Value::as_f64)
                .unwrap_or(default)
        };
        let host_maximum = if observed_type == 12 {
            1024.0
        } else {
            row.get("valid_max")
                .and_then(Value::as_f64)
                .unwrap_or(default)
        };
        let observed_host_range = row.get("valid_min").and_then(Value::as_f64).zip(
            row.get("valid_max").and_then(Value::as_f64),
        );
        let observed_user_range = row.get("slider_min").and_then(Value::as_f64).zip(
            row.get("slider_max").and_then(Value::as_f64),
        );
        let component_count = match observed_type {
            3 => 1,
            6 => 2,
            18 => 3,
            _ => 0,
        };
        let initial_value = if let Some(value) = row.get("default").and_then(Value::as_f64) {
            json!(value)
        } else if observed_type == 5 {
            let color = row.get("default_color");
            ["alpha", "red", "green", "blue"]
                .iter()
                .map(|name| color.and_then(|value| value.get(name)).and_then(Value::as_u64))
                .collect::<Option<Vec<_>>>()
                .map(Value::from)
                .unwrap_or(Value::Null)
        } else if component_count > 0 {
            row
                .get("default_components")
                .and_then(Value::as_array)
                .filter(|values| values.len() >= component_count)
                .and_then(|values| {
                    values
                        .iter()
                        .take(component_count)
                        .map(Value::as_f64)
                        .collect::<Option<Vec<_>>>()
                })
                .map(Value::from)
                .unwrap_or(Value::Null)
        } else if observed_type == 0 {
            row.get("layer_default").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        parameter_metadata.push(json!({
            "index": observed_index,
            "type": metadata_kind,
            "initial_value": initial_value,
            "host_range": observed_host_range.map(|(minimum, maximum)| json!({"minimum": minimum, "maximum": maximum})),
            "user_range": observed_user_range.map(|(minimum, maximum)| json!({"minimum": minimum, "maximum": maximum}))
        }));
        if !matches!(
            observed_type,
            0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 18
        ) {
            continue;
        }
        parameters.push(InteractiveParameter {
            slot: observed_index as u32,
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Parameter")
                .to_owned(),
            kind: runtime_kind.into(),
            minimum: host_minimum,
            maximum: host_maximum,
            value: default,
            choices: row
                .get("choices")
                .and_then(Value::as_str)
                .map(|text| text.split('|').map(str::to_owned).collect())
                .unwrap_or_default(),
            color: [
                channel("alpha"),
                channel("red"),
                channel("green"),
                channel("blue"),
            ],
            components: {
                let mut result = [0.0; 3];
                if let Some(values) = row.get("default_components").and_then(Value::as_array) {
                    for (index, value) in values.iter().take(3).enumerate() {
                        result[index] = value.as_f64().unwrap_or(0.0);
                    }
                }
                result
            },
            component_count,
            layer_path: None,
            enabled: ui_flags & (1 << 5) == 0,
            visible: ui_flags & (1 << 9) == 0,
            supervised: row.get("flags").and_then(Value::as_u64).unwrap_or(0) & (1 << 6) != 0,
            debug_summary: row
                .get("arbitrary_summary")
                .and_then(Value::as_str)
                .map(str::to_owned),
            custom_ui_events,
            control_size: [
                row.get("ui_width").and_then(Value::as_u64).unwrap_or(0) as u16,
                row.get("ui_height").and_then(Value::as_u64).unwrap_or(0) as u16,
            ],
        });
    }
    diagnostics["parameter_metadata"] = Value::Array(parameter_metadata);
    Ok((parameters, diagnostics))
}

pub fn probe_experimental_custom_ui_cursor(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-adjust-cursor".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI cursor worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI cursor report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("cursor") != Some(&json!(13))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI cursor contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_draw(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-draw-event".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI draw worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI draw report is invalid"))?;
    let command_count = report
        .get("drawbot_paint_rect_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + report
            .get("drawbot_fill_path_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
        + report
            .get("drawbot_stroke_path_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
        + report
            .get("overlay_stroke_path_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0);
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || command_count == 0
        || report.get("drawbot_objects_created") != report.get("drawbot_objects_released")
        || report.get("drawbot_invalid_operations") != Some(&json!(0))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI draw contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_lifecycle(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-ui-lifecycle".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI lifecycle worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI lifecycle report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("ui_lifecycle"))
        || report.get("lifecycle_errors") != Some(&json!([0, 0, 0, 0, -1]))
        || report.get("lifecycle_context_stable") != Some(&json!(true))
        || report.get("lifecycle_host_state_cleared") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI lifecycle contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_idle(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-ui-idle".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI idle worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI idle report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("ui_idle"))
        || report.get("lifecycle_errors") != Some(&json!([0, 0, 0, 0, 0]))
        || report.get("lifecycle_context_stable") != Some(&json!(true))
        || report.get("lifecycle_host_state_cleared") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI idle contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_keydown(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    point: [u16; 2],
    keycode: u32,
    modifiers: u16,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if keycode & 0x3fff_0000 != 0 {
        return Err(invalid("custom UI keycode contains unsupported bits"));
    }
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-ui-keydown".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        format!("{},{},{},{}", point[0], point[1], keycode, modifiers),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI keydown worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI keydown report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("ui_keydown"))
        || report.get("lifecycle_errors") != Some(&json!([0, 0, 0, 0, 0]))
        || report.get("keydown_code") != Some(&json!(keycode))
        || report.get("keydown_modifiers") != Some(&json!(modifiers))
        || report.get("lifecycle_context_stable") != Some(&json!(true))
        || report.get("lifecycle_host_state_cleared") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI keydown contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_mouse_exited(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-ui-mouse-exited".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI mouse-exited worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI mouse-exited report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("ui_mouse_exited"))
        || !matches!(
            report.get("event_target").and_then(Value::as_str),
            Some("layer" | "comp")
        )
        || report.get("lifecycle_errors") != Some(&json!([0, 0, 0, 0, 0]))
        || report.get("lifecycle_context_stable") != Some(&json!(true))
        || report.get("lifecycle_host_state_cleared") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI mouse-exited contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_click(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    point: [u16; 2],
    color: [f32; 4],
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if color
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(invalid("custom UI click color is invalid"));
    }
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let payload = format!(
        "{},{},{},{},{},{}",
        point[0], point[1], color[0], color[1], color[2], color[3]
    );
    let args_before_plugin = vec!["--l2-click-event".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        payload,
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI click worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI click report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("do_click"))
        || report
            .get("event_out_flags")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            & 9
            != 9
        || report.get("app_color_picker_calls") != Some(&json!(1))
        || report.get("app_invalidate_rect_calls") != Some(&json!(1))
        || report.get("changed_value") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI click contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_drag(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    start: [u16; 2],
    end: [u16; 2],
    steps: u8,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if steps == 0 || steps > 32 {
        return Err(invalid("custom UI drag step count is invalid"));
    }
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--l2-drag-event".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        format!("{},{},{},{},{}", start[0], start[1], end[0], end[1], steps),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(8_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI drag worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI drag report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("drag_sequence"))
        || report.get("drag_requested") != Some(&json!(true))
        || report.get("drag_calls") != Some(&json!(steps))
        || report.get("drag_terminated") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI drag contract failed"));
    }
    Ok(report)
}

pub fn trigger_experimental_button(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    slot: u32,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if slot == 0 || slot > MAX_PARAMETERS {
        return Err(invalid("button parameter slot is invalid"));
    }
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    if !parameters
        .iter()
        .any(|parameter| parameter.slot == slot && parameter.supervised)
    {
        return Err(invalid("parameter is not supervised by the AEX"));
    }
    let payload = encode_interactive_payload(parameters)?;
    let args_before_plugin = vec!["--user-changed".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), slot.to_string(), payload];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEX button worker failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("button worker report is invalid"))?;
    if report.get("user_changed_param_requested") != Some(&json!(true))
        || report.get("user_changed_param_slot") != Some(&json!(slot))
        || report.get("user_changed_param_error") != Some(&json!(0))
    {
        return Err(invalid("AEX rejected PF_Cmd_USER_CHANGED_PARAM"));
    }
    Ok(report)
}

pub fn initialize_experimental_aegp(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-init".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP initialization failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP worker report is invalid"))?;
    if report.get("stage") != Some(&json!("aegp_init"))
        || report.get("init_error") != Some(&json!(0))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP initialization contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_update_menu(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-update-menu".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP update-menu event failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP event worker report is invalid"))?;
    if report.get("event_requested") != Some(&json!("update_menu"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("hooks_invoked").and_then(Value::as_u64) == Some(0)
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP update-menu event contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_idle(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-idle".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP idle event failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP idle worker report is invalid"))?;
    if report.get("event_requested") != Some(&json!("idle"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("hooks_invoked").and_then(Value::as_u64) == Some(0)
        || report
            .get("idle_max_sleep")
            .and_then(Value::as_i64)
            .unwrap_or(-1)
            < 0
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP idle event contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_command_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-command-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP command roundtrip failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP command worker report is invalid"))?;
    if report.get("event_requested") != Some(&json!("command_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("command_hooks_invoked").and_then(Value::as_u64) != Some(2)
        || report.get("command_handled_count").and_then(Value::as_u64) != Some(2)
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP command roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_active_idle_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-active-idle-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP active idle roundtrip failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP active idle report is invalid"))?;
    if report.get("event_requested") != Some(&json!("active_idle_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("command_hooks_invoked").and_then(Value::as_u64) != Some(2)
        || report.get("command_handled_count").and_then(Value::as_u64) != Some(2)
        || report.get("hooks_invoked").and_then(Value::as_u64) != Some(1)
        || report.get("idle_max_sleep").and_then(Value::as_i64) != Some(33)
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP active idle roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_comp_idle_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-comp-idle-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(5_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP comp idle roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP comp idle report is invalid"))?;
    let effect_count_calls = report
        .get("effect_count_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let effect_contract_valid = effect_count_calls == 0
        || (report
            .get("effect_acquires")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            && report.get("effect_acquires") == report.get("effect_disposes")
            && report
                .get("effect_metadata_calls")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                >= 4);
    let stream_acquires = report
        .get("stream_acquires")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let scene_layer_count = report
        .get("scene_layer_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let idle_ticks = 3;
    let effect_param_value_calls = report
        .get("effect_param_value_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let expected_streams = if stream_acquires > 0 {
        idle_ticks * scene_layer_count * 7 + effect_param_value_calls
    } else {
        0
    };
    let layer_name_calls = report
        .get("layer_name_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let item_name_calls = report
        .get("item_name_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let effect_param_name_calls = report
        .get("effect_param_name_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let item_metadata_contract_valid = item_name_calls == 0
        || (item_name_calls == idle_ticks
            && report.get("item_duration_calls").and_then(Value::as_u64) == Some(idle_ticks));
    let layer_name_contract_valid = layer_name_calls == 0
        || (layer_name_calls == idle_ticks * scene_layer_count
            && item_name_calls == idle_ticks
            && report.get("aegp_memory_created").and_then(Value::as_u64)
                == Some(layer_name_calls * 2 + item_name_calls + effect_param_name_calls)
            && report.get("aegp_memory_freed") == report.get("aegp_memory_created"));
    let stream_contract_valid = stream_acquires == expected_streams
        && (stream_acquires == 0
            || (effect_param_value_calls > 0
                && effect_param_value_calls <= 31
                && effect_param_name_calls == effect_param_value_calls
                && report.get("stream_acquires") == report.get("stream_disposes")
                && report.get("stream_value_acquires") == report.get("stream_value_disposes")
                && report.get("stream_value_acquires").and_then(Value::as_u64)
                    == Some(expected_streams)
                && report.get("keyframe_count_calls").and_then(Value::as_u64)
                    == Some(expected_streams)
                && report
                    .get("keyframed_stream_reports")
                    .and_then(Value::as_u64)
                    == Some(1)
                && report
                    .get("stream_sampled_selector_mask")
                    .and_then(Value::as_u64)
                    == Some(799)));
    if report.get("event_requested") != Some(&json!("comp_idle_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("command_hooks_invoked").and_then(Value::as_u64) != Some(2)
        || report.get("command_handled_count").and_then(Value::as_u64) != Some(2)
        || report.get("menu_hooks_invoked").and_then(Value::as_u64) != Some(4)
        || report.get("command_enable_calls").and_then(Value::as_u64) != Some(4)
        || report.get("command_check_calls").and_then(Value::as_u64) != Some(4)
        || report
            .get("command_checked_true_calls")
            .and_then(Value::as_u64)
            != Some(3)
        || report
            .get("command_checked_false_calls")
            .and_then(Value::as_u64)
            != Some(1)
        || report.get("hooks_invoked").and_then(Value::as_u64) != Some(idle_ticks)
        || scene_layer_count != 3
        || report
            .get("scene_selected_layer_count")
            .and_then(Value::as_u64)
            != Some(2)
        || report.get("idle_max_sleep").and_then(Value::as_i64) != Some(33)
        || report
            .get("scene_first_observed_frame")
            .and_then(Value::as_i64)
            != Some(1)
        || report
            .get("scene_last_observed_frame")
            .and_then(Value::as_i64)
            != Some(3)
        || report
            .get("item_current_time_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            == 0
        || report
            .get("comp_from_item_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            == 0
        || report
            .get("comp_framerate_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            == 0
        || (report
            .get("layer_count_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            && report
                .get("layer_attribute_calls")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                < idle_ticks * scene_layer_count * 4)
        || report
            .get("suite_acquires")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            < 4
        || report.get("suite_leases_balanced") != Some(&json!(true))
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || !effect_contract_valid
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || !layer_name_contract_valid
        || !item_metadata_contract_valid
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || (report
            .get("collection_creates")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            && (report.get("collection_creates") != report.get("collection_disposes")
                || report.get("collection_creates").and_then(Value::as_u64) != Some(idle_ticks)
                || report.get("collection_item_reads").and_then(Value::as_u64)
                    != Some(idle_ticks * 2)))
        || !stream_contract_valid
    {
        return Err(invalid("AEGP comp idle roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_keyframe_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-keyframe-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(8_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP keyframe roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP keyframe roundtrip report is invalid"))?;
    if report.get("event_requested") != Some(&json!("keyframe_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("keyframe_pipe_connected") != Some(&json!(true))
        || report.get("keyframe_pipe_request_sent") != Some(&json!(true))
        || report.get("keyframe_pipe_response_received") != Some(&json!(true))
        || report.get("keyframe_pipe_response_valid") != Some(&json!(true))
        || report
            .get("keyframe_pipe_response_bytes")
            .and_then(Value::as_u64)
            != Some(192)
        || report.get("keyframe_time_calls").and_then(Value::as_u64) != Some(2)
        || report.get("keyframe_value_calls").and_then(Value::as_u64) != Some(2)
        || report
            .get("keyframe_interpolation_calls")
            .and_then(Value::as_u64)
            != Some(2)
        || report
            .get("keyframed_stream_reports")
            .and_then(Value::as_u64)
            != Some(2)
        || report.get("stream_acquires").and_then(Value::as_u64) != Some(71)
        || report.get("stream_disposes") != report.get("stream_acquires")
        || report.get("stream_value_acquires").and_then(Value::as_u64) != Some(69)
        || report.get("stream_value_disposes") != report.get("stream_value_acquires")
        || report.get("aegp_memory_created").and_then(Value::as_u64) != Some(26)
        || report.get("aegp_memory_freed") != report.get("aegp_memory_created")
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP keyframe roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_seek_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-seek-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(8_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP seek roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP seek roundtrip report is invalid"))?;
    if report.get("event_requested") != Some(&json!("seek_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("seek_pipe_connected") != Some(&json!(true))
        || report.get("seek_pipe_request_sent") != Some(&json!(true))
        || report.get("seek_pipe_ack_received") != Some(&json!(true))
        || report.get("seek_pipe_ack_valid") != Some(&json!(true))
        || report
            .get("item_set_current_time_calls")
            .and_then(Value::as_u64)
            != Some(1)
        || report
            .get("item_last_set_time_value")
            .and_then(Value::as_i64)
            != Some(75)
        || report
            .get("item_last_set_time_scale")
            .and_then(Value::as_u64)
            != Some(30)
        || report.get("scene_current_frame").and_then(Value::as_i64) != Some(75)
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP seek roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_trim_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-trim-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(8_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP trim roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP trim roundtrip report is invalid"))?;
    if report.get("event_requested") != Some(&json!("trim_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("trim_pipe_connected") != Some(&json!(true))
        || report.get("trim_pipe_request_sent") != Some(&json!(true))
        || report.get("trim_pipe_ack_received") != Some(&json!(true))
        || report.get("trim_pipe_ack_valid") != Some(&json!(true))
        || report.get("layer_trim_set_calls").and_then(Value::as_u64) != Some(1)
        || report.get("layer_1_in_point_value").and_then(Value::as_i64) != Some(30)
        || report.get("layer_1_in_point_scale").and_then(Value::as_u64) != Some(30)
        || report.get("layer_1_duration_value").and_then(Value::as_i64) != Some(210)
        || report.get("layer_1_duration_scale").and_then(Value::as_u64) != Some(30)
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP trim roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_switch_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = format!("{:X}", Sha256::digest(fs::read(plugin_path)?));
    if !actual.eq_ignore_ascii_case(approved_sha256) {
        return Err(invalid("selected AEX changed after session approval"));
    }
    let args_before_plugin = vec!["--aegp-switch-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Duration::from_millis(8_000),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP switch roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP switch roundtrip report is invalid"))?;
    if report.get("event_requested") != Some(&json!("switch_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("switch_pipe_connected") != Some(&json!(true))
        || report.get("switch_pipe_request_sent") != Some(&json!(true))
        || report.get("switch_pipe_ack_received") != Some(&json!(true))
        || report.get("switch_pipe_ack_valid") != Some(&json!(true))
        || report.get("layer_flag_set_calls").and_then(Value::as_u64) != Some(4)
        || report.get("layer_1_flags").and_then(Value::as_u64) != Some(0x4026)
        || report.get("layer_2_flags").and_then(Value::as_u64) != Some(0x5)
        || report.get("layer_3_flags").and_then(Value::as_u64) != Some(0x5)
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP switch roundtrip contract failed"));
    }
    Ok(report)
}

/// Encodes interactive parameters into the worker payload transport form
/// (`v2|`..`v5|`). Public so integration tests can compute the exact payload
/// a session frame carries; production callers stay inside the crate.
pub fn encode_interactive_payload(parameters: &[InteractiveParameter]) -> io::Result<String> {
    let parameters = parameters
        .iter()
        .filter(|item| {
            !matches!(
                item.kind.as_str(),
                "layer" | "group_start" | "group_end" | "button" | "custom" | "no_data"
            )
        })
        .collect::<Vec<_>>();
    let mut payload = if parameters.iter().any(|item| item.kind == "arbitrary_data") {
        "v5|".to_owned()
    } else if parameters
        .iter()
        .any(|item| matches!(item.kind.as_str(), "angle" | "point" | "point3d"))
    {
        "v4|".to_owned()
    } else if parameters.iter().any(|item| item.kind == "color") {
        "v3|".to_owned()
    } else {
        "v2|".to_owned()
    };
    for (index, item) in parameters.iter().enumerate() {
        if item.slot == 0
            || item.slot > MAX_PARAMETERS
            || !item.value.is_finite()
            || item.value < item.minimum
            || item.value > item.maximum
        {
            return Err(invalid("interactive parameter is out of range"));
        }
        if index != 0 {
            payload.push(';');
        }
        let id = format!("param_{}", item.slot);
        match item.kind.as_str() {
            "integer" | "path" if item.value.fract() == 0.0 => {
                payload.push_str(&format!("{id}@{}:i32={}", item.slot, item.value as i64))
            }
            "float" => payload.push_str(&format!("{id}@{}:f64={}", item.slot, item.value)),
            "color" => payload.push_str(&format!(
                "{id}@{}:argb8={},{},{},{}",
                item.slot, item.color[0], item.color[1], item.color[2], item.color[3]
            )),
            "angle" | "point" | "point3d" => {
                let expected = match item.kind.as_str() {
                    "angle" => 1,
                    "point" => 2,
                    _ => 3,
                };
                if item.component_count != expected
                    || item.components[..expected]
                        .iter()
                        .any(|value| !value.is_finite() || *value < -32768.0 || *value > 32768.0)
                {
                    return Err(invalid("interactive component parameter is invalid"));
                }
                let values = item.components[..expected]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                payload.push_str(&format!("{id}@{}:{}={values}", item.slot, item.kind));
            }
            "arbitrary_data" => {
                let text = item
                    .debug_summary
                    .as_deref()
                    .ok_or_else(|| invalid("arbitrary parameter has no printable text"))?;
                if text.is_empty() || text.len() > 4096 || text.as_bytes().contains(&0) {
                    return Err(invalid("arbitrary parameter text is invalid"));
                }
                let encoded = text
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                payload.push_str(&format!("{id}@{}:arbhex={encoded}", item.slot));
            }
            _ => return Err(invalid("unsupported interactive parameter kind")),
        }
    }
    if payload.len() > 16384 {
        return Err(invalid("interactive parameter payload is too large"));
    }
    Ok(payload)
}

fn render_with_artifact(
    repository: &Path,
    plugin_id: &str,
    plugin_path: &Path,
    plugin_sha256: &str,
    timeout_ms: u64,
    input_path: &Path,
    output_path: &Path,
    payload_override: Option<String>,
    interactive_parameters: Option<&[InteractiveParameter]>,
    host_context: Option<&crate::render_request::HostContext>,
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    gpu_backend: RenderGpuBackend,
    custom_ui_action: Option<RenderUiAction>,
    audio_sidecar: Option<&Path>,
    parameter_animation: Option<&[ParameterAnimation]>,
    timed_layers: Option<&[TimedLayerImage]>,
    dependencies: Vec<ApprovedImageArtifact>,
    gpu_runtime_policy: Option<GpuRuntimePolicyInput<'_>>,
    deep_png_output: bool,
) -> io::Result<Value> {
    if !timing.is_valid() {
        return Err(invalid("render timing is invalid"));
    }
    if deep_png_output && pixel_format != RenderPixelFormat::Argb16 {
        return Err(invalid(
            "16-bit deep PNG output requires the Argb16 render format",
        ));
    }
    const MAX_AUDIO_SAMPLES: usize = 10_000_000;
    let audio = if let Some(path) = audio_sidecar {
        if smart
            || pixel_format != RenderPixelFormat::Argb8
            || host_context.is_some()
            || custom_ui_action.is_some()
        {
            return Err(invalid(
                "audio sidecar currently requires plain classic ARGB8 rendering",
            ));
        }
        let bytes = fs::read(path)?;
        if bytes.is_empty() || bytes.len() % 4 != 0 || bytes.len() / 4 > MAX_AUDIO_SAMPLES {
            return Err(invalid(
                "audio sidecar must contain 1..10000000 float32 samples",
            ));
        }
        if bytes.chunks_exact(4).any(|sample| {
            !f32::from_le_bytes(sample.try_into().expect("four-byte sample")).is_finite()
        }) {
            return Err(invalid("audio sidecar contains a non-finite sample"));
        }
        Some(bytes)
    } else {
        None
    };
    let spatial = host_context.and_then(|context| context.spatial).unwrap_or(
        crate::render_request::SpatialContext {
            downsample_x: crate::render_request::RationalScale {
                numerator: 1,
                denominator: 1,
            },
            downsample_y: crate::render_request::RationalScale {
                numerator: 1,
                denominator: 1,
            },
            pixel_aspect_ratio: crate::render_request::RationalScale {
                numerator: 1,
                denominator: 1,
            },
            full_resolution_width: None,
            full_resolution_height: None,
            pre_effect_source_origin_x: None,
            pre_effect_source_origin_y: None,
        },
    );
    let render_environment = host_context
        .and_then(|context| context.render_environment)
        .unwrap_or(crate::render_request::RenderEnvironment {
            quality: crate::render_request::RenderQuality::High,
            field: crate::render_request::RenderField::Frame,
            shutter_angle: 0.0,
            shutter_phase: 0.0,
        });
    let expected_quality = match render_environment.quality {
        crate::render_request::RenderQuality::Low => 0,
        crate::render_request::RenderQuality::High => 1,
    };
    let expected_field = match render_environment.field {
        crate::render_request::RenderField::Frame => 0,
        crate::render_request::RenderField::Upper => 1,
        crate::render_request::RenderField::Lower => 2,
    };
    let expected_shutter_angle = (render_environment.shutter_angle * 65536.0).round() as i32;
    let expected_shutter_phase = (render_environment.shutter_phase * 65536.0).round() as i32;
    if output_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "output image already exists",
        ));
    }
    let preserved_output = pixel_format
        .raw_extension()
        .map(|extension| output_path.with_extension(extension));
    if preserved_output.as_ref().is_some_and(|path| path.exists()) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "depth-preserving output already exists",
        ));
    }
    let conformance_render_settings = conformance_render_settings_transport()?;
    let conformance_premultiplication = conformance_render_settings
        .as_deref()
        .map(|settings| settings.split('|').nth(1).expect("validated settings mode"));
    let decoded = decode_bounded_image(input_path, "input")?;
    let (width, height) = (decoded.width(), decoded.height());
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(invalid("input dimensions exceed the ARGB8 harness limit"));
    }
    let mut rgba = decoded.into_rgba8().into_raw();
    if let Some(mode) = conformance_premultiplication {
        apply_conformance_premultiplication(&mut rgba, mode);
    }
    crate::render_request::validate_image_buffer_layout(
        u64::from(width),
        u64::from(height),
        u64::from(width) * 4,
        4,
        Some(rgba.len() as u64),
        u64::from(MAX_DIMENSION),
        MAX_PIXELS,
        MAX_RGBA_TRANSPORT_BYTES,
    )?;

    let selected_layers = interactive_parameters
        .unwrap_or_default()
        .iter()
        .filter(|item| item.kind == "layer" && item.layer_path.is_some())
        .collect::<Vec<_>>();
    if selected_layers.len() > 8 {
        return Err(invalid("secondary layer count exceeds the transport limit"));
    }
    if selected_layers.iter().enumerate().any(|(index, layer)| {
        selected_layers[..index]
            .iter()
            .any(|prior| prior.slot == layer.slot)
    }) {
        return Err(invalid("secondary layer slots must be unique"));
    }
    let mut secondaries = Vec::with_capacity(selected_layers.len());
    for layer in selected_layers {
        let path = layer.layer_path.as_ref().expect("filtered layer path");
        let decoded = decode_bounded_image(path, "secondary")?;
        let (layer_width, layer_height) = (decoded.width(), decoded.height());
        if layer.slot == 0
            || layer.slot > MAX_PARAMETERS
            || layer_width == 0
            || layer_height == 0
            || layer_width > MAX_DIMENSION
            || layer_height > MAX_DIMENSION
            || u64::from(layer_width) * u64::from(layer_height) > MAX_PIXELS
        {
            return Err(invalid("secondary image exceeds the layer transport limit"));
        }
        let mut layer_rgba = decoded.into_rgba8().into_raw();
        if let Some(mode) = conformance_premultiplication {
            apply_conformance_premultiplication(&mut layer_rgba, mode);
        }
        crate::render_request::validate_image_buffer_layout(
            u64::from(layer_width),
            u64::from(layer_height),
            u64::from(layer_width) * 4,
            4,
            Some(layer_rgba.len() as u64),
            u64::from(MAX_DIMENSION),
            MAX_PIXELS,
            MAX_RGBA_TRANSPORT_BYTES,
        )?;
        secondaries.push((layer.slot, layer_width, layer_height, layer_rgba));
    }
    let timed_layers = timed_layers.unwrap_or_default();
    if timed_layers.len() > 64 || secondaries.len() + timed_layers.len() > 64 {
        return Err(invalid(
            "timed secondary layer image count exceeds the transport limit",
        ));
    }
    let layer_slots = interactive_parameters
        .unwrap_or_default()
        .iter()
        .filter(|item| item.kind == "layer")
        .map(|item| item.slot)
        .collect::<HashSet<_>>();
    validate_timed_layer_identities(timed_layers, &layer_slots)?;
    let mut timed_secondaries = Vec::with_capacity(timed_layers.len());
    for layer in timed_layers {
        let decoded = decode_bounded_image(&layer.image_path, "timed secondary")?;
        let (layer_width, layer_height) = (decoded.width(), decoded.height());
        let mut rgba = decoded.into_rgba8().into_raw();
        if let Some(mode) = conformance_premultiplication {
            apply_conformance_premultiplication(&mut rgba, mode);
        }
        crate::render_request::validate_image_buffer_layout(
            u64::from(layer_width),
            u64::from(layer_height),
            u64::from(layer_width) * 4,
            4,
            Some(rgba.len() as u64),
            u64::from(MAX_DIMENSION),
            MAX_PIXELS,
            MAX_RGBA_TRANSPORT_BYTES,
        )?;
        timed_secondaries.push((layer.slot, layer.time, layer_width, layer_height, rgba));
    }

    // Experimental rendering always receives the generic interactive payload.
    // Fixture manifests are resolved by fixture-specific public entrypoints,
    // never by this AEX-agnostic transport path.
    let payload = payload_override.unwrap_or_else(|| "v2|".to_owned());

    // Issue #98 stage W2: plain classic CPU renders route through a resident
    // length-1 render session so the one-shot argv transport can eventually
    // retire. Anything the session transport cannot carry yet keeps the
    // one-shot dispatch below, as does any session-infrastructure failure
    // (the one-shot re-run then reports through the original path). Gate
    // failures inside the session path are final: they are the same
    // fail-closed validation the one-shot path applies.
    // The session transport carries mask, spatial, render-environment context
    // (W1-3), alpha-as-coverage parameter slots (W1-4c, published once at
    // launch), and aux channels (#211): every HostContext field is now
    // session-representable.
    //
    // Aux channels ride the same broker-created manifest sidecar the one-shot
    // path consumes (`--aux-manifest-v1 <manifest>`), so prepare it once here
    // and share it with both transports rather than building it twice. The
    // sidecars and manifest live under the broker-owned `target/image-transport`
    // root; the worker only ever reads broker-written files there (the sample
    // source paths are canonicalized, bounded, and copied by the broker inside
    // prepare_aux_transport), so session-izing adds no new worker-side path
    // re-resolution beyond the pre-existing one-shot transport. `root` and
    // `nonce` are established here so the one-shot path below reuses them.
    let root = repository.join("target/image-transport");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| invalid(error.to_string()))?
        .as_nanos();
    let aux_channels: &[crate::render_request::AuxChannel] =
        host_context.map_or(&[], |context| context.aux_channels.as_slice());
    let aux_transport = if aux_channels.is_empty() {
        None
    } else {
        // Only aux-carrying renders touch the transport root up front; a plain
        // session render still creates nothing here. create_dir_all and the
        // stale sweep are idempotent with the one-shot path's own calls below.
        fs::create_dir_all(&root)?;
        cleanup_stale_image_transport(&root, SystemTime::now())?;
        prepare_aux_transport(repository, aux_channels, &root, nonce)?
    };
    let alpha_as_coverage_params: &[u32] =
        host_context.map_or(&[], |context| context.alpha_as_coverage_params.as_slice());
    // Layer pixels travel as inherited per-layer file HANDLEs (#268), not
    // section slots, so the section is header + input + output only and its
    // aggregate cap is no longer a function of layer count or size. A layered
    // config the one-shot per-file path accepts now always fits the session, so
    // the former session_section_fits carve-out is gone (#264): every eligible
    // render below can be carried by the length-1 session.
    if !smart
        && audio.is_none()
        && gpu_backend == RenderGpuBackend::Auto
        && payload == encode_interactive_payload(interactive_parameters.unwrap_or_default())?
        && std::env::var_os(DISABLE_SESSION_WRAPPER_ENV).is_none()
        // RenderSession::open and the session worker now admit total_time == 0
        // (the zero-duration t=0 render the one-shot worker also produces), so a
        // zero-duration render is carried by the session too (#272); no
        // total_time carve-out remains.
        // RenderSession::open also rejects time_scale > i32::MAX (the worker
        // parses the per-frame scale as signed 32-bit), which is_valid admits.
        // The one-shot worker cannot render it either, so this loses no working
        // capability, but keeping the eligibility gate a strict superset of the
        // session's config preconditions means an eligible render never
        // fail-closes on a config the session structurally rejects (#264).
        && timing.time_scale <= i32::MAX as u32
        // The length-1 session path launches through SessionOpenRequest, which
        // carries no conformance render-settings trailer, so the worker would
        // report its legacy premultiplied/null settings while the broker has
        // already pre-transformed the input for the requested alpha mode. Only
        // the one-shot path below forwards --conformance-render-settings-v1, so
        // bypass the session optimization whenever a conformance trailer is set
        // to keep the pre-transform and the worker-reported settings consistent.
        && conformance_render_settings.is_none()
    {
        // Only a layered session touches target/image-transport: RenderSession::
        // open writes per-layer sidecars there (#268), so a file-free session
        // (no secondary/timed layers) must not be forced to create or sweep the
        // directory. When this session does write sidecars, run the same stale
        // sweep the aux and one-shot paths do, since a pure layered session
        // reaches neither: it reclaims leaked layer-session-* files from a prior
        // crash. This render's own sidecars do not exist yet (open writes them
        // with a fresh nonce) and freshly written aux sidecars survive the age
        // cutoff, so it is idempotent with the aux/one-shot calls.
        if !secondaries.is_empty() || !timed_secondaries.is_empty() {
            fs::create_dir_all(&root)?;
            cleanup_stale_image_transport(&root, SystemTime::now())?;
        }
        // Static secondaries render on every frame; timed secondaries (issue
        // #98 W1-4b) carry their rational admission time so the worker selects
        // the matching entry per frame, the same as the one-shot transport.
        // Every match arm below returns, so this branch never falls through to
        // the one-shot code that reads `secondaries`/`timed_secondaries`; move
        // the decoded RGBA buffers into the session layers instead of cloning
        // them, so a large layered render does not double broker memory (#268).
        let session_layers = secondaries
            .into_iter()
            .map(|(slot, width, height, rgba)| crate::render_session::SessionLayer {
                slot,
                width,
                height,
                rgba,
                timed: None,
            })
            .chain(timed_secondaries.into_iter().map(
                |(slot, time, width, height, rgba)| crate::render_session::SessionLayer {
                    slot,
                    width,
                    height,
                    rgba,
                    timed: Some((time.value, time.scale)),
                },
            ))
            .collect::<Vec<_>>();
        // A host context always sends the mask trailer (the one-shot path does
        // too, even for an empty mask scene), keeping the argv shapes identical.
        let mask_trailer = match host_context {
            Some(context) => Some(crate::render_request::encode_mask_context(context)?),
            None => None,
        };
        let spatial_trailer = match host_context {
            Some(context) => crate::render_request::encode_spatial_context(context)?,
            None => None,
        };
        let render_environment_trailer = match host_context {
            Some(context) => crate::render_request::encode_render_environment(context)?,
            None => None,
        };
        match render_classic_via_length_one_session(&SessionWrapperRequest {
            repository,
            plugin_id,
            plugin_path,
            plugin_sha256,
            timeout_ms,
            output_path,
            preserved_output: preserved_output.as_deref(),
            interactive_parameters,
            parameter_animation,
            layers: session_layers,
            mask_trailer,
            spatial_trailer,
            render_environment_trailer,
            alpha_as_coverage_params,
            aux_manifest: aux_transport.as_ref().map(|aux| aux.manifest_path.as_path()),
            timing,
            pixel_format,
            deep_png_output,
            dependencies: &dependencies,
            rgba: &rgba,
            width,
            height,
            spatial,
            expected_quality,
            expected_field,
            expected_shutter_angle,
            expected_shutter_phase,
            custom_ui_action: custom_ui_action.as_ref(),
        }) {
            SessionWrapperOutcome::Report(report) => return Ok(report),
            SessionWrapperOutcome::Failure(error) => return Err(error),
            // Fail closed (#98 W4, #264): a resident-session infrastructure
            // failure no longer silently falls back to the one-shot transport,
            // which would let the session path rot undetected. Surface it as an
            // explicit diagnostic. The one-shot transport stays available for
            // renders the session cannot serve (the ineligible shapes handled
            // below) and via the AEXCOMPAT_DISABLE_RENDER_SESSION_WRAPPER
            // override, which bypasses the session attempt entirely.
            SessionWrapperOutcome::Fallback(reason) => {
                return Err(invalid(format!(
                    "the resident render session could not carry this render ({reason}); the \
                     automatic one-shot fallback is disabled. Diagnose the session failure, or \
                     set {DISABLE_SESSION_WRAPPER_ENV}=1 to force the one-shot transport for \
                     this render. Any world-dump snapshots from the failed session are preserved \
                     for diagnosis; clear the dump directory before re-running, since it must \
                     start empty."
                )));
            }
        }
    }

    // `root` and `nonce` were established above and shared with the aux-manifest
    // sidecar. Ensure the directory exists for renders that had no aux channels
    // to prepare; create_dir_all and the stale sweep are idempotent, so a
    // fallback after an aux-carrying session attempt repeats them harmlessly
    // (the just-written aux sidecars are fresh and survive the sweep).
    fs::create_dir_all(&root)?;
    cleanup_stale_image_transport(&root, SystemTime::now())?;
    let input_raw = root.join(format!("input-{nonce}.rgba"));
    let output_raw = root.join(format!("output-{nonce}.rgba"));
    let audio_raw = audio
        .as_ref()
        .map(|_| root.join(format!("audio-{nonce}.f32")));
    let layer_raws = (0..secondaries.len())
        .map(|index| root.join(format!("layer-{nonce}-{index}.rgba")))
        .collect::<Vec<_>>();
    let timed_layer_raws = (0..timed_secondaries.len())
        .map(|index| root.join(format!("layer-{nonce}-{}.rgba", secondaries.len() + index)))
        .collect::<Vec<_>>();
    let report_path = root.join(format!("report-{nonce}.json"));
    let animation_path = parameter_animation
        .filter(|animations| !animations.is_empty())
        .map(|_| root.join(format!("parameter-animation-{nonce}.json")));
    let mut cleanup_paths = vec![input_raw.clone(), output_raw.clone(), report_path.clone()];
    cleanup_paths.extend(layer_raws.iter().cloned());
    cleanup_paths.extend(timed_layer_raws.iter().cloned());
    cleanup_paths.extend(audio_raw.iter().cloned());
    cleanup_paths.extend(animation_path.iter().cloned());
    let cleanup = Cleanup(cleanup_paths);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&input_raw)?
        .write_all(&rgba)?;
    for (layer_raw, (_, _, _, layer_rgba)) in layer_raws.iter().zip(&secondaries) {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(layer_raw)?
            .write_all(layer_rgba)?;
    }
    for (layer_raw, (_, _, _, _, layer_rgba)) in timed_layer_raws.iter().zip(&timed_secondaries) {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(layer_raw)?
            .write_all(layer_rgba)?;
    }
    if let (Some(path), Some(bytes)) = (&audio_raw, &audio) {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(bytes)?;
    }
    if let (Some(path), Some(animations)) = (&animation_path, parameter_animation) {
        let bytes = parameter_animation_sidecar_json(animations)?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    // `aux_transport` was prepared once above and is shared with the session
    // wrapper; the one-shot worker consumes the same `--aux-manifest-v1`
    // manifest when the session path fell back or was ineligible.
    let world_dump_dir = requested_world_dump_dir(repository)?;
    let minidump_directory = requested_minidump_directory(repository)?;
    let output_checksum_detail = output_checksum_detail_requested();

    let worker_kind = if smart {
        WorkerKind::Smart
    } else {
        WorkerKind::Render
    };
    let plugin = ApprovedImageArtifact {
        path: plugin_path.to_path_buf(),
        expected_sha256: decode_sha256_hex(plugin_sha256)?,
        expected_size: fs::metadata(plugin_path)?.len(),
    };
    let command = if audio.is_some() {
        "--render-image-audio"
    } else {
        image_worker_command(
            smart,
            pixel_format,
            !secondaries.is_empty() || !timed_secondaries.is_empty(),
            gpu_backend,
        )?
    };
    let mut args_after_plugin = vec![
        plugin_sha256.to_ascii_lowercase(),
        payload,
        input_raw.to_string_lossy().into_owned(),
        output_raw.to_string_lossy().into_owned(),
        width.to_string(),
        height.to_string(),
        timing.current_time.to_string(),
        timing.time_step.to_string(),
        timing.total_time.to_string(),
        timing.time_scale.to_string(),
    ];
    for (layer_raw, (slot, layer_width, layer_height, _)) in layer_raws.iter().zip(&secondaries) {
        args_after_plugin.extend([
            slot.to_string(),
            layer_raw.to_string_lossy().into_owned(),
            layer_width.to_string(),
            layer_height.to_string(),
        ]);
    }
    for (layer_raw, (slot, time, layer_width, layer_height, _)) in
        timed_layer_raws.iter().zip(&timed_secondaries)
    {
        args_after_plugin.extend([
            format!("v1|{slot}|{}|{}", time.value, time.scale),
            layer_raw.to_string_lossy().into_owned(),
            layer_width.to_string(),
            layer_height.to_string(),
        ]);
    }
    if let (Some(path), Some(bytes)) = (&audio_raw, &audio) {
        args_after_plugin.extend([
            path.to_string_lossy().into_owned(),
            (bytes.len() / 4).to_string(),
            "44100".into(),
        ]);
    }
    if let Some(context) = host_context {
        args_after_plugin.push(crate::render_request::encode_mask_context(context)?);
        if let Some(spatial) = crate::render_request::encode_spatial_context(context)? {
            args_after_plugin.push(spatial);
        }
        if let Some(environment) = crate::render_request::encode_render_environment(context)? {
            args_after_plugin.push(environment);
        }
    }
    if let Some(action) = custom_ui_action {
        args_after_plugin.push(action.encode_ui_field()?);
    }
    // Named transports must remain after positional UI/context trailers. The
    // native workers peel these pairs from argv's tail before decoding the
    // positional image contract.
    if let Some(context) = host_context {
        if !context.alpha_as_coverage_params.is_empty() {
            let mut slots = context.alpha_as_coverage_params.clone();
            slots.sort_unstable();
            if slots.windows(2).any(|pair| pair[0] == pair[1])
                || slots.iter().any(|slot| *slot > 1024)
            {
                return Err(invalid("alpha-as-coverage parameter slots are invalid"));
            }
            args_after_plugin.extend([
                "--alpha-as-coverage-v1".into(),
                slots
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ]);
        }
    }
    if let Some(aux) = &aux_transport {
        args_after_plugin.extend([
            "--aux-manifest-v1".into(),
            aux.manifest_path.to_string_lossy().into_owned(),
        ]);
    }
    if let Some(path) = &animation_path {
        args_after_plugin.extend([
            "--parameter-animation-v1".into(),
            path.to_string_lossy().into_owned(),
        ]);
    }
    if let Some(dump) = &world_dump_dir {
        args_after_plugin.extend([
            "--dump-worlds-v1".into(),
            dump.path.to_string_lossy().into_owned(),
        ]);
    }
    // No minidump flag is passed to the worker: the broker creates the dump file
    // and hands over an inherited pipe at the launch boundary (minidump_policy).
    // `minidump_directory` here is only for the report.
    if output_checksum_detail {
        args_after_plugin.extend(["--output-checksum-detail-v1".into(), "1".into()]);
    }
    if let Some(settings) = &conformance_render_settings {
        args_after_plugin.extend(["--conformance-render-settings-v1".into(), settings.clone()]);
    }
    let mut args_before_plugin = vec![command.into()];
    let started = Instant::now();
    let initial_dispatch = SecureImageDispatch {
        repository,
        worker_kind,
        plugin: plugin.clone(),
        dependencies: dependencies.clone(),
        args_before_plugin: &args_before_plugin,
        args_after_plugin: &args_after_plugin,
        timeout: Duration::from_millis(timeout_ms),
    };
    let gpu_initial_attempt = smart
        && pixel_format == RenderPixelFormat::Argb32f
        && secondaries.is_empty()
        && timed_secondaries.is_empty()
        && audio.is_none()
        && runtime_backend(gpu_backend).is_some();
    let auto_gpu_cpu_fallback = gpu_backend == RenderGpuBackend::Auto
        && smart
        && pixel_format == RenderPixelFormat::Argb32f
        && gpu_initial_attempt;
    let mut gpu_fallback_used = false;
    let mut gpu_fallback_reason: Option<String> = None;
    let mut gpu_attempt: Option<Value> = None;
    let mut isolated = if gpu_initial_attempt {
        let gpu_result = (|| -> io::Result<_> {
            let policy_input = gpu_runtime_policy.ok_or_else(|| {
                invalid(
                    "GPU render requires a session-bound authenticated runtime module policy report; use the runtime-policy render API or select CPU",
                )
            })?;
            let backend = runtime_backend(gpu_backend).expect("GPU attempt has a runtime backend");
            let report = authenticate_gpu_worker_report(
                policy_input.module_report_json,
                &policy_input.session_identity,
                backend,
                WorkerModuleValidation {
                    policy: policy_input.policy,
                    sealed: policy_input.sealed_modules,
                    trusted: policy_input.trusted_modules,
                    system32: policy_input.system32,
                },
            )?;
            dispatch_secure_gpu_image(
                initial_dispatch,
                GpuRuntimeAuthorization {
                    backend,
                    session_identity: policy_input.session_identity,
                    module_report: &report,
                },
            )
        })();
        match gpu_result {
            Ok(result) => result,
            Err(error) if auto_gpu_cpu_fallback && is_auto_gpu_preflight_error(&error) => {
                gpu_fallback_used = true;
                gpu_fallback_reason = Some(error.to_string());
                gpu_attempt = Some(json!({
                    "classification": "gpu_preflight_error",
                    "error": error.to_string(),
                }));
                args_before_plugin[0] =
                    image_worker_command(smart, pixel_format, false, RenderGpuBackend::Cpu)?.into();
                dispatch_secure_image(SecureImageDispatch {
                    repository,
                    worker_kind,
                    plugin: plugin.clone(),
                    dependencies: dependencies.clone(),
                    args_before_plugin: &args_before_plugin,
                    args_after_plugin: &args_after_plugin,
                    timeout: Duration::from_millis(timeout_ms),
                })?
            }
            Err(error) => return Err(error),
        }
    } else {
        dispatch_secure_image(initial_dispatch)?
    };
    let mut diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    let initial_report: Option<Value> = serde_json::from_str(isolated.stdout.trim()).ok();
    if let Some(report) = &initial_report {
        propagate_missing_suites(&mut diagnostics, report);
    }
    if initial_report
        .as_ref()
        .is_some_and(|report| report.get("output_pixels_valid") == Some(&Value::Bool(false)))
    {
        diagnostics["failure_stage"] = json!("output_validation");
    }
    let gpu_trace_inferred =
        initial_report.is_none() && diagnostics_contains_gpu_stage(&diagnostics);
    let gpu_attempt_failed = gpu_backend == RenderGpuBackend::Auto
        && smart
        && pixel_format == RenderPixelFormat::Argb32f
        && (gpu_trace_inferred
            || initial_report.as_ref().is_some_and(|worker_report| {
                worker_report.get("gpu_render_dispatched") == Some(&Value::Bool(true))
                    && (isolated.classification.as_str() != "ok"
                        || worker_report.get("smart_render_error") != Some(&json!(0))
                        || worker_report.get("gpu_device_setup_error") != Some(&json!(0))
                        || worker_report.get("gpu_device_setdown_error") != Some(&json!(0)))
            }));
    let worker_report = if gpu_attempt_failed {
        gpu_fallback_reason = Some("GPU worker attempt failed; CPU retry used".into());
        gpu_attempt = Some(json!({
            "worker_classification": isolated.classification.as_str(),
            "worker_diagnostics": diagnostics,
            "report_available": initial_report.is_some(),
            "gpu_trace_inferred": gpu_trace_inferred,
            "smart_render_error": initial_report.as_ref().and_then(|report| report.get("smart_render_error")),
            "smart_render_selector_error": initial_report.as_ref().and_then(|report| report.get("smart_render_selector_error")),
            "output_pixels_valid": initial_report.as_ref().and_then(|report| report.get("output_pixels_valid")),
            "suite_timeline": initial_report.as_ref().and_then(|report| report.get("suite_timeline")),
            "gpu_device_setup_error": initial_report.as_ref().and_then(|report| report.get("gpu_device_setup_error")),
            "gpu_device_setdown_error": initial_report.as_ref().and_then(|report| report.get("gpu_device_setdown_error")),
            "gpu_device_setdown_exception_code": initial_report.as_ref().and_then(|report| report.get("gpu_device_setdown_exception_code")),
            "gpu_render_possible": initial_report.as_ref().and_then(|report| report.get("gpu_render_possible")),
            "gpu_render_dispatched": initial_report.as_ref().and_then(|report| report.get("gpu_render_dispatched")),
        }));
        match fs::remove_file(&output_raw) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Some(dump) = &world_dump_dir {
            clear_world_dump_files(&dump.path)?;
        }
        args_before_plugin[0] = image_worker_command(
            smart,
            pixel_format,
            !secondaries.is_empty() || !timed_secondaries.is_empty(),
            RenderGpuBackend::Cpu,
        )?
        .into();
        let retry_started = Instant::now();
        isolated = dispatch_secure_image(SecureImageDispatch {
            repository,
            worker_kind,
            plugin,
            dependencies,
            args_before_plugin: &args_before_plugin,
            args_after_plugin: &args_after_plugin,
            timeout: Duration::from_millis(timeout_ms),
        })?;
        diagnostics = isolated_worker_diagnostics(&isolated, retry_started.elapsed().as_millis());
        let retry_report = serde_json::from_str(isolated.stdout.trim()).map_err(|_| {
            invalid(format!(
                "CPU fallback worker report unavailable: {diagnostics}"
            ))
        })?;
        propagate_missing_suites(&mut diagnostics, &retry_report);
        gpu_fallback_used = true;
        retry_report
    } else {
        initial_report
            .ok_or_else(|| invalid(format!("worker report unavailable: {diagnostics}")))?
    };
    let (output_origin_ok, parameter_count_ok, spatial_ok) = validate_interactive_worker_report(
        &worker_report,
        &diagnostics,
        &InteractiveGateFacts {
            smart,
            pixel_format,
            spatial,
            expected_quality,
            expected_field,
            expected_shutter_angle,
            expected_shutter_phase,
            custom_ui_action: custom_ui_action.as_ref(),
            audio_present: audio.is_some(),
            interactive_parameters,
            classification: isolated.classification.as_str(),
            time_step: timing.time_step,
            input_width: width,
            input_height: height,
        },
    )?;
    let rendered_width = worker_report
        .get("width")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid("worker output width is invalid"))?;
    let rendered_height = worker_report
        .get("height")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| invalid("worker output height is invalid"))?;
    let rendered_rowbytes = worker_report
        .get("rowbytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("worker output rowbytes is invalid"))?;
    // A SmartFX run that legally answered an empty result_rect renders
    // nothing: zero dimensions and a zero-byte raw output are that contract's
    // fulfillment, and no PNG can represent them, so the pixel pipeline is
    // skipped while the report still carries the empty-geometry fields.
    let empty_smart_result =
        smart && worker_report.get("empty_result_rect") == Some(&Value::Bool(true));
    let mut deep_overrange_samples = None;
    if empty_smart_result {
        if rendered_width != 0 || rendered_height != 0 || rendered_rowbytes != 0 {
            return Err(invalid(format!(
                "empty SmartFX result reported nonzero output geometry: {rendered_width}x{rendered_height} rowbytes={rendered_rowbytes}"
            )));
        }
        let actual_bytes = fs::metadata(&output_raw)
            .map_err(|error| invalid(format!("validated worker output unavailable: {error}")))?
            .len();
        if actual_bytes != 0 {
            return Err(invalid(format!(
                "empty SmartFX result produced {actual_bytes} output bytes"
            )));
        }
    } else {
        crate::render_request::validate_image_buffer_layout(
            u64::from(rendered_width),
            u64::from(rendered_height),
            rendered_rowbytes,
            pixel_format.bytes_per_pixel(),
            None,
            u64::from(MAX_DIMENSION),
            MAX_PIXELS,
            MAX_INTERNAL_IMAGE_BYTES,
        )?;
        let expected_bytes_u64 = crate::render_request::validate_image_buffer_layout(
            u64::from(rendered_width),
            u64::from(rendered_height),
            u64::from(rendered_width) * pixel_format.bytes_per_pixel(),
            pixel_format.bytes_per_pixel(),
            None,
            u64::from(MAX_DIMENSION),
            MAX_PIXELS,
            MAX_INTERNAL_IMAGE_BYTES,
        )?;
        let expected_bytes = usize::try_from(expected_bytes_u64)
            .map_err(|_| invalid("worker output size does not fit this broker"))?;
        let actual_bytes = fs::metadata(&output_raw)
            .map_err(|error| invalid(format!("validated worker output unavailable: {error}")))?
            .len();
        if actual_bytes != expected_bytes_u64 {
            return Err(invalid(format!(
                "validated worker output size mismatch: expected={expected_bytes_u64}, actual={actual_bytes}, diagnostics={diagnostics}"
            )));
        }
        let rendered = fs::read(&output_raw).map_err(|error| {
            invalid(format!(
                "validated worker output unavailable: error={error}, diagnostics={diagnostics}, report={worker_report}"
            ))
        })?;
        if rendered.len() != expected_bytes {
            return Err(invalid(format!(
                "validated worker output size mismatch: expected={expected_bytes}, actual={}, diagnostics={diagnostics}",
                rendered.len()
            )));
        }
        if let Some(path) = &preserved_output {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?
                .write_all(&rendered)?;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if deep_png_output {
            let (samples, overrange_samples) = rgba16_transport_to_png16(&rendered)?;
            deep_overrange_samples = Some(overrange_samples);
            let image = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
                rendered_width,
                rendered_height,
                samples,
            )
            .ok_or_else(|| invalid("worker output dimensions are invalid"))?;
            image
                .save_with_format(output_path, ImageFormat::Png)
                .map_err(|error| invalid(format!("output PNG save failed: {error}")))?;
        } else {
            let preview = native_rgba_to_preview(&rendered, pixel_format)?;
            let image = image::RgbaImage::from_raw(rendered_width, rendered_height, preview)
                .ok_or_else(|| invalid("worker output dimensions are invalid"))?;
            image
                .save_with_format(output_path, ImageFormat::Png)
                .map_err(|error| invalid(format!("output PNG save failed: {error}")))?;
        }
    }
    // The empty branch never writes the preserved raw sidecar, so pointing
    // the report at that path would name a file that does not exist.
    let output_raw = if empty_smart_result {
        None
    } else {
        preserved_output
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
    };
    let facts = InteractiveImageReportFacts {
        plugin_id: plugin_id.to_owned(),
        smart,
        pixel_format,
        rendered_width,
        rendered_height,
        input_width: width,
        input_height: height,
        output_png: output_path.to_path_buf(),
        timing,
        worker_classification: isolated.classification.as_str().to_owned(),
        diagnostics,
        gpu_fallback_used,
        gpu_fallback_reason,
        gpu_attempt,
        secondary_layers: json!(secondaries
            .iter()
            .map(|item| json!({"slot": item.0, "width": item.1, "height": item.2}))
            .collect::<Vec<_>>()),
        empty_smart_result,
        output_raw,
        deep_png_output,
        deep_overrange_samples,
        world_dump_display: world_dump_dir.as_ref().map(|dump| dump.display.clone()),
        minidump_display: minidump_directory.clone(),
        output_checksum_detail,
        output_origin_ok,
        parameter_count_ok,
        spatial_ok,
        audio_input_sha256: audio
            .as_ref()
            .map(|bytes| format!("{:x}", Sha256::digest(bytes))),
    };
    drop(cleanup);
    Ok(build_interactive_image_report(&worker_report, facts))
}


/// Escape hatch for A/B verification against the one-shot argv transport;
/// the equivalence test renders both ways and diffs the public reports.
pub const DISABLE_SESSION_WRAPPER_ENV: &str = "AEXCOMPAT_DISABLE_RENDER_SESSION_WRAPPER";

/// Test-only fault injection (#264): when set, the length-1 classic wrapper
/// reports a `Fallback` before opening the session, so a test can exercise the
/// fail-closed caller arm (an attempted session that fails must surface an
/// explicit error, not silently rerun the one-shot transport) without a
/// deterministic real session failure, which #262 made hard to induce. Compiled
/// only in debug builds (see `forced_session_fallback`), so a release/production
/// broker cannot be made to fail-close by inheriting this variable.
#[cfg(debug_assertions)]
pub const FORCE_SESSION_FALLBACK_ENV: &str = "AEXCOMPAT_FORCE_SESSION_FALLBACK";

/// The test-only fault injection above, gated so it is entirely absent from
/// release builds: the debug variant reads the env var, the release variant is a
/// constant `None` (no env lookup, nothing to break a real render).
#[cfg(debug_assertions)]
fn forced_session_fallback() -> Option<SessionWrapperOutcome> {
    std::env::var_os(FORCE_SESSION_FALLBACK_ENV).is_some().then(|| {
        SessionWrapperOutcome::Fallback(
            "forced session fallback (AEXCOMPAT_FORCE_SESSION_FALLBACK)".into(),
        )
    })
}
#[cfg(not(debug_assertions))]
fn forced_session_fallback() -> Option<SessionWrapperOutcome> {
    None
}

/// The audio counterpart of `forced_session_fallback`, reading the same
/// debug-only `FORCE_SESSION_FALLBACK_ENV` knob so a test can exercise the audio
/// fail-closed caller arm. Absent from release builds.
#[cfg(debug_assertions)]
fn forced_audio_session_fallback() -> Option<AudioWrapperOutcome> {
    std::env::var_os(FORCE_SESSION_FALLBACK_ENV).is_some().then(|| {
        AudioWrapperOutcome::Fallback(
            "forced session fallback (AEXCOMPAT_FORCE_SESSION_FALLBACK)".into(),
        )
    })
}
#[cfg(not(debug_assertions))]
fn forced_audio_session_fallback() -> Option<AudioWrapperOutcome> {
    None
}

/// Diagnostic counter for tests: incremented whenever a render is carried by
/// the length-1 session wrapper instead of the one-shot argv transport.
pub static RENDER_SESSION_WRAPPER_RENDERS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

struct SessionWrapperRequest<'a> {
    repository: &'a Path,
    plugin_id: &'a str,
    plugin_path: &'a Path,
    plugin_sha256: &'a str,
    timeout_ms: u64,
    output_path: &'a Path,
    preserved_output: Option<&'a Path>,
    interactive_parameters: Option<&'a [InteractiveParameter]>,
    parameter_animation: Option<&'a [ParameterAnimation]>,
    layers: Vec<crate::render_session::SessionLayer>,
    mask_trailer: Option<String>,
    spatial_trailer: Option<String>,
    render_environment_trailer: Option<String>,
    alpha_as_coverage_params: &'a [u32],
    /// Absolute path to the broker-prepared aux-channel manifest sidecar
    /// (`--aux-manifest-v1`), shared with the one-shot transport. `None` when
    /// the host context carries no aux channels (#211).
    aux_manifest: Option<&'a Path>,
    timing: RenderTiming,
    pixel_format: RenderPixelFormat,
    deep_png_output: bool,
    dependencies: &'a [ApprovedImageArtifact],
    rgba: &'a [u8],
    width: u32,
    height: u32,
    spatial: crate::render_request::SpatialContext,
    expected_quality: i32,
    expected_field: i32,
    expected_shutter_angle: i32,
    expected_shutter_phase: i32,
    /// Per-frame custom-UI action (#238) driven on the wrapper's single frame
    /// through the v:2 `ui_action` attribute. `None` for a plain render.
    custom_ui_action: Option<&'a RenderUiAction>,
}

enum SessionWrapperOutcome {
    /// The session rendered and validated the frame; this is the public
    /// report, identical in shape to the one-shot flattening.
    Report(Value),
    /// The session ran but the shared fail-closed validation rejected the
    /// worker's output; the one-shot path would have failed identically, so
    /// this is final rather than a fallback.
    Failure(io::Error),
    /// The session infrastructure could not carry the render (open failure,
    /// worker crash or invalidation, malformed close summary). The string is the
    /// reason. The automatic one-shot fallback is removed (#98 W4, #264): the
    /// caller turns this into an explicit fail-closed error so a session
    /// infrastructure failure surfaces instead of being masked by a silent
    /// one-shot rerun. The one-shot transport stays reachable only for renders
    /// the session cannot serve (ineligible shapes) or the explicit
    /// `AEXCOMPAT_DISABLE_RENDER_SESSION_WRAPPER` override.
    Fallback(String),
}

// The whole `image_render` module is `#[cfg(windows)]` (lib.rs), and the render
// workers are Windows executables, so there is no non-Windows render path. The
// former `#[cfg(not(windows))]` shim here was dead code that never compiled on
// any target and misleadingly suggested a non-Windows fallback; it is removed so
// the fail-closed caller is not misread as breaking non-Windows renders.
fn render_classic_via_length_one_session(
    request: &SessionWrapperRequest<'_>,
) -> SessionWrapperOutcome {
    use crate::render_session::{FrameStatus, RenderSession, SessionOpenRequest};

    // Test-only fault injection (debug builds only); a no-op in release.
    if let Some(outcome) = forced_session_fallback() {
        return outcome;
    }

    let world_dump_dir = match requested_world_dump_dir(request.repository) {
        Ok(value) => value,
        Err(error) => return SessionWrapperOutcome::Failure(error),
    };
    let minidump_directory = match requested_minidump_directory(request.repository) {
        Ok(value) => value,
        Err(error) => return SessionWrapperOutcome::Failure(error),
    };
    let output_checksum_detail = output_checksum_detail_requested();
    let mut session = match RenderSession::open(SessionOpenRequest {
        repository: request.repository,
        plugin_path: request.plugin_path,
        plugin_sha256: request.plugin_sha256,
        parameters: request.interactive_parameters,
        parameter_animation: request.parameter_animation,
        aux_manifest: request.aux_manifest,
        world_dump_dir: world_dump_dir.as_ref().map(|dump| dump.path.as_path()),
        output_checksum_detail,
        layers: &request.layers,
        mask_trailer: request.mask_trailer.clone(),
        spatial_trailer: request.spatial_trailer.clone(),
        render_environment_trailer: request.render_environment_trailer.clone(),
        alpha_as_coverage_params: request.alpha_as_coverage_params,
        dependencies: request.dependencies.to_vec(),
        width: request.width,
        height: request.height,
        pixel_format: request.pixel_format,
        time_step: request.timing.time_step,
        total_time: request.timing.total_time,
        time_scale: request.timing.time_scale,
        frame_deadline: Duration::from_millis(request.timeout_ms),
        smart: false,
        gpu_backend: RenderGpuBackend::Cpu,
        gpu_runtime_policy: None,
    }) {
        Ok(session) => session,
        Err(error) => {
            return SessionWrapperOutcome::Fallback(format!("session open failed: {error}"));
        }
    };
    // Fail-closed diagnostics (#264): the session may have written world-dump
    // snapshots before failing. When a one-shot retry followed a Fallback we
    // cleared them (the one-shot resolver requires a fresh directory); now a
    // Fallback becomes a fail-closed error with no retry, so the snapshots are
    // exactly the evidence the user needs to investigate the session failure.
    // Preserve them: return the Fallback without clearing the dump directory.
    let outcome = match session.render_frame_with_attributes(
        0,
        request.timing.current_time,
        request.rgba,
        None,
        request.custom_ui_action,
    ) {
        Ok(outcome) => outcome,
        // Invalidation (worker crash, deadline, or a host-protection invariant).
        Err(error) => {
            let reason = format!("the render session was invalidated: {error}");
            let _ = session.close();
            return SessionWrapperOutcome::Fallback(reason);
        }
    };
    // An expand-output effect that overran the launch slot no longer surfaces
    // here: render_frame grows the shared section in place and returns Rendered
    // at the expanded dimensions (protocol §3, issue #262), so the session route
    // carries the expand without a re-open (which would replay SEQUENCE/FRAME
    // setup and setdown).
    let close = session.close();
    if close.get("session_clean") != Some(&Value::Bool(true))
        || close.get("invalidated") != Some(&Value::Bool(false))
    {
        return SessionWrapperOutcome::Fallback(
            "the render session did not close cleanly".into(),
        );
    }
    let Some(final_report) = close.get("final_report").filter(|value| value.is_object()).cloned()
    else {
        return SessionWrapperOutcome::Fallback(
            "the render session close carried no final report".into(),
        );
    };
    let classification = close["worker"]["classification"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    let diagnostics = close["worker"]["diagnostics"].clone();
    let gate = validate_interactive_worker_report(
        &final_report,
        &diagnostics,
        &InteractiveGateFacts {
            smart: false,
            pixel_format: request.pixel_format,
            spatial: request.spatial,
            expected_quality: request.expected_quality,
            expected_field: request.expected_field,
            expected_shutter_angle: request.expected_shutter_angle,
            expected_shutter_phase: request.expected_shutter_phase,
            custom_ui_action: request.custom_ui_action,
            audio_present: false,
            interactive_parameters: request.interactive_parameters,
            classification: &classification,
            time_step: request.timing.time_step,
            input_width: request.width,
            input_height: request.height,
        },
    );
    let (output_origin_ok, parameter_count_ok, spatial_ok) = match gate {
        Ok(values) => values,
        Err(error) => return SessionWrapperOutcome::Failure(error),
    };
    // The frame's actual (possibly expanded/shrunk) dimensions drive the PNG
    // encode and the public report, not the launch render dimensions (#261).
    let (pixels, rendered_width, rendered_height) = match outcome.status {
        FrameStatus::Rendered {
            pixels,
            width,
            height,
            ..
        } => (pixels, width, height),
        FrameStatus::FrameError { render_error } => {
            // The gate above rejects any final report carrying a render
            // error, so this arm is defensive only.
            return SessionWrapperOutcome::Failure(invalid(format!(
                "session frame reported error {render_error} past a clean final report"
            )));
        }
    };
    if let Some(path) = request.preserved_output {
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return SessionWrapperOutcome::Failure(error);
            }
        }
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut file| file.write_all(&pixels));
        if let Err(error) = written {
            return SessionWrapperOutcome::Failure(error);
        }
    }
    if let Some(parent) = request.output_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return SessionWrapperOutcome::Failure(error);
        }
    }
    let mut deep_overrange_samples = None;
    let png_written = if request.deep_png_output {
        rgba16_transport_to_png16(&pixels).and_then(|(samples, overrange)| {
            deep_overrange_samples = Some(overrange);
            let image = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
                rendered_width,
                rendered_height,
                samples,
            )
            .ok_or_else(|| invalid("worker output dimensions are invalid"))?;
            image
                .save_with_format(request.output_path, ImageFormat::Png)
                .map_err(|error| invalid(format!("output PNG save failed: {error}")))
        })
    } else {
        native_rgba_to_preview(&pixels, request.pixel_format).and_then(|preview| {
            let image = image::RgbaImage::from_raw(rendered_width, rendered_height, preview)
                .ok_or_else(|| invalid("worker output dimensions are invalid"))?;
            image
                .save_with_format(request.output_path, ImageFormat::Png)
                .map_err(|error| invalid(format!("output PNG save failed: {error}")))
        })
    };
    if let Err(error) = png_written {
        return SessionWrapperOutcome::Failure(error);
    }
    let facts = InteractiveImageReportFacts {
        plugin_id: request.plugin_id.to_owned(),
        smart: false,
        pixel_format: request.pixel_format,
        rendered_width,
        rendered_height,
        input_width: request.width,
        input_height: request.height,
        output_png: request.output_path.to_path_buf(),
        timing: request.timing,
        worker_classification: classification,
        diagnostics,
        gpu_fallback_used: false,
        gpu_fallback_reason: None,
        gpu_attempt: None,
        // Same shape as the one-shot path so a layered session render reports
        // its layers instead of falsely claiming none (issue #98 W1-4). The
        // one-shot `secondary_layers` field lists only the static secondaries;
        // timed layers (W1-4b) ride the same trailer but stay out of this field
        // so both routes report the identical set.
        secondary_layers: json!(request
            .layers
            .iter()
            .filter(|layer| layer.timed.is_none())
            .map(|layer| json!({
                "slot": layer.slot,
                "width": layer.width,
                "height": layer.height,
            }))
            .collect::<Vec<_>>()),
        empty_smart_result: false,
        output_raw: request
            .preserved_output
            .map(|path| path.to_string_lossy().into_owned()),
        deep_png_output: request.deep_png_output,
        deep_overrange_samples,
        world_dump_display: world_dump_dir.as_ref().map(|dump| dump.display.clone()),
        minidump_display: minidump_directory.clone(),
        output_checksum_detail,
        output_origin_ok,
        parameter_count_ok,
        spatial_ok,
        audio_input_sha256: None,
    };
    RENDER_SESSION_WRAPPER_RENDERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    SessionWrapperOutcome::Report(build_interactive_image_report(&final_report, facts))
}

/// Static configuration for a resident interactive render session
/// (issue #107): one sealed worker process carries many live renders, and
/// per-frame parameter values ride the v:2 `render_frame` message. Everything
/// here is fixed for the session's lifetime; a change (dimensions, depth,
/// timing, the declared parameter set, the plug-in itself) means close and
/// open a new session.
#[cfg(windows)]
pub struct InteractiveSessionOpen<'a> {
    pub repository: &'a Path,
    pub plugin_id: &'a str,
    pub plugin_path: &'a Path,
    pub plugin_sha256: &'a str,
    /// Declared parameter set; launch values double as the fallback for
    /// frames rendered without a per-frame update.
    pub parameters: Option<&'a [InteractiveParameter]>,
    pub dependencies: Vec<ApprovedImageArtifact>,
    pub width: u32,
    pub height: u32,
    pub pixel_format: RenderPixelFormat,
    pub time_step: i32,
    pub total_time: i32,
    pub time_scale: u32,
    /// Per-frame watchdog deadline in milliseconds.
    pub timeout_ms: u64,
}

/// A resident render session producing `interactive_image_render`-shaped
/// per-frame reports for the harness. This is the default (crash-containment)
/// tier: the plug-in hash binds the observation and no receipt applies. The
/// per-frame host-protection validation lives in [`crate::render_session`];
/// the evidence-grade final-report gate runs once at [`Self::close`].
#[cfg(windows)]
pub struct InteractiveRenderSession {
    session: crate::render_session::RenderSession,
    plugin_id: String,
    pixel_format: RenderPixelFormat,
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
    frame_serial: u32,
    frames_ok: u32,
    frames_errored: u32,
}

#[cfg(windows)]
impl InteractiveRenderSession {
    pub fn open(request: InteractiveSessionOpen<'_>) -> io::Result<Self> {
        let session = crate::render_session::RenderSession::open(
            crate::render_session::SessionOpenRequest {
                repository: request.repository,
                plugin_path: request.plugin_path,
                plugin_sha256: request.plugin_sha256,
                parameters: request.parameters,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                render_environment_trailer: None,
                alpha_as_coverage_params: &[],
                layers: &[],
                smart: false,
                gpu_backend: RenderGpuBackend::Auto,
                gpu_runtime_policy: None,
                dependencies: request.dependencies,
                width: request.width,
                height: request.height,
                pixel_format: request.pixel_format,
                time_step: request.time_step,
                total_time: request.total_time,
                time_scale: request.time_scale,
                frame_deadline: Duration::from_millis(request.timeout_ms),
            },
        )?;
        Ok(Self {
            session,
            plugin_id: request.plugin_id.to_owned(),
            pixel_format: request.pixel_format,
            width: request.width,
            height: request.height,
            time_step: request.time_step,
            total_time: request.total_time,
            time_scale: request.time_scale,
            frame_serial: 0,
            frames_ok: 0,
            frames_errored: 0,
        })
    }

    /// True once the underlying session refused further frames fail-closed;
    /// an `Err` from [`Self::render`] without this flag was a rejected
    /// request (bad input size, out-of-range time or parameters) and the
    /// session keeps rendering.
    pub fn invalidated(&self) -> bool {
        self.session.invalidation().is_some()
    }

    /// Renders one frame at `current_time`. `parameters` carries the values
    /// for exactly this frame (protocol §4.2.1); `None` reuses the launch
    /// values. On success the output PNG (8-bit preview, matching the
    /// one-shot interactive entry) and the depth-preserving raw sidecar are
    /// written and the returned report carries `passed: true`; a frame-local
    /// compatibility error returns `passed: false` with the session intact.
    pub fn render(
        &mut self,
        rgba: &[u8],
        current_time: i32,
        parameters: Option<&[InteractiveParameter]>,
        output_path: &Path,
    ) -> io::Result<Value> {
        use crate::render_session::FrameStatus;
        // The one-shot transport refuses to overwrite an existing output or
        // depth-preserving sidecar; the resident path enforces the same guard
        // before dispatching the frame.
        let preserved_probe = self
            .pixel_format
            .raw_extension()
            .map(|extension| output_path.with_extension(extension));
        if output_path.exists() || preserved_probe.as_ref().is_some_and(|path| path.exists()) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "render output already exists",
            ));
        }
        let started = Instant::now();
        let frame_index = self.frame_serial;
        let outcome = self.session.render_frame_with_parameters(
            frame_index,
            current_time,
            rgba,
            parameters,
        )?;
        let render_ms = started.elapsed().as_millis() as u64;
        let session_facts = |frames_ok: u32, frames_errored: u32| {
            json!({
                "frame_index": frame_index,
                "frames_ok": frames_ok,
                "frames_errored": frames_errored,
                "parameter_update": parameters.is_some(),
                "render_ms": render_ms,
            })
        };
        match outcome.status {
            FrameStatus::Rendered {
                pixels,
                checksum,
                width: frame_width,
                height: frame_height,
            } => {
                self.frame_serial += 1;
                self.frames_ok += 1;
                let preserved = self
                    .pixel_format
                    .raw_extension()
                    .map(|extension| output_path.with_extension(extension));
                if let Some(parent) = output_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                if let Some(raw_path) = &preserved {
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(raw_path)?
                        .write_all(&pixels)?;
                }
                let preview = native_rgba_to_preview(&pixels, self.pixel_format)?;
                // The frame's actual (possibly shrunk) dimensions (#261).
                let image = image::RgbaImage::from_raw(frame_width, frame_height, preview)
                    .ok_or_else(|| invalid("session output dimensions are invalid"))?;
                image
                    .save_with_format(output_path, ImageFormat::Png)
                    .map_err(|error| invalid(format!("output PNG save failed: {error}")))?;
                Ok(json!({
                    "schema_version": 1,
                    "stage": "interactive_image_render",
                    "plugin_id": self.plugin_id,
                    "render_path": "classic",
                    "pixel_format": self.pixel_format.report_name(),
                    "width": frame_width,
                    "height": frame_height,
                    "input_width": self.width,
                    "input_height": self.height,
                    // Deep formats ship the depth-preserving raw next to an
                    // 8-bit preview PNG, matching the one-shot contract.
                    "output_transport": if self.pixel_format == RenderPixelFormat::Argb8 {
                        "rgba8_png"
                    } else {
                        "native_raw+rgba8_png_preview"
                    },
                    "output_png": output_path,
                    "output_raw": preserved,
                    // The slot-transfer checksum (protocol §4.3), not the
                    // one-shot internal-ARGB output_hash definition.
                    "output_slot_sha256": checksum,
                    "current_time": current_time,
                    "time_step": self.time_step,
                    "total_time": self.total_time,
                    "time_scale": self.time_scale,
                    // The worker is still alive, so there is no exit
                    // classification to report; the honest value names the
                    // resident path instead of faking an exit state.
                    "worker_classification": "resident_session",
                    "resident_session": session_facts(self.frames_ok, self.frames_errored),
                    "passed": true,
                }))
            }
            FrameStatus::FrameError { render_error } => {
                self.frames_errored += 1;
                Ok(json!({
                    "schema_version": 1,
                    "stage": "interactive_image_render",
                    "plugin_id": self.plugin_id,
                    "render_path": "classic",
                    "pixel_format": self.pixel_format.report_name(),
                    "current_time": current_time,
                    "worker_classification": "resident_session",
                    "resident_session": session_facts(self.frames_ok, self.frames_errored),
                    "render_error": render_error,
                    "passed": false,
                }))
            }
        }
    }

    /// Ends the session and returns the close summary, including the final
    /// report and the `session_clean` verdict (`render_session.rs`).
    pub fn close(self) -> Value {
        self.session.close()
    }
}

/// Host-side expectations the isolated worker report is validated against.
/// Extracted from `render_with_artifact` so a length-1 render session can run
/// the identical gate over its final report (issue #98 stage W2).
pub(crate) struct InteractiveGateFacts<'a> {
    pub(crate) smart: bool,
    pub(crate) pixel_format: RenderPixelFormat,
    pub(crate) spatial: crate::render_request::SpatialContext,
    pub(crate) expected_quality: i32,
    pub(crate) expected_field: i32,
    pub(crate) expected_shutter_angle: i32,
    pub(crate) expected_shutter_phase: i32,
    pub(crate) custom_ui_action: Option<&'a RenderUiAction>,
    pub(crate) audio_present: bool,
    pub(crate) interactive_parameters: Option<&'a [InteractiveParameter]>,
    pub(crate) classification: &'a str,
    pub(crate) time_step: i32,
    pub(crate) input_width: u32,
    pub(crate) input_height: u32,
}

/// Fails closed exactly like the inline gate did; on success returns the
/// diagnostic contract booleans (output origin, parameter count, spatial)
/// that the public report carries as warnings rather than failures.
pub(crate) fn validate_interactive_worker_report(
    worker_report: &Value,
    diagnostics: &Value,
    facts: &InteractiveGateFacts<'_>,
) -> io::Result<(bool, bool, bool)> {
    let selector_ok = if facts.smart {
        worker_report.get("pre_render_error") == Some(&json!(0))
            && worker_report.get("smart_render_error") == Some(&json!(0))
            && worker_report.get("gpu_device_setup_error") == Some(&json!(0))
            && worker_report.get("gpu_device_setdown_error") == Some(&json!(0))
            && worker_report.get("gpu_memory_lifetimes_balanced") == Some(&Value::Bool(true))
            && worker_report.get("result_rects_valid") == Some(&Value::Bool(true))
            && worker_report.get("output_pixels_valid") == Some(&Value::Bool(true))
    } else {
        worker_report.get("render_error") == Some(&json!(0))
            && worker_report.get("gpu_memory_lifetimes_balanced") == Some(&Value::Bool(true))
            && worker_report.get("pf_path_lifetimes_balanced") == Some(&Value::Bool(true))
    };
    let pixel_format_ok =
        worker_report.get("pixel_format") == Some(&json!(facts.pixel_format.report_name()));
    let output_origin_ok = worker_report
        .get("output_origin")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.len() == 2
                && values.iter().all(|value| {
                    value
                        .as_i64()
                        .is_some_and(|value| i32::try_from(value).is_ok())
                })
        });
    let parameter_count_ok = worker_report
        .get("in_data_num_params")
        .and_then(Value::as_u64)
        .is_some_and(|count| {
            (1..=u64::from(MAX_PARAMETERS) + 1).contains(&count)
                && facts
                    .interactive_parameters
                    .is_none_or(|parameters| count == parameters.len() as u64 + 1)
        });
    let spatial_ok = worker_report.get("downsample_x")
        == Some(&json!([
            facts.spatial.downsample_x.numerator,
            facts.spatial.downsample_x.denominator
        ]))
        && worker_report.get("downsample_y")
            == Some(&json!([
                facts.spatial.downsample_y.numerator,
                facts.spatial.downsample_y.denominator
            ]))
        && worker_report.get("pixel_aspect_ratio")
            == Some(&json!([
                facts.spatial.pixel_aspect_ratio.numerator,
                facts.spatial.pixel_aspect_ratio.denominator
            ]))
        && worker_report.get("full_resolution_dimensions")
            == Some(&json!([
                facts.spatial.full_resolution_width.unwrap_or(facts.input_width),
                facts.spatial.full_resolution_height.unwrap_or(facts.input_height)
            ]))
        && worker_report.get("in_data_dimensions")
            == Some(&json!([
                facts.spatial.full_resolution_width.unwrap_or(facts.input_width),
                facts.spatial.full_resolution_height.unwrap_or(facts.input_height)
            ]))
        && worker_report.get("pre_effect_source_origin")
            == Some(&json!([
                facts.spatial.pre_effect_source_origin_x.unwrap_or(0),
                facts.spatial.pre_effect_source_origin_y.unwrap_or(0)
            ]))
        && worker_report.get("quality") == Some(&json!(facts.expected_quality))
        && worker_report.get("local_time_step") == Some(&json!(facts.time_step))
        && worker_report.get("field") == Some(&json!(facts.expected_field))
        && worker_report.get("shutter_angle_fixed") == Some(&json!(facts.expected_shutter_angle))
        && worker_report.get("shutter_phase_fixed") == Some(&json!(facts.expected_shutter_phase));
    let custom_ui_action_ok = match facts.custom_ui_action {
        None => true,
        Some(RenderUiAction::Click { .. }) => {
            worker_report.get("custom_ui_click_dispatched") == Some(&json!(true))
                && worker_report.get("custom_ui_click_error") == Some(&json!(0))
                && worker_report
                    .get("custom_ui_click_out_flags")
                    .and_then(Value::as_u64)
                    .is_some_and(|flags| flags & 9 == 9)
                && worker_report.get("custom_ui_click_changed_value") == Some(&json!(true))
                && worker_report.get("app_color_picker_calls") == Some(&json!(1))
                && worker_report.get("app_invalidate_rect_calls") == Some(&json!(1))
                && worker_report.get("custom_ui_lifecycle_errors") == Some(&json!([0, 0, 0, 0]))
                && worker_report.get("custom_ui_context_closed") == Some(&json!(true))
        }
        Some(RenderUiAction::Draw) => {
            worker_report.get("custom_ui_draw_dispatched") == Some(&json!(true))
                && worker_report.get("custom_ui_draw_error") == Some(&json!(0))
                && worker_report
                    .get("custom_ui_draw_out_flags")
                    .and_then(Value::as_u64)
                    .is_some_and(|flags| flags & 1 == 1)
                && worker_report.get("custom_ui_lifecycle_errors") == Some(&json!([0, 0, 0, 0]))
                && worker_report.get("custom_ui_context_closed") == Some(&json!(true))
        }
    };
    let worker_passed = facts.classification == "ok"
        && selector_ok
        && pixel_format_ok
        && custom_ui_action_ok
        && (!facts.audio_present
            || (worker_report.get("audio_usage_advertised") == Some(&json!(true))
                && worker_report.get("audio_checkout_allowed") == Some(&json!(true))
                && worker_report.get("audio_source_available") == Some(&json!(true))
                && worker_report.get("audio_lifetimes_balanced") == Some(&json!(true))
                && worker_report.get("invalid_audio_operations") == Some(&json!(0))))
        && worker_report.get("guard_bytes_intact") == Some(&Value::Bool(true));
    if !worker_passed {
        return Err(invalid(format!(
            "isolated AEX image render failed validation: diagnostics={diagnostics}, report={worker_report}"
        )));
    }
    Ok((output_origin_ok, parameter_count_ok, spatial_ok))
}
/// Everything the flattened `interactive_image_render` report carries beyond
/// the worker report itself. Kept separate from `render_with_artifact` so a
/// length-1 render session can synthesize the same public report shape from
/// its final report (issue #98 stage W2); the flattening below is the single
/// source of truth for that schema.
pub(crate) struct InteractiveImageReportFacts {
    pub(crate) plugin_id: String,
    pub(crate) smart: bool,
    pub(crate) pixel_format: RenderPixelFormat,
    pub(crate) rendered_width: u32,
    pub(crate) rendered_height: u32,
    pub(crate) input_width: u32,
    pub(crate) input_height: u32,
    pub(crate) output_png: PathBuf,
    pub(crate) timing: RenderTiming,
    pub(crate) worker_classification: String,
    pub(crate) diagnostics: Value,
    pub(crate) gpu_fallback_used: bool,
    pub(crate) gpu_fallback_reason: Option<String>,
    pub(crate) gpu_attempt: Option<Value>,
    pub(crate) secondary_layers: Value,
    pub(crate) empty_smart_result: bool,
    pub(crate) output_raw: Option<String>,
    pub(crate) deep_png_output: bool,
    pub(crate) deep_overrange_samples: Option<u64>,
    pub(crate) world_dump_display: Option<String>,
    pub(crate) minidump_display: Option<String>,
    pub(crate) output_checksum_detail: bool,
    pub(crate) output_origin_ok: bool,
    pub(crate) parameter_count_ok: bool,
    pub(crate) spatial_ok: bool,
    pub(crate) audio_input_sha256: Option<String>,
}

pub(crate) fn build_interactive_image_report(
    worker_report: &Value,
    facts: InteractiveImageReportFacts,
) -> Value {
    let gpu_memory = json!({
        "lifetimes_balanced": worker_report.get("gpu_memory_lifetimes_balanced"),
        "allocations_created": worker_report.get("gpu_allocations_created"),
        "allocations_freed": worker_report.get("gpu_allocations_freed"),
        "live_allocation_count": worker_report.get("live_gpu_allocation_count"),
        "live_bytes": worker_report.get("live_gpu_memory_bytes"),
        "exclusive_access_depth": worker_report.get("gpu_exclusive_access_depth"),
        "invalid_operations": worker_report.get("invalid_gpu_memory_operations"),
    });
    let mut report = json!({
        "schema_version": 1, "stage": "interactive_image_render", "plugin_id": facts.plugin_id,
        "render_path": if facts.smart { "smartfx" } else { "classic" },
        "pixel_format": facts.pixel_format.report_name(),
        "width": facts.rendered_width, "height": facts.rendered_height,
        "input_width": facts.input_width, "input_height": facts.input_height,
        "output_transport": "rgba8_png", "output_png": facts.output_png,
        "current_time": facts.timing.current_time, "time_step": facts.timing.time_step,
        "total_time": facts.timing.total_time, "time_scale": facts.timing.time_scale,
        "worker_classification": facts.worker_classification,
        "worker_diagnostics": facts.diagnostics,
        "suite_leases_balanced": worker_report.get("suite_leases_balanced"),
        "suite_lease_warning": worker_report.get("suite_lease_warning"),
        "suite_acquires": worker_report.get("suite_acquires"),
        "suite_releases": worker_report.get("suite_releases"),
        "live_suite_leases": worker_report.get("live_suite_leases"),
        "handle_lifetimes_balanced": worker_report.get("handle_lifetimes_balanced"),
        "world_lifetimes_balanced": worker_report.get("world_lifetimes_balanced"),
        "param_checkouts_balanced": worker_report.get("param_checkouts_balanced"),
        "gpu_device_setup_error": worker_report.get("gpu_device_setup_error"),
        "gpu_device_setdown_error": worker_report.get("gpu_device_setdown_error"),
        "gpu_device_setdown_exception_code": worker_report.get("gpu_device_setdown_exception_code"),
        "gpu_render_possible": worker_report.get("gpu_render_possible"),
        "gpu_render_dispatched": worker_report.get("gpu_render_dispatched"),
        "gpu_memory": gpu_memory,
        "gpu_fallback_used": facts.gpu_fallback_used,
        "input_sha256": worker_report.get("input_sha256"),
        "output_sha256": worker_report.get("output_sha256"),
        "requested_parameters": worker_report.get("requested_parameters"),
        "secondary_layers": facts.secondary_layers,
        "guard_bytes_intact": true, "passed": true
    });
    let report_object = report
        .as_object_mut()
        .expect("interactive render report is an object");
    report_object.insert(
        "gpu_fallback_reason".into(),
        json!(facts.gpu_fallback_reason),
    );
    report_object.insert(
        "gpu_attempt".into(),
        facts.gpu_attempt.unwrap_or(Value::Null),
    );
    for (name, source) in [
        ("row_bytes", "rowbytes"),
        ("pixel_format", "pixel_format"),
        ("premultiplication", "premultiplication"),
        ("result_rect", "result_rect"),
        ("max_result_rect", "max_result_rect"),
        ("input_world", "input_world"),
        ("output_world", "output_world"),
        ("suite_timeline", "suite_timeline"),
        ("empty_result_rect", "empty_result_rect"),
        ("returns_extra_pixels", "returns_extra_pixels"),
        ("result_within_request", "result_within_request"),
        ("extra_pixels_contract_violation", "extra_pixels_contract_violation"),
        ("smart_render_selector_dispatched", "smart_render_selector_dispatched"),
        ("input_checkout_result_rect", "input_checkout_result_rect"),
    ] {
        report_object.insert(
            name.into(),
            worker_report.get(source).cloned().unwrap_or(Value::Null),
        );
    }
    if facts.empty_smart_result {
        // Nothing was rendered, so there is no PNG to point at.
        report_object.insert("output_png".into(), Value::Null);
    }
    if facts.deep_png_output {
        report_object.insert("output_transport".into(), json!("native_raw+rgba16_png"));
        report_object.insert(
            "output_overrange_samples".into(),
            json!(facts.deep_overrange_samples),
        );
    } else if facts.pixel_format != RenderPixelFormat::Argb8 {
        report_object.insert(
            "output_transport".into(),
            json!("native_raw+rgba8_png_preview"),
        );
    }
    report_object.insert("output_raw".into(), json!(facts.output_raw));
    if let Some(display) = &facts.world_dump_display {
        report_object.insert(
            "world_dumps".into(),
            json!({
                "directory": display,
                "written": worker_report.get("world_dumps_written"),
                "skipped": worker_report.get("world_dumps_skipped"),
                "bytes": worker_report.get("world_dump_bytes"),
            }),
        );
    }
    if let Some(display) = &facts.minidump_display {
        report_object.insert("minidump_directory".into(), json!(display));
    }
    if facts.output_checksum_detail {
        for field in ["output_row_crc32", "output_channel_sha256"] {
            report_object.insert(
                field.into(),
                worker_report.get(field).cloned().unwrap_or(Value::Null),
            );
        }
    }
    for field in [
        "comp_bg_color_success_count",
        "comp_bg_color_rejection_count",
        "guid_mix_in_call_count",
        "guid_mix_in_success_count",
        "guid_mix_in_rejection_count",
        "guid_mix_in_last_size",
        "guid_mix_in_max_size",
        "guid_mix_in_size_limit",
        "guid_mix_in_last_result",
    ] {
        report_object.insert(
            field.into(),
            worker_report.get(field).cloned().unwrap_or(Value::Null),
        );
    }
    report_object.insert(
        "output_origin_contract_ok".into(),
        json!(facts.output_origin_ok),
    );
    report_object.insert(
        "parameter_count_contract_ok".into(),
        json!(facts.parameter_count_ok),
    );
    report_object.insert("spatial_contract_ok".into(), json!(facts.spatial_ok));
    report_object.insert(
        "host_contract_warning".into(),
        json!(!(facts.output_origin_ok && facts.parameter_count_ok && facts.spatial_ok)),
    );
    if let Some(audio_sha) = &facts.audio_input_sha256 {
        report_object.insert("audio_sidecar_transport".into(), json!("mono_f32le_44100"));
        report_object.insert("audio_sidecar_input_sha256".into(), json!(audio_sha));
        for field in [
            "audio_usage_advertised",
            "audio_checkout_allowed",
            "audio_checkout_calls",
            "audio_checkin_calls",
            "audio_get_data_calls",
            "invalid_audio_operations",
            "rejected_unadvertised_audio_checkouts",
            "rejected_audio_format_requests",
            "audio_handle_exhaustions",
            "peak_live_audio_handles",
            "last_audio_checkout_start_time",
            "last_audio_checkout_duration",
            "last_audio_checkout_time_scale",
            "last_audio_window_start_sample",
            "last_audio_window_sample_count",
            "last_audio_window_silence_samples",
            "last_audio_output_rate_fixed",
            "last_audio_output_bytes_per_sample",
            "last_audio_output_channels",
            "last_audio_output_format",
            "last_audio_returned_sample_frames",
            "audio_lifetimes_balanced",
        ] {
            report_object.insert(
                field.into(),
                worker_report.get(field).cloned().unwrap_or(Value::Null),
            );
        }
    }
    for field in [
        "downsample_x",
        "downsample_y",
        "pixel_aspect_ratio",
        "full_resolution_dimensions",
        "in_data_dimensions",
        "pre_effect_source_origin",
        "output_origin",
        "in_data_num_params",
        "quality",
        "local_time_step",
        "field",
        "shutter_angle_fixed",
        "shutter_phase_fixed",
        "smart_render_selector_error",
        "smart_render_error",
        "output_pixels_valid",
        "cuda_context_used",
        "cuda_upload_bytes",
        "cuda_download_bytes",
        "cuda_sync_failures",
        "cuda_device_count",
        "cuda_device_index",
        "opencl_context_used",
        "opencl_upload_bytes",
        "opencl_download_bytes",
        "opencl_sync_failures",
        "opencl_device_count",
        "opencl_device_index",
        "custom_ui_click_dispatched",
        "custom_ui_click_error",
        "custom_ui_click_out_flags",
        "custom_ui_click_changed_value",
        "custom_ui_draw_dispatched",
        "custom_ui_draw_error",
        "custom_ui_draw_out_flags",
        "custom_ui_lifecycle_errors",
        "custom_ui_context_closed",
        "pf_path_lifetimes_balanced",
        "pf_path_checkout_calls",
        "pf_path_checkin_calls",
        "pf_path_mask_calls",
        "pf_path_preps_created",
        "pf_path_preps_disposed",
        "invalid_pf_path_operations",
        "pf_path_reject_reason",
    ] {
        report_object.insert(field.into(), worker_report[field].clone());
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_render_advertised_follows_out_flags2_bit_10() {
        assert!(!smart_render_advertised(0));
        assert!(smart_render_advertised(PF_OUTFLAG2_SUPPORTS_SMART_RENDER));
        // ntsc-rs (SmartFX-only) and the ONMK MaskOffset fixture advertise the
        // bit inside larger flag words (issue #105).
        assert!(smart_render_advertised(142_611_592));
        assert!(smart_render_advertised(525_312));
        // Every other flag set without bit 10 stays Classic.
        assert!(!smart_render_advertised(u64::MAX & !PF_OUTFLAG2_SUPPORTS_SMART_RENDER));
    }

    #[test]
    fn conformance_alpha_mode_transforms_rgba_transports_consistently() {
        let source = [200, 100, 50, 128, 9, 8, 7, 0];
        for mode in ["straight", "premultiplied", "opaque"] {
            let mut primary = source;
            let mut secondary = source;
            let mut timed_secondary = source;
            apply_conformance_premultiplication(&mut primary, mode);
            apply_conformance_premultiplication(&mut secondary, mode);
            apply_conformance_premultiplication(&mut timed_secondary, mode);
            assert_eq!(secondary, primary);
            assert_eq!(timed_secondary, primary);
        }

        let mut premultiplied = source;
        apply_conformance_premultiplication(&mut premultiplied, "premultiplied");
        assert_eq!(premultiplied, [100, 50, 25, 128, 0, 0, 0, 0]);
        let mut opaque = source;
        apply_conformance_premultiplication(&mut opaque, "opaque");
        assert_eq!(opaque, [200, 100, 50, 255, 9, 8, 7, 255]);
    }

    #[test]
    fn bounded_decode_rejects_header_only_images() {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-truncated-input-{}-{}.png",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]))
            .save(&path)
            .unwrap();
        let complete = fs::read(&path).unwrap();
        let header_only = (33..complete.len())
            .find(|&length| {
                fs::write(&path, &complete[..length]).unwrap();
                matches!(image::image_dimensions(&path), Ok((1, 1)))
                    && decode_bounded_image(&path, "input preflight").is_err()
            })
            .expect("fixture with readable dimensions and truncated pixels");
        fs::write(&path, &complete[..header_only]).unwrap();

        assert_eq!(image::image_dimensions(&path).unwrap(), (1, 1));
        assert!(decode_bounded_image(&path, "input preflight").is_err());
        fs::remove_file(path).unwrap();
    }

    fn timed_layer(slot: u32, value: i32, scale: u32) -> TimedLayerImage {
        TimedLayerImage {
            slot,
            time: AnimationTime { value, scale },
            image_path: PathBuf::from("unused.png"),
        }
    }

    #[test]
    fn timed_layer_identities_are_slot_bound_bounded_and_rationally_unique() {
        let slots = HashSet::from([6]);
        validate_timed_layer_identities(&[timed_layer(6, 1, 2), timed_layer(6, 3, 4)], &slots)
            .unwrap();
        assert!(validate_timed_layer_identities(
            &[timed_layer(6, 1, 2), timed_layer(6, 2, 4)],
            &slots,
        )
        .is_err());
        assert!(validate_timed_layer_identities(&[timed_layer(7, 1, 2)], &slots).is_err());
        assert!(validate_timed_layer_identities(&[timed_layer(6, 1, 0)], &slots).is_err());
        assert!(validate_timed_layer_identities(&vec![timed_layer(6, 1, 2); 65], &slots).is_err());
    }

    #[test]
    fn stale_transport_cleanup_removes_only_old_owned_regular_files() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-stale-transport-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let now = SystemTime::now();
        let old_time = now - Duration::from_secs(120);
        let old_names = [
            "input-123.rgba",
            "output-123.rgba",
            "audio-123.f32",
            "layer-123-0.rgba",
            "layer-session-123-0.rgba",
            "report-123.json",
            "parameter-animation-123.json",
            "aux-manifest-123.json",
            "aux-123-0.f32le",
        ];
        for name in old_names {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            file.set_times(fs::FileTimes::new().set_modified(old_time))
                .unwrap();
        }
        let new_file = root.join("input-456.rgba");
        fs::write(&new_file, b"new").unwrap();
        let unknown = root.join("input-123.rgba.bak");
        fs::write(&unknown, b"unknown").unwrap();
        let directory = root.join("output-789.rgba");
        fs::create_dir(&directory).unwrap();

        cleanup_stale_image_transport_before(&root, now, Duration::from_secs(60)).unwrap();

        for name in old_names {
            assert!(
                !root.join(name).exists(),
                "old owned file was retained: {name}"
            );
        }
        assert!(new_file.exists());
        assert!(unknown.exists());
        assert!(directory.is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_transport_cleanup_keeps_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-stale-transport-link-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("unknown-target");
        fs::write(&target, b"target").unwrap();
        let link = root.join("input-999.rgba");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(not(any(windows, unix)))]
        let linked = false;

        cleanup_stale_image_transport_before(
            &root,
            SystemTime::now() + Duration::from_secs(120),
            Duration::from_secs(60),
        )
        .unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"target");
        if linked {
            assert!(fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink());
        }
        fs::remove_dir_all(root).unwrap();
    }

    fn aux_fixture(root: &Path, name: &str, values: &[f32]) -> crate::render_request::AuxChannel {
        let path = root.join(name);
        let bytes = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        fs::write(&path, bytes).unwrap();
        crate::render_request::AuxChannel {
            param_index: 0,
            channel: crate::render_request::AuxChannelDescriptor {
                channel_type: AUX_CHANNEL_DEPTH,
                name: "depth".into(),
                data_type: crate::render_request::AuxDataType::F32le,
                dimension: 1,
                width: values.len() as u32,
                height: 1,
                row_bytes: None,
                origin_x: 0,
                origin_y: 0,
                downsample_x: crate::render_request::RationalScale {
                    numerator: 1,
                    denominator: 1,
                },
                downsample_y: crate::render_request::RationalScale {
                    numerator: 1,
                    denominator: 1,
                },
                coordinate_space: "source_pixel".into(),
                units: "unitless".into(),
                samples: vec![crate::render_request::AuxChannelSample {
                    time: 0,
                    time_scale: 30,
                    path,
                    sampling: crate::render_request::AuxSampling::Exact,
                    interpretation: crate::render_request::AuxInterpretation::Depth,
                }],
            },
        }
    }

    #[test]
    fn aux_manifest_owns_raw_data_and_cleans_everything() {
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-aux-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let transport_root = repository.join("transport");
        fs::create_dir_all(&transport_root).unwrap();
        let channel = aux_fixture(&repository, "source.f32", &[1.0, 2.5]);
        let transport = prepare_aux_transport(&repository, &[channel], &transport_root, 7)
            .unwrap()
            .unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(&transport.manifest_path).unwrap()).unwrap();
        assert_eq!(manifest["schema"], "aux-manifest-v1");
        assert_eq!(manifest["channels"][0]["data_type"], "f32le");
        assert_eq!(manifest["channels"][0]["samples"][0]["time_scale"], 30);
        assert_eq!(
            manifest["channels"][0]["samples"][0]["expected_byte_length"],
            8
        );
        let raw = PathBuf::from(
            manifest["channels"][0]["samples"][0]["path"]
                .as_str()
                .unwrap(),
        );
        let raw_bytes = fs::read(&raw).unwrap();
        assert_eq!(raw_bytes.len(), 8);
        assert_eq!(
            manifest["channels"][0]["samples"][0]["sha256"],
            format!("{:x}", Sha256::digest(&raw_bytes))
        );
        let manifest_path = transport.manifest_path.clone();
        drop(transport);
        assert!(!manifest_path.exists());
        assert!(!raw.exists());
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn prepare_aux_transport_output_satisfies_the_session_aux_manifest_contract() {
        // #211: the length-1 session wrapper now carries aux channels through
        // the same broker-built manifest the one-shot path uses, passing its
        // path to SessionOpenRequest::aux_manifest. This guards the handoff:
        // the manifest prepare_aux_transport produces must satisfy the session's
        // `--aux-manifest-v1` precondition (render_session.rs requires an
        // absolute, existing file) and the worker's top-level manifest gate
        // (session_protocol_worker mirrors the real load_aux_manifest contract:
        // exactly {schema, nonce, channels}, the v1 schema string, a digit
        // nonce, and a non-empty channel list). A manifest that failed any of
        // these would make the wrapper's session route dead-on-arrival while the
        // one-shot route kept working, exactly the silent split #211 removes.
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-aux-session-contract-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let transport_root = repository.join("transport");
        fs::create_dir_all(&transport_root).unwrap();
        let channel = aux_fixture(&repository, "depth.f32", &[0.0, 0.25, 1.0, 2.0]);
        let transport = prepare_aux_transport(&repository, &[channel], &transport_root, 42)
            .unwrap()
            .expect("aux channels present, so a manifest is produced");

        // The session gate rejects a non-absolute or missing manifest before it
        // ever launches the worker (render_session.rs `aux_manifest` handling).
        assert!(
            transport.manifest_path.is_absolute(),
            "session aux_manifest must be an absolute path"
        );
        assert!(
            transport.manifest_path.is_file(),
            "session aux_manifest must point at an existing file"
        );

        // The worker's top-level manifest gate.
        let document: Value =
            serde_json::from_slice(&fs::read(&transport.manifest_path).unwrap()).unwrap();
        let object = document.as_object().expect("manifest is a JSON object");
        assert_eq!(
            object.len(),
            3,
            "manifest carries exactly schema/nonce/channels"
        );
        assert_eq!(object["schema"], "aux-manifest-v1");
        let nonce = object["nonce"].as_str().expect("nonce is a string");
        assert!(
            !nonce.is_empty() && nonce.bytes().all(|byte| byte.is_ascii_digit()),
            "nonce is a non-empty digit string"
        );
        let channels = object["channels"]
            .as_array()
            .expect("channels is an array");
        assert!(
            !channels.is_empty() && channels.iter().all(Value::is_object),
            "channels is a non-empty list of objects"
        );

        drop(transport);
        fs::remove_dir_all(repository).unwrap();
    }

    // Issue #231: the worker's aux loader gates every declared path on
    // `absolute().lexically_normal() == canonical()`, and MSVC drops the `\\?\`
    // verbatim prefix in `canonical` but keeps it in `absolute`, so a manifest or
    // sidecar path carrying that prefix (as `Path::canonicalize()` produces on
    // Windows) is rejected and the render exits 3. prepare_aux_transport must
    // hand the worker plain absolute paths. This guards the de-verbatim without
    // needing the real worker; it is Windows-only because the prefix is.
    #[cfg(windows)]
    #[test]
    fn aux_transport_de_verbatims_manifest_and_sidecar_paths() {
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-aux-verbatim-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&repository).unwrap();
        // canonicalize() yields the `\\?\C:\...` verbatim form that reproduced
        // the bug; derive the transport root from it exactly as the render path
        // does (repository.join("target/image-transport")).
        let canonical_repository = repository.canonicalize().unwrap();
        assert!(
            canonical_repository.as_os_str().to_string_lossy().starts_with(r"\\?\"),
            "canonicalize() is expected to produce a verbatim root on Windows"
        );
        let transport_root = canonical_repository.join("transport");
        fs::create_dir_all(&transport_root).unwrap();
        let channel = aux_fixture(&canonical_repository, "depth.f32", &[0.0, 0.5, 1.0, 2.0]);
        let transport = prepare_aux_transport(&canonical_repository, &[channel], &transport_root, 231)
            .unwrap()
            .expect("aux channels present, so a manifest is produced");

        assert!(
            !transport
                .manifest_path
                .as_os_str()
                .to_string_lossy()
                .starts_with(r"\\?\"),
            "manifest path handed to the worker must not carry the \\?\\ prefix"
        );
        let document: Value =
            serde_json::from_slice(&fs::read(&transport.manifest_path).unwrap()).unwrap();
        let sample_path = document["channels"][0]["samples"][0]["path"]
            .as_str()
            .expect("sample path is a string");
        assert!(
            !sample_path.starts_with(r"\\?\"),
            "sidecar path written into the manifest must not carry the \\?\\ prefix, got {sample_path}"
        );
        // The de-verbatimed manifest path still resolves to the written file.
        assert!(transport.manifest_path.is_file());
        assert!(Path::new(sample_path).is_file());

        drop(transport);
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn aux_transport_rejects_nonfinite_duplicate_time_and_path_aliases() {
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-aux-reject-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let transport_root = repository.join("transport");
        fs::create_dir_all(&transport_root).unwrap();
        let nonfinite = aux_fixture(&repository, "nan.f32", &[f32::NAN]);
        assert!(prepare_aux_transport(&repository, &[nonfinite], &transport_root, 1).is_err());

        let mut duplicate = aux_fixture(&repository, "valid.f32", &[1.0]);
        duplicate
            .channel
            .samples
            .push(duplicate.channel.samples[0].clone());
        assert!(prepare_aux_transport(&repository, &[duplicate], &transport_root, 2).is_err());

        let mut wrong_dimension = aux_fixture(&repository, "wrong-dimension.f32", &[1.0]);
        wrong_dimension.channel.dimension = 2;
        assert!(
            prepare_aux_transport(&repository, &[wrong_dimension], &transport_root, 4).is_err()
        );

        let first = aux_fixture(&repository, "alias.f32", &[1.0]);
        let mut second = first.clone();
        second.channel.name = "other".into();
        assert!(prepare_aux_transport(&repository, &[first, second], &transport_root, 3).is_err());
        assert!(fs::read_dir(&transport_root).unwrap().next().is_none());
        fs::remove_dir_all(repository).unwrap();
    }

    fn crashkit_parameters(mode: f64) -> Vec<InteractiveParameter> {
        [
            (1, "Fault mode", mode, 1.0, 5.0),
            (2, "Fault stage", 2.0, 1.0, 2.0),
        ]
        .into_iter()
        .map(
            |(slot, name, value, minimum, maximum)| InteractiveParameter {
                slot,
                name: name.into(),
                kind: "integer".into(),
                minimum,
                maximum,
                value,
                choices: vec![],
                color: [0; 4],
                components: [0.0; 3],
                component_count: 0,
                layer_path: None,
                enabled: true,
                visible: true,
                supervised: false,
                debug_summary: None,
                custom_ui_events: 4,
                control_size: [0, 0],
            },
        )
        .collect()
    }

    #[cfg(windows)]
    #[test]
    fn crashkit_event_crash_and_hang_remain_inside_the_worker() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let plugin = repository
            .join("target/instruments-sdk-build/pf-crashkit")
            .join(["pf_crashkit", "aex"].join("."));
        if !plugin.exists() {
            return;
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(&plugin).unwrap()));

        let crash = probe_experimental_custom_ui_idle(
            repository,
            &plugin,
            &hash,
            &crashkit_parameters(2.0),
        )
        .unwrap_err();
        assert!(crash.to_string().contains("failed safely"));

        let started = Instant::now();
        let hang = probe_experimental_custom_ui_idle(
            repository,
            &plugin,
            &hash,
            &crashkit_parameters(3.0),
        )
        .unwrap_err();
        assert!(hang.to_string().contains("failed safely"));
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[cfg(windows)]
    #[test]
    fn histogrid_draw_and_smartfx_render_share_one_isolated_worker() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let plugin = repository
            .join("target/sdk-fixtures/histogrid")
            .join(["HistoGrid", "aex"].join("."));
        if !plugin.exists()
            || !repository
                .join("target/minihost-build/aex_smart_worker.exe")
                .exists()
        {
            return;
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(&plugin).unwrap()));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let input = repository.join(format!("target/histogrid-broker-input-{nonce}.png"));
        let pixels = image::RgbaImage::from_fn(37, 23, |x, y| {
            image::Rgba([x as u8 * 7, y as u8 * 11, ((x + y) % 23) as u8 * 11, 255])
        });
        pixels.save(&input).unwrap();
        let formats = [
            RenderPixelFormat::Argb8,
            RenderPixelFormat::Argb16,
            RenderPixelFormat::Argb32f,
        ];
        let outputs = formats
            .iter()
            .map(|format| {
                repository.join(format!(
                    "target/histogrid-broker-output-{}-{nonce}.png",
                    format.report_name()
                ))
            })
            .collect::<Vec<_>>();

        let result = (|| {
            let parameters = inspect_experimental(repository, &plugin, &hash)?;
            formats
                .iter()
                .zip(&outputs)
                .map(|(format, output)| {
                    if *format == RenderPixelFormat::Argb32f {
                        render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
                            repository,
                            &plugin,
                            &hash,
                            &input,
                            output,
                            &parameters,
                            RenderTiming::default(),
                            true,
                            *format,
                            None,
                            Some(RenderUiAction::Draw),
                            RenderGpuBackend::Cpu,
                        )
                    } else {
                        render_experimental_image_at_time_with_format_context_and_ui_action(
                            repository,
                            &plugin,
                            &hash,
                            &input,
                            output,
                            &parameters,
                            RenderTiming::default(),
                            true,
                            *format,
                            None,
                            Some(RenderUiAction::Draw),
                        )
                    }
                })
                .collect::<io::Result<Vec<_>>>()
        })();
        let _ = fs::remove_file(&input);
        let outputs_exist = outputs.iter().all(|output| output.exists());
        for output in &outputs {
            let _ = fs::remove_file(output);
        }

        let reports = result.unwrap();
        assert!(outputs_exist);
        let mut output_hashes = std::collections::HashSet::new();
        for (report, format) in reports.iter().zip(formats) {
            assert_eq!(report["pixel_format"], format.report_name());
            assert_eq!(report["custom_ui_draw_dispatched"], true);
            assert_eq!(report["custom_ui_draw_error"], 0);
            assert_eq!(report["custom_ui_draw_out_flags"], 1);
            assert_eq!(report["custom_ui_lifecycle_errors"], json!([0, 0, 0, 0]));
            assert_eq!(report["custom_ui_context_closed"], true);
            assert_eq!(report["passed"], true);
            output_hashes.insert(report["output_sha256"].as_str().unwrap().to_owned());
        }
        assert_eq!(output_hashes.len(), 3);
    }

    #[test]
    fn interactive_payload_is_slot_bound_and_range_checked() {
        let parameters = vec![InteractiveParameter {
            slot: 2,
            name: "Direction".into(),
            kind: "integer".into(),
            minimum: 1.0,
            maximum: 3.0,
            value: 2.0,
            choices: vec!["Horizontal".into(), "Vertical".into(), "Both".into()],
            color: [255, 0, 0, 0],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }];
        assert_eq!(
            encode_interactive_payload(&parameters).unwrap(),
            "v2|param_2@2:i32=2"
        );
        let mut invalid = parameters;
        invalid[0].value = 4.0;
        assert!(encode_interactive_payload(&invalid).is_err());
    }

    #[test]
    fn empty_parameter_set_uses_valid_default_payload() {
        assert_eq!(encode_interactive_payload(&[]).unwrap(), "v2|");
    }

    #[test]
    fn arbitrary_payload_is_hex_encoded_and_bounded() {
        let parameter = InteractiveParameter {
            slot: 1,
            name: "Grid".into(),
            kind: "arbitrary_data".into(),
            minimum: 0.0,
            maximum: 0.0,
            value: 0.0,
            choices: vec![],
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: Some("value=7".into()),
            custom_ui_events: 0,
            control_size: [0, 0],
        };
        assert_eq!(
            encode_interactive_payload(std::slice::from_ref(&parameter)).unwrap(),
            "v5|param_1@1:arbhex=76616c75653d37"
        );
        let mut invalid = parameter.clone();
        invalid.debug_summary = Some("\0".into());
        assert!(encode_interactive_payload(&[invalid]).is_err());
        let mut too_long = parameter;
        too_long.debug_summary = Some("x".repeat(4097));
        assert!(encode_interactive_payload(&[too_long]).is_err());
    }

    #[test]
    fn ui_only_descriptors_are_filtered_but_path_is_render_assignable() {
        let parameters = [
            "group_start",
            "group_end",
            "button",
            "custom",
            "no_data",
            "path",
        ]
        .map(|kind| InteractiveParameter {
            slot: 1,
            name: kind.into(),
            kind: kind.into(),
            minimum: 0.0,
            maximum: 0.0,
            value: 0.0,
            choices: vec![],
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        });
        assert_eq!(
            encode_interactive_payload(&parameters).unwrap(),
            "v2|param_1@1:i32=0"
        );
    }

    #[test]
    fn component_payload_is_slot_bound_and_range_checked() {
        let parameters = vec![InteractiveParameter {
            slot: 2,
            name: "Center".into(),
            kind: "point".into(),
            minimum: 0.0,
            maximum: 0.0,
            value: 0.0,
            choices: vec![],
            color: [0; 4],
            components: [50.0, 25.5, 0.0],
            component_count: 2,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }];
        assert_eq!(
            encode_interactive_payload(&parameters).unwrap(),
            "v4|param_2@2:point=50,25.5"
        );
        let mut invalid = parameters;
        invalid[0].components[1] = 40_000.0;
        assert!(encode_interactive_payload(&invalid).is_err());
    }

    #[test]
    fn render_timing_is_bounded_and_monotonic() {
        assert!(RenderTiming {
            current_time: 3,
            time_step: 1,
            total_time: 4,
            time_scale: 30,
        }
        .is_valid());
        assert!(!RenderTiming {
            current_time: 3,
            time_step: 0,
            total_time: 2,
            time_scale: 0,
        }
        .is_valid());
    }

    #[test]
    fn pixel_formats_expose_their_full_argb_stride() {
        assert_eq!(RenderPixelFormat::Argb8.bytes_per_pixel(), 4);
        assert_eq!(RenderPixelFormat::Argb16.bytes_per_pixel(), 8);
        assert_eq!(RenderPixelFormat::Argb32f.bytes_per_pixel(), 16);
    }

    #[test]
    fn world_dump_dir_is_fail_closed_under_the_target_tree() {
        let repository = std::env::temp_dir().join(format!(
            "aexcompat-world-dump-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(repository.join("target")).unwrap();

        let accepted =
            resolve_world_dump_dir(&repository, Path::new("target/world-dumps")).unwrap();
        assert!(accepted.path.is_dir());
        assert_eq!(accepted.display, "target/world-dumps");

        // A non-empty directory is refused so stale snapshots cannot be
        // mistaken for the coming run's output.
        fs::write(
            accepted.path.join("000-classic-input-2x2.rgba8"),
            [0_u8; 16],
        )
        .unwrap();
        assert!(resolve_world_dump_dir(&repository, Path::new("target/world-dumps")).is_err());

        assert!(resolve_world_dump_dir(&repository, Path::new("")).is_err());
        assert!(resolve_world_dump_dir(&repository, Path::new("target/../escape")).is_err());
        assert!(resolve_world_dump_dir(&repository, Path::new("not-target/dumps")).is_err());
        let outside = std::env::temp_dir().join(format!(
            "aexcompat-world-dump-outside-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(resolve_world_dump_dir(&repository, &outside).is_err());
        assert!(!outside.exists() || fs::remove_dir_all(&outside).is_ok());
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn world_dump_cleanup_removes_only_owned_snapshot_files() {
        let directory = std::env::temp_dir().join(format!(
            "aexcompat-world-dump-clear-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let owned = [
            "000-classic-input-4x2.rgba8",
            "001-smart-output-4x2.rgba16le",
            "002-smart-layer-slot7-4x2.rgba32f-le",
        ];
        let foreign = ["notes.txt", "xyz-classic-input-4x2.rgba8", "003-.png"];
        for name in owned.iter().chain(foreign.iter()) {
            fs::write(directory.join(name), b"x").unwrap();
        }
        clear_world_dump_files(&directory).unwrap();
        for name in owned {
            assert!(!directory.join(name).exists(), "{name} should be removed");
        }
        for name in foreign {
            assert!(directory.join(name).exists(), "{name} should survive");
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deep16_png_expands_ae_range_and_counts_overrange_samples() {
        let rgba16 = [0u16, 16_384, 32_768, 65_535]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let (samples, overrange) = rgba16_transport_to_png16(&rgba16).unwrap();
        assert_eq!(samples, vec![0, 32_768, 65_535, 65_535]);
        assert_eq!(overrange, 1);
        assert!(rgba16_transport_to_png16(&rgba16[..6]).is_err());
    }

    #[test]
    fn deep16_png_rounds_to_the_same_8_bit_values_as_the_preview() {
        // The 16-bit PNG must stay interchangeable with the 8-bit preview:
        // rounding its full-range samples back to 8 bits has to reproduce the
        // preview quantization for every representable AE-range value.
        for value in 0..=32_768u32 {
            let bytes = (value as u16).to_le_bytes();
            let transport = [bytes[0], bytes[1], 0, 0, 0, 0, 0, 0];
            let (samples, overrange) = rgba16_transport_to_png16(&transport).unwrap();
            assert_eq!(overrange, 0);
            let png16 = u32::from(samples[0]);
            let rounded8 = (png16 * 255 + 32_767) / 65_535;
            let preview8 = (value * 255 + 16_384) / 32_768;
            assert_eq!(rounded8, preview8, "value {value}");
            // Full-range expansion must be lossless for AE-range data.
            assert_eq!((png16 * 32_768 + 32_767) / 65_535, value, "value {value}");
        }
    }

    #[test]
    fn native_depth_transport_keeps_raw_precision_and_builds_preview() {
        assert_eq!(RenderPixelFormat::Argb16.raw_extension(), Some("rgba16le"));
        assert_eq!(
            RenderPixelFormat::Argb32f.raw_extension(),
            Some("rgba32f-le")
        );
        let rgba16 = [0u16, 16_384, 32_768, 65_535]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            native_rgba_to_preview(&rgba16, RenderPixelFormat::Argb16).unwrap(),
            vec![0, 128, 255, 255]
        );
        let rgba32 = [-1.0f32, 0.5, 2.0, f32::NAN]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            native_rgba_to_preview(&rgba32, RenderPixelFormat::Argb32f).unwrap(),
            vec![0, 128, 255, 0]
        );
    }

    #[test]
    fn worker_stage_diagnostics_identify_active_and_failed_selectors() {
        let diagnostics = worker_diagnostics(
            "untrusted C:\\private\\plugin\nstage:global_setup_begin\nstage:global_setup_end error=0\nstage:render_begin\n",
            false,
            "crash",
            0xC0000005,
            42,
        );
        assert_eq!(diagnostics["active_stage"], "render");
        assert_eq!(diagnostics["last_completed_stage"], "global_setup");
        assert_eq!(diagnostics["stage_events"].as_array().unwrap().len(), 3);

        let failed = worker_diagnostics(
            "stage:smart_render_begin\nstage:smart_render_end pre_error=0 render_error=25\n",
            false,
            "ok",
            0,
            8,
        );
        assert_eq!(failed["failure_stage"], "smart_render");
        assert_eq!(failed["first_failure_stage"], "smart_render");
        assert!(failed.to_string().find("private").is_none());

        let nested_crash = worker_diagnostics(
            "stage:render_begin\nstage:sequence_setup_begin\nstage:sequence_setup_end error=0\nstage:frame_setup_begin\nstage:frame_setup_end error=0\n",
            false,
            "crashed",
            0xC0000005,
            29,
        );
        assert_eq!(nested_crash["active_stage"], "render");
        assert_eq!(nested_crash["failure_stage"], "render");
        assert_eq!(nested_crash["first_failure_stage"], "render");
        assert_eq!(nested_crash["last_completed_stage"], "frame_setup");

        let cleanup_failure = worker_diagnostics(
            "stage:global_setup_begin\nstage:global_setup_end error=512\nstage:global_setdown_begin\nstage:global_setdown_end error=-1\n",
            false,
            "nonzero_exit",
            14,
            12,
        );
        assert_eq!(cleanup_failure["first_failure_stage"], "global_setup");
        assert_eq!(cleanup_failure["failure_stage"], "global_setdown");
    }

    #[test]
    fn worker_stage_diagnostics_are_bounded() {
        let trace = "stage:render_begin\n".repeat(MAX_STAGE_EVENTS + 20);
        let diagnostics = worker_diagnostics(&trace, true, "timeout", 1, 5_000);
        assert_eq!(
            diagnostics["stage_events"].as_array().unwrap().len(),
            MAX_STAGE_EVENTS
        );
        assert_eq!(diagnostics["stderr_truncated"], true);
    }

    #[test]
    fn structured_worker_report_supplies_bounded_unique_missing_suites() {
        let mut trace = String::from(
            "stage:suite_acquire_failed name=PF World Suite version=2\n\
             stage:suite_acquire_failed name=PF World Suite version=2\n\
             stage:suite_acquire_failed name=C:\\private\\suite version=1\n\
             stage:suite_acquire_failed name=Bad Suite version=-1\n",
        );
        for index in 0..(MAX_MISSING_SUITES + 3) {
            trace.push_str(&format!(
                "stage:suite_acquire_failed name=Safe Suite {index} version=1\n"
            ));
        }
        trace.push_str(&format!(
            "stage:suite_acquire_failed name={} version=1\n",
            "A".repeat(MAX_SUITE_NAME_LEN + 1)
        ));

        let mut diagnostics = worker_diagnostics(&trace, false, "nonzero_exit", 1, 2);
        assert!(diagnostics["missing_suites"].as_array().unwrap().is_empty());
        let mut reported = vec![
            json!({"name": "PF World Suite", "version": 2}),
            json!({"name": "PF World Suite", "version": 2}),
            json!({"name": "C:\\private\\suite", "version": 1}),
            json!({"name": "Bad Suite", "version": -1}),
        ];
        for index in 0..(MAX_MISSING_SUITES + 3) {
            reported.push(json!({"name": format!("Safe Suite {index}"), "version": 1}));
        }
        propagate_missing_suites(&mut diagnostics, &json!({"missing_suites": reported}));
        let suites = diagnostics["missing_suites"].as_array().unwrap();
        assert_eq!(suites.len(), MAX_MISSING_SUITES);
        assert_eq!(suites[0], json!({"name": "PF World Suite", "version": 2}));
        assert_eq!(
            suites
                .iter()
                .filter(|suite| suite["name"] == "PF World Suite")
                .count(),
            1
        );
        assert!(!diagnostics.to_string().contains("private"));
    }

    #[test]
    fn gpu_trace_inference_requires_an_explicit_gpu_stage() {
        let gpu = json!({
            "failure_stage": "gpu_device_setdown",
            "stage_events": [{"stage":"smart_render_gpu","state":"begin"}]
        });
        let cpu = json!({
            "failure_stage": "smart_render_cpu",
            "stage_events": [{"stage":"smart_render_cpu","state":"begin"}]
        });
        assert!(diagnostics_contains_gpu_stage(&gpu));
        assert!(!diagnostics_contains_gpu_stage(&cpu));
    }

    #[test]
    fn minidump_marker_accepts_only_worker_owned_shapes() {
        // Legitimate worker lines normalize to a path-free marker.
        assert_eq!(
            minidump_marker("stage:minidump_written bytes=51790"),
            Some("written bytes=51790".to_owned())
        );
        assert_eq!(
            minidump_marker("stage:minidump_failed reason=dbghelp_unavailable"),
            Some("failed reason=dbghelp_unavailable".to_owned())
        );
        assert_eq!(
            minidump_marker("stage:minidump_failed reason=create_failed code=5"),
            Some("failed reason=create_failed".to_owned())
        );
        assert_eq!(
            minidump_marker("stage:minidump_failed reason=writer_timeout"),
            Some("failed reason=writer_timeout".to_owned())
        );

        // A plug-in cannot smuggle a path or fake reason through the marker.
        assert_eq!(
            minidump_marker("stage:minidump_written name=C:\\Users\\secret\\a.dmp bytes=1"),
            None
        );
        assert_eq!(minidump_marker("stage:minidump_written bytes=../etc"), None);
        assert_eq!(
            minidump_marker("stage:minidump_failed reason=totally_made_up"),
            None
        );
        assert_eq!(minidump_marker("stage:minidump_written whatever"), None);
        assert_eq!(minidump_marker("stage:other"), None);
    }

    #[test]
    fn isolated_worker_diagnostics_expose_kill_reason_and_memory_peaks() {
        let isolated = crate::secure_launch::SecureLaunchResult {
            classification: crate::ExitClassification::NonzeroExit,
            exit_code: 42,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            kill_reason: Some("memory_limit"),
            worker_peak_commit_bytes: Some(529_000_000),
            peak_process_memory_bytes: Some(530_000_000),
            peak_job_memory_bytes: Some(531_000_000),
            process_memory_limit_bytes: 536_870_912,
            memory_limit_reached: true,
        };
        let diagnostics = isolated_worker_diagnostics(&isolated, 1_234);
        assert_eq!(diagnostics["kill_reason"], "memory_limit");
        assert_eq!(diagnostics["memory_limit_reached"], true);
        assert_eq!(diagnostics["worker_peak_commit_bytes"], 529_000_000u64);
        assert_eq!(diagnostics["peak_process_memory_bytes"], 530_000_000u64);
        assert_eq!(diagnostics["peak_job_memory_bytes"], 531_000_000u64);
        assert_eq!(diagnostics["process_memory_limit_bytes"], 536_870_912u64);
        assert_eq!(diagnostics["classification"], "nonzero_exit");
        assert_eq!(diagnostics["elapsed_ms"], 1_234);

        let alive = crate::secure_launch::SecureLaunchResult {
            classification: crate::ExitClassification::Ok,
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            kill_reason: None,
            worker_peak_commit_bytes: Some(900_000),
            peak_process_memory_bytes: Some(1_000_000),
            peak_job_memory_bytes: Some(1_000_000),
            process_memory_limit_bytes: 536_870_912,
            memory_limit_reached: false,
        };
        let diagnostics = isolated_worker_diagnostics(&alive, 5);
        assert_eq!(diagnostics["kill_reason"], Value::Null);
        assert_eq!(diagnostics["memory_limit_reached"], false);
    }

    #[test]
    fn runtime_module_backend_matches_the_worker_gpu_command() {
        assert_eq!(
            runtime_backend(RenderGpuBackend::Auto),
            Some(RuntimeBackend::Cuda)
        );
        assert_eq!(
            runtime_backend(RenderGpuBackend::Cuda),
            Some(RuntimeBackend::Cuda)
        );
        assert_eq!(
            runtime_backend(RenderGpuBackend::OpenCl),
            Some(RuntimeBackend::Opencl)
        );
        assert_eq!(
            runtime_backend(RenderGpuBackend::DirectX),
            Some(RuntimeBackend::Directx)
        );
        assert_eq!(runtime_backend(RenderGpuBackend::Cpu), None);
    }

    #[test]
    fn auto_gpu_fallback_is_limited_to_host_preflight_failures() {
        for message in [
            "GPU render requires a session-bound authenticated runtime module policy report",
            "GPU infrastructure is unavailable",
            "GPU backend unavailable on this host",
            "host policy rejected GPU dispatch",
            "runtime module policy is expired",
            "trusted worker staging failed",
            "restricted process launch failed",
        ] {
            assert!(is_auto_gpu_preflight_error(&io::Error::other(message)));
        }
        for message in [
            "isolated AEX image render failed validation: selector_error=17",
            "worker crashed during SMART_RENDER",
            "validated worker output size mismatch",
        ] {
            assert!(!is_auto_gpu_preflight_error(&io::Error::other(message)));
        }
    }

    #[test]
    fn image_worker_commands_are_depth_and_layer_explicit() {
        assert_eq!(
            image_worker_command(
                false,
                RenderPixelFormat::Argb16,
                false,
                RenderGpuBackend::Auto
            )
            .unwrap(),
            "--render-image16"
        );
        assert_eq!(
            image_worker_command(
                true,
                RenderPixelFormat::Argb32f,
                true,
                RenderGpuBackend::Auto
            )
            .unwrap(),
            "--smart-image32-layer"
        );
        assert_ne!(
            image_worker_command(
                false,
                RenderPixelFormat::Argb8,
                false,
                RenderGpuBackend::Auto
            )
            .unwrap(),
            image_worker_command(
                false,
                RenderPixelFormat::Argb16,
                false,
                RenderGpuBackend::Auto
            )
            .unwrap()
        );
        for (backend, expected) in [
            (RenderGpuBackend::Auto, "--smart-image32"),
            (RenderGpuBackend::Cuda, "--smart-image32"),
            (RenderGpuBackend::OpenCl, "--smart-image32-opencl"),
            (RenderGpuBackend::DirectX, "--smart-image32-directx"),
            (RenderGpuBackend::Cpu, "--smart-image32-cpu"),
        ] {
            assert_eq!(
                image_worker_command(true, RenderPixelFormat::Argb32f, false, backend).unwrap(),
                expected
            );
        }
        assert!(image_worker_command(
            true,
            RenderPixelFormat::Argb32f,
            true,
            RenderGpuBackend::Cpu
        )
        .is_err());
        assert!(image_worker_command(
            false,
            RenderPixelFormat::Argb32f,
            false,
            RenderGpuBackend::Cuda
        )
        .is_err());
        assert_eq!(
            serde_json::to_string(&RenderGpuBackend::OpenCl).unwrap(),
            "\"opencl\""
        );
    }
}
