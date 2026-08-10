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
    // The worker's session request carries time_scale as a signed 32-bit value,
    // so a larger launch scale could never round-trip. `RenderSession::open`
    // rejects it too, but as a session-open failure; checking it here keeps it
    // the plain caller error it is. This bound used to sit in the
    // session-eligibility gate, where exceeding it silently chose the one-shot
    // transport instead (#365).
    if timing.time_scale > i32::MAX as u32 {
        return Err(invalid("render time scale is out of range"));
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

    // Issue #98 stage W2, completed by #365: every render routes through a
    // resident length-1 render session, and the one-shot argv transport it was
    // meant to retire is gone. The session carries mask, spatial,
    // render-environment context (W1-3), alpha-as-coverage parameter slots
    // (W1-4c, published once at launch), and aux channels (#211): every
    // HostContext field is session-representable.
    //
    // Aux channels ride a broker-created manifest sidecar
    // (`--aux-manifest-v1 <manifest>`) prepared here. The sidecars and manifest
    // live under the broker-owned `target/image-transport` root; the worker only
    // ever reads broker-written files there (the sample source paths are
    // canonicalized, bounded, and copied by the broker inside
    // prepare_aux_transport), so the worker re-resolves no caller-supplied path.
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
        // stale sweep are idempotent with the layer/audio calls below.
        fs::create_dir_all(&root)?;
        cleanup_stale_image_transport(&root, SystemTime::now())?;
        prepare_aux_transport(repository, aux_channels, &root, nonce)?
    };
    let alpha_as_coverage_params: &[u32] =
        host_context.map_or(&[], |context| context.alpha_as_coverage_params.as_slice());
    // #365 (W4): the length-1 render session is the only image transport. The
    // one-shot argv commands and the eligibility gate that chose between them
    // are gone, so a shape the session cannot carry is an explicit error from
    // `RenderSession::open` (a GPU backend with no authenticated runtime-module
    // policy, a depth/backend pair with no session command) rather than a
    // silent reroute onto a second implementation.
    //
    // Deleting the gate widened what the host accepts, because the session
    // command table is a superset of the one-shot one: classic and smart
    // Argb8/Argb16 now accept an explicit Cpu backend, and a layered Argb32f
    // render carrying a policy now opens the real GPU session instead of
    // falling to the one-shot's CPU render. The one shape it narrowed is an
    // explicit GPU backend with no policy: the one-shot let the worker take the
    // device unauthorized, which was the only route that could, and the session
    // fails it closed.
    //
    // Layer pixels travel as inherited per-layer file HANDLEs (#268), not
    // section slots, so the section is header + input + output only and its
    // aggregate cap is no longer a function of layer count or size.
    // An audio sidecar rides the session's launch trailer the same way the
    // static context trailers do (issue #339), while secondary layers use
    // inherited file handles. The two transports are orthogonal, and the
    // combined native fixture proves that a classic session consumes both in
    // one render (issue #341).
    //
    // The block scopes the audio sidecar's cleanup guard so it is dropped on
    // every arm of the match below, including the error arms.
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
            .map(
                |(slot, width, height, rgba)| crate::render_session::SessionLayer {
                    slot,
                    width,
                    height,
                    rgba,
                    timed: None,
                    dynamic: false,
                },
            )
            .chain(
                timed_secondaries
                    .into_iter()
                    .map(
                        |(slot, time, width, height, rgba)| crate::render_session::SessionLayer {
                            slot,
                            width,
                            height,
                            rgba,
                            timed: Some((time.value, time.scale)),
                            dynamic: false,
                        },
                    ),
            )
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
        // The worker loads the span from a file, exactly as it does for the
        // deleted one-shot's --render-image-audio, so the session writes the
        // sidecar before opening and names it in the trailer (issue #339).
        // One binding, so "a sidecar was written" and "a trailer was emitted"
        // cannot come apart: there is no shape here that writes the file and
        // then renders as if the render carried no audio.
        let prepared_audio = match &audio {
            Some(bytes) => {
                fs::create_dir_all(&root)?;
                cleanup_stale_image_transport(&root, SystemTime::now())?;
                let path = root.join(format!("audio-{nonce}.f32"));
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                // The file now exists on disk; track it for cleanup BEFORE the
                // fallible write so a mid-write failure (or any later early
                // return) still removes it instead of leaking a partial file.
                // Same rule the layer sidecars follow in render_session.rs.
                let cleanup = Cleanup(vec![path.clone()]);
                file.write_all(bytes)?;
                Some((
                    SessionAudioSource {
                        trailer: format!(
                            "session-audio:v1|{}|44100|{}",
                            bytes.len() / 4,
                            path.to_string_lossy()
                        ),
                        input_sha256: format!("{:x}", Sha256::digest(bytes)),
                    },
                    cleanup,
                ))
            }
            None => None,
        };
        // The sidecar outlives the session open (the worker reads it at launch)
        // and must not outlive this scope. The guard is named, not `_`, so it
        // lives to the end of the scope and drops after `session_outcome` on
        // every return arm and on unwind.
        let (session_audio, _audio_cleanup) = match prepared_audio {
            Some((source, cleanup)) => (Some(source), Some(cleanup)),
            None => (None, None),
        };
        let session_outcome = render_classic_via_length_one_session(&SessionWrapperRequest {
            repository,
            plugin_id,
            plugin_path,
            plugin_sha256,
            timeout_ms,
            output_path,
            preserved_output: preserved_output.as_deref(),
            interactive_parameters,
            payload_override: payload_override.as_deref(),
            parameter_animation,
            layers: session_layers,
            mask_trailer,
            spatial_trailer,
            render_environment_trailer,
            audio: session_audio,
            alpha_as_coverage_params,
            conformance_render_settings: conformance_render_settings.as_deref(),
            aux_manifest: aux_transport
                .as_ref()
                .map(|aux| aux.manifest_path.as_path()),
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
            smart,
            gpu_backend,
            // `RenderSession::open` decides what to do with this: it folds Auto
            // to CPU when the policy is absent, requires one for any real GPU
            // attempt, and ignores it entirely below float32 or on classic.
            gpu_runtime_policy,
        });
        match session_outcome {
            SessionWrapperOutcome::Report(report) => return Ok(report),
            SessionWrapperOutcome::Failure(error) => return Err(error),
            // Fail closed (#98 W4, #264, #365): there is no second transport to
            // fall back to, so an infrastructure failure is reported as one.
            SessionWrapperOutcome::Fallback(reason) => {
                return Err(invalid(format!(
                    "the resident render session could not carry this render ({reason}). \
                     Diagnose the session failure; there is no alternate transport. Any \
                     world-dump snapshots from the failed session are preserved for diagnosis; \
                     clear the dump directory before re-running, since it must start empty."
                )));
            }
        }
    }
}

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
    std::env::var_os(FORCE_SESSION_FALLBACK_ENV)
        .is_some()
        .then(|| {
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
    std::env::var_os(FORCE_SESSION_FALLBACK_ENV)
        .is_some()
        .then(|| {
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

/// The audio source handed to a length-1 session: the launch trailer naming the
/// sidecar the worker reads, plus the digest of the bytes that were written.
/// One value carries both so no route can gate on audio being present and then
/// report it absent, or the reverse (issue #339).
struct SessionAudioSource {
    /// `session-audio:v1|<samples>|<rate>|<path>`, the session's form of the
    /// three bare argv slots the deleted one-shot spent under
    /// `--render-image-audio`.
    trailer: String,
    input_sha256: String,
}

struct SessionWrapperRequest<'a> {
    repository: &'a Path,
    plugin_id: &'a str,
    plugin_path: &'a Path,
    plugin_sha256: &'a str,
    timeout_ms: u64,
    output_path: &'a Path,
    preserved_output: Option<&'a Path>,
    interactive_parameters: Option<&'a [InteractiveParameter]>,
    /// Pre-encoded worker payload for the fixture-manifest route, whose
    /// descriptor-profile encoding `interactive_parameters` cannot express
    /// (see `SessionOpenRequest::payload_override`). `None` on every
    /// interactive route.
    payload_override: Option<&'a str>,
    parameter_animation: Option<&'a [ParameterAnimation]>,
    layers: Vec<crate::render_session::SessionLayer>,
    mask_trailer: Option<String>,
    spatial_trailer: Option<String>,
    render_environment_trailer: Option<String>,
    /// The session's audio source, or `None` when the render carries no audio.
    /// The trailer and the sidecar digest travel together so the gate, the
    /// launch argv, and the public report cannot disagree about whether this
    /// render had audio (issue #339).
    audio: Option<SessionAudioSource>,
    alpha_as_coverage_params: &'a [u32],
    /// Conformance render-settings trailer (`--conformance-render-settings-v1`),
    /// forwarded so the session worker reports the same render_settings block the
    /// one-shot path does (#275). The input pre-transform is applied on both
    /// routes before this point, so this only aligns the report. `None` leaves
    /// the option unset.
    conformance_render_settings: Option<&'a str>,
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
    smart: bool,
    gpu_backend: RenderGpuBackend,
    /// Authenticated GPU runtime-module policy for this render (#290), threaded
    /// into the length-one session so a GPU single-image render routes through
    /// the session like the CPU shapes do instead of staying on the one-shot
    /// transport. `None` for CPU or policy-less renders; the session-eligibility
    /// gate only sets `Some` for Argb32f with a GPU backend.
    gpu_runtime_policy: Option<GpuRuntimePolicyInput<'a>>,
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
    /// reason. There is no second transport (#98 W4, #264, #365): the caller
    /// turns this into an explicit fail-closed error so a session
    /// infrastructure failure surfaces instead of being masked.
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
    let mut dependency_search_dirs = match request.plugin_path.parent() {
        Some(parent) => vec![parent.to_path_buf()],
        None => {
            return SessionWrapperOutcome::Failure(invalid(
                "plugin path has no parent directory to search for dependencies",
            ));
        }
    };
    for dependency in request.dependencies {
        let Some(parent) = dependency.path.parent() else {
            return SessionWrapperOutcome::Failure(invalid(
                "approved dependency has no parent directory",
            ));
        };
        if !dependency_search_dirs.iter().any(|root| root == parent) {
            dependency_search_dirs.push(parent.to_path_buf());
        }
    }
    let session_request = SessionOpenRequest {
        repository: request.repository,
        plugin_path: request.plugin_path,
        plugin_sha256: request.plugin_sha256,
        parameters: request.interactive_parameters,
        payload_override: request.payload_override,
        parameter_animation: request.parameter_animation,
        aux_manifest: request.aux_manifest,
        world_dump_dir: world_dump_dir.as_ref().map(|dump| dump.path.as_path()),
        output_checksum_detail,
        layers: &request.layers,
        mask_trailer: request.mask_trailer.clone(),
        spatial_trailer: request.spatial_trailer.clone(),
        render_environment_trailer: request.render_environment_trailer.clone(),
        audio_trailer: request.audio.as_ref().map(|audio| audio.trailer.clone()),
        alpha_as_coverage_params: request.alpha_as_coverage_params,
        conformance_render_settings: request.conformance_render_settings,
        dependencies: Vec::new(),
        // #816 made a non-empty search root set part of the in-place protocol,
        // so an empty one fails session open for every route that reaches here.
        dependency_search_dirs,
        width: request.width,
        height: request.height,
        pixel_format: request.pixel_format,
        time_step: request.timing.time_step,
        total_time: request.timing.total_time,
        time_scale: request.timing.time_scale,
        frame_deadline: Duration::from_millis(request.timeout_ms),
        smart: request.smart,
        gpu_backend: request.gpu_backend,
        // Threaded from the wrapper request (#290): a GPU single-image render
        // carries its authenticated runtime-module policy into the session so the
        // session opens the GPU command and authorizes the device dispatch,
        // exactly like the one-shot GPU path. `None` keeps CPU/policy-less
        // renders on the CPU session command.
        gpu_runtime_policy: request.gpu_runtime_policy,
        launch_environment: Default::default(),
    };
    // A custom UI action is the explicit interactive harness route (#107/#238)
    // and must retain the caller's desktop. Plain discovery/render workers stay
    // on the private desktop boundary from RenderSession::open.
    let mut session = match if request.custom_ui_action.is_some() {
        RenderSession::open_on_current_desktop(session_request)
    } else {
        RenderSession::open(session_request)
    } {
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
    // `close` consumes the session before any return below.  In particular a
    // deadline/crash/invalidation or rejected final report cannot leak a live
    // worker, its lease state, or the frame pixels into a subsequent render.
    let close = session.close();
    let final_report = match validated_wrapper_final_report(&close, request.smart) {
        Ok(report) => report,
        Err(invariant) => {
            return SessionWrapperOutcome::Fallback(close_failure_diagnostic(&close, invariant));
        }
    };
    let classification = close["worker"]["classification"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    let mut diagnostics = close["worker"]["diagnostics"].clone();
    // Lift the worker's structured suite records into the diagnostics, the way
    // the deleted one-shot dispatch did after every launch. Without this a
    // session render silently drops `missing_suites` /
    // `unsupported_suite_calls`, which is exactly the "a compatibility gap must
    // become a reproducible diagnostic" contract those fields exist for. The
    // one-shot was the only caller before #365 deleted it, so the propagation
    // had to move here rather than go with it.
    propagate_missing_suites(&mut diagnostics, &final_report);
    propagate_unsupported_suite_calls(&mut diagnostics, &final_report);
    propagate_suite_call_slot_probe(&mut diagnostics, &final_report);
    propagate_selector_invocations(&mut diagnostics, &final_report);
    propagate_host_callback_timeline(&mut diagnostics, &final_report);
    propagate_compute_cache_timeline(&mut diagnostics, &final_report);
    propagate_extended_lookup_timeline(&mut diagnostics, &final_report);
    propagate_extended_allocation_timeline(&mut diagnostics, &final_report);
    propagate_suite_timeline(&mut diagnostics, &final_report);
    // Name the stage when the worker itself rejected its output pixels. The
    // gate below turns that into an error carrying these diagnostics, so
    // without this the failure reads as an unattributed validation failure.
    // The deleted one-shot set the same annotation (#365).
    if final_report.get("output_pixels_valid") == Some(&Value::Bool(false)) {
        diagnostics["failure_stage"] = json!("output_validation");
    }
    let gate = validate_interactive_worker_report(
        &final_report,
        &diagnostics,
        &InteractiveGateFacts {
            smart: request.smart,
            pixel_format: request.pixel_format,
            spatial: request.spatial,
            expected_quality: request.expected_quality,
            expected_field: request.expected_field,
            expected_shutter_angle: request.expected_shutter_angle,
            expected_shutter_phase: request.expected_shutter_phase,
            custom_ui_action: request.custom_ui_action,
            // The session carries an audio source whenever the caller supplied
            // the `session-audio:v1|` trailer, so the audio gate must apply on
            // this route exactly as it does on the one-shot (issue #339). A
            // hardcoded `false` here would let a plug-in that never advertises
            // audio usage pass the session while the one-shot rejects it.
            audio_present: request.audio.is_some(),
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
        FrameStatus::FrameError { render_error, .. } => {
            // The gate above rejects any final report carrying a render
            // error, so this arm is defensive only.
            return SessionWrapperOutcome::Failure(invalid(format!(
                "session frame reported error {render_error} past a clean final report"
            )));
        }
    };
    // A SmartFX render whose PreRender returned a legally empty result_rect
    // (#278) rendered no pixels: 0x0 geometry with no bytes is the contract's
    // fulfillment, and no PNG or raw can represent it, so skip the pixel pipeline
    // while the report carries the empty-geometry fields — the same contract the
    // one-shot smart path applies. Only a smart session can produce this
    // (validate_ok_frame requires it).
    let empty_smart_result = request.smart && rendered_width == 0 && rendered_height == 0;
    if !empty_smart_result {
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
    }
    let mut deep_overrange_samples = None;
    let png_written = if empty_smart_result {
        Ok(())
    } else if request.deep_png_output {
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
        smart: request.smart,
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
        secondary_layers: json!(
            request
                .layers
                .iter()
                .filter(|layer| layer.timed.is_none())
                .map(|layer| json!({
                    "slot": layer.slot,
                    "width": layer.width,
                    "height": layer.height,
                }))
                .collect::<Vec<_>>()
        ),
        empty_smart_result,
        // The empty branch above never writes the preserved raw sidecar, so
        // pointing the report at that path would name a file that does not
        // exist. Mirror the one-shot smart guard and report no raw for an
        // empty result.
        output_raw: if empty_smart_result {
            None
        } else {
            request
                .preserved_output
                .map(|path| path.to_string_lossy().into_owned())
        },
        deep_png_output: request.deep_png_output,
        deep_overrange_samples,
        world_dump_display: world_dump_dir.as_ref().map(|dump| dump.display.clone()),
        minidump_display: minidump_directory.clone(),
        output_checksum_detail,
        output_origin_ok,
        parameter_count_ok,
        spatial_ok,
        audio_input_sha256: request
            .audio
            .as_ref()
            .map(|audio| audio.input_sha256.clone()),
    };
    RENDER_SESSION_WRAPPER_RENDERS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    SessionWrapperOutcome::Report(build_interactive_image_report(&final_report, facts))
}

/// A bounded failure summary for the public one-shot-compatible wrapper.  The
/// typed invariant comes from `render_session`; the only worker values exposed
/// are the four small lease counters.  Do not add report strings, paths, or
/// launch details here: this path is also used after crashes and timeouts.
fn close_failure_diagnostic(
    close: &Value,
    invariant: crate::render_session::CloseReportInvariant,
) -> String {
    let report = close.get("final_report");
    let counter = |key: &str| {
        report
            .and_then(|report| report.get(key))
            .and_then(Value::as_u64)
            .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
    };
    let invalidated = close
        .get("invalidated")
        .and_then(Value::as_bool)
        .map_or("unknown", |value| if value { "true" } else { "false" });
    let worker_ok = close
        .get("worker")
        .and_then(|worker| worker.get("classification"))
        .and_then(Value::as_str)
        .is_some_and(|classification| classification == "ok");
    format!(
        "render session close rejected invariant={} invalidated={} worker_ok={} suite_acquires={} suite_releases={} live_suite_lease_count={} live_suite_reference_count={}",
        invariant.as_str(),
        invalidated,
        worker_ok,
        counter("suite_acquires"),
        counter("suite_releases"),
        counter("live_suite_lease_count"),
        counter("live_suite_reference_count"),
    )
}

/// The wrapper has no retained close state: each render consumes its session,
/// validates that close summary, then clones only that summary's final report.
/// Keeping this boundary explicit makes a rejected close unable to carry its
/// image, lease diagnostics, or report into the next independently opened
/// session.
fn validated_wrapper_final_report(
    close: &Value,
    smart: bool,
) -> Result<Value, crate::render_session::CloseReportInvariant> {
    crate::render_session::validate_close_report(close, smart)?;
    close
        .get("final_report")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or(crate::render_session::CloseReportInvariant::FinalReportMissing)
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
    /// A validated, closed selection snapshot.  Callers cannot pair an
    /// arbitrary source string with a path or reopen a session with stale
    /// inspection state.
    pub selection: InteractiveSessionSelection,
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
    selection: InteractiveSessionSelection,
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
                payload_override: None,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                render_environment_trailer: None,
                audio_trailer: None,
                alpha_as_coverage_params: &[],
                conformance_render_settings: None,
                layers: &[],
                smart: request.selection.path.is_smart(),
                gpu_backend: RenderGpuBackend::Auto,
                gpu_runtime_policy: None,
                dependencies: request.dependencies,
                dependency_search_dirs: vec![
                    request
                        .plugin_path
                        .parent()
                        .ok_or_else(|| invalid("interactive plugin path has no parent directory"))?
                        .to_path_buf(),
                ],
                width: request.width,
                height: request.height,
                pixel_format: request.pixel_format,
                time_step: request.time_step,
                total_time: request.total_time,
                time_scale: request.time_scale,
                frame_deadline: Duration::from_millis(request.timeout_ms),
                launch_environment: Default::default(),
            },
        )?;
        Ok(Self {
            session,
            plugin_id: request.plugin_id.to_owned(),
            selection: request.selection,
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
                width: frame_width,
                height: frame_height,
                ..
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
                let mut report = json!({
                    "schema_version": 1,
                    "stage": "interactive_image_render",
                    "plugin_id": self.plugin_id,
                    "render_path": self.selection.path.report_name(),
                    "smart_capability_source": self.selection.source.report_name(),
                    "smart_capability_identity": self.selection.capability_identity,
                    "smart_capability_version": self.selection.capability_version,
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
                });
                annotate_interactive_selection(&mut report, self.selection);
                Ok(report)
            }
            FrameStatus::FrameError {
                render_error,
                missing_dependency,
                return_message,
            } => {
                self.frames_errored += 1;
                let mut report = json!({
                    "schema_version": 1,
                    "stage": "interactive_image_render",
                    "plugin_id": self.plugin_id,
                    "render_path": self.selection.path.report_name(),
                    "smart_capability_source": self.selection.source.report_name(),
                    "smart_capability_identity": self.selection.capability_identity,
                    "smart_capability_version": self.selection.capability_version,
                    "pixel_format": self.pixel_format.report_name(),
                    "current_time": current_time,
                    "worker_classification": "resident_session",
                    "resident_session": session_facts(self.frames_ok, self.frames_errored),
                    "render_error": render_error,
                    "missing_dependency": missing_dependency,
                    // The plug-in's own account of the failure (issue #707).
                    "return_message": return_message,
                    "passed": false,
                });
                annotate_interactive_selection(&mut report, self.selection);
                Ok(report)
            }
        }
    }

    /// Ends the session and returns the close summary, including the final
    /// report and the `session_clean` verdict (`render_session.rs`).
    pub fn close(self) -> Value {
        let Self {
            session, selection, ..
        } = self;
        let mut summary = session.close();
        annotate_interactive_selection(&mut summary, selection);
        summary
    }
}

/// The only selector paths supported by the resident image session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractiveRenderPath {
    Classic,
    SmartFx,
}

impl InteractiveRenderPath {
    pub fn is_smart(self) -> bool {
        matches!(self, Self::SmartFx)
    }

    pub fn report_name(self) -> &'static str {
        if self.is_smart() {
            "smartfx"
        } else {
            "classic"
        }
    }
}

/// Provenance is deliberately closed: receipts cannot claim an unrecognised
/// capability source supplied by a caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractiveCapabilitySource {
    AdvertisedSmart,
    AdvertisedClassic,
    ManualSmart,
    ManualClassic,
}

