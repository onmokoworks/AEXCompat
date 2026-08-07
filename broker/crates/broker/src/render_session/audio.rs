use super::*;

// ---------------------------------------------------------------------------
// Audio render session (protocol §10). Independent of the image session above;
// it reuses the same launch/transport/reader/watcher machinery (inheritable
// pipes, SessionTransport, dispatch_secure_image_session, SessionChildHandles,
// the reader thread + process-death watcher, SecureSessionProcess/CollectedExit,
// the three-way await) but carries a bulk audio span instead of image frames.
// ---------------------------------------------------------------------------

const AUDIO_HEADER_MAGIC: u32 = 0x5355_4141; // "AAUS" little-endian
const AUDIO_MAX_SAMPLES_OFFSET: usize = 8;
const AUDIO_CHANNELS_OFFSET: usize = 12;
const AUDIO_INPUT_GENERATION_OFFSET: usize = 16;
const AUDIO_OUTPUT_GENERATION_OFFSET: usize = 20;
const AUDIO_OUTPUT_SAMPLES_OFFSET: usize = 28;
// v1 audio is mono; the channel bound stays in the geometry for the extension.
const AUDIO_MAX_SAMPLES_CAP: u32 = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
struct AudioSessionGeometry {
    max_samples: u32,
    channels: u32,
}

impl AudioSessionGeometry {
    fn slot_bytes(&self) -> usize {
        self.max_samples as usize * self.channels as usize * 4
    }
    fn output_slot_offset(&self) -> usize {
        HEADER_BYTES + align_slot(self.slot_bytes())
    }
    fn section_bytes(&self) -> usize {
        self.output_slot_offset() + align_slot(self.slot_bytes())
    }
}

pub struct AudioSessionOpenRequest<'a> {
    pub repository: &'a Path,
    pub plugin_path: &'a Path,
    pub plugin_sha256: &'a str,
    pub parameters: Option<&'a [InteractiveParameter]>,
    pub dependencies: Vec<ApprovedImageArtifact>,
    /// In-place load mode (issue #751): non-empty opens the session on the
    /// plug-in's real path with these directories admitted into the worker's
    /// DLL search set, exactly like the image session's field. Mutually
    /// exclusive with `dependencies`.
    pub dependency_search_dirs: Vec<PathBuf>,
    pub max_samples: u32,
    pub channels: u32,
    pub time_scale: u32,
    pub frame_deadline: Duration,
    /// Per-launch environment inputs (issue #910), forwarded to the worker
    /// launch instead of the broker mutating its own environment.
    pub launch_environment: crate::secure_launch::LaunchEnvironment,
}

#[derive(Debug)]
pub enum AudioSpanStatus {
    /// The span rendered and every invariant held. `samples` are the rendered
    /// f32 output bytes copied out of the output slot (LE, matching the
    /// one-shot .f32 output); `checksum` is their lowercase SHA-256.
    Rendered {
        samples: Vec<u8>,
        checksum: String,
        /// The plugin's AUDIO_SETUP output start sample; the wrapper reports it
        /// as `output_start_sample` instead of hard-coding 0 (Codex #252).
        output_start: i64,
    },
    /// A per-span compatibility diagnostic; the session stays usable.
    SpanError { render_error: i64 },
}

