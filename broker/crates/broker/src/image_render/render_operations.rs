/// Render the approved ScatterMap fixture through the session-only transport.
///
/// The explicit name is intentional: this is not a generic AEX path and must
/// not be confused with the deleted worker `--render-image` argv command.
pub fn render_scattermap_fixture(
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
    let worker = profile
        .classic_worker
        .ok_or_else(|| invalid("classic render is unavailable"))?;
    let approved = crate::render::secure_entry(repository, plugin_id)?;
    if approved.worker_path != repository.join(worker.executable) {
        return Err(invalid(
            "approved render worker differs from registered worker",
        ));
    }
    let plugin_path = approved.main.source.clone();
    let plugin_sha256 = approved
        .main
        .expected_sha256
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let dependencies = approved
        .dependencies
        .iter()
        .map(|entry| ApprovedImageArtifact {
            path: entry.source.clone(),
            expected_sha256: entry.expected_sha256,
            expected_size: entry.expected_size,
        })
        .collect::<Vec<_>>();
    let manifest = load_manifest(repository, plugin_id, profile.descriptor_manifest)?;
    if !manifest.plugin_sha256.eq_ignore_ascii_case(&plugin_sha256) {
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
        &plugin_path,
        &plugin_sha256,
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
        dependencies,
        Vec::new(),
        None,
        false,
        None,
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
    /// report, in the shape the deleted one-shot path asserted.
    Report(Value),
    /// The session carried the render but the render itself failed: a per-span
    /// compatibility error, or a post-render output/write failure. This is the
    /// render's verdict, not an infrastructure fault, so it is final.
    Failure(io::Error),
    /// The session infrastructure could not carry the render (open failure,
    /// worker crash or invalidation, malformed close). The string is the reason;
    /// there is no second transport (#98 W4, #264, #365), so the caller turns
    /// this into an explicit fail-closed error.
    Fallback(String),
}

/// Renders a single audio buffer through a length-1 AudioRenderSession (§10).
/// This is the only audio transport since #365 deleted the one-shot
/// `--render-audio` argv mode (issue #98 W4 / #239). The session's own
/// validation (guards, checksum, generation, and the clean-close gate on the
/// worker's audio report) enforces the contract the one-shot report used to be
/// checked against, so the synthesized report carries the asserted fields.
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
        dependency_search_dirs: match plugin_path.parent() {
            Some(parent) => vec![parent.to_path_buf()],
            None => {
                return AudioWrapperOutcome::Fallback(
                    "plugin path has no parent directory to search for dependencies".to_string(),
                );
            }
        },
        max_samples: samples.len() as u32,
        channels: 1,
        time_scale: SAMPLE_RATE,
        frame_deadline: Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS),
        launch_environment: Default::default(),
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
        // A per-span compatibility error: the effect could not render this
        // input. That is a final compatibility failure, not a
        // session-infrastructure fault, so fail closed with the error the
        // effect reported (#98 W4, #264).
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
        return AudioWrapperOutcome::Fallback("the audio session did not close cleanly".into());
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
    // Keep the public audio report schema unchanged by #365: start from the
    // worker's aggregate audio report (which carries the audio_* telemetry the
    // SDK audio contract consumes, at parity with the deleted
    // emit_audio_render_report) and add or override the fields the one-shot
    // exposed, so a caller sees the same shape it always did.
    let Some(mut report) = close
        .get("final_report")
        .and_then(Value::as_object)
        .cloned()
    else {
        return AudioWrapperOutcome::Fallback(
            "the audio session close carried no final report".into(),
        );
    };
    // Override/add the fields the deleted one-shot emit_audio_render_report
    // exposed, so the public JSON shape callers parse is unchanged by #365. The
    // wrapper only reaches here on a clean close (every span succeeded), so the
    // selector errors are 0, the ranges valid, and the output was created.
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
        (
            "input_sha256",
            json!(format!("{:x}", Sha256::digest(input))),
        ),
        (
            "output_sha256",
            json!(format!("{:x}", Sha256::digest(&output))),
        ),
        ("output_transport", json!("mono_f32le_44100")),
        ("render_path", json!("audio_session")),
    ] {
        report.insert(key.to_owned(), value);
    }
    // Keep `worker_diagnostics` (classification / stage events) at the top
    // level, where the deleted one-shot put it, rather than only nesting it
    // under session_close, so a caller keeps that field.
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
    const MAX_SAMPLES: usize = 10_000_000;
    let plugin_bytes = fs::read(plugin_path)?;
    let actual = observe_selected_plugin_bytes(&plugin_bytes, approved_sha256)?;
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

    // The length-1 audio session (protocol §10) is the only audio transport
    // since #365: the one-shot `--render-audio` argv mode and the escape hatch
    // that reached it are gone, so a session failure is a failure rather than a
    // routing choice.
    match render_audio_via_length_one_session(
        repository,
        plugin_path,
        // The observed identity, so the session binds the bytes on disk.
        &actual,
        &input,
        output_path,
        parameters,
    ) {
        AudioWrapperOutcome::Report(report) => Ok(report),
        AudioWrapperOutcome::Failure(error) => Err(error),
        // Fail closed (#98 W4, #264): an audio-session infrastructure failure
        // no longer silently falls back. There is no second transport to fall
        // back to since #365, so an infrastructure failure is reported as one.
        AudioWrapperOutcome::Fallback(reason) => Err(invalid(format!(
            "the resident audio render session could not carry this render ({reason}). \
             Diagnose the session failure; there is no alternate transport."
        ))),
    }
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
    render_experimental_image_with_approved_dependencies_and_search_dirs(
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
        Vec::new(),
    )
}