impl InteractiveCapabilitySource {
    pub fn report_name(self) -> &'static str {
        match self {
            Self::AdvertisedSmart => "advertised_smart",
            Self::AdvertisedClassic => "advertised_classic",
            Self::ManualSmart => "manual_smart",
            Self::ManualClassic => "manual_classic",
        }
    }

    fn matches_path(self, path: InteractiveRenderPath) -> bool {
        matches!(
            (self, path),
            (
                Self::AdvertisedSmart | Self::ManualSmart,
                InteractiveRenderPath::SmartFx
            ) | (
                Self::AdvertisedClassic | Self::ManualClassic,
                InteractiveRenderPath::Classic
            )
        )
    }
}

/// Inspection-bound path choice supplied to an interactive session.  Version
/// and identity are part of the session key in the harness, so a changed
/// descriptor cannot reuse a worker opened from an older observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InteractiveSessionSelection {
    pub path: InteractiveRenderPath,
    pub source: InteractiveCapabilitySource,
    pub capability_version: u32,
    pub capability_identity: u64,
}

impl InteractiveSessionSelection {
    pub fn new(
        path: InteractiveRenderPath,
        source: InteractiveCapabilitySource,
        capability_version: u32,
        capability_identity: u64,
    ) -> io::Result<Self> {
        if capability_version == 0 || !source.matches_path(path) {
            return Err(invalid(
                "interactive render capability selection is invalid",
            ));
        }
        Ok(Self {
            path,
            source,
            capability_version,
            capability_identity,
        })
    }
}