#[derive(Debug)]
pub struct AudioSpanOutcome {
    pub request_index: u32,
    pub status: AudioSpanStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioDoneOutput {
    start_sample: i64,
    sample_count: u32,
    rate: u32,
    channels: u32,
    sample_size: u32,
    checksum: String,
    guards_intact: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioDone {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    request_index: u32,
    status: String,
    #[serde(default)]
    output: Option<AudioDoneOutput>,
    audio_render_error: i64,
    #[serde(default)]
    generation: Option<u32>,
}

pub struct AudioRenderSession {
    process: Option<SecureSessionProcess>,
    collected: Option<CollectedExit>,
    transport: SessionTransport,
    receiver: mpsc::Receiver<SessionEvent>,
    process_exit_observed: bool,
    geometry: AudioSessionGeometry,
    rate: u32,
    frame_deadline: Duration,
    invalidation: Option<SessionInvalidation>,
    last_output_generation: u32,
    requests_ok: u32,
    requests_errored: u32,
    opened: Instant,
    plugin_sha256: String,
}

impl AudioRenderSession {
    pub fn open(request: AudioSessionOpenRequest<'_>) -> io::Result<AudioRenderSession> {
        if request.time_scale == 0
            || request.time_scale > i32::MAX as u32
            || request.frame_deadline.is_zero()
        {
            return Err(invalid("audio session timing is invalid"));
        }
        if request.max_samples == 0 || request.max_samples > AUDIO_MAX_SAMPLES_CAP {
            return Err(invalid("audio session max_samples is out of range"));
        }
        if request.channels != 1 {
            return Err(invalid("audio session v1 is mono (channels must be 1)"));
        }
        let geometry = AudioSessionGeometry {
            max_samples: request.max_samples,
            channels: request.channels,
        };
        if geometry.section_bytes() as u64 > SECTION_HARD_CAP_BYTES {
            return Err(invalid("audio session section exceeds the hard cap"));
        }
        let payload = encode_interactive_payload(request.parameters.unwrap_or_default())?;

        let (request_read, request_write) = inheritable_pipe(false)?;
        let (response_read, response_write) = inheritable_pipe(true)?;
        let section_bytes = geometry.section_bytes();
        let mut security = inheritable_security();
        let section = OwnedHandle::new(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                &mut security,
                PAGE_READWRITE,
                ((section_bytes as u64) >> 32) as u32,
                section_bytes as u32,
                null(),
            )
        })?;
        let view_address = unsafe { MapViewOfFile(section.raw(), FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view_address.Value.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut transport = SessionTransport {
            request_write: Some(request_write),
            section,
            view: view_address.Value as *mut u8,
            section_bytes,
        };
        transport.write_header_u32(MAGIC_OFFSET, AUDIO_HEADER_MAGIC);
        // The audio session's shared-memory layout is unchanged (no layer
        // slots), so its header version stays 1 (matching the audio worker's
        // own `kProtocolVersion`); only the classic layout bumped to 2 (#264).
        transport.write_header_u32(VERSION_OFFSET, PROTOCOL_VERSION);
        transport.write_header_u32(AUDIO_MAX_SAMPLES_OFFSET, geometry.max_samples);
        transport.write_header_u32(AUDIO_CHANNELS_OFFSET, geometry.channels);
        transport.write_header_u32(AUDIO_INPUT_GENERATION_OFFSET, 0);
        transport.write_header_u32(AUDIO_OUTPUT_GENERATION_OFFSET, 0);
        transport.write_header_u32(AUDIO_OUTPUT_SAMPLES_OFFSET, 0);

        let plugin = ApprovedImageArtifact {
            path: request.plugin_path.to_path_buf(),
            expected_sha256: decode_sha256_hex(request.plugin_sha256)?,
            expected_size: fs::metadata(request.plugin_path)?.len(),
        };
        let args_before_plugin = vec!["--render-audio-session-v1".to_owned()];
        let args_after_plugin = vec![
            request.plugin_sha256.to_ascii_lowercase(),
            payload,
            geometry.max_samples.to_string(),
            geometry.channels.to_string(),
            request.time_scale.to_string(),
        ];
        let dispatch = SecureImageDispatch {
            repository: request.repository,
            worker_kind: WorkerKind::Render,
            plugin,
            dependencies: request.dependencies,
            dependency_search_dirs: request.dependency_search_dirs,
            args_before_plugin: &args_before_plugin,
            args_after_plugin: &args_after_plugin,
            timeout: Some(request.frame_deadline),
            launch_environment: request.launch_environment,
        };
        let child_handles = SessionChildHandles {
            request_read: request_read.raw(),
            response_write: response_write.raw(),
            section: transport.section.raw(),
            // Audio sessions carry no layers.
            layers: Vec::new(),
        };
        let process = dispatch_secure_image_session(dispatch, &child_handles)?;
        drop(request_read);
        drop(response_write);

        let (sender, receiver) = mpsc::channel::<SessionEvent>();
        let response_handle = response_read.take() as usize;
        let reader_sender = sender.clone();
        thread::spawn(move || {
            let handle = response_handle as HANDLE;
            let _owner = match OwnedHandle::new(handle) {
                Ok(owner) => owner,
                Err(_) => return,
            };
            loop {
                let mut prefix = [0u8; 4];
                if !read_exact_handle(handle, &mut prefix) {
                    return;
                }
                let length = u32::from_le_bytes(prefix) as usize;
                if length == 0 || length > MAX_MESSAGE_BYTES {
                    let _ = reader_sender.send(SessionEvent::ReaderViolation);
                    return;
                }
                let mut body = vec![0u8; length];
                if !read_exact_handle(handle, &mut body) {
                    let _ = reader_sender.send(SessionEvent::ReaderViolation);
                    return;
                }
                if reader_sender.send(SessionEvent::Message(body)).is_err() {
                    return;
                }
            }
        });
        let watched_process = process.duplicated_process_handle()?;
        thread::spawn(move || {
            use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
            let handle = watched_process as HANDLE;
            unsafe {
                WaitForSingleObject(handle, INFINITE);
                CloseHandle(handle);
            }
            let _ = sender.send(SessionEvent::ProcessExited);
        });

        Ok(AudioRenderSession {
            process: Some(process),
            collected: None,
            transport,
            receiver,
            process_exit_observed: false,
            geometry,
            rate: request.time_scale,
            frame_deadline: request.frame_deadline,
            invalidation: None,
            last_output_generation: 0,
            requests_ok: 0,
            requests_errored: 0,
            opened: Instant::now(),
            plugin_sha256: request.plugin_sha256.to_ascii_lowercase(),
        })
    }

    fn collect_exit(&mut self, wait: Duration) {
        if self.collected.is_some() {
            return;
        }
        let Some(process) = self.process.take() else {
            return;
        };
        self.collected = Some(match process.finish(Some(wait)) {
            Ok(result) => CollectedExit {
                result: Some(result),
                error: None,
            },
            Err(error) => CollectedExit {
                result: None,
                error: Some(error.to_string()),
            },
        });
    }

    fn invalidate(&mut self, reason: &'static str, detail: String, wait: Duration) -> io::Error {
        if let Some(process) = self.process.as_ref() {
            let _ = process.terminate_job();
        }
        self.collect_exit(wait);
        self.invalidation = Some(SessionInvalidation { reason, detail });
        let stored = self.invalidation.as_ref().expect("just stored");
        invalid(format!(
            "audio session invalidated ({}): {}",
            stored.reason, stored.detail
        ))
    }

    fn await_response(&mut self) -> FrameWait {
        let deadline = Instant::now() + self.frame_deadline;
        loop {
            let mut remaining = deadline.saturating_duration_since(Instant::now());
            if self.process_exit_observed {
                remaining = remaining.min(PROCESS_EXIT_DRAIN);
            }
            if remaining.is_zero() {
                return if self.process_exit_observed {
                    FrameWait::WorkerGone
                } else {
                    FrameWait::Deadline
                };
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(SessionEvent::Message(body)) => return FrameWait::Message(body),
                Ok(SessionEvent::ReaderViolation) => return FrameWait::FramingViolation,
                Ok(SessionEvent::ProcessExited) => self.process_exit_observed = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return if self.process_exit_observed {
                        FrameWait::WorkerGone
                    } else {
                        FrameWait::Deadline
                    };
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return FrameWait::WorkerGone,
            }
        }
    }

    fn static_header_ok(&self) -> bool {
        self.transport.read_header_u32(MAGIC_OFFSET) == AUDIO_HEADER_MAGIC
            // Audio layout unchanged: header version stays 1 (see open).
            && self.transport.read_header_u32(VERSION_OFFSET) == PROTOCOL_VERSION
            && self.transport.read_header_u32(AUDIO_MAX_SAMPLES_OFFSET) == self.geometry.max_samples
            && self.transport.read_header_u32(AUDIO_CHANNELS_OFFSET) == self.geometry.channels
    }

    /// Renders one bulk audio span: `input` is the interleaved f32 samples the
    /// broker places in the input slot; the worker reads them, drives
    /// AUDIO_SETUP/RENDER/SETDOWN, and returns the rendered f32 output.
    pub fn render_span(
        &mut self,
        request_index: u32,
        input: &[f32],
    ) -> io::Result<AudioSpanOutcome> {
        if let Some(invalidation) = &self.invalidation {
            return Err(invalid(format!(
                "audio session is invalidated ({}): {}",
                invalidation.reason, invalidation.detail
            )));
        }
        loop {
            match self.receiver.try_recv() {
                Ok(SessionEvent::ProcessExited) => self.process_exit_observed = true,
                Ok(SessionEvent::ReaderViolation) => {
                    return Err(self.invalidate(
                        "response_framing_violation",
                        format!("the worker broke the response framing before {request_index}"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                Ok(SessionEvent::Message(_)) => {
                    return Err(self.invalidate(
                        "unsolicited_response",
                        format!(
                            "a response arrived with no request in flight before {request_index}"
                        ),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                Err(_) => break,
            }
        }
        if self.process_exit_observed {
            return Err(self.invalidate(
                "worker_exited",
                format!("the worker exited before request {request_index} was dispatched"),
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let max_samples = self.geometry.max_samples as usize * self.geometry.channels as usize;
        if input.len() > max_samples {
            return Err(invalid("audio span sample count exceeds the session slot"));
        }
        let expected_generation = request_index
            .checked_add(1)
            .ok_or_else(|| invalid("request index overflows the generation counter"))?;
        if expected_generation <= self.last_output_generation {
            return Err(invalid(format!(
                "request index {request_index} does not advance the last completed generation {}",
                self.last_output_generation
            )));
        }
        let input_bytes =
            unsafe { std::slice::from_raw_parts(input.as_ptr() as *const u8, input.len() * 4) };
        self.transport.write_input_slot(input_bytes);
        self.transport
            .write_header_u32(AUDIO_INPUT_GENERATION_OFFSET, expected_generation);
        let message = format!(
            "{{\"v\":1,\"type\":\"audio_render\",\"request_index\":{request_index},\"input_samples\":{}}}",
            input.len()
        );
        if !self.transport.send_message(&message) {
            return Err(self.invalidate(
                "request_pipe_closed",
                "the session request pipe rejected an audio_render message".into(),
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let body = match self.await_response() {
            FrameWait::Message(body) => body,
            FrameWait::Deadline => {
                return Err(self.invalidate(
                    "request_deadline",
                    format!("request {request_index} exceeded the deadline"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            FrameWait::WorkerGone => {
                return Err(self.invalidate(
                    "worker_exited",
                    format!("the worker was gone before request {request_index} completed"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            FrameWait::FramingViolation => {
                return Err(self.invalidate(
                    "response_framing_violation",
                    format!("the worker broke the response framing during request {request_index}"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        let done: AudioDone = match serde_json::from_slice(&body) {
            Ok(done) => done,
            Err(error) => {
                return Err(self.invalidate(
                    "malformed_audio_done",
                    format!("request {request_index} response did not parse strictly: {error}"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        if done.v != PROTOCOL_VERSION
            || done.kind != "audio_done"
            || done.request_index != request_index
        {
            return Err(self.invalidate(
                "audio_done_mismatch",
                format!(
                    "request {request_index} response carried v={} type={} request_index={}",
                    done.v, done.kind, done.request_index
                ),
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        match done.status.as_str() {
            "error" => {
                // A zero error code on an "error" status is a success in
                // disguise; reject it so a failed selector cannot be recorded
                // as SpanError(0) (Codex #252, mirroring the image session).
                if done.output.is_some()
                    || done.generation.is_some()
                    || done.audio_render_error == 0
                {
                    return Err(self.invalidate(
                        "malformed_error_response",
                        format!("request {request_index} error response carried output fields or zero error"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                if !self.static_header_ok()
                    || self
                        .transport
                        .read_header_u32(AUDIO_OUTPUT_GENERATION_OFFSET)
                        != self.last_output_generation
                {
                    return Err(self.invalidate(
                        "request_invariant_failure",
                        format!("request {request_index} error response mutated the header"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                self.requests_errored += 1;
                Ok(AudioSpanOutcome {
                    request_index,
                    status: AudioSpanStatus::SpanError {
                        render_error: done.audio_render_error,
                    },
                })
            }
            "ok" => {
                let (Some(output), Some(generation)) = (done.output, done.generation) else {
                    return Err(self.invalidate(
                        "malformed_ok_response",
                        format!("request {request_index} ok response missed output or generation"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                };
                if done.audio_render_error != 0
                    || !output.guards_intact
                    || output.channels != self.geometry.channels
                    || output.rate != self.rate
                    || output.sample_size != 4
                    || output.sample_count as usize > max_samples
                    // Bound the reported output window against the submitted
                    // input span (Codex #252): a negative start or a
                    // start+count past input.len() is an invalid range the
                    // one-shot path rejects via setup_range_valid; the session
                    // must independently reject it before publishing it.
                    || output.start_sample < 0
                    // saturating_add so a huge start_sample cannot overflow i64
                    // (debug panic / release wrap) before the range check (Codex #252).
                    || output.start_sample.saturating_add(output.sample_count as i64)
                        > input.len() as i64
                    || generation != expected_generation
                    || !self.static_header_ok()
                    || self.transport.read_header_u32(AUDIO_OUTPUT_GENERATION_OFFSET)
                        != expected_generation
                    || self.transport.read_header_u32(AUDIO_OUTPUT_SAMPLES_OFFSET)
                        != output.sample_count
                {
                    return Err(self.invalidate(
                        "request_invariant_failure",
                        format!("request {request_index} ok response failed validation"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                let samples = self.transport.read_output_slot(
                    self.geometry.output_slot_offset(),
                    output.sample_count as usize * 4,
                );
                // The worker's checksum is the sha256 of the output bytes it
                // wrote; recompute over the bytes the broker read to confirm the
                // shared slot was not disturbed mid-flight.
                let checksum = {
                    use sha2::{Digest, Sha256};
                    format!("{:x}", Sha256::digest(&samples))
                };
                if checksum != output.checksum {
                    return Err(self.invalidate(
                        "request_invariant_failure",
                        format!("request {request_index} output checksum mismatch"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                self.last_output_generation = expected_generation;
                self.requests_ok += 1;
                Ok(AudioSpanOutcome {
                    request_index,
                    status: AudioSpanStatus::Rendered {
                        samples,
                        checksum,
                        output_start: output.start_sample,
                    },
                })
            }
            other => Err(self.invalidate(
                "audio_done_status",
                format!("request {request_index} carried unknown status {other}"),
                POST_TERMINATION_COLLECT_TIMEOUT,
            )),
        }
    }

    pub fn close(mut self) -> Value {
        if self.invalidation.is_none() && self.process.is_some() {
            loop {
                match self.receiver.try_recv() {
                    Ok(SessionEvent::ProcessExited) => self.process_exit_observed = true,
                    Ok(SessionEvent::ReaderViolation) => {
                        self.invalidation = Some(SessionInvalidation {
                            reason: "response_framing_violation",
                            detail: "the worker broke the response framing before close".into(),
                        });
                        break;
                    }
                    Ok(SessionEvent::Message(_)) => {
                        self.invalidation = Some(SessionInvalidation {
                            reason: "unsolicited_response",
                            detail: "a response arrived with no request in flight before close"
                                .into(),
                        });
                        break;
                    }
                    Err(_) => break,
                }
            }
            if self.process_exit_observed
                || self
                    .process
                    .as_ref()
                    .is_some_and(SecureSessionProcess::has_exited)
            {
                self.process_exit_observed = true;
            }
            if self.invalidation.is_none() && self.process_exit_observed {
                self.invalidation = Some(SessionInvalidation {
                    reason: "premature_exit",
                    detail: "the worker exited before the close handshake".into(),
                });
            }
            if self.invalidation.is_none()
                && !self.transport.send_message("{\"v\":1,\"type\":\"close\"}")
            {
                self.invalidation = Some(SessionInvalidation {
                    reason: "close_send_failed",
                    detail: "the close message could not be delivered".into(),
                });
            }
        }
        self.transport.close_request_pipe();
        self.collect_exit(CLOSE_COLLECT_TIMEOUT);
        let elapsed_ms = self.opened.elapsed().as_millis();
        let collected = self.collected.take();
        let (worker, final_report) = match &collected {
            Some(CollectedExit {
                result: Some(result),
                ..
            }) => {
                let report: Option<Value> =
                    crate::worker_module_audit::parse_report_prefix(&result.stdout).ok();
                (
                    json!({
                        "classification": result.classification.as_str(),
                        "exit_code": result.exit_code,
                        "diagnostics": isolated_worker_diagnostics(result, elapsed_ms),
                    }),
                    report,
                )
            }
            Some(CollectedExit {
                error: Some(error), ..
            }) => (json!({ "collection_error": error }), None),
            _ => (
                json!({ "collection_error": "worker was never collected" }),
                None,
            ),
        };
        let session_clean = self.invalidation.is_none()
            && matches!(
                &collected,
                Some(CollectedExit { result: Some(result), .. })
                    if result.classification == crate::ExitClassification::Ok
            )
            && final_report.as_ref().is_some_and(audio_final_report_clean);
        json!({
            "stage": "audio_session_close",
            "plugin_sha256": self.plugin_sha256,
            "max_samples": self.geometry.max_samples,
            "channels": self.geometry.channels,
            "requests_ok": self.requests_ok,
            "requests_errored": self.requests_errored,
            "invalidated": self.invalidation.is_some(),
            "invalidated_reason": self.invalidation.as_ref().map(|invalidation| json!({
                "reason": invalidation.reason,
                "detail": invalidation.detail,
            })),
            "worker": worker,
            "final_report": final_report,
            "session_clean": session_clean,
        })
    }
}

/// A clean audio session close requires the worker's aggregate report to agree
/// (protocol §10.3): the session completed, GLOBAL_SETDOWN was clean, no
/// protocol violation or invariant failure, and the audio ownership ledger
/// balanced. Missing keys fail closed.
fn audio_final_report_clean(report: &Value) -> bool {
    report.get("status") == Some(&json!("session_completed"))
        && report.get("global_setup_error") == Some(&json!(0))
        && report.get("params_setup_error") == Some(&json!(0))
        && report.get("global_setdown_error") == Some(&json!(0))
        && report.get("session_protocol_violation") == Some(&Value::Bool(false))
        && report.get("session_invariant_failure") == Some(&Value::Bool(false))
        && report.get("audio_lifetimes_balanced") == Some(&Value::Bool(true))
        && report.get("invalid_audio_operations") == Some(&json!(0))
        && report.get("session_clean") == Some(&Value::Bool(true))
}
