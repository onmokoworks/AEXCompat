/// Per-frame watchdog deadline for resident-session renders, matching the
/// broker's interactive one-shot timeout.
const LIVE_RENDER_FRAME_DEADLINE_MS: u64 = 30_000;

const INTERACTIVE_CAPABILITY_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InspectedRenderCapability {
    smart_render_advertised: bool,
    out_flags2: u64,
}

fn inspected_render_capability(
    report: &serde_json::Value,
) -> Result<InspectedRenderCapability, String> {
    let diagnostics = report
        .get("worker_diagnostics")
        .and_then(serde_json::Value::as_object)
        .ok_or("inspection has no worker diagnostics")?;
    let out_flags2 = diagnostics
        .get("advertised_out_flags2")
        .and_then(serde_json::Value::as_u64)
        .ok_or("inspection has no valid advertised_out_flags2")?;
    let smart_render_advertised = diagnostics
        .get("smart_render_advertised")
        .and_then(serde_json::Value::as_bool)
        .ok_or("inspection has no valid smart_render_advertised")?;
    let bit_advertised = out_flags2 & (1 << 10) != 0;
    if smart_render_advertised != bit_advertised {
        return Err("inspection SmartFX capability contradicts advertised_out_flags2".into());
    }
    Ok(InspectedRenderCapability {
        smart_render_advertised,
        out_flags2,
    })
}

fn selected_interactive_session_selection(
    capability: InspectedRenderCapability,
    requested_smart: bool,
    manual_override: bool,
) -> Result<aexcompat_broker::image_render::InteractiveSessionSelection, String> {
    use aexcompat_broker::image_render::{
        InteractiveCapabilitySource, InteractiveRenderPath, InteractiveSessionSelection,
    };
    // The inspection contract only asserts one supported selector sequence.
    // Until a future descriptor provides a separate dual-capability fact, an
    // override to the other sequence is unsafe and is rejected before either
    // resident or one-shot dispatch.
    if requested_smart != capability.smart_render_advertised {
        return Err(
            "requested render path is not supported by the inspected AEX capability".into(),
        );
    }
    let path = if requested_smart {
        InteractiveRenderPath::SmartFx
    } else {
        InteractiveRenderPath::Classic
    };
    let source = match (manual_override, requested_smart) {
        (false, true) => InteractiveCapabilitySource::AdvertisedSmart,
        (false, false) => InteractiveCapabilitySource::AdvertisedClassic,
        (true, true) => InteractiveCapabilitySource::ManualSmart,
        (true, false) => InteractiveCapabilitySource::ManualClassic,
    };
    InteractiveSessionSelection::new(
        path,
        source,
        INTERACTIVE_CAPABILITY_VERSION,
        capability.out_flags2,
    )
    .map_err(|error| error.to_string())
}

/// Static configuration a resident render session was opened with (issue
/// #107). A key change means the running worker cannot carry the next render:
/// the session thread closes it and opens a fresh one (SEQUENCE_SETUP runs
/// again; the reopen is visible in the report's `resident_session` facts).
#[cfg(windows)]
#[derive(Clone, PartialEq)]
struct LiveSessionKey {
    plugin_sha256: String,
    /// Identity of every approved dependency: staged basename, size, and
    /// content hash. The sealed tree stages dependencies by basename, so a
    /// renamed DLL with identical bytes still needs a fresh session.
    dependency_identities: Vec<String>,
    /// Parameter structure only (slots, kinds, ranges, choices); values ride
    /// each frame's v:2 message and must not force a reopen.
    parameter_signature: String,
    /// The inspection snapshot includes path, provenance, schema version, and
    /// capability identity, so each of those changes forces a clean reopen.
    selection: aexcompat_broker::image_render::InteractiveSessionSelection,
    width: u32,
    height: u32,
    pixel_format: aexcompat_broker::image_render::RenderPixelFormat,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
}