/// Interactive in-place render with the same explicitly approved runtime
/// roots used for Effect Controls inspection. The secure session boundary
/// canonicalizes and bounds the roots again; this API does not turn a root
/// into an approved dependency artifact.
pub fn render_experimental_image_with_approved_dependencies_and_search_dirs(
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
    dependency_search_dirs: Vec<PathBuf>,
) -> io::Result<Value> {
    render_experimental_image_with_approved_dependencies_and_runtime_policy_and_search_dirs(
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
        dependency_search_dirs,
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
    render_experimental_image_with_approved_dependencies_and_runtime_policy_and_search_dirs(
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
        Vec::new(),
        gpu_runtime_policy,
    )
}

fn render_experimental_image_with_approved_dependencies_and_runtime_policy_and_search_dirs(
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
    dependency_search_dirs: Vec<PathBuf>,
    gpu_runtime_policy: Option<GpuRuntimePolicyInput<'_>>,
) -> io::Result<Value> {
    let bytes = fs::read(plugin_path)?;
    let actual = observe_selected_plugin_bytes(&bytes, approved_sha256)?;
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
        dependency_search_dirs,
        gpu_runtime_policy,
        false,
        None,
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
    render_experimental_image_with_approved_dependencies_and_deep16_png(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timing,
        smart,
        Vec::new(),
    )
}

pub fn render_experimental_image_with_approved_dependencies_and_deep16_png(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    dependencies: Vec<ApprovedImageArtifact>,
) -> io::Result<Value> {
    let bytes = fs::read(plugin_path)?;
    let actual = observe_selected_plugin_bytes(&bytes, approved_sha256)?;
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
        dependencies,
        Vec::new(),
        None,
        true,
        None,
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
    render_experimental_image_with_timed_layers_and_context(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timed_layers,
        timing,
        smart,
        pixel_format,
        None,
        RenderGpuBackend::Auto,
        Vec::new(),
    )
}

/// Timed image inputs without discarding the caller's host context or runtime
/// dependencies. Uses the same bounded sample validation and worker transport.
pub fn render_experimental_image_with_timed_layers_and_context(
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
    host_context: Option<&crate::render_request::HostContext>,
    gpu_backend: RenderGpuBackend,
    dependencies: Vec<ApprovedImageArtifact>,
) -> io::Result<Value> {
    render_experimental_image_with_timed_layers_context_and_search_dirs(
        repository,
        plugin_path,
        approved_sha256,
        input_path,
        output_path,
        parameters,
        timed_layers,
        timing,
        smart,
        pixel_format,
        host_context,
        None,
        gpu_backend,
        dependencies,
        Vec::new(),
    )
}

/// GUI timed-image dispatch with the same context and dependency roots as an
/// ordinary one-shot render. No sample is routed through the static resident API.
pub fn render_experimental_image_with_timed_layers_context_and_search_dirs(
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
    host_context: Option<&crate::render_request::HostContext>,
    custom_ui_action: Option<RenderUiAction>,
    gpu_backend: RenderGpuBackend,
    dependencies: Vec<ApprovedImageArtifact>,
    dependency_search_dirs: Vec<PathBuf>,
) -> io::Result<Value> {
    let bytes = fs::read(plugin_path)?;
    let actual = observe_selected_plugin_bytes(&bytes, approved_sha256)?;
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
        host_context,
        timing,
        smart,
        pixel_format,
        gpu_backend,
        custom_ui_action,
        None,
        None,
        Some(timed_layers),
        dependencies,
        dependency_search_dirs,
        None,
        false,
        None,
    )
}

