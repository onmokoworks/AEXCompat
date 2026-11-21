// GUI から見たワーカーの窓口の macOS 版。Windows 版の画面が呼ぶ broker の関数と
// 同じ形で、x64 guest の常駐セッションを使って描く。guest でできないもの
// （GPU、マスクやカメラの文脈、Custom UI の操作、0 以外の時刻、色深度を
// 保ったままのチェーン）は Unsupported として明示的に失敗させる。
// 依存 DLL と依存フォルダは画面が AEX の隣から自動で集めて渡してくるので
// 拒否はせず、guest が読まなかったことを報告に残す。DLL が本当に要る AEX は
// guest の読み込みで明示的に失敗する。

use super::{
    GuestWorkerCandidate, MacRenderFormat, RESIDENT_RENDER_DEADLINE, ResidentCommand,
    ResidentSessionHandle, discover_parameters, guest_worker_candidates, selected_layer_paths,
    start_resident_session_with_geometry,
};
use aexcompat_broker::image_render::{
    NativeImageInput, RenderGpuBackend, RenderPixelFormat, RenderTiming, RenderUiAction,
};
use aexcompat_broker::render_fixture::InteractiveParameter;
use aexcompat_broker::render_request::HostContext;
use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use serde_json::{Value, json};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn unsupported(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message.into())
}

fn failed(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

pub(crate) fn inspect_experimental_via_discovery_in_place(
    repository: &Path,
    plugin: ApprovedImageArtifact,
    dependency_search_dirs: Vec<PathBuf>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    let (parameters, _defaults, report, _identity) =
        discover_parameters(repository, &plugin.path).map_err(failed)?;
    let mut diagnostics: Value = serde_json::from_str(&report)
        .map_err(|error| failed(format!("guest setup report is not JSON: {error}")))?;
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
    // One row per declared parameter, supported or not, so the GUI can tell
    // "declared none" from "declared only unsupported controls".
    let metadata = diagnostics["parameters"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| json!({"index": row["slot"], "type": row["param_type"]}))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    diagnostics["parameter_metadata"] = Value::Array(metadata);
    Ok((parameters, diagnostics))
}

/// Describes why no guest worker can start, or `None` when one is built.
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

/// The session stays open between renders, the way the macOS GUI kept it:
/// the same AEX, input, depth and layer inputs reuse the loaded guest.
struct CachedSession {
    plugin: PathBuf,
    input: PathBuf,
    format: MacRenderFormat,
    layers: Vec<(u32, PathBuf)>,
    session: ResidentSessionHandle,
}

static SESSION: Mutex<Option<CachedSession>> = Mutex::new(None);

/// Closes the resident guest, if any. Called when the GUI drops its AEX.
pub(crate) fn close_resident_session() {
    let cached = SESSION
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    drop(cached);
}

#[allow(clippy::too_many_arguments)]
fn render_in_guest(
    repository: &Path,
    plugin_path: &Path,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    pixel_format: RenderPixelFormat,
) -> io::Result<Value> {
    let format = guest_format(pixel_format);
    let layers = selected_layer_paths(parameters);
    let mut guard = SESSION.lock().unwrap_or_else(|error| error.into_inner());
    let reusable = guard.as_ref().is_some_and(|cached| {
        cached.plugin == plugin_path
            && cached.input == input_path
            && cached.format == format
            && cached.layers == layers
    });
    if !reusable {
        // Dropping the previous handle performs its bounded shutdown.
        *guard = None;
        let candidates: Vec<GuestWorkerCandidate> =
            guest_worker_candidates(repository).map_err(failed)?;
        let output_directory = output_path
            .parent()
            .ok_or_else(|| failed("render output has no parent folder"))?;
        let session = start_resident_session_with_geometry(
            &candidates,
            plugin_path,
            input_path,
            output_directory,
            format,
            layers.clone(),
            false,
        )
        .map_err(failed)?;
        *guard = Some(CachedSession {
            plugin: plugin_path.to_path_buf(),
            input: input_path.to_path_buf(),
            format,
            layers,
            session,
        });
    }
    let cached = guard.as_mut().expect("session was just ensured");
    // Deep formats write a new folder holding the artifact and a PNG preview;
    // the artifact writer creates it and refuses an existing one.
    let guest_output = match format {
        MacRenderFormat::PngArgb8 => output_path.to_path_buf(),
        _ => output_path.with_extension("artifact"),
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
    if let Some(preview) = result.preview.as_deref()
        && preview != output_path
    {
        std::fs::copy(preview, output_path)?;
    }
    let guest_report: Value =
        serde_json::from_str(&result.report).unwrap_or(Value::String(result.report));
    Ok(json!({
        "passed": true,
        "platform": "macos_x64_guest",
        "pixel_format": pixel_format.report_name(),
        "artifact": (result.output != output_path).then(|| result.output.display().to_string()),
        "guest_report": guest_report,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_experimental_image_with_approved_dependencies_and_search_dirs(
    repository: &Path,
    plugin_path: &Path,
    _approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    _smart: bool,
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
    if timing.current_time != 0 {
        return Err(unsupported(
            "the macOS x64 guest renders frame 0 only; set the current time to 0",
        ));
    }
    let mut report = render_in_guest(
        repository,
        plugin_path,
        input_path,
        output_path,
        parameters,
        pixel_format,
    )?;
    report["guest_ignored_dependencies"] = json!(dependencies.len());
    report["guest_ignored_dependency_folders"] = json!(dependency_search_dirs.len());
    Ok(report)
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