// Platform-independent (and unit-tested) even though the resident session
// that consumes it is Windows-only.
#[cfg_attr(not(windows), allow(dead_code))]
fn parameter_structure_signature(
    parameters: &[aexcompat_broker::image_render::InteractiveParameter],
) -> String {
    serde_json::to_string(
        &parameters
            .iter()
            .map(|parameter| {
                serde_json::json!({
                    "slot": parameter.slot,
                    "kind": parameter.kind,
                    "name": parameter.name,
                    "minimum": parameter.minimum,
                    "maximum": parameter.maximum,
                    "choices": parameter.choices,
                    "component_count": parameter.component_count,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default()
}

#[cfg(windows)]
struct LiveRenderRequest {
    repository: PathBuf,
    plugin_path: PathBuf,
    plugin_sha256: String,
    dependencies: Vec<aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact>,
    parameters: Vec<aexcompat_broker::image_render::InteractiveParameter>,
    selection: aexcompat_broker::image_render::InteractiveSessionSelection,
    input_path: PathBuf,
    timing: aexcompat_broker::image_render::RenderTiming,
    pixel_format: aexcompat_broker::image_render::RenderPixelFormat,
    output: PathBuf,
    respond: mpsc::Sender<TaskResult>,
    identity: DispatchIdentity,
    diagnostic_eligible: bool,
}

#[cfg(windows)]
enum LiveCommand {
    Render(Box<LiveRenderRequest>),
    /// AEX change, approval invalidation, or shutdown: the resident worker
    /// must not outlive the selection it was opened for.
    Close,
}

#[cfg(windows)]
struct LiveSessionHandle {
    sender: mpsc::Sender<LiveCommand>,
}

#[cfg(windows)]
struct DecodedInput {
    path: PathBuf,
    content_sha256: [u8; 32],
    width: u32,
    height: u32,
    rgba: std::sync::Arc<Vec<u8>>,
}

/// State owned by the GUI's single background session thread (issue #107
/// design: one async session thread inside the GUI process drives the
/// out-of-process resident worker; the UI thread never blocks on it).
#[cfg(windows)]
struct LiveSessionState {
    decoded: Option<DecodedInput>,
    open: Option<(
        LiveSessionKey,
        aexcompat_broker::image_render::InteractiveRenderSession,
    )>,
    /// Counts session opens; each reopen means temporal state restarted.
    session_generation: u64,
    /// Unclean close summary of the previous session, surfaced in the next
    /// render report instead of being dropped silently.
    pending_close_summary: Option<serde_json::Value>,
}

#[cfg(windows)]
impl LiveSessionState {
    fn close_current(&mut self) {
        if let Some((_, session)) = self.open.take() {
            let summary = session.close();
            if summary.get("session_clean") != Some(&serde_json::json!(true)) {
                self.pending_close_summary = Some(summary);
            }
        }
    }

    fn decode_input(&mut self, path: &Path) -> Result<&DecodedInput, String> {
        // The cache key is the file's content hash, not its metadata: the
        // one-shot path re-decoded every render, so an overwrite that
        // preserves size and mtime (mtime-keeping tools, coarse filesystem
        // timestamps) must still invalidate here. Hashing the encoded bytes
        // per render is cheap next to a decode; only the decode is reused.
        // The hash streams in bounded chunks, so memory stays flat for any
        // encoded size and the decoder's own limits keep bounding what is
        // actually accepted (no encoded-size cap of its own: legitimate
        // encodings can be larger than the decoded transport cap).
        let content_sha256: [u8; 32] = {
            let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 1024 * 1024];
            loop {
                let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            hasher.finalize().into()
        };
        let stale = !self.decoded.as_ref().is_some_and(|cached| {
            cached.path.as_path() == path && cached.content_sha256 == content_sha256
        });
        if stale {
            let decoded = aexcompat_broker::image_render::decode_bounded_image(path, "input")
                .map_err(|error| error.to_string())?;
            let (width, height) = (decoded.width(), decoded.height());
            self.decoded = Some(DecodedInput {
                path: path.to_path_buf(),
                content_sha256,
                width,
                height,
                rgba: std::sync::Arc::new(decoded.into_rgba8().into_raw()),
            });
        }
        Ok(self.decoded.as_ref().expect("just cached"))
    }
}

/// One live render on the session thread: decode (cached), reopen the session
/// when the static key changed, render through the resident worker, and fall
/// back to the one-shot transport when the session infrastructure cannot
/// carry the render (open failure or invalidation), mirroring the broker's
/// length-1 wrapper fallback policy.
#[cfg(windows)]
fn live_render(
    state: &mut LiveSessionState,
    request: &LiveRenderRequest,
) -> Result<(String, Option<PathBuf>), String> {
    use aexcompat_broker::image_render::{InteractiveRenderSession, InteractiveSessionOpen};

    let (width, height, rgba) = {
        let decoded = state.decode_input(&request.input_path)?;
        (decoded.width, decoded.height, decoded.rgba.clone())
    };
    let key = LiveSessionKey {
        plugin_sha256: request.plugin_sha256.clone(),
        dependency_identities: request
            .dependencies
            .iter()
            .map(|artifact| {
                let basename = artifact
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let hash: String = artifact
                    .expected_sha256
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                format!("{basename}:{}:{hash}", artifact.expected_size)
            })
            .collect(),
        parameter_signature: parameter_structure_signature(&request.parameters),
        selection: request.selection,
        width,
        height,
        pixel_format: request.pixel_format,
        time_step: request.timing.time_step,
        total_time: request.timing.total_time,
        time_scale: request.timing.time_scale,
    };
    if state
        .open
        .as_ref()
        .is_some_and(|(open_key, _)| *open_key != key)
    {
        state.close_current();
    }
    let one_shot = |reason: String| -> Result<(String, Option<PathBuf>), String> {
        let mut report =
            aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                &request.repository,
                &request.plugin_path,
                &request.plugin_sha256,
                &request.input_path,
                &request.output,
                &request.parameters,
                request.timing,
                request.selection.path.is_smart(),
                request.pixel_format,
                None,
                None,
                aexcompat_broker::image_render::RenderGpuBackend::Auto,
                request.dependencies.clone(),
            )
            .map_err(|error| format!("{error} (after resident session fallback: {reason})"))?;
        report["resident_session_fallback"] = serde_json::json!(reason);
        let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
        Ok((body, Some(request.output.clone())))
    };
    if state.open.is_none() {
        let opened = InteractiveRenderSession::open(InteractiveSessionOpen {
            repository: &request.repository,
            plugin_id: "experimental",
            plugin_path: &request.plugin_path,
            plugin_sha256: &request.plugin_sha256,
            parameters: (!request.parameters.is_empty()).then_some(request.parameters.as_slice()),
            selection: request.selection,
            dependencies: request.dependencies.clone(),
            width,
            height,
            pixel_format: request.pixel_format,
            time_step: request.timing.time_step,
            total_time: request.timing.total_time,
            time_scale: request.timing.time_scale,
            timeout_ms: LIVE_RENDER_FRAME_DEADLINE_MS,
        });
        match opened {
            Ok(session) => {
                state.session_generation += 1;
                state.open = Some((key.clone(), session));
            }
            Err(error) => return one_shot(format!("session open failed: {error}")),
        }
    }
    let (_, session) = state.open.as_mut().expect("session just ensured");
    let parameters = (!request.parameters.is_empty()).then_some(request.parameters.as_slice());
    match session.render(
        &rgba,
        request.timing.current_time,
        parameters,
        &request.output,
    ) {
        Ok(mut report) => {
            report["resident_session"]["session_generation"] =
                serde_json::json!(state.session_generation);
            if let Some(summary) = state.pending_close_summary.take() {
                report["previous_session_close"] = summary;
            }
            let passed = report.get("passed") == Some(&serde_json::json!(true));
            let body = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
            if passed {
                Ok((body, Some(request.output.clone())))
            } else {
                // Frame-local compatibility error: the session stays open and
                // the failure reports like a one-shot render failure.
                Err(body)
            }
        }
        Err(error) => {
            let invalidated = state
                .open
                .as_ref()
                .is_some_and(|(_, session)| session.invalidated());
            if invalidated {
                state.close_current();
                one_shot(format!("session invalidated: {error}"))
            } else {
                // Rejected request (bad timing or parameter set); the session
                // itself is still usable for the next render.
                Err(format!("resident session rejected the render: {error}"))
            }
        }
    }
}

#[cfg(windows)]
fn spawn_live_session_thread(receiver: mpsc::Receiver<LiveCommand>) {
    thread::spawn(move || {
        let mut state = LiveSessionState {
            decoded: None,
            open: None,
            session_generation: 0,
            pending_close_summary: None,
        };
        loop {
            match receiver.recv() {
                Ok(LiveCommand::Render(request)) => {
                    let outcome = live_render(&mut state, &request);
                    let (success, body, output) = match outcome {
                        Ok((body, output)) => (true, body, output),
                        Err(body) => (false, body, None),
                    };
                    let _ = request.respond.send(TaskResult {
                        success,
                        body,
                        output,
                        identity: Some(request.identity.clone()),
                        operation: Some("render_image".into()),
                        diagnostic_eligible: request.diagnostic_eligible,
                    });
                }
                Ok(LiveCommand::Close) => state.close_current(),
                // The app dropped the handle: close the worker and exit.
                Err(_) => {
                    state.close_current();
                    return;
                }
            }
        }
    });
}