/// Attach the complete open-time selection snapshot to every route's public
/// report.  The one-shot fallback deliberately calls this too; reducing the
/// selection to a `smart: bool` there would make receipts lie about why the
/// worker used a selector sequence.
pub fn annotate_interactive_selection(report: &mut Value, selection: InteractiveSessionSelection) {
    report["render_path"] = json!(selection.path.report_name());
    report["smart_capability_source"] = json!(selection.source.report_name());
    report["smart_capability_identity"] = json!(selection.capability_identity);
    report["smart_capability_version"] = json!(selection.capability_version);
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
                facts
                    .spatial
                    .full_resolution_width
                    .unwrap_or(facts.input_width),
                facts
                    .spatial
                    .full_resolution_height
                    .unwrap_or(facts.input_height)
            ]))
        && worker_report.get("in_data_dimensions")
            == Some(&json!([
                facts
                    .spatial
                    .full_resolution_width
                    .unwrap_or(facts.input_width),
                facts
                    .spatial
                    .full_resolution_height
                    .unwrap_or(facts.input_height)
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
        (
            "extra_pixels_contract_violation",
            "extra_pixels_contract_violation",
        ),
        (
            "smart_render_selector_dispatched",
            "smart_render_selector_dispatched",
        ),
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
            "automatic_audio_checkins",
            "audio_get_data_calls",
            "invalid_audio_operations",
            "unadvertised_audio_checkout_calls",
            "rejected_unadvertised_audio_checkouts",
            "rejected_audio_format_requests",
            "audio_handle_exhaustions",
            "peak_live_audio_handles",
            "last_audio_checkout_index",
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
        // Keep probe-specific lifecycle and selector telemetry visible on the
        // session projection as it was on the deleted one-shot report.  The
        // built-artifact probes use these fields as their byte/lifetime oracle.
        "pre_render_error",
        "result_rects_valid",
        "last_seh_exception_code",
        "receipt_lifetimes_balanced",
        "receipts_created",
        "receipts_checked_in",
        "live_receipts",
        "live_receipt_bytes",
        "invalid_receipt_operations",
        "async_layer_requests_balanced",
        "async_layer_requests_created",
        "async_layer_requests_completed",
        "async_layer_requests_canceled",
        "live_async_layer_requests",
        "async_layer_reserved_bytes",
        "malformed_checkout_request_count",
        "empty_checkout_pixel_denial_count",
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