fn parameter_animation_launch_payload(
    parameters: &[InteractiveParameter],
    animations: &[ParameterAnimation],
) -> io::Result<String> {
    parameter_animation_sidecar_json(animations)?;
    validate_animation_bindings(parameters, animations)?;
    encode_default_interactive_payload(parameters)
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
    let launch_payload = parameter_animation_launch_payload(parameters, animations)?;
    let bytes = fs::read(plugin_path)?;
    let actual = observe_selected_plugin_bytes(&bytes, approved_sha256)?;
    // Use the same launch-default normalization as RenderSession::open. In
    // particular, an arbitrary parameter that cannot PRINT has no textual base
    // assignment: the worker retains the plug-in-owned default until the raw
    // animation key replaces it through ARB_UNFLATTEN. Requiring an explicit
    // interactive payload here rejected that valid raw-data path before the
    // worker could consume its sidecar.
    render_with_artifact(
        repository,
        "experimental-parameter-animation",
        plugin_path,
        &actual,
        INTERACTIVE_RENDER_TIMEOUT_MS,
        input_path,
        output_path,
        Some(launch_payload),
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
        Vec::new(),
        None,
        false,
        None,
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
    let actual = observe_selected_plugin_bytes(&bytes, approved_sha256)?;
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
        Vec::new(),
        None,
        false,
        None,
    )
}

pub fn render_experimental_artifact_at_time(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    input_path: &Path,
    output_directory: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    pixel_format: RenderPixelFormat,
    artifact_kind: RenderArtifactKind,
) -> io::Result<Value> {
    if artifact_kind == RenderArtifactKind::Float32Exr && pixel_format != RenderPixelFormat::Argb32f
    {
        return Err(invalid("render-exr requires argb32f"));
    }
    let bytes = fs::read(plugin_path)?;
    let actual = observe_selected_plugin_bytes(&bytes, approved_sha256)?;
    render_with_artifact(
        repository,
        "experimental",
        plugin_path,
        &actual,
        INTERACTIVE_RENDER_TIMEOUT_MS,
        input_path,
        output_directory,
        Some(encode_interactive_payload(parameters)?),
        Some(parameters),
        None,
        timing,
        smart,
        pixel_format,
        RenderGpuBackend::Cpu,
        None,
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        None,
        false,
        Some(artifact_kind),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let check_type = if missing_only { 2 } else { 1 };
    let args_before_plugin = vec!["--l2-external-dependencies".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), check_type.to_string()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Discovery,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-do-dialog".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Discovery,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "options dialog probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("options dialog worker report is invalid"))?;
    // A dialog the broker closed did not complete; it was cancelled by the
    // host, and reporting that as a pass would turn a compatibility gap into
    // fixture-shaped silent success. The probe fails explicitly instead, naming
    // what was closed (issue #351).
    let closed_by_host: Vec<_> = isolated
        .dismissed_windows
        .iter()
        .filter(|window| window.asked_to_close)
        .collect();
    if !closed_by_host.is_empty() {
        return Err(invalid(format!(
            "options dialog was closed by the host, so it did not complete: {}",
            serde_json::to_string(&closed_by_host).unwrap_or_default()
        )));
    }
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-auto-dialog".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Discovery,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    let diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "automatic options dialog probe failed safely: {diagnostics}"
        )));
    }
    let mut report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("automatic options dialog worker report is invalid"))?;
    // Same reasoning as the manual probe: a dialog the host closed did not
    // complete (issue #351).
    let closed_by_host: Vec<_> = isolated
        .dismissed_windows
        .iter()
        .filter(|window| window.asked_to_close)
        .collect();
    if !closed_by_host.is_empty() {
        return Err(invalid(format!(
            "automatic options dialog was closed by the host, so it did not complete: {}",
            serde_json::to_string(&closed_by_host).unwrap_or_default()
        )));
    }
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "default".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Classic,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS)),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
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
        Some(Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS)),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "default".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Classic,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS)),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
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
        Some(Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS)),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "default".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Classic,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS)),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "persistent_sequence".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Classic,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS)),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), "flattened_sequence".into()];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Classic,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS)),
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
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--render".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        "copied_flattened_sequence".into(),
    ];
    let started = Instant::now();
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::Classic,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS)),
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

/// Inspects one in-place plug-in through the shipping discovery-session
/// contract. A cleanup crash can succeed only through the authenticated,
/// single-use retry authorization minted by that session.
pub fn inspect_experimental_via_discovery_in_place(
    repository: &Path,
    plugin: ApprovedImageArtifact,
    dependency_search_dirs: Vec<std::path::PathBuf>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    use crate::render_session::{DiscoverySession, InPlaceDiscoverySessionOpenRequest};

    if dependency_search_dirs.is_empty() {
        return Err(invalid(
            "in-place discovery inspection requires dependency search directories",
        ));
    }
    let mut session = DiscoverySession::open_in_place(InPlaceDiscoverySessionOpenRequest {
        repository,
        plugins: vec![plugin],
        dependency_search_dirs,
        module_bound: crate::cluster_manifest::MAX_CLUSTER_MODULE_BOUND,
        inspect_deadline: None,
        launch_environment: Default::default(),
    })?;
    let outcome = session.inspect_plugin(0, 0)?;
    let close = session.close();
    finish_discovery_inspection(outcome, repository, close, |authorization, repository| {
        inspect_experimental_cleanup_contained_in_place(authorization, repository)
    })
}

