// GUI から見たワーカーの窓口の macOS 版。Windows 版の画面が呼ぶ broker の関数と
// 同じ形で、x64 guest の常駐セッションを使って描く。guest でできないもの
// （GPU、マスクやカメラの文脈、Custom UI の操作、frame 0 / 30fps 以外の時刻、
// 依存 DLL、色深度を保ったままのチェーン）は Unsupported として明示的に失敗させる。
// 依存フォルダは画面が AEX の隣から自動で渡してくるので拒否せず、guest が
// 読まなかったことを報告に残す。
//
// 読み込んだ AEX のハッシュは記録するだけで、起動の条件にはしない
// （CLAUDE.md の record, never enforce）。常駐セッションの再利用は、パスでは
// なく AEX と入力の中身のハッシュで決める。

use super::{
    DynamicParameterUi, GuestWorkerCandidate, MacRenderFormat, RESIDENT_RENDER_DEADLINE,
    ResidentCommand, ResidentSessionHandle, discover_parameters, guest_worker_candidates,
    selected_layer_paths, start_resident_session_with_geometry,
};
use aexcompat_broker::image_render::{
    NativeImageInput, RenderGpuBackend, RenderPixelFormat, RenderTiming, RenderUiAction,
};
use aexcompat_broker::render_fixture::InteractiveParameter;
use aexcompat_broker::render_request::HostContext;
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};

/// The guest renders frame 0 of a 30 fps timeline with a one-frame step.
const GUEST_TIME_SCALE: u32 = 30;

fn unsupported(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message.into())
}

