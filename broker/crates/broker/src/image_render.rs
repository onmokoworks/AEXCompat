use crate::host_core::descriptor_manifest::load as load_manifest;
use crate::host_core::parameter::{ValidatedAssignments, apply_defaults, encode_worker_payload};
use crate::runtime_module_authorization::{
    RuntimeModulePurpose, encode_runtime_module_authorization,
};
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
const MAX_SUITE_NAME_LEN: usize = 96;
const MAX_UNSUPPORTED_SUITE_SLOT: u64 = 1023;
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
        dependencies,
        args_before_plugin,
        args_after_plugin,
        timeout,
    })
}

pub(crate) struct RuntimeAuthorizationTransport {
    path: PathBuf,
    artifact: ApprovedImageArtifact,
    basename: String,
    /// The per-render session identity embedded in the manifest. The GPU
    /// module-audit preflight (#290) needs it to bind the worker's report to the
    /// same session the render dispatch authorizes; the params-inspect path
    /// ignores it.
    session_identity: [u8; 32],
}

impl RuntimeAuthorizationTransport {
    /// The sealed manifest basename to pass as the worker's
    /// `--runtime-module-authorization-v1` trailer.
    pub(crate) fn basename(&self) -> &str {
        &self.basename
    }

    /// The manifest as a sealed dependency for the worker's load tree.
    pub(crate) fn artifact(&self) -> ApprovedImageArtifact {
        self.artifact.clone()
    }
}

impl Drop for RuntimeAuthorizationTransport {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn prepare_runtime_authorization_transport(
    repository: &Path,
    policy: &RuntimeModulePolicy,
    backend: RuntimeBackend,
) -> io::Result<RuntimeAuthorizationTransport> {
    let mut session_identity = rand::random::<[u8; 32]>();
    if session_identity.iter().all(|byte| *byte == 0) {
        session_identity[0] = 1;
    }
    prepare_runtime_authorization_transport_with_identity(
        repository,
        policy,
        backend,
        session_identity,
    )
}

/// Like [`prepare_runtime_authorization_transport`] but embeds a caller-supplied
/// `session_identity` instead of a fresh random one. A GPU render reuses the
/// preflight's session identity here so the render worker parses the same
/// identity the [`PreparedGpuRuntimePolicy`] report was authenticated against,
/// keeping the manifest/report/session binding intact (#301 review). The identity
/// must be nonzero (the preflight's is, by construction and prior authentication).
pub(crate) fn prepare_runtime_authorization_transport_with_identity(
    repository: &Path,
    policy: &RuntimeModulePolicy,
    backend: RuntimeBackend,
    session_identity: [u8; 32],
) -> io::Result<RuntimeAuthorizationTransport> {
    if session_identity.iter().all(|byte| *byte == 0) {
        return Err(invalid("runtime module session identity must be nonzero"));
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
        session_identity,
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
        authorization.basename.clone(),
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
            "GPU module-audit preflight worker did not succeed (classification: {})",
            isolated.classification.as_str()
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
    limits.max_image_height = So…63922 tokens truncated…     color: [255, 0, 0, 0],
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
        assert!(
            RenderTiming {
                current_time: 3,
                time_step: 1,
                total_time: 4,
                time_scale: 30,
            }
            .is_valid()
        );
        assert!(
            !RenderTiming {
                current_time: 3,
                time_step: 0,
                total_time: 2,
                time_scale: 0,
            }
            .is_valid()
        );
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

        // The length-one session wrapper resolves the environment value once
        // before passing it to `RenderSession::open`, so the second resolver
        // receives the canonical Windows path. Keep that production shape in
        // the contract test (#372).
        let canonical_request = repository.join("target/canonical-world-dumps");
        fs::create_dir_all(&canonical_request).unwrap();
        let canonical_request = canonical_request.canonicalize().unwrap();
        let canonical_accepted =
            resolve_managed_dump_dir(&repository, &canonical_request, true).unwrap();
        assert_eq!(canonical_accepted.display, "target/canonical-world-dumps");

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
    fn module_audit_summary_exposes_only_bounded_safe_unknown_basenames() {
        let summary = module_audit_summary(&json!({
            "status": "failed",
            "unknown_count": 4,
            "phase_count": 3,
            "observed_union": {
                "policy": ["approved.dll"],
                "unknown": ["outside.dll", "C:\\private\\leak.dll", "bad:name.dll"]
            }
        }))
        .unwrap();

        assert_eq!(
            summary["authorized_policy_modules"],
            json!(["approved.dll"])
        );
        assert_eq!(summary["unknown_modules"], json!(["outside.dll"]));
        assert!(!summary.to_string().contains("private"));
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
    fn structured_worker_report_supplies_bounded_unique_unsupported_suite_calls() {
        let mut diagnostics = json!({});
        let mut reported = vec![
            json!({"name": "AEGP Comp Suite", "version": 21, "slot": 7, "call_count": 2}),
            json!({"name": "AEGP Comp Suite", "version": 21, "slot": 7, "call_count": 9}),
            json!({"name": "C:\\private\\suite", "version": 1, "slot": 1, "call_count": 1}),
            json!({"name": "Bad Suite", "version": 1, "slot": 2048, "call_count": 1}),
        ];
        for index in 0..(MAX_UNSUPPORTED_SUITE_CALLS + 3) {
            reported.push(json!({
                "name": format!("Safe Suite {index}"),
                "version": 1,
                "slot": index,
                "call_count": 1,
            }));
        }

        propagate_unsupported_suite_calls(
            &mut diagnostics,
            &json!({"unsupported_suite_calls": reported}),
        );
        let calls = diagnostics["unsupported_suite_calls"].as_array().unwrap();
        assert_eq!(calls.len(), MAX_UNSUPPORTED_SUITE_CALLS);
        assert_eq!(
            calls[0],
            json!({"name": "AEGP Comp Suite", "version": 21, "slot": 7, "call_count": 2})
        );
        assert_eq!(
            calls
                .iter()
                .filter(|call| call["name"] == "AEGP Comp Suite" && call["slot"] == 7)
                .count(),
            1
        );
        assert!(!diagnostics.to_string().contains("private"));
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
            dismissed_windows: Vec::new(),
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
            dismissed_windows: Vec::new(),
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
}