fn finish_discovery_inspection<F>(
    outcome: crate::render_session::InspectOutcome,
    repository: &Path,
    close: Value,
    cleanup_retry: F,
) -> io::Result<(Vec<InteractiveParameter>, Value)>
where
    F: FnOnce(
        crate::render_session::CleanupCrashAuthorization,
        &Path,
    ) -> io::Result<(Vec<InteractiveParameter>, Value)>,
{
    use crate::render_session::InspectOutcome;

    match outcome {
        InspectOutcome::Inspected { report } => {
            if close.get("session_clean") != Some(&json!(true)) {
                return Err(invalid(format!(
                    "discovery inspection session did not close cleanly: {close}"
                )));
            }
            inspection_result_from_report(
                report,
                json!({
                    "classification": "ok",
                    "inspection_transport": "discovery_session",
                }),
                false,
            )
        }
        InspectOutcome::InspectError { error_kind, report } => Err(invalid(format!(
            "discovery inspection failed ({error_kind}): report={report:?}, close={close}"
        ))),
        InspectOutcome::CleanupCrashCheckpoint { authorization } => {
            let (parameters, mut diagnostics) = cleanup_retry(authorization, repository)?;
            diagnostics["inspection_transport"] = json!("discovery_cleanup_contained");
            diagnostics["discovery_close"] = close;
            Ok((parameters, diagnostics))
        }
    }
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

/// In-place variant (issue #751): the plug-in loads from its real path and
/// its dependency closure resolves through `dependency_search_dirs` inside
/// the worker, so nothing is staged and no dependency closure is walked.
pub fn inspect_experimental_in_place(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    dependency_search_dirs: Vec<std::path::PathBuf>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    if dependency_search_dirs.is_empty() {
        return Err(invalid(
            "in-place inspection requires at least one dependency search directory",
        ));
    }
    inspect_experimental_impl(
        repository,
        plugin_path,
        approved_sha256,
        Vec::new(),
        dependency_search_dirs,
        None,
        "--l2-params-only",
        None,
    )
}

pub fn inspect_experimental_in_place_plugin_data_effect(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    dependency_search_dirs: Vec<std::path::PathBuf>,
    selector: &crate::render_session::PluginDataEffectSelector,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    if dependency_search_dirs.is_empty() {
        return Err(invalid(
            "in-place inspection requires at least one dependency search directory",
        ));
    }
    inspect_experimental_impl(
        repository,
        plugin_path,
        approved_sha256,
        Vec::new(),
        dependency_search_dirs,
        None,
        "--l2-params-only",
        Some(selector),
    )
}

/// Performs the single cleanup-contained retry authorized by an authenticated
/// discovery-session checkpoint followed by an OS-classified cleanup crash.
/// The worker recomputes the report and deliberately does not enter
/// `GLOBAL_SETDOWN`; this route must never be used as ordinary discovery.
pub fn inspect_experimental_cleanup_contained_in_place(
    authorization: crate::render_session::CleanupCrashAuthorization,
    repository: &Path,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    let (plugin_path, approved_sha256, expected_size, dependency_search_dirs) =
        authorization.into_retry_identity();
    if dependency_search_dirs.is_empty() {
        return Err(invalid(
            "cleanup-contained inspection requires dependency search directories",
        ));
    }
    if fs::metadata(&plugin_path)?.len() != expected_size {
        return Err(invalid(
            "cleanup-contained authorized plugin size changed before retry",
        ));
    }
    let inspected = inspect_experimental_impl(
        repository,
        &plugin_path,
        &approved_sha256,
        Vec::new(),
        dependency_search_dirs,
        None,
        "--l2-params-inspect-cleanup-contained-v1",
        None,
    )?;
    let diagnostics = &inspected.1;
    if !cleanup_contained_report_is_valid(diagnostics) {
        return Err(invalid(
            "cleanup-contained inspection report failed its mode-specific contract",
        ));
    }
    Ok(inspected)
}

fn cleanup_contained_report_is_valid(diagnostics: &Value) -> bool {
    diagnostics.get("inspection_status").and_then(Value::as_str)
        == Some("parameters_inspected_cleanup_contained")
        && diagnostics
            .get("global_setup_error")
            .and_then(Value::as_i64)
            == Some(0)
        && diagnostics
            .get("params_setup_error")
            .and_then(Value::as_i64)
            == Some(0)
        && diagnostics
            .get("global_setdown_error")
            .and_then(Value::as_i64)
            == Some(-1)
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
