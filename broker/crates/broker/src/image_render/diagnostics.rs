use crate::host_core::descriptor_manifest::load as load_manifest;
use crate::host_core::parameter::{ValidatedAssignments, apply_defaults, encode_worker_payload};
use crate::runtime_module_authorization::prepare_runtime_authorization_transport;
use crate::runtime_module_policy::{
    ApprovedClassifiedModule, RuntimeBackend, RuntimeModulePolicy, WorkerModuleValidation,
    authenticate_gpu_worker_report,
};
use crate::secure_image_dispatch::{
    ApprovedImageArtifact, SecureImageDispatch, WorkerKind, dispatch_secure_image,
};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const MAX_DIMENSION: u32 = 4096;
pub(crate) const MAX_PIXELS: u64 = 16_777_216;
pub(crate) const MAX_RGBA_TRANSPORT_BYTES: u64 = MAX_PIXELS * 4;
const MAX_PARAMETERS: u32 = 1024;
pub(crate) const INTERACTIVE_RENDER_TIMEOUT_MS: u64 = 30_000;
const MAX_STAGE_EVENTS: usize = 32;
const MAX_MISSING_SUITES: usize = 16;
const MAX_UNSUPPORTED_SUITE_CALLS: usize = 32;
const MAX_SUITE_CALL_SLOT_PROBE_SLOTS: u64 = 32;
const MAX_SUITE_CALL_SLOT_PROBE_TARGETS: usize = 8;
const MAX_SUITE_NAME_LEN: usize = 64;
const MAX_SUITE_TIMELINE_EVENTS: usize = 512;
const MAX_SELECTOR_INVOCATIONS: usize = 64;
const MAX_HOST_CALLBACK_TIMELINE_RECORDS: usize = 128;
const MAX_COMPUTE_CACHE_TIMELINE_RECORDS: usize = 128;
const MAX_EXTENDED_LOOKUP_TIMELINE_RECORDS: usize = 128;
const MAX_EXTENDED_ALLOCATION_TIMELINE_RECORDS: usize = 128;
const MAX_SUITE_VERSION: i64 = u16::MAX as i64;
const MAX_UNSUPPORTED_SUITE_SLOT: u64 = 1023;
const STALE_IMAGE_TRANSPORT_AGE: Duration = Duration::from_secs(15 * 60);
const CONFORMANCE_RENDER_SETTINGS_ENV: &str = "AEXCOMPAT_CONFORMANCE_RENDER_SETTINGS";