fn failed(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn file_sha256(path: &Path) -> io::Result<String> {
    Ok(format!("{:x}", Sha256::digest(std::fs::read(path)?)))
}

/// Replaces worker paths and stderr text with a count so a report stays shareable.
fn scrub_security(report: &mut Value) {
    if let Some(security) = report
        .get_mut("macos_security")
        .and_then(Value::as_object_mut)
        && let Some(reasons) = security.get("fallback_reasons").and_then(Value::as_array)
    {
        let count = reasons.len();
        security.insert("fallback_reasons".into(), json!(count));
    }
    if let Some(object) = report.as_object_mut()
        && let Some(reasons) = object.get("fallback_reasons").and_then(Value::as_array)
    {
        let count = reasons.len();
        object.insert("fallback_reasons".into(), json!(count));
    }
}

pub(crate) fn inspect_experimental_via_discovery_in_place(
    repository: &Path,
    plugin: ApprovedImageArtifact,
    dependency_search_dirs: Vec<PathBuf>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    let (parameters, _defaults, report, (staged_sha256, _post_setup_sha256)) =
        discover_parameters(repository, &plugin.path).map_err(failed)?;
    let mut diagnostics: Value = serde_json::from_str(&report)
        .map_err(|error| failed(format!("guest setup report is not JSON: {error}")))?;
    scrub_security(&mut diagnostics);
    // The GUI reads the same capability fields the Windows discovery
    // inspection derives from the worker's out_flags/out_flags2.
    let out_flags = diagnostics["out_flags"]
        .as_u64()
        .ok_or_else(|| failed("guest setup report has no valid out_flags"))?;
    let out_flags2 = diagnostics["out_flags2"]
        .as_u64()
        .ok_or_else(|| failed("guest setup report has no valid out_flags2"))?;
    if diagnostics["params_setup_error"] != json!(0) {
        return Err(failed("AEX rejected PF_PARAMS_SETUP"));
    }
    // One row per declared parameter, supported or not, so the GUI can tell
    // "declared none" from "declared only unsupported controls".
    let metadata = diagnostics["parameters"]
        .as_array()
        .ok_or_else(|| failed("guest setup report has no parameters array"))?
        .iter()
        .map(|row| json!({"index": row["slot"], "type": row["param_type"]}))
        .collect::<Vec<_>>();
    let audio_effect_only = out_flags & (1_u64 << 31) != 0;
    diagnostics["advertised_out_flags"] = json!(out_flags);
    diagnostics["advertised_out_flags2"] = json!(out_flags2);
    diagnostics["smart_render_advertised"] = json!(
        aexcompat_broker::image_render::smart_render_advertised(out_flags2)
    );
    diagnostics["audio_effect_only"] = json!(audio_effect_only);
    diagnostics["image_render_supported"] = json!(!audio_effect_only);
    diagnostics["runtime_module_policy_applied"] = json!(false);
    diagnostics["guest_ignored_dependency_folders"] = json!(dependency_search_dirs.len());
    diagnostics["parameter_metadata"] = Value::Array(metadata);
    // What the guest actually staged, next to what the GUI selected.
    diagnostics["plugin_sha256"] = json!(staged_sha256);
    diagnostics["inspection_plugin_sha256"] = json!(hex_lower(&plugin.expected_sha256));
    Ok((parameters, diagnostics))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Describes why no guest worker can start, or `None` when one is built. The
/// guest picks the SmartFX or Classic path itself, so `smart` does not choose a
/// different worker here.
pub(crate) fn missing_render_worker(repository: &Path, _smart: bool) -> Option<String> {
    guest_worker_candidates(repository).err()
}

fn guest_format(pixel_format: RenderPixelFormat) -> MacRenderFormat {
    match pixel_format {
        RenderPixelFormat::Argb8 => MacRenderFormat::PngArgb8,
        RenderPixelFormat::Argb16 => MacRenderFormat::RawArgb16,
        RenderPixelFormat::Argb32f => MacRenderFormat::ExrArgb32f,
    }
}

/// What a resident guest was opened with. Content hashes, not paths: a
/// rebuilt AEX or an edited input at the same path must reopen the guest.
#[derive(Clone, PartialEq)]
struct SessionKey {
    plugin_sha256: String,
    input_sha256: String,
    format: MacRenderFormat,
    layers: Vec<(u32, PathBuf, String)>,
}

struct CachedSession {
    key: SessionKey,
    session: ResidentSessionHandle,
}

static SESSION: Mutex<Option<CachedSession>> = Mutex::new(None);
/// Set when a close arrives while a render holds the session; the render
/// drops the session as soon as it finishes.
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Set once the window has closed: no new guest may start after that.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
/// Sessions being shut down on detached threads, waited for at exit.
static PENDING_DROPS: AtomicUsize = AtomicUsize::new(0);

fn lock_session() -> MutexGuard<'static, Option<CachedSession>> {
    let mut guard = SESSION.lock().unwrap_or_else(|error| error.into_inner());
    if CLOSE_REQUESTED.swap(false, Ordering::AcqRel) {
        *guard = None;
    }
    guard
}

/// Closes the resident guest without blocking the GUI: the bounded shutdown
/// runs on its own thread, and if a render or a USER_CHANGED_PARAM holds the
/// session, the close is applied when that call returns.
pub(crate) fn close_resident_session() {
    let taken = match SESSION.try_lock() {
        Ok(mut guard) => {
            CLOSE_REQUESTED.store(false, Ordering::Release);
            guard.take()
        }
        Err(TryLockError::Poisoned(error)) => error.into_inner().take(),
        Err(TryLockError::WouldBlock) => {
            CLOSE_REQUESTED.store(true, Ordering::Release);
            None
        }
    };
    if let Some(cached) = taken {
        PENDING_DROPS.fetch_add(1, Ordering::AcqRel);
        std::thread::spawn(move || {
            drop(cached);
            PENDING_DROPS.fetch_sub(1, Ordering::AcqRel);
        });
    }
}

/// Closes the resident guest and waits for its bounded shutdown. Used after
/// the window has closed, when nothing is left to freeze and a detached
/// thread would be cut off by process exit.
pub(crate) fn close_resident_session_blocking() {
    SHUTTING_DOWN.store(true, Ordering::Release);
    // A running call releases the lock within its own response deadline.
    let mut guard = SESSION.lock().unwrap_or_else(|error| error.into_inner());
    CLOSE_REQUESTED.store(false, Ordering::Release);
    let taken = guard.take();
    drop(guard);
    drop(taken);
    // Each detached shutdown is itself bounded; wait a little longer than one.
    let deadline = std::time::Instant::now() + super::RESIDENT_CLOSE_DEADLINE * 3;
    while PENDING_DROPS.load(Ordering::Acquire) > 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn apply_requested_close(guard: &mut MutexGuard<'static, Option<CachedSession>>) {
    if CLOSE_REQUESTED.swap(false, Ordering::AcqRel) {
        **guard = None;
    }
}

fn session_key(
    plugin_path: &Path,
    input_path: &Path,
    format: MacRenderFormat,
    parameters: &[InteractiveParameter],
) -> io::Result<SessionKey> {
    let layers = selected_layer_paths(parameters)
        .into_iter()
        .map(|(slot, path)| file_sha256(&path).map(|hash| (slot, path, hash)))
        .collect::<io::Result<Vec<_>>>()?;
    Ok(SessionKey {
        plugin_sha256: file_sha256(plugin_path)?,
        input_sha256: file_sha256(input_path)?,
        format,
        layers,
    })
}

fn ensure_session<'a>(
    guard: &'a mut MutexGuard<'static, Option<CachedSession>>,
    repository: &Path,
    plugin_path: &Path,
    input_path: &Path,
    key: SessionKey,
) -> io::Result<&'a mut CachedSession> {
    if guard.as_ref().is_none_or(|cached| cached.key != key) {
        // Dropping the previous handle performs its bounded shutdown.
        **guard = None;
        if SHUTTING_DOWN.load(Ordering::Acquire) {
            return Err(failed(
                "the GUI is closing; no new guest session is started",
            ));
        }
        let candidates: Vec<GuestWorkerCandidate> =
            guest_worker_candidates(repository).map_err(failed)?;
        let layers = key
            .layers
            .iter()
            .map(|(slot, path, _)| (*slot, path.clone()))
            .collect();
        let session = start_resident_session_with_geometry(
            &candidates,
            plugin_path,
            input_path,
            Path::new("."),
            key.format,
            layers,
            false,
        )
        .map_err(failed)?;
        **guard = Some(CachedSession { key, session });
    }
    Ok(guard.as_mut().expect("session was just ensured"))
}