fn conformance_render_settings_transport() -> io::Result<Option<String>> {
    let encoded = if let Some(value) = fixture_render_settings_override() {
        value
    } else if let Ok(value) = std::env::var(CONFORMANCE_RENDER_SETTINGS_ENV) {
        value
    } else {
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

// #816 made a non-empty search root set part of the in-place protocol, and the
// loader resolves the closure from the plug-in's own directory.
fn search_root(plugin_path: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let parent = plugin_path
        .parent()
        .ok_or_else(|| invalid("plugin path has no parent directory to search for dependencies"))?;
    Ok(vec![parent.to_path_buf()])
}

fn dispatch_approved_image(
    repository: &Path,
    worker_kind: WorkerKind,
    plugin_path: &Path,
    approved_sha256: &str,
    args_before_plugin: &[String],
    args_after_plugin: &[String],
    timeout: Option<Duration>,
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
        dependency_search_dirs: search_root(plugin_path)?,
        args_before_plugin,
        args_after_plugin,
        timeout,
        launch_environment: Default::default(),
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
    timeout: Option<Duration>,
) -> io::Result<crate::secure_launch::SecureLaunchResult> {
    let mut dependency_search_dirs = search_root(plugin_path)?;
    for dependency in &dependencies {
        let parent = dependency
            .path
            .parent()
            .ok_or_else(|| invalid("approved dependency has no parent directory"))?
            .to_path_buf();
        if !dependency_search_dirs.contains(&parent) {
            dependency_search_dirs.push(parent);
        }
    }
    dispatch_secure_image(SecureImageDispatch {
        repository,
        worker_kind,
        plugin: ApprovedImageArtifact {
            path: plugin_path.to_path_buf(),
            expected_sha256: decode_sha256_hex(approved_sha256)?,
            expected_size: fs::metadata(plugin_path)?.len(),
        },
        dependencies: vec![],
        dependency_search_dirs,
        args_before_plugin,
        args_after_plugin,
        timeout,
        launch_environment: Default::default(),
    })
}

/// A broker-owned GPU runtime-module policy assembled by a preflight worker run
/// (#290). Owns everything a GPU single-image render borrows as a
/// [`GpuRuntimePolicyInput`]: the authenticated policy, the classified module
/// report the preflight worker emitted, the per-render session identity, and the
/// canonical System32 path.
///
/// The sealed and trusted module tables are intentionally empty. The report only
/// classifies the authorized GPU runtime modules (`policy`), which live at stable
/// System32 / driver-store paths the render-time re-authentication can resolve
/// and re-hash. The plug-in and worker load through the sealed load tree and the
/// trusted worker stage under ephemeral, per-dispatch temp paths, so reporting
/// them would fail re-authentication once those directories are gone; the
/// always-on DLL-load module audit covers them instead.
pub struct PreparedGpuRuntimePolicy {
    policy: RuntimeModulePolicy,
    module_report_json: Vec<u8>,
    session_identity: [u8; 32],
    system32: PathBuf,
}

impl PreparedGpuRuntimePolicy {
    /// The classified GPU module report the preflight worker emitted, as UTF-8
    /// JSON. Exposed for diagnostics and the A/B gate; the render path uses
    /// [`Self::as_input`] instead.
    pub fn report_json(&self) -> &str {
        std::str::from_utf8(&self.module_report_json).unwrap_or_default()
    }

    /// Borrows the owned fields as the input the render path authenticates and
    /// dispatches with. Safe to call for each render on the same session.
    pub fn as_input(&self) -> GpuRuntimePolicyInput<'_> {
        GpuRuntimePolicyInput {
            policy: &self.policy,
            module_report_json: &self.module_report_json,
            session_identity: self.session_identity,
            sealed_modules: &[],
            trusted_modules: &[],
            system32: &self.system32,
        }
    }
}

fn canonical_system32() -> io::Result<PathBuf> {
    let root = std::env::var_os("SystemRoot")
        .ok_or_else(|| invalid("the SystemRoot environment variable is not set"))?;
    fs::canonicalize(PathBuf::from(root).join("System32"))
}

/// Runs the GPU module-audit preflight worker under `policy` and assembles the
/// authenticated [`PreparedGpuRuntimePolicy`] a GPU single-image render borrows
/// (#290). The preflight authorizes the policy's GPU runtime modules for
/// `gpu_backend`, loads them, and emits the classified report; the report is
/// authenticated here (fail-fast) and again at render dispatch.
///
/// `policy` must already be parsed and validated (see
/// [`crate::runtime_module_policy::parse_and_validate`]). An explicit CPU backend
/// is rejected: only a GPU backend has a runtime module policy.
///
/// `dependencies` must be the same approved dependency artifacts the render will
/// dispatch with. The preflight loads the staged plug-in natively, so a plug-in
/// that imports an approved helper DLL fails the preflight unless that DLL is
/// sealed next to it here too (#301 review).
pub fn prepare_gpu_runtime_policy(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    gpu_backend: RenderGpuBackend,
    policy: RuntimeModulePolicy,
    dependencies: Vec<ApprovedImageArtifact>,
) -> io::Result<PreparedGpuRuntimePolicy> {
    let backend = runtime_backend(gpu_backend)
        .ok_or_else(|| invalid("the CPU backend has no GPU runtime module policy"))?;
    let authorization = prepare_runtime_authorization_transport(repository, &policy, backend)?;
    let session_identity = authorization.session_identity;
    let args_before_plugin = vec!["--gpu-module-report-v1".to_owned()];
    let args_after_plugin = vec![
        approved_sha256.to_ascii_lowercase(),
        "--runtime-module-authorization-v1".to_owned(),
        authorization.artifact.path.to_string_lossy().into_owned(),
    ];
    // The manifest rides as a sealed dependency next to the plug-in, exactly like
    // the params-inspect path, so the worker resolves it by basename. The caller's
    // approved dependency artifacts are sealed alongside it: the preflight loads
    // the plug-in natively, so its imports must resolve here just as they do for
    // the render dispatch (#301 review).
    let mut preflight_dependencies = dependencies;
    preflight_dependencies.push(authorization.artifact.clone());
    let isolated = dispatch_approved_image_with_dependencies(
        repository,
        WorkerKind::Smart,
        plugin_path,
        approved_sha256,
        preflight_dependencies,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(30_000)),
    )?;
    // The synchronous dispatch has returned, so the worker has consumed the
    // manifest; drop the transport to remove the temp file.
    drop(authorization);
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "GPU module-audit preflight worker did not succeed (classification: {}, exit_code: {}, stderr: {})",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let stdout = isolated.stdout.trim();
    if stdout.is_empty() {
        return Err(invalid("GPU module-audit preflight produced no report"));
    }
    // The preflight prints a combined report: the DLL-load `module_audit` the
    // secure dispatch already validated (require_module_audit), plus the
    // classified `gpu_module_report` this producer authenticates. Extract the
    // latter and authenticate it on its own.
    let combined: Value = serde_json::from_slice(stdout.as_bytes()).map_err(|error| {
        invalid(format!(
            "GPU module-audit preflight report is not valid JSON: {error}"
        ))
    })?;
    let report = combined.get("gpu_module_report").ok_or_else(|| {
        invalid("GPU module-audit preflight output is missing the gpu_module_report")
    })?;
    // Defense in depth: a report with an empty `modules` array authenticates
    // vacuously (zero modules to validate), so reject it here regardless of the
    // worker's own guarantee. A valid preflight loaded at least one authorized
    // module (the manifest carries at least one for the backend).
    if !report
        .get("modules")
        .and_then(Value::as_array)
        .is_some_and(|modules| !modules.is_empty())
    {
        return Err(invalid(
            "GPU module-audit preflight reported no authorized modules",
        ));
    }
    let module_report_json = serde_json::to_vec(report).map_err(|error| {
        invalid(format!(
            "could not re-serialize the GPU module report: {error}"
        ))
    })?;
    let system32 = canonical_system32()?;
    // Fail-fast: the render path re-authenticates before dispatch, but validate
    // here too so a mismatched policy/report surfaces at prepare time.
    authenticate_gpu_worker_report(
        &module_report_json,
        &session_identity,
        backend,
        WorkerModuleValidation {
            policy: &policy,
            sealed: &[],
            trusted: &[],
            system32: &system32,
        },
    )?;
    Ok(PreparedGpuRuntimePolicy {
        policy,
        module_report_json,
        session_identity,
        system32,
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

    // `layer-session-<nonce>-<index>.rgba` is the resident session's per-layer
    // transport (#268). `layer-<nonce>-<index>.rgba` was the deleted one-shot's
    // (#365); the prefix stays claimable so a file leaked by a crash before that
    // deletion is still reclaimed rather than left behind forever. Both are
    // broker-owned and must be reachable by the stale sweep when a crash skips
    // their normal deletion.
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
/// same fail-closed limits every render entry applies.
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

/// How much of the worker's stderr the extended trace carries out. The tail
/// rather than the head: the interesting end of a failing selector is the last
/// thing it did, and the head is the same start-up lines every run.
///
/// Visible to the crate because `windows_process::STDERR_CAPTURE_LIMIT` has to
/// stay above it: the capture is what this cuts from, so two equal limits
/// leave nothing to cut and this function silently becomes a no-op over the
/// head of the stream (issue #1290).
pub(crate) const MAX_STDERR_TAIL_BYTES: usize = 64 * 1024;

/// The tail of the worker's stderr, or `None` unless `AEXCOMPAT_EXTENDED_DIAG`
/// is set.
fn extended_diagnostics_stderr_tail(stderr: &str) -> Option<String> {
    if std::env::var_os("AEXCOMPAT_EXTENDED_DIAG").is_none() {
        return None;
    }
    Some(stderr_tail(stderr))
}

/// The cut itself, without the environment gate: on a line boundary so the
/// first line is whole, and truncated from the front with a marker rather than
/// silently. Separate from the gate so the composition with the capture's own
/// retention is testable (issue #1290) - the two were individually right while
/// the pair handed the report the head of the stream.
pub(crate) fn stderr_tail(stderr: &str) -> String {
    if stderr.len() <= MAX_STDERR_TAIL_BYTES {
        return stderr.to_owned();
    }
    // Cut forward to the next line rather than at the byte: the index lands on
    // a char boundary (it follows a newline) and on something a reader can
    // parse. Indexing the byte slice is what keeps the search itself from
    // needing a boundary. No newline past the cut leaves an empty tail, which
    // is the honest answer for a single enormous line.
    let cut = stderr.len() - MAX_STDERR_TAIL_BYTES;
    let start = stderr.as_bytes()[cut..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(stderr.len(), |newline| cut + newline + 1);
    format!(
        "[truncated to the last {MAX_STDERR_TAIL_BYTES} bytes]\n{}",
        &stderr[start..]
    )
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
    // The worker's own trace, when the operator asked for one. Everything above
    // is derived from this stream, and a derivation only answers the questions
    // it was written for: a plug-in failing inside a selector leaves its account
    // in the host-callback trace, which nothing else carries out of the worker.
    // Off unless AEXCOMPAT_EXTENDED_DIAG is set, because the trace is unbounded
    // in shape (it can carry paths a report should not) and large.
    if let Some(tail) = extended_diagnostics_stderr_tail(&isolated.stderr) {
        object.insert("stderr_tail".into(), json!(tail));
    }
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
    // Absent when nothing appeared, which is the ordinary case; present the
    // moment a plug-in tried to ask the user something (issue #351). Titles are
    // already path-redacted where they are captured.
    if !isolated.dismissed_windows.is_empty() {
        object.insert(
            "dismissed_windows".into(),
            json!(isolated.dismissed_windows),
        );
    }
    // Recorded at admission when the local worker could not be confirmed
    // current against its sources or build provenance (issue #729). A warning,
    // not a gate: dispatch proceeded, and the observation is bound to the
    // worker hash either way.
    if let Some(reason) = isolated.worker_freshness_warning {
        object.insert("worker_freshness_warning".into(), json!(reason));
    }
    // Recorded when a required module audit could not confirm that only known
    // modules loaded (issue #730). A warning, not a gate: the module list
    // explains observations, it does not decide their validity.
    if let Some(reason) = &isolated.module_audit_warning {
        object.insert("module_audit_warning".into(), json!(reason));
    }
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
        // Per frame, unlike "render" which brackets the whole session. Without
        // these a classic session's frame errors carried no stage at all
        // (issue #722). Only "classic_render" is the plug-in's own selector:
        // "classic_output_resize" is the host refusing the requested output
        // resize before RENDER runs, and "classic_finalize" appears only when
        // the host's own finalize changed the error the selector returned.
        // Keeping three names is the point - a host-side refusal filed under
        // the plug-in's selector is what this issue was about. For the same
        // reason none of them is the "output_validation" that session.rs
        // assigns from `output_pixels_valid`: that is a smart-only check on the
        // pixels that came back, not a refused resize.
        "classic_render",
        "classic_output_resize",
        "classic_finalize",
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
        // Not a selector either: one entry into the Premiere GPU-filter route
        // (xGPUFilterEntry, the VR family), whose `_end` carries a `reason`
        // naming how it ended - `committed` when it produced the frame, or the
        // decline that sent the render back to the ordinary PF path (issue
        // #1271). The route's faults are contained, so without this a plug-in
        // whose GPU route died on entry would land in `rendered` off the PF
        // path with nothing in the record saying the route was tried at all.
        "pr_gpu_route",
        // Not a selector either: the host emitting the effect's input in place
        // of a frame a SmartFX PreRender promised nothing for (an empty
        // `result_rect`; issue #1285). The render selector is skipped, so
        // without this a copied frame would land in `rendered` with nothing in
        // the record saying the plug-in did not draw it. The `_end` carries a
        // `reason`: `input_copied`, or why the host declined to copy.
        "smart_empty_result_passthrough",
        // Not a selector: the worker's own refusal to run with a utility
        // callback table whose entries do not sit at the offsets the generated
        // contract names for them. It aborts immediately after, so without this
        // the abort reads as an unattributed 0xC0000409 crash (issue #777).
        "utility_table_mismatch",
    ];
    let mut events = Vec::new();
    let mut active_stages: Vec<String> = Vec::new();
    let mut first_failure_stage: Option<String> = None;
    let mut failure_stage: Option<String> = None;
    let mut last_completed_stage: Option<String> = None;
    let mut plugin_kind: Option<&str> = None;
    let mut minidump: Option<String> = None;
    let load_failure = load_failure_marker(stderr, exit_code);
    let mut suite_acquire_failures = suite_acquire_failures(stderr);
    let mut callback_addr_denials = callback_addr_denials(stderr);
    let mut callback_denials = callback_denials(stderr);

    for line in stderr.lines() {
        plugin_kind = plugin_kind.or_else(|| match line.trim() {
            "plugin_kind:aegp_candidate" => Some("aegp_candidate"),
            "plugin_kind:invalid_pipl" => Some("invalid_pipl"),
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
        if !allowed.contains(&stage) {
            continue;
        }
        let errors = detail
            .split_whitespace()
            .filter_map(|item| {
                let (name, value) = item.split_once('=')?;
                match name {
                    "error" | "pre_error" | "render_error" => {
                        value.parse::<i64>().ok().map(|value| (name, json!(value)))
                    }
                    // Why the host refused, when the numeric code alone cannot
                    // say: the classic output resize has several refusals that
                    // all report 4, and without this they are distinguishable
                    // only in a stderr tail that most runs do not carry
                    // (issue #984).
                    //
                    // Shape-checked, not merely length-capped. The plug-in
                    // shares the worker's stderr and can print any `stage:` line
                    // it likes, so an unconstrained value would let plug-in
                    // authored text - a user's file path, say - into a report
                    // the repository treats as shareable. Every reason the host
                    // emits is a lower-case identifier, which is what this
                    // admits; anything else is dropped rather than truncated,
                    // because a truncated path is still a path.
                    "reason"
                        if !value.is_empty()
                            && value.len() <= 32
                            && value
                                .bytes()
                                .all(|byte| byte.is_ascii_lowercase() || byte == b'_') =>
                    {
                        Some((name, json!(value)))
                    }
                    _ => None,
                }
            })
            .map(|(name, value)| (name.to_owned(), value))
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
        // The cap bounds the reported list, not the failure tracking above: a
        // long session used to stop noticing failures entirely once the list
        // filled, which is exactly when a frame that fails every time overruns
        // it (issue #722).
        if events.len() < MAX_STAGE_EVENTS {
            events.push(json!({"stage": stage, "state": state, "errors": errors}));
        }
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
        "missing_suites_truncated": false,
        // What the worker refused to hand out, read off its own stderr rather
        // than out of its final report. `missing_suites` comes from the report
        // and is filled by `propagate_missing_suites`, which the inspection and
        // image-render paths call and the render session's close does not - and
        // a session that ends on a frame error often has no parsable report at
        // all, so on exactly the runs a sweep is classifying, the suite a
        // plug-in could not acquire was reaching nobody (issue #957).
        "suite_acquire_failures": Value::Array(std::mem::take(&mut suite_acquire_failures.0)),
        "suite_acquire_failures_truncated": suite_acquire_failures.1,
        // The utility `get_callback_addr` requests the worker refused, read off
        // its `stage:callback_addr_denied` lines the same way. A refused id is
        // what a `frame_error:516` frame otherwise cannot attribute: the
        // plug-in answers PF_Err_BAD_CALLBACK_PARAM and says nothing, and the
        // requested id was recoverable only by disassembly (issue #985).
        "callback_addr_denials": Value::Array(std::mem::take(&mut callback_addr_denials.0)),
        "callback_addr_denials_truncated": callback_addr_denials.1,
        // Host-callback refusals with the condition that refused them, same
        // source. Which callback answered 516 is in the extended-diag trace;
        // which of its checks said no is what this carries (issue #995).
        "callback_denials": Value::Array(std::mem::take(&mut callback_denials.0)),
        "callback_denials_truncated": callback_denials.1,
        "unsupported_suite_calls": [],
        "unsupported_suite_calls_truncated": false,
        "callback_history": [],
        "suite_call_slot_probe": null,
        "suite_timeline": [],
        "suite_timeline_truncated": false,
        "plugin_kind": plugin_kind,
        "minidump": minidump,
        "load_failure": load_failure,
    })
}

/// How many distinct suites a single run may report as unacquirable. A plug-in
/// probing versions downward asks for several in a row, and a malformed stream
/// must not be able to grow the diagnostic without bound.
const MAX_SUITE_ACQUIRE_FAILURES: usize = 32;

/// The suites the worker refused, from its `stage:suite_acquire_failed` lines,
/// and whether the list was cut short. Deduplicated on (name, version): a
/// plug-in that retries the same acquire every frame would otherwise fill the
/// list with one fact.
///
/// The name is held to the same shape the report side accepts, through the same
/// predicate, so a suite name and a line of a plug-in's own chatter cannot be
/// confused; anything else is dropped rather than passed through.
fn suite_acquire_failures(stderr: &str) -> (Vec<Value>, bool) {
    let mut failures: Vec<(String, i64)> = Vec::new();
    let mut truncated = false;
    for line in stderr.lines() {
        let Some(body) = line.trim().strip_prefix("stage:suite_acquire_failed ") else {
            continue;
        };
        let Some((name, version)) = body.split_once(" version=") else {
            continue;
        };
        let Some(name) = name.strip_prefix("name=") else {
            continue;
        };
        // Dropping a line is the same loss as running out of room for it, so
        // it sets the same flag: a name or a version this cannot vouch for is
        // still a suite the worker refused, and a list that hides its own gaps
        // reads as a complete one.
        let Ok(version) = version.trim().parse::<i64>() else {
            truncated = true;
            continue;
        };
        if !schema_safe_suite_name(name) || !(0..=MAX_SUITE_VERSION).contains(&version) {
            truncated = true;
            continue;
        }
        let entry = (name.to_owned(), version);
        if failures.contains(&entry) {
            continue;
        }
        // Reporting the cut, not just making it: a bounded list read as a
        // complete one turns "the sweep did not look further" into "there was
        // nothing further", which is the reading this field exists to prevent.
        if failures.len() >= MAX_SUITE_ACQUIRE_FAILURES {
            truncated = true;
            break;
        }
        failures.push(entry);
    }
    let failures = failures
        .into_iter()
        .map(|(name, version)| json!({ "name": name, "version": version }))
        .collect();
    (failures, truncated)
}

/// How many distinct refused `get_callback_addr` requests a single run may
/// report. One id per call site is the observed shape; the bound exists so a
/// plug-in probing ids in a loop cannot grow the diagnostic without bound.
const MAX_CALLBACK_ADDR_DENIALS: usize = 32;

/// The utility `get_callback_addr` requests the worker refused, from its
/// `stage:callback_addr_denied` lines, and whether the list was cut short.
/// Deduplicated on (id, quality, mode): a plug-in that retries the same
/// request every frame would otherwise fill the list with one fact.
///
/// stderr is mixed worker/plug-in output, so the line is held to the exact
/// shape the worker emits - three named integer fields and nothing else.
/// Anything that does not parse is dropped and flagged rather than passed
/// through, for the same reason `suite_acquire_failures` does it: a value
/// this cannot vouch for must not reach a shareable report.
fn callback_addr_denials(stderr: &str) -> (Vec<Value>, bool) {
    let mut denials: Vec<(i64, i64, i64)> = Vec::new();
    let mut truncated = false;
    for line in stderr.lines() {
        let Some(body) = line.trim().strip_prefix("stage:callback_addr_denied ") else {
            continue;
        };
        let mut fields = body.split_whitespace();
        let mut field = |key: &str, low: i64, high: i64| -> Option<i64> {
            fields
                .next()?
                .strip_prefix(key)?
                .parse::<i64>()
                .ok()
                .filter(|value| (low..=high).contains(value))
        };
        // The worker prints `id` and `quality` as int32 and `mode` as uint32;
        // a value outside those ranges is a fabricated line, not a denial.
        let parsed = (|| {
            Some((
                field("id=", i32::MIN.into(), i32::MAX.into())?,
                field("quality=", i32::MIN.into(), i32::MAX.into())?,
                field("mode=", 0, u32::MAX.into())?,
            ))
        })();
        let (Some(entry), None) = (parsed, fields.next()) else {
            truncated = true;
            continue;
        };
        if denials.contains(&entry) {
            continue;
        }
        if denials.len() >= MAX_CALLBACK_ADDR_DENIALS {
            truncated = true;
            break;
        }
        denials.push(entry);
    }
    let denials = denials
        .into_iter()
        .map(|(id, quality, mode)| json!({ "id": id, "quality": quality, "mode": mode }))
        .collect();
    (denials, truncated)
}

/// Bound and identifier shape shared by `callback_denials`. Both fields are
/// worker-owned vocabulary: a lower-case identifier the emitting call site
/// chose, never plug-in text.
const MAX_CALLBACK_DENIALS: usize = 32;

fn worker_denial_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Host-callback refusals with the condition that refused them, from the
/// worker's `stage:callback_denied callback=<name> reason=<identifier>` lines.
/// The numeric error a refusal answers (usually 516) reaches the plug-in,
/// which typically passes it through as its frame error and says nothing; the
/// callback that refused is visible in the extended-diag trace, but *why* it
/// refused was recoverable only by rebuilding the worker with prints
/// (issue #995, Tile refused by one of transform_world's dozen checks).
///
/// Same discipline as the parsers above: exactly two fields of worker-owned
/// identifier shape, dedup on the pair, bounded, and anything else is dropped
/// and flagged rather than passed through.
fn callback_denials(stderr: &str) -> (Vec<Value>, bool) {
    let mut denials: Vec<(String, String, Option<i64>)> = Vec::new();
    let mut truncated = false;
    for line in stderr.lines() {
        let Some(body) = line.trim().strip_prefix("stage:callback_denied ") else {
            continue;
        };
        let mut fields = body.split_whitespace();
        let parsed = (|| {
            let callback = fields.next()?.strip_prefix("callback=")?;
            let reason = fields.next()?.strip_prefix("reason=")?;
            if !worker_denial_identifier(callback) || !worker_denial_identifier(reason) {
                return None;
            }
            // Where one scalar is the whole story - which transfer mode, how
            // many matrices - the marker carries the plug-in's value. The
            // value is plug-in-authored, so it is admitted only as an integer,
            // and only in the range the worker's call sites can emit (int32,
            // uint32 and uint16 arguments): outside it, the line is a
            // fabrication, not a denial.
            let value =
                match fields.next() {
                    None => None,
                    Some(field) => Some(field.strip_prefix("value=")?.parse::<i64>().ok().filter(
                        |value| (i64::from(i32::MIN)..=i64::from(u32::MAX)).contains(value),
                    )?),
                };
            Some((callback.to_owned(), reason.to_owned(), value))
        })();
        let (Some(entry), None) = (parsed, fields.next()) else {
            truncated = true;
            continue;
        };
        if denials.contains(&entry) {
            continue;
        }
        if denials.len() >= MAX_CALLBACK_DENIALS {
            truncated = true;
            break;
        }
        denials.push(entry);
    }
    let denials = denials
        .into_iter()
        .map(|(callback, reason, value)| match value {
            Some(value) => {
                json!({ "callback": callback, "reason": reason, "value": value })
            }
            None => json!({ "callback": callback, "reason": reason }),
        })
        .collect();
    (denials, truncated)
}

fn load_failure_marker(stderr: &str, exit_code: u32) -> Option<Value> {
    if exit_code != 11 {
        return None;
    }
    stderr.lines().rev().find_map(|line| {
        let body = line.trim().strip_prefix("stage:load_failure ")?;
        let mut fields = body.split_whitespace();
        let stage = fields.next()?.strip_prefix("stage=")?;
        let error = fields.next()?.strip_prefix("win32_error=")?;
        if fields.next().is_some()
            || !matches!(
                stage,
                "set_default_dll_directories" | "add_dll_directory" | "load_library"
            )
            || error.is_empty()
            || !error.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let win32_error_code = error.parse::<u32>().ok().filter(|code| *code != 0)?;
        Some(json!({
            "stage": stage,
            "win32_error_code": win32_error_code,
        }))
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

fn schema_safe_suite_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=MAX_SUITE_NAME_LEN).contains(&bytes.len())
        && bytes[0].is_ascii_alphabetic()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'_' | b'-'))
}

fn schema_safe_suite_selector(selector: &str) -> bool {
    let bytes = selector.as_bytes();
    (1..=MAX_SUITE_NAME_LEN).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'_' | b'-'))
}

fn reported_truncation(worker_report: &Value, key: &str) -> bool {
    match worker_report.get(key) {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => true,
    }
}

fn propagate_missing_suites(diagnostics: &mut Value, worker_report: &Value) {
    let mut suites = Vec::new();
    let mut seen = BTreeSet::new();
    let mut truncated = reported_truncation(worker_report, "missing_suites_truncated");
    let reported = match worker_report.get("missing_suites") {
        Some(Value::Array(reported)) => reported.as_slice(),
        Some(_) => {
            truncated = true;
            &[]
        }
        None => &[],
    };
    for suite in reported {
        if suites.len() >= MAX_MISSING_SUITES {
            truncated = true;
            break;
        }
        let Some(name) = suite["name"]
            .as_str()
            .filter(|name| schema_safe_suite_name(name))
        else {
            truncated = true;
            continue;
        };
        let Some(version) = suite["version"]
            .as_i64()
            .filter(|version| *version > 0 && *version <= MAX_SUITE_VERSION)
        else {
            truncated = true;
            continue;
        };
        if seen.insert((name.to_owned(), version)) {
            suites.push(json!({"name": name, "version": version}));
        }
    }
    diagnostics["missing_suites"] = Value::Array(suites);
    diagnostics["missing_suites_truncated"] = Value::Bool(truncated);
}

fn propagate_unsupported_suite_calls(diagnostics: &mut Value, worker_report: &Value) {
    let mut calls = Vec::new();
    let mut seen = BTreeSet::new();
    let mut truncated = reported_truncation(worker_report, "unsupported_suite_calls_truncated");
    let reported = match worker_report.get("unsupported_suite_calls") {
        Some(Value::Array(reported)) => reported.as_slice(),
        Some(_) => {
            truncated = true;
            &[]
        }
        None => &[],
    };
    for call in reported {
        if calls.len() >= MAX_UNSUPPORTED_SUITE_CALLS {
            truncated = true;
            break;
        }
        let Some(name) = call["name"]
            .as_str()
            .filter(|name| schema_safe_suite_name(name))
        else {
            truncated = true;
            continue;
        };
        let Some(version) = call["version"]
            .as_i64()
            .filter(|version| *version > 0 && *version <= MAX_SUITE_VERSION)
        else {
            truncated = true;
            continue;
        };
        let Some(slot) = call["slot"]
            .as_u64()
            .filter(|slot| *slot <= MAX_UNSUPPORTED_SUITE_SLOT)
        else {
            truncated = true;
            continue;
        };
        let Some(call_count) = call["call_count"]
            .as_u64()
            .filter(|count| *count > 0 && *count <= u32::MAX as u64)
        else {
            truncated = true;
            continue;
        };
        if seen.insert((name.to_owned(), version, slot)) {
            calls.push(json!({
                "name": name,
                "version": version,
                "slot": slot,
                "call_count": call_count,
            }));
        }
    }
    diagnostics["unsupported_suite_calls"] = Value::Array(calls);
    diagnostics["unsupported_suite_calls_truncated"] = Value::Bool(truncated);
    let history = worker_report
        .get("callback_history")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .rev()
                .take(32)
                .rev()
                .filter_map(|entry| {
                    let sequence = entry.get("sequence")?.as_u64()?;
                    let callback = entry.get("callback")?.as_str()?;
                    let result = entry.get("result")?.as_i64()?;
                    let reason = entry.get("reason")?.as_str()?;
                    if callback.len() > 64
                        || reason.len() > 64
                        || result < i32::MIN as i64
                        || result > i32::MAX as i64
                    {
                        return None;
                    }
                    Some(json!({
                        "sequence": sequence,
                        "callback": callback,
                        "result": result,
                        "reason": reason,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    diagnostics["callback_history"] = Value::Array(history);
}

fn probe_hex(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| {
        text.len() == 18
            && text.starts_with("0x")
            && text[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn probe_argument_shape(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|shape| matches!(*shape, "zero" | "nonzero"))
}

fn schema_safe_module_basename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 260
        && value.is_ascii()
        && !value.contains(['/', '\\', ':'])
        && !value.chars().any(char::is_control)
        && !value.ends_with(['.', ' '])
}

fn safe_pointer_classification(value: &Value) -> Option<Value> {
    let pointer = value.as_object()?;
    let classification = pointer.get("classification")?.as_str()?;
    let module = pointer.get("module")?;
    let relative_offset = pointer.get("relative_offset")?;
    let token = pointer.get("token")?;
    match classification {
        "null" | "low" if module.is_null() && relative_offset.is_null() && token.is_null() => {
            Some(json!({
                "classification": classification,
                "module": null,
                "relative_offset": null,
                "token": null,
            }))
        }
        "plugin" | "module"
            if module.as_str().is_some_and(schema_safe_module_basename)
                && probe_hex(relative_offset).is_some()
                && token.is_null() =>
        {
            Some(json!({
                "classification": classification,
                "module": module.as_str().unwrap(),
                "relative_offset": probe_hex(relative_offset)
                    .unwrap()
                    .to_ascii_lowercase(),
                "token": null,
            }))
        }
        "heap_or_unknown"
            if module.is_null()
                && relative_offset.is_null()
                && token.as_str().is_some_and(|token| {
                    token.len() == 20
                        && token.starts_with("ptr-")
                        && token[4..].bytes().all(|byte| byte.is_ascii_hexdigit())
                }) =>
        {
            Some(json!({
                "classification": classification,
                "module": null,
                "relative_offset": null,
                "token": token.as_str().unwrap().to_ascii_lowercase(),
            }))
        }
        _ => None,
    }
}

fn safe_process_local_pointer_token(value: &Value) -> Option<&str> {
    value.as_str().filter(|token| {
        token.len() == 20
            && token.starts_with("ptr-")
            && token[4..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn safe_global_data_state(value: &Value) -> Option<Value> {
    let state = value.as_object().filter(|state| state.len() == 3)?;
    let presence = state.get("state")?.as_str()?;
    let classification = state.get("classification")?.as_str()?;
    let token = state.get("process_local_token")?;
    match presence {
        "null" if classification == "null" && token.is_null() => Some(json!({
            "state": "null",
            "classification": "null",
            "process_local_token": null,
        })),
        "non_null"
            if matches!(
                classification,
                "low" | "plugin" | "module" | "heap_or_unknown"
            ) =>
        {
            Some(json!({
                "state": "non_null",
                "classification": classification,
                "process_local_token": safe_process_local_pointer_token(token)?,
            }))
        }
        _ => None,
    }
}

fn safe_global_data_handoff(value: &Value) -> Option<Value> {
    let handoff = value.as_object().filter(|handoff| handoff.len() == 3)?;
    let normalize_state = |name| match handoff.get(name)? {
        Value::Null => Some(Value::Null),
        value => safe_global_data_state(value),
    };
    let input_at_entry = normalize_state("input_at_entry")?;
    let output_after_return = normalize_state("output_after_return")?;
    let same_identity = match handoff.get("same_identity_as_previous_output")? {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        _ => return None,
    };
    Some(json!({
        "input_at_entry": input_at_entry,
        "output_after_return": output_after_return,
        "same_identity_as_previous_output": same_identity,
    }))
}

fn safe_effect_ref_entry(value: &Value) -> Option<Value> {
    if value.is_null() {
        return Some(Value::Null);
    }
    let entry = value.as_object().filter(|entry| entry.len() == 4)?;
    let state = safe_global_data_state(&json!({
        "state": entry.get("state")?,
        "classification": entry.get("classification")?,
        "process_local_token": entry.get("process_local_token")?,
    }))?;
    let same_identity = match entry.get("same_identity_as_global_setup_entry")? {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        _ => return None,
    };
    Some(json!({
        "state": state["state"],
        "classification": state["classification"],
        "process_local_token": state["process_local_token"],
        "same_identity_as_global_setup_entry": same_identity,
    }))
}

fn safe_application_id_entry(value: &Value) -> Option<Value> {
    if value.is_null() {
        return Some(Value::Null);
    }
    let entry = value.as_object().filter(|entry| entry.len() == 4)?;
    let hex = entry.get("hex_u32")?.as_str().filter(|hex| {
        hex.len() == 10
            && hex.starts_with("0x")
            && hex[2..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })?;
    let numeric = u32::from_str_radix(&hex[2..], 16).ok()?;
    let mut expected_code = String::new();
    for shift in [24, 16, 8, 0] {
        let byte = ((numeric >> shift) & 0xff) as u8;
        if (0x20..=0x7e).contains(&byte) {
            expected_code.push(char::from(byte));
        } else {
            expected_code.push_str(&format!("\\x{byte:02x}"));
        }
    }
    let printable_code = entry
        .get("printable_code")?
        .as_str()
        .filter(|code| *code == expected_code)?;
    let same_value = match entry.get("same_value_as_global_setup_entry")? {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        _ => return None,
    };
    if entry.get("host_setting_source")?.as_str() != Some("worker_effect_bootstrap") {
        return None;
    }
    Some(json!({
        "printable_code": printable_code,
        "hex_u32": hex,
        "same_value_as_global_setup_entry": same_value,
        "host_setting_source": "worker_effect_bootstrap",
    }))
}

fn safe_spec_version_entry(value: &Value) -> Option<Value> {
    if value.is_null() {
        return Some(Value::Null);
    }
    let entry = value.as_object().filter(|entry| entry.len() == 5)?;
    let raw = entry.get("raw_packed_u32")?.as_str().filter(|raw| {
        raw.len() == 10
            && raw.starts_with("0x")
            && raw[2..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })?;
    let numeric = u32::from_str_radix(&raw[2..], 16).ok()?;
    let major = entry
        .get("major")?
        .as_i64()
        .filter(|value| i16::try_from(*value).is_ok())?;
    let minor = entry
        .get("minor")?
        .as_i64()
        .filter(|value| i16::try_from(*value).is_ok())?;
    if i16::try_from(major).ok()? != (numeric as u16) as i16
        || i16::try_from(minor).ok()? != ((numeric >> 16) as u16) as i16
    {
        return None;
    }
    let same_value = match entry.get("same_value_as_global_setup_entry")? {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        _ => return None,
    };
    if entry.get("host_setting_source")?.as_str() != Some("worker_effect_bootstrap") {
        return None;
    }
    Some(json!({
        "raw_packed_u32": raw,
        "major": major,
        "minor": minor,
        "same_value_as_global_setup_entry": same_value,
        "host_setting_source": "worker_effect_bootstrap",
    }))
}

fn propagate_suite_call_slot_probe(diagnostics: &mut Value, worker_report: &Value) {
    let Some(probe) = worker_report
        .get("suite_call_slot_probe")
        .and_then(Value::as_object)
    else {
        return;
    };
    let Some(enabled) = probe.get("enabled").and_then(Value::as_bool) else {
        return;
    };
    let Some(slot_count) = probe
        .get("slot_count")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0 && *count <= MAX_SUITE_CALL_SLOT_PROBE_SLOTS)
    else {
        return;
    };
    let Some(maximum_targets) = probe
        .get("maximum_targets")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0 && *count <= MAX_SUITE_CALL_SLOT_PROBE_TARGETS as u64)
    else {
        return;
    };
    let Some(reported_targets) = probe.get("targets").and_then(Value::as_array) else {
        return;
    };
    let mut configuration_truncated = probe
        .get("configuration_truncated")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut targets = Vec::new();
    let mut seen_targets = BTreeSet::new();
    for target in reported_targets {
        if targets.len() >= maximum_targets as usize {
            configuration_truncated = true;
            break;
        }
        let Some(name) = target
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| schema_safe_suite_name(name))
        else {
            configuration_truncated = true;
            continue;
        };
        let Some(version) = target
            .get("version")
            .and_then(Value::as_i64)
            .filter(|version| {
                *version > 0
                    && *version <= MAX_SUITE_VERSION
                    && seen_targets.insert((name.to_owned(), *version))
            })
        else {
            configuration_truncated = true;
            continue;
        };
        let Some(target_enabled) = target.get("enabled").and_then(Value::as_bool) else {
            configuration_truncated = true;
            continue;
        };
        let Some(reported_calls) = target.get("calls").and_then(Value::as_array) else {
            configuration_truncated = true;
            continue;
        };
        let mut target_truncated = target
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let mut calls = Vec::new();
        let mut seen_slots = BTreeSet::new();
        for call in reported_calls {
            if calls.len() >= slot_count as usize {
                target_truncated = true;
                break;
            }
            let Some(slot) = call
                .get("slot")
                .and_then(Value::as_u64)
                .filter(|slot| *slot < slot_count && seen_slots.insert(*slot))
            else {
                target_truncated = true;
                continue;
            };
            let Some(call_count) = call
                .get("call_count")
                .and_then(Value::as_u64)
                .filter(|count| *count > 0 && *count <= u32::MAX as u64)
            else {
                target_truncated = true;
                continue;
            };
            let Some(exception_code) = call
                .get("exception_code")
                .and_then(Value::as_u64)
                .filter(|code| *code <= u32::MAX as u64)
            else {
                target_truncated = true;
                continue;
            };
            let Some(argument_word_count) = call
                .get("argument_word_count")
                .and_then(Value::as_u64)
                .filter(|count| *count == 8)
            else {
                target_truncated = true;
                continue;
            };
            let Some(nonzero_word_count) = call
                .get("nonzero_word_count")
                .and_then(Value::as_u64)
                .filter(|count| *count <= argument_word_count)
            else {
                target_truncated = true;
                continue;
            };
            let Some(registers) = call.get("registers").and_then(Value::as_object) else {
                target_truncated = true;
                continue;
            };
            let register_values: Option<Vec<&str>> = ["rcx", "rdx", "r8", "r9"]
                .iter()
                .map(|name| registers.get(*name).and_then(probe_argument_shape))
                .collect();
            let Some(register_values) = register_values else {
                target_truncated = true;
                continue;
            };
            let Some(stack) = call
                .get("stack")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 4)
                .and_then(|values| {
                    values
                        .iter()
                        .map(probe_argument_shape)
                        .collect::<Option<Vec<_>>>()
                })
            else {
                target_truncated = true;
                continue;
            };
            let observed_nonzero_count = register_values
                .iter()
                .chain(stack.iter())
                .filter(|shape| **shape == "nonzero")
                .count() as u64;
            if observed_nonzero_count != nonzero_word_count {
                target_truncated = true;
                continue;
            }
            let caller_rva = match call.get("caller_rva") {
                Some(Value::Null) => Value::Null,
                Some(value) => match probe_hex(value) {
                    Some(value) => json!(value),
                    None => {
                        target_truncated = true;
                        continue;
                    }
                },
                None => {
                    target_truncated = true;
                    continue;
                }
            };
            calls.push(json!({
                "slot": slot,
                "call_count": call_count,
                "exception_code": exception_code,
                "argument_word_count": argument_word_count,
                "nonzero_word_count": nonzero_word_count,
                "registers": {
                    "rcx": register_values[0],
                    "rdx": register_values[1],
                    "r8": register_values[2],
                    "r9": register_values[3],
                },
                "stack": stack,
                "caller_rva": caller_rva,
            }));
        }
        targets.push(json!({
            "name": name,
            "version": version,
            "enabled": target_enabled,
            "calls": calls,
            "truncated": target_truncated,
        }));
    }
    diagnostics["suite_call_slot_probe"] = json!({
        "enabled": enabled,
        "slot_count": slot_count,
        "maximum_targets": maximum_targets,
        "targets": targets,
        "configuration_truncated": configuration_truncated,
    });
}

fn propagate_selector_invocations(diagnostics: &mut Value, worker_report: &Value) {
    let Some(container) = worker_report
        .get("selector_invocations")
        .and_then(Value::as_object)
    else {
        return;
    };
    if container.get("maximum_records").and_then(Value::as_u64)
        != Some(MAX_SELECTOR_INVOCATIONS as u64)
    {
        return;
    }
    let Some(reported) = container.get("records").and_then(Value::as_array) else {
        return;
    };
    let Some(reported_truncated) = container.get("truncated").and_then(Value::as_bool) else {
        return;
    };
    let safe_i32 = |value: &Value| {
        value
            .as_i64()
            .filter(|number| *number >= i32::MIN as i64 && *number <= i32::MAX as i64)
    };
    let mut records = Vec::new();
    let mut truncated = reported_truncated || reported.len() > MAX_SELECTOR_INVOCATIONS;
    for record in reported.iter().take(MAX_SELECTOR_INVOCATIONS) {
        let Some(selector) = record
            .get("selector")
            .and_then(Value::as_str)
            .filter(|selector| schema_safe_suite_selector(selector))
        else {
            truncated = true;
            continue;
        };
        let Some(completed_normally) = record
            .get("invocation_completed_normally")
            .and_then(Value::as_bool)
        else {
            truncated = true;
            continue;
        };
        let raw_return_code = match record.get("raw_return_code") {
            Some(Value::Null) if !completed_normally => Value::Null,
            Some(value) if completed_normally => {
                let Some(raw) = safe_i32(value) else {
                    truncated = true;
                    continue;
                };
                json!(raw)
            }
            _ => {
                truncated = true;
                continue;
            }
        };
        let Some(host_result_code) = record.get("host_result_code").and_then(safe_i32) else {
            truncated = true;
            continue;
        };
        let Some(seh_caught) = record.get("seh_caught").and_then(Value::as_bool) else {
            truncated = true;
            continue;
        };
        let seh_code = match record.get("seh_code") {
            Some(Value::Null) if !seh_caught => Value::Null,
            Some(value) if seh_caught => {
                let Some(code) = value
                    .as_u64()
                    .filter(|code| *code > 0 && *code <= u32::MAX as u64)
                else {
                    truncated = true;
                    continue;
                };
                json!(code)
            }
            _ => {
                truncated = true;
                continue;
            }
        };
        let fault_module_class = match record.get("fault_module_class") {
            Some(Value::Null) if !seh_caught => Value::Null,
            Some(Value::String(classification))
                if seh_caught
                    && matches!(
                        classification.as_str(),
                        "plugin" | "worker" | "other_module" | "unmapped" | "unknown"
                    ) =>
            {
                json!(classification)
            }
            _ => {
                truncated = true;
                continue;
            }
        };
        let fault_module = match record.get("fault_module") {
            Some(Value::Null) if !seh_caught => Value::Null,
            Some(Value::Null) if seh_caught => Value::Null,
            Some(Value::String(module)) if seh_caught && schema_safe_module_basename(module) => {
                json!(module)
            }
            _ => {
                truncated = true;
                continue;
            }
        };
        let plugin_rva = match record.get("plugin_rva") {
            Some(Value::Null) => Value::Null,
            Some(value)
                if fault_module_class.as_str() == Some("plugin") && probe_hex(value).is_some() =>
            {
                json!(probe_hex(value).unwrap().to_ascii_lowercase())
            }
            _ => {
                truncated = true;
                continue;
            }
        };
        let access_type = match record.get("access_type") {
            Some(Value::Null) => Value::Null,
            Some(Value::String(access_type))
                if matches!(
                    access_type.as_str(),
                    "read" | "write" | "execute" | "unknown"
                ) =>
            {
                json!(access_type)
            }
            _ => {
                truncated = true;
                continue;
            }
        };
        let fault_address = match record.get("fault_address") {
            Some(Value::Null) => Value::Null,
            Some(value) => {
                let Some(pointer) = safe_pointer_classification(value) else {
                    truncated = true;
                    continue;
                };
                pointer
            }
            None => {
                truncated = true;
                continue;
            }
        };
        let registers = match record.get("registers") {
            Some(Value::Null) => Value::Null,
            Some(Value::Object(registers)) if registers.len() == 5 => {
                let mut normalized = serde_json::Map::new();
                let mut valid = true;
                for name in ["rcx", "rdx", "r8", "r9", "rsp"] {
                    let Some(value) = registers.get(name).and_then(safe_pointer_classification)
                    else {
                        valid = false;
                        break;
                    };
                    normalized.insert(name.to_owned(), value);
                }
                if !valid {
                    truncated = true;
                    continue;
                }
                Value::Object(normalized)
            }
            _ => {
                truncated = true;
                continue;
            }
        };
        let stack_pointer_values = match record.get("stack_pointer_values") {
            Some(Value::Null) => Value::Null,
            Some(Value::Array(values)) if values.len() == 6 => {
                let mut normalized = Vec::with_capacity(values.len());
                let mut valid = true;
                for (index, value) in values.iter().enumerate() {
                    if value.get("offset_bytes").and_then(Value::as_u64) != Some((index * 8) as u64)
                    {
                        valid = false;
                        break;
                    }
                    let pointer = match value.get("value") {
                        Some(Value::Null) => Value::Null,
                        Some(value) => {
                            let Some(pointer) = safe_pointer_classification(value) else {
                                valid = false;
                                break;
                            };
                            pointer
                        }
                        None => {
                            valid = false;
                            break;
                        }
                    };
                    normalized.push(json!({
                        "offset_bytes": index * 8,
                        "value": pointer,
                    }));
                }
                if !valid {
                    truncated = true;
                    continue;
                }
                Value::Array(normalized)
            }
            _ => {
                truncated = true;
                continue;
            }
        };
        let Some(global_data_handoff) = record
            .get("global_data_handoff")
            .and_then(safe_global_data_handoff)
        else {
            truncated = true;
            continue;
        };
        let Some(effect_ref_at_entry) = record
            .get("effect_ref_at_entry")
            .and_then(safe_effect_ref_entry)
        else {
            truncated = true;
            continue;
        };
        let Some(appl_id_at_entry) = record
            .get("appl_id_at_entry")
            .and_then(safe_application_id_entry)
        else {
            truncated = true;
            continue;
        };
        let Some(version_at_entry) = record
            .get("version_at_entry")
            .and_then(safe_spec_version_entry)
        else {
            truncated = true;
            continue;
        };
        records.push(json!({
            "selector": selector,
            "invocation_completed_normally": completed_normally,
            "raw_return_code": raw_return_code,
            "host_result_code": host_result_code,
            "seh_caught": seh_caught,
            "seh_code": seh_code,
            "fault_module_class": fault_module_class,
            "fault_module": fault_module,
            "plugin_rva": plugin_rva,
            "access_type": access_type,
            "fault_address": fault_address,
            "registers": registers,
            "stack_pointer_values": stack_pointer_values,
            "global_data_handoff": global_data_handoff,
            "effect_ref_at_entry": effect_ref_at_entry,
            "appl_id_at_entry": appl_id_at_entry,
            "version_at_entry": version_at_entry,
        }));
    }
    diagnostics["selector_invocations"] = json!({
        "maximum_records": MAX_SELECTOR_INVOCATIONS,
        "records": records,
        "truncated": truncated,
    });
}

fn schema_safe_callback_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn propagate_host_callback_timeline(diagnostics: &mut Value, worker_report: &Value) {
    let Some(container) = worker_report
        .get("host_callback_timeline")
        .and_then(Value::as_object)
        .filter(|container| container.len() == 3)
    else {
        return;
    };
    if container.get("maximum_records").and_then(Value::as_u64)
        != Some(MAX_HOST_CALLBACK_TIMELINE_RECORDS as u64)
    {
        return;
    }
    let Some(reported) = container.get("records").and_then(Value::as_array) else {
        return;
    };
    let Some(reported_truncated) = container.get("truncated").and_then(Value::as_bool) else {
        return;
    };
    let mut records = Vec::new();
    let mut truncated = reported_truncated || reported.len() > MAX_HOST_CALLBACK_TIMELINE_RECORDS;
    let mut previous_sequence = None;
    for record in reported.iter().take(MAX_HOST_CALLBACK_TIMELINE_RECORDS) {
        let Some(record) = record.as_object().filter(|record| record.len() == 7) else {
            truncated = true;
            continue;
        };
        let Some(sequence) = record
            .get("sequence")
            .and_then(Value::as_u64)
            .filter(|sequence| *sequence <= u32::MAX as u64)
            .filter(|sequence| previous_sequence.is_none_or(|previous| *sequence > previous))
        else {
            truncated = true;
            continue;
        };
        let Some(callback) = record
            .get("callback")
            .and_then(Value::as_str)
            .filter(|callback| schema_safe_callback_id(callback))
        else {
            truncated = true;
            continue;
        };
        let Some(selector) = record
            .get("selector")
            .and_then(Value::as_str)
            .filter(|selector| matches!(*selector, "GLOBAL_SETUP" | "PARAMS_SETUP"))
        else {
            truncated = true;
            continue;
        };
        let Some(call_count) = record
            .get("call_count")
            .and_then(Value::as_u64)
            .filter(|count| *count > 0 && *count <= u32::MAX as u64)
        else {
            truncated = true;
            continue;
        };
        let Some(status) = record
            .get("status")
            .and_then(Value::as_str)
            .filter(|status| matches!(*status, "success" | "failure"))
        else {
            truncated = true;
            continue;
        };
        let Some(return_code) = record
            .get("return_code")
            .and_then(Value::as_i64)
            .filter(|code| *code >= i32::MIN as i64 && *code <= i32::MAX as i64)
        else {
            truncated = true;
            continue;
        };
        if (status == "success") != (return_code == 0) {
            truncated = true;
            continue;
        }
        let Some(classification) =
            record
                .get("classification")
                .and_then(Value::as_str)
                .filter(|classification| {
                    matches!(*classification, "implemented" | "unsupported" | "fallback")
                })
        else {
            truncated = true;
            continue;
        };
        previous_sequence = Some(sequence);
        records.push(json!({
            "sequence": sequence,
            "callback": callback,
            "selector": selector,
            "call_count": call_count,
            "status": status,
            "return_code": return_code,
            "classification": classification,
        }));
    }
    diagnostics["host_callback_timeline"] = json!({
        "maximum_records": MAX_HOST_CALLBACK_TIMELINE_RECORDS,
        "records": records,
        "truncated": truncated,
    });
}

fn fail_closed_extended_lookup_timeline(diagnostics: &mut Value) {
    diagnostics["extended_lookup_timeline"] = json!({
        "maximum_records": MAX_EXTENDED_LOOKUP_TIMELINE_RECORDS,
        "records": [],
        "truncated": true,
    });
}

fn fail_closed_compute_cache_timeline(diagnostics: &mut Value) {
    diagnostics["compute_cache_timeline"] = json!({
        "maximum_records": MAX_COMPUTE_CACHE_TIMELINE_RECORDS,
        "records": [],
        "truncated": true,
    });
}

fn propagate_compute_cache_timeline(diagnostics: &mut Value, worker_report: &Value) {
    let Some(reported_container) = worker_report.get("compute_cache_timeline") else {
        return;
    };
    let Some(container) = reported_container.as_object().filter(|container| {
        container.len() == 3
            && container.contains_key("maximum_records")
            && container.contains_key("records")
            && container.contains_key("truncated")
    }) else {
        fail_closed_compute_cache_timeline(diagnostics);
        return;
    };
    if container.get("maximum_records").and_then(Value::as_u64)
        != Some(MAX_COMPUTE_CACHE_TIMELINE_RECORDS as u64)
    {
        fail_closed_compute_cache_timeline(diagnostics);
        return;
    }
    let Some(reported) = container.get("records").and_then(Value::as_array) else {
        fail_closed_compute_cache_timeline(diagnostics);
        return;
    };
    let Some(reported_truncated) = container.get("truncated").and_then(Value::as_bool) else {
        fail_closed_compute_cache_timeline(diagnostics);
        return;
    };
    let mut records = Vec::new();
    let mut truncated = reported_truncated || reported.len() > MAX_COMPUTE_CACHE_TIMELINE_RECORDS;
    let mut previous_sequence = None;
    for record in reported.iter().take(MAX_COMPUTE_CACHE_TIMELINE_RECORDS) {
        let Some(record) = record.as_object().filter(|record| {
            record.len() == 7
                && record.contains_key("sequence")
                && record.contains_key("selector")
                && record.contains_key("slot")
                && record.contains_key("operation")
                && record.contains_key("outcome")
                && record.contains_key("return_code")
                && record.contains_key("call_count")
        }) else {
            truncated = true;
            continue;
        };
        let Some(sequence) = record
            .get("sequence")
            .and_then(Value::as_u64)
            .filter(|value| *value <= u32::MAX as u64)
        else {
            truncated = true;
            continue;
        };
        if previous_sequence.is_some_and(|previous| sequence <= previous) {
            truncated = true;
            continue;
        }
        let Some(selector) = record
            .get("selector")
            .and_then(Value::as_str)
            .filter(|selector| schema_safe_suite_selector(selector))
        else {
            truncated = true;
            continue;
        };
        let Some(slot) = record
            .get("slot")
            .and_then(Value::as_u64)
            .filter(|slot| *slot <= 5)
        else {
            truncated = true;
            continue;
        };
        let Some(operation) = record
            .get("operation")
            .and_then(Value::as_str)
            .filter(|operation| {
                matches!(
                    *operation,
                    "class_register"
                        | "class_unregister"
                        | "compute_if_needed_and_checkout"
                        | "checkout_cached"
                        | "get_receipt_compute_value"
                        | "checkin_compute_receipt"
                        | "global_teardown"
                )
            })
        else {
            truncated = true;
            continue;
        };
        let Some(outcome) = record
            .get("outcome")
            .and_then(Value::as_str)
            .filter(|outcome| {
                matches!(
                    *outcome,
                    "registered"
                        | "unregistered"
                        | "computed"
                        | "cache_hit"
                        | "cache_miss"
                        | "compute_pending"
                        | "value_returned"
                        | "checked_in"
                        | "invalid"
                        | "callback_failure"
                        | "capacity_failure"
                        | "cleanup"
                        | "cleanup_deferred"
                )
            })
        else {
            truncated = true;
            continue;
        };
        let Some(return_code) = record
            .get("return_code")
            .and_then(Value::as_i64)
            .filter(|value| *value >= i32::MIN as i64 && *value <= i32::MAX as i64)
        else {
            truncated = true;
            continue;
        };
        let Some(call_count) = record
            .get("call_count")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0 && *value <= u32::MAX as u64)
        else {
            truncated = true;
            continue;
        };
        previous_sequence = Some(sequence);
        records.push(json!({
            "sequence": sequence,
            "selector": selector,
            "slot": slot,
            "operation": operation,
            "outcome": outcome,
            "return_code": return_code,
            "call_count": call_count,
        }));
    }
    diagnostics["compute_cache_timeline"] = json!({
        "maximum_records": MAX_COMPUTE_CACHE_TIMELINE_RECORDS,
        "records": records,
        "truncated": truncated,
    });
}

fn propagate_extended_lookup_timeline(diagnostics: &mut Value, worker_report: &Value) {
    let Some(reported_container) = worker_report.get("extended_lookup_timeline") else {
        return;
    };
    let Some(container) = reported_container.as_object().filter(|container| {
        container.len() == 3
            && container.contains_key("maximum_records")
            && container.contains_key("records")
            && container.contains_key("truncated")
    }) else {
        fail_closed_extended_lookup_timeline(diagnostics);
        return;
    };
    if container.get("maximum_records").and_then(Value::as_u64)
        != Some(MAX_EXTENDED_LOOKUP_TIMELINE_RECORDS as u64)
    {
        fail_closed_extended_lookup_timeline(diagnostics);
        return;
    }
    let Some(reported) = container.get("records").and_then(Value::as_array) else {
        fail_closed_extended_lookup_timeline(diagnostics);
        return;
    };
    let Some(reported_truncated) = container.get("truncated").and_then(Value::as_bool) else {
        fail_closed_extended_lookup_timeline(diagnostics);
        return;
    };
    let mut records = Vec::new();
    let mut truncated = reported_truncated || reported.len() > MAX_EXTENDED_LOOKUP_TIMELINE_RECORDS;
    let mut previous_sequence = None;
    for record in reported.iter().take(MAX_EXTENDED_LOOKUP_TIMELINE_RECORDS) {
        let Some(record) = record.as_object().filter(|record| {
            record.len() == 9
                && record.contains_key("sequence")
                && record.contains_key("selector")
                && record.contains_key("call_count")
                && record.contains_key("opaque_table_classification")
                && record.contains_key("raw_private_table_state")
                && record.contains_key("windows_resource_source_state")
                && record.contains_key("lookup_id")
                && record.contains_key("outcome")
                && record.contains_key("return_code")
        }) else {
            truncated = true;
            continue;
        };
        let Some(sequence) = record
            .get("sequence")
            .and_then(Value::as_u64)
            .filter(|sequence| *sequence <= u32::MAX as u64)
            .filter(|sequence| previous_sequence.is_none_or(|previous| *sequence > previous))
        else {
            truncated = true;
            continue;
        };
        let Some(selector) = record
            .get("selector")
            .and_then(Value::as_str)
            .filter(|selector| matches!(*selector, "GLOBAL_SETUP" | "PARAMS_SETUP"))
        else {
            truncated = true;
            continue;
        };
        let Some(call_count) = record
            .get("call_count")
            .and_then(Value::as_u64)
            .filter(|count| *count > 0 && *count <= u32::MAX as u64)
        else {
            truncated = true;
            continue;
        };
        let Some(opaque_table_classification) = record
            .get("opaque_table_classification")
            .and_then(Value::as_str)
            .filter(|classification| {
                matches!(
                    *classification,
                    "null"
                        | "active_effect_module"
                        | "active_resource_module"
                        | "other_loaded_sealed_module"
                        | "other_loaded_system_module"
                        | "unrecognized"
                )
            })
        else {
            truncated = true;
            continue;
        };
        let Some(raw_private_table_state) = record
            .get("raw_private_table_state")
            .and_then(Value::as_str)
            .filter(|state| matches!(*state, "valid" | "none" | "invalid"))
        else {
            truncated = true;
            continue;
        };
        let Some(windows_resource_source_state) = record
            .get("windows_resource_source_state")
            .and_then(Value::as_str)
            .filter(|state| matches!(*state, "valid" | "none" | "invalid"))
        else {
            truncated = true;
            continue;
        };
        let Some(lookup_id) = record
            .get("lookup_id")
            .and_then(Value::as_i64)
            .filter(|id| *id >= i32::MIN as i64 && *id <= i32::MAX as i64)
        else {
            truncated = true;
            continue;
        };
        let Some(outcome) = record
            .get("outcome")
            .and_then(Value::as_str)
            .filter(|outcome| matches!(*outcome, "found" | "missing" | "invalid"))
        else {
            truncated = true;
            continue;
        };
        let Some(return_code) = record
            .get("return_code")
            .and_then(Value::as_i64)
            .filter(|code| *code >= i32::MIN as i64 && *code <= i32::MAX as i64)
        else {
            truncated = true;
            continue;
        };
        let has_valid_source =
            raw_private_table_state == "valid" || windows_resource_source_state == "valid";
        let invariant_valid = match (outcome, return_code) {
            ("found", 0) => has_valid_source,
            ("missing", 4) => {
                has_valid_source
                    || (raw_private_table_state == "none"
                        && windows_resource_source_state == "none")
            }
            ("invalid", 4) => true,
            _ => false,
        };
        if !invariant_valid {
            truncated = true;
            continue;
        }
        previous_sequence = Some(sequence);
        records.push(json!({
            "sequence": sequence,
            "selector": selector,
            "call_count": call_count,
            "opaque_table_classification": opaque_table_classification,
            "raw_private_table_state": raw_private_table_state,
            "windows_resource_source_state": windows_resource_source_state,
            "lookup_id": lookup_id,
            "outcome": outcome,
            "return_code": return_code,
        }));
    }
    diagnostics["extended_lookup_timeline"] = json!({
        "maximum_records": MAX_EXTENDED_LOOKUP_TIMELINE_RECORDS,
        "records": records,
        "truncated": truncated,
    });
}

fn propagate_extended_allocation_timeline(diagnostics: &mut Value, worker_report: &Value) {
    let Some(container) = worker_report
        .get("extended_allocation_timeline")
        .and_then(Value::as_object)
        .filter(|container| container.len() == 3)
    else {
        return;
    };
    if container.get("maximum_records").and_then(Value::as_u64)
        != Some(MAX_EXTENDED_ALLOCATION_TIMELINE_RECORDS as u64)
    {
        return;
    }
    let Some(reported) = container.get("records").and_then(Value::as_array) else {
        return;
    };
    let Some(reported_truncated) = container.get("truncated").and_then(Value::as_bool) else {
        return;
    };
    let mut records = Vec::new();
    let mut truncated =
        reported_truncated || reported.len() > MAX_EXTENDED_ALLOCATION_TIMELINE_RECORDS;
    let mut previous_sequence = None;
    for record in reported
        .iter()
        .take(MAX_EXTENDED_ALLOCATION_TIMELINE_RECORDS)
    {
        let Some(record) = record.as_object().filter(|record| record.len() == 10) else {
            truncated = true;
            continue;
        };
        let Some(sequence) = record
            .get("sequence")
            .and_then(Value::as_u64)
            .filter(|sequence| *sequence <= u32::MAX as u64)
            .filter(|sequence| previous_sequence.is_none_or(|previous| *sequence > previous))
        else {
            truncated = true;
            continue;
        };
        let Some(selector) = record
            .get("selector")
            .and_then(Value::as_str)
            .filter(|selector| schema_safe_suite_selector(selector))
        else {
            truncated = true;
            continue;
        };
        let Some(boundary) = record
            .get("boundary")
            .and_then(Value::as_str)
            .filter(|boundary| matches!(*boundary, "entry" | "exit"))
        else {
            truncated = true;
            continue;
        };
        let count = |name| {
            record
                .get(name)
                .and_then(Value::as_u64)
                .filter(|count| *count <= u32::MAX as u64)
        };
        let Some(live_allocation_count) = count("live_allocation_count") else {
            truncated = true;
            continue;
        };
        let Some(new_allocations) = count("new_allocations") else {
            truncated = true;
            continue;
        };
        let Some(frees) = count("frees") else {
            truncated = true;
            continue;
        };
        let Some(invalid_frees) = count("invalid_frees") else {
            truncated = true;
            continue;
        };
        let Some(double_frees) = count("double_frees") else {
            truncated = true;
            continue;
        };
        let Some(global_setup_live_allocation_count) = count("global_setup_live_allocation_count")
        else {
            truncated = true;
            continue;
        };
        let Some(reported_allocations) = record
            .get("allocations")
            .and_then(Value::as_array)
            .filter(|allocations| allocations.len() <= MAX_EXTENDED_ALLOCATION_TIMELINE_RECORDS)
        else {
            truncated = true;
            continue;
        };
        let mut allocations = Vec::new();
        let mut computed_live_count = 0u64;
        let mut computed_global_live_count = 0u64;
        let mut allocations_valid = true;
        for allocation in reported_allocations {
            let Some(allocation) = allocation
                .as_object()
                .filter(|allocation| allocation.len() == 3)
            else {
                allocations_valid = false;
                break;
            };
            let Some(token) = allocation
                .get("allocation_token")
                .and_then(safe_process_local_pointer_token)
            else {
                allocations_valid = false;
                break;
            };
            let Some(state) = allocation
                .get("state")
                .and_then(Value::as_str)
                .filter(|state| matches!(*state, "live" | "non_live"))
            else {
                allocations_valid = false;
                break;
            };
            let Some(owner_selector) = allocation
                .get("owner_selector")
                .and_then(Value::as_str)
                .filter(|selector| schema_safe_suite_selector(selector))
            else {
                allocations_valid = false;
                break;
            };
            if state == "live" {
                computed_live_count += 1;
                if owner_selector == "GLOBAL_SETUP" {
                    computed_global_live_count += 1;
                }
            }
            allocations.push(json!({
                "allocation_token": token,
                "state": state,
                "owner_selector": owner_selector,
            }));
        }
        if !allocations_valid
            || computed_live_count != live_allocation_count
            || computed_global_live_count != global_setup_live_allocation_count
        {
            truncated = true;
            continue;
        }
        previous_sequence = Some(sequence);
        records.push(json!({
            "sequence": sequence,
            "selector": selector,
            "boundary": boundary,
            "live_allocation_count": live_allocation_count,
            "new_allocations": new_allocations,
            "frees": frees,
            "invalid_frees": invalid_frees,
            "double_frees": double_frees,
            "global_setup_live_allocation_count": global_setup_live_allocation_count,
            "allocations": allocations,
        }));
    }
    diagnostics["extended_allocation_timeline"] = json!({
        "maximum_records": MAX_EXTENDED_ALLOCATION_TIMELINE_RECORDS,
        "records": records,
        "truncated": truncated,
    });
}

fn propagate_suite_timeline(diagnostics: &mut Value, worker_report: &Value) {
    let mut timeline = Vec::new();
    let mut truncated = reported_truncation(worker_report, "suite_timeline_truncated");
    let reported = match worker_report.get("suite_timeline") {
        Some(Value::Array(reported)) => reported.as_slice(),
        Some(_) => {
            truncated = true;
            &[]
        }
        None => &[],
    };
    for event in reported {
        if timeline.len() >= MAX_SUITE_TIMELINE_EVENTS {
            truncated = true;
            break;
        }
        let Some(sequence) = event["sequence"]
            .as_u64()
            .filter(|value| *value <= u32::MAX as u64)
        else {
            truncated = true;
            continue;
        };
        let Some(action) = event["action"]
            .as_str()
            .filter(|action| matches!(*action, "acquire" | "release"))
        else {
            truncated = true;
            continue;
        };
        let Some(name) = event["name"]
            .as_str()
            .filter(|name| schema_safe_suite_name(name))
        else {
            truncated = true;
            continue;
        };
        let Some(version) = event["version"]
            .as_i64()
            .filter(|version| *version > 0 && *version <= MAX_SUITE_VERSION)
        else {
            truncated = true;
            continue;
        };
        let Some(selector) = event["selector"]
            .as_str()
            .filter(|selector| schema_safe_suite_selector(selector))
        else {
            truncated = true;
            continue;
        };
        let Some(result) = event["result"]
            .as_i64()
            .filter(|result| *result >= i32::MIN as i64 && *result <= i32::MAX as i64)
        else {
            truncated = true;
            continue;
        };
        timeline.push(json!({
            "sequence": sequence,
            "action": action,
            "name": name,
            "version": version,
            "selector": selector,
            "result": result,
        }));
    }
    diagnostics["suite_timeline"] = Value::Array(timeline);
    diagnostics["suite_timeline_truncated"] = Value::Bool(truncated);
}

fn module_audit_summary(audit: &Value) -> Option<Value> {
    let union = audit.get("observed_union")?;
    let safe_names = |field: &str| -> Option<Vec<&str>> {
        Some(
            union
                .get(field)?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .filter(|name| {
                    !name.is_empty()
                        && name.len() <= 260
                        && name.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'.' | b'_' | b'-' | b' ')
                        })
                })
                .take(MAX_MISSING_SUITES)
                .collect(),
        )
    };
    let policy = safe_names("policy")?;
    let unknown = safe_names("unknown")?;
    Some(json!({
        "status": audit.get("status").and_then(Value::as_str),
        "unknown_count": audit.get("unknown_count").and_then(Value::as_u64),
        "phase_count": audit.get("phase_count").and_then(Value::as_u64),
        "authorized_policy_modules": policy,
        "unknown_modules": unknown,
    }))
}

fn module_audit_failure_summary(
    worker_report: &Value,
    selector_phase: Option<&str>,
) -> Option<Value> {
    let failure = worker_report
        .get("module_audit_failure")
        .and_then(Value::as_object)?;
    if failure.get("status").and_then(Value::as_str) != Some("failed") {
        return None;
    }
    let reason = failure
        .get("reason")
        .and_then(Value::as_str)
        .filter(|reason| {
            matches!(
                *reason,
                "loaded_module_policy_rejection" | "module_enumeration_or_path_resolution_failed"
            )
        })?;
    let unknown_count = failure
        .get("unknown_count")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0 && *count <= 4096)?;
    let unattributed_count = failure
        .get("unattributed_count")
        .and_then(Value::as_u64)
        .filter(|count| *count <= unknown_count)?;
    let rejections_truncated = failure
        .get("rejections_truncated")
        .and_then(Value::as_bool)?;
    let reported = failure.get("rejections").and_then(Value::as_array)?;
    let mut rejections = Vec::new();
    let mut seen = BTreeSet::new();
    for rejection in reported.iter().take(MAX_MISSING_SUITES) {
        let Some(basename) = rejection
            .get("basename")
            .and_then(Value::as_str)
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 260
                    && !name.contains(['/', '\\', ':'])
                    && !name.chars().any(char::is_control)
                    && !name.ends_with(['.', ' '])
            })
        else {
            continue;
        };
        let path_token = match rejection.get("canonical_path_token") {
            Some(Value::Null) => Value::Null,
            Some(Value::String(token))
                if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
            {
                json!(token.to_ascii_lowercase())
            }
            _ => continue,
        };
        let Some(path_class) = rejection
            .get("path_class")
            .and_then(Value::as_str)
            .filter(|value| matches!(*value, "sealed_root" | "external"))
        else {
            continue;
        };
        let Some(rejection_reason) =
            rejection
                .get("reason")
                .and_then(Value::as_str)
                .filter(|value| {
                    matches!(
                        *value,
                        "undeclared_sealed_module" | "outside_allowed_roots_or_unapproved_policy"
                    )
                })
        else {
            continue;
        };
        if seen.insert((basename.to_ascii_lowercase(), path_class, rejection_reason)) {
            rejections.push(json!({
                "basename": basename,
                "canonical_path_token": path_token,
                "path_class": path_class,
                "reason": rejection_reason,
            }));
        }
    }
    let selector_phase = selector_phase
        .filter(|phase| schema_safe_suite_selector(phase))
        .map_or(Value::Null, |phase| json!(phase));
    Some(json!({
        "status": "failed",
        "reason": reason,
        "unknown_count": unknown_count,
        "unattributed_count": unattributed_count,
        "selector_phase": selector_phase,
        "rejections": rejections,
        "rejections_truncated": rejections_truncated
            || reported.len() > MAX_MISSING_SUITES,
    }))
}

fn failed_module_audit_summary(stdout: &str) -> Option<Value> {
    let report: Value = serde_json::from_str(stdout.trim()).ok()?;
    if report.get("stage")? != "module_audit" {
        return None;
    }
    module_audit_summary(report.get("module_audit")?)
}