/// Removes a render's staging PNG or artifact folder unless the render
/// completed, so no error path leaves partial files next to the output.
struct StagedOutput {
    path: PathBuf,
    directory: bool,
    keep: bool,
}

impl Drop for StagedOutput {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        let _ = if self.directory {
            std::fs::remove_dir_all(&self.path)
        } else {
            std::fs::remove_file(&self.path)
        };
    }
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn render_in_guest(
    repository: &Path,
    plugin_path: &Path,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    pixel_format: RenderPixelFormat,
    smart: bool,
) -> io::Result<Value> {
    let format = guest_format(pixel_format);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let key = session_key(plugin_path, input_path, format, parameters)?;
    let mut guard = lock_session();
    let cached = ensure_session(&mut guard, repository, plugin_path, input_path, key)?;
    // Deep formats write a new folder holding the artifact and a PNG preview;
    // the artifact writer creates it and refuses an existing one, so every
    // render gets its own folder next to the PNG.
    // An 8 bpc frame is written to a staging name and moved over the output
    // only after the render path check below passes.
    let suffix = unique_suffix();
    let deep = format != MacRenderFormat::PngArgb8;
    let guest_output = if deep {
        output_path.with_extension(format!("{suffix}.artifact"))
    } else {
        output_path.with_extension(format!("{suffix}.partial.png"))
    };
    let mut staged = StagedOutput {
        path: guest_output.clone(),
        directory: deep,
        keep: false,
    };
    let frame_index = cached.session.next_frame;
    cached.session.next_frame += 1;
    let sent = cached.session.sender.send(ResidentCommand::Render {
        frame_index,
        parameters: parameters.to_vec(),
        output: guest_output,
    });
    let result = match sent {
        Ok(()) => cached
            .session
            .receiver
            .recv_timeout(RESIDENT_RENDER_DEADLINE * 2)
            .map_err(|error| format!("resident guest did not answer: {error}"))
            .and_then(|result| result),
        Err(error) => Err(format!("resident guest session stopped: {error}")),
    };
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            *guard = None;
            return Err(failed(error));
        }
    };
    let staged_plugin_sha256 = cached.session.staged_plugin_sha256.clone();
    apply_requested_close(&mut guard);
    drop(guard);
    let mut guest_report: Value =
        serde_json::from_str(&result.report).unwrap_or(Value::String(result.report));
    scrub_security(&mut guest_report);
    // The GUI stamps the path it asked for into its receipt. A guest that ran
    // the other path must not pass as the requested one (#606).
    let requested_path = if smart { "smartfx" } else { "classic" };
    let ran_path = guest_report["frame"]["output"]["render_path"].as_str();
    if ran_path != Some(requested_path) {
        return Err(unsupported(format!(
            "the macOS x64 guest ran the {} path but {requested_path} was requested",
            ran_path.unwrap_or("unknown")
        )));
    }
    if deep {
        let preview = result
            .preview
            .as_deref()
            .ok_or_else(|| failed("guest wrote no preview for the deep artifact"))?;
        std::fs::copy(preview, output_path)?;
    } else {
        std::fs::rename(&staged.path, output_path)?;
    }
    staged.keep = true;
    let artifact = deep.then(|| {
        result
            .output
            .strip_prefix(output_path.parent().unwrap_or(Path::new("")))
            .unwrap_or(&result.output)
            .display()
            .to_string()
    });
    Ok(json!({
        "passed": true,
        "platform": "macos_x64_guest",
        "pixel_format": pixel_format.report_name(),
        "plugin_sha256": staged_plugin_sha256,
        "artifact": artifact,
        "guest_report": guest_report,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_experimental_image_with_approved_dependencies_and_search_dirs(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    host_context: Option<&HostContext>,
    custom_ui_action: Option<RenderUiAction>,
    gpu_backend: RenderGpuBackend,
    dependencies: Vec<ApprovedImageArtifact>,
    dependency_search_dirs: Vec<PathBuf>,
) -> io::Result<Value> {
    if host_context.is_some() {
        return Err(unsupported(
            "mask, spatial and render context are not available in the macOS x64 guest",
        ));
    }
    if custom_ui_action.is_some() {
        return Err(unsupported(
            "custom UI actions are not available in the macOS x64 guest",
        ));
    }
    if !matches!(gpu_backend, RenderGpuBackend::Auto | RenderGpuBackend::Cpu) {
        return Err(unsupported(
            "GPU backends are not available in the macOS x64 guest; use Auto or CPU",
        ));
    }
    if timing.current_time != 0 || timing.time_step != 1 || timing.time_scale != GUEST_TIME_SCALE {
        return Err(unsupported(
            "the macOS x64 guest renders frame 0 at 30 fps with a one-frame step only",
        ));
    }
    if !dependencies.is_empty() {
        // An AEX that loads a DLL at run time would get nothing and could
        // still look successful, so this fails instead of rendering.
        return Err(unsupported(
            "dependency DLLs are not loaded by the macOS x64 guest",
        ));
    }
    let mut report = render_in_guest(
        repository,
        plugin_path,
        input_path,
        output_path,
        parameters,
        pixel_format,
        smart,
    )?;
    report["inspection_plugin_sha256"] = json!(approved_sha256.to_ascii_lowercase());
    report["guest_ignored_dependency_folders"] = json!(dependency_search_dirs.len());
    report["requested_timing"] = json!({
        "current_time": timing.current_time,
        "time_step": timing.time_step,
        "total_time": timing.total_time,
        "time_scale": timing.time_scale,
    });
    Ok(report)
}

/// PF_Cmd_USER_CHANGED_PARAM through the open resident guest, as the former
/// macOS GUI did. The guest needs an input, so a render must have opened the
/// session for this AEX first.
pub(crate) fn trigger_experimental_button(
    _repository: &Path,
    plugin_path: &Path,
    _approved_sha256: &str,
    slot: u32,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if !parameters
        .iter()
        .any(|parameter| parameter.slot == slot && parameter.supervised)
    {
        return Err(failed("parameter is not supervised by the AEX"));
    }
    let payload = crate::guest_parameter_transport::fixture_parameter_payload(parameters)
        .map_err(failed)?
        .transport;
    let plugin_sha256 = file_sha256(plugin_path)?;
    let mut guard = lock_session();
    let Some(cached) = guard
        .as_ref()
        .filter(|cached| cached.key.plugin_sha256 == plugin_sha256)
    else {
        return Err(unsupported(
            "render this AEX once first: the macOS x64 guest sends USER_CHANGED_PARAM through its open render session",
        ));
    };
    let (reply, receiver) = std::sync::mpsc::channel();
    // The lock is held across the guest's answer (bounded by its deadline).
    // Only worker threads call this; the GUI thread closes with try_lock.
    if let Err(error) = cached.session.sender.send(ResidentCommand::UserChanged {
        slot,
        parameters: payload,
        reply,
    }) {
        *guard = None;
        return Err(failed(format!("resident guest session stopped: {error}")));
    }
    let staged_plugin_sha256 = cached.session.staged_plugin_sha256.clone();
    let answer = receiver
        .recv_timeout(RESIDENT_RENDER_DEADLINE * 2)
        .map_err(|error| format!("resident guest did not answer: {error}"))
        .and_then(|result| result);
    match &answer {
        Ok(_) => apply_requested_close(&mut guard),
        Err(_) => *guard = None,
    }
    drop(guard);
    user_changed_report(
        parameters,
        slot,
        staged_plugin_sha256,
        answer.map_err(failed)?,
    )
}

/// The report the GUI's dynamic-UI parser reads. Like the former macOS GUI,
/// a partial table or a changed slot order is a failure, not a partial update.
fn user_changed_report(
    parameters: &[InteractiveParameter],
    slot: u32,
    plugin_sha256: String,
    updates: Vec<DynamicParameterUi>,
) -> io::Result<Value> {
    if updates.len() != parameters.len() {
        return Err(failed(
            "USER_CHANGED_PARAM returned a partial parameter table",
        ));
    }
    if parameters
        .iter()
        .zip(&updates)
        .any(|(parameter, update)| parameter.slot != update.slot)
    {
        return Err(failed("USER_CHANGED_PARAM parameter identity changed"));
    }
    Ok(json!({
        "user_changed_param_requested": true,
        "user_changed_param_slot": slot,
        "user_changed_param_error": 0,
        "platform": "macos_x64_guest",
        "plugin_sha256": plugin_sha256,
        "parameters": updates
            .iter()
            .map(|update| json!({"index": update.slot, "ui_flags": update.ui_flags, "flags": update.flags}))
            .collect::<Vec<_>>(),
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_experimental_image_with_auto_opencl_policy(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
    _input_path: &Path,
    _output_path: &Path,
    _parameters: &[InteractiveParameter],
    _timing: RenderTiming,
    _host_context: Option<&HostContext>,
    _custom_ui_action: Option<RenderUiAction>,
    _dependencies: Vec<ApprovedImageArtifact>,
    _dependency_search_dirs: Vec<PathBuf>,
) -> io::Result<Value> {
    Err(unsupported(
        "OpenCL rendering is not available in the macOS x64 guest",
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_native_image_with_dependencies(
    _repository: &Path,
    _plugin_path: &Path,
    _inspected_sha256: &str,
    _input: &NativeImageInput,
    _output_path: &Path,
    _parameters: &[InteractiveParameter],
    _timing: RenderTiming,
    _smart: bool,
    _host_context: Option<&HostContext>,
    _custom_ui_action: Option<RenderUiAction>,
    _gpu_backend: RenderGpuBackend,
    _dependencies: Vec<ApprovedImageArtifact>,
    _search_dirs: Vec<PathBuf>,
) -> io::Result<Value> {
    Err(unsupported(
        "16/32 bpc effect chains are not available in the macOS x64 guest yet; use 8 bpc",
    ))
}

/// Builds the USER_CHANGED_PARAM report from (slot, ui_flags, flags) rows so
/// GUI tests can feed it to the same parser the GUI uses.
#[cfg(test)]
pub(crate) fn user_changed_report_from_rows(
    parameters: &[InteractiveParameter],
    slot: u32,
    rows: &[(u32, u32, u32)],
) -> io::Result<Value> {
    let updates = rows
        .iter()
        .map(|&(slot, ui_flags, flags)| DynamicParameterUi {
            slot,
            ui_flags,
            flags,
        })
        .collect();
    user_changed_report(parameters, slot, String::new(), updates)
}
