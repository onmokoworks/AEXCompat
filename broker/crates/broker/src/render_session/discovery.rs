use super::*;

// ---------------------------------------------------------------------------
// Discovery session (issue #405, docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md
// §4.2). A cluster discovery session launches the worker with
// `--discovery-session-v1 --cluster-manifest-v2 <path>` and no positional
// plugin, then inspects the manifest's plugins one by one over the session
// control channel — the session-mode replacement for per-plugin one-shot
// `--l2-params-only` dispatches. It reuses the same machinery as the image
// and audio sessions above: inheritable pipes, SessionTransport (with a
// header-only section, since discovery carries no pixel transport), the
// cluster dispatch, the reader thread + process-death watcher, and the
// three-way wait with a per-inspect deadline (design §7).
// ---------------------------------------------------------------------------

/// Spawns the response-pipe reader thread and the process-death watcher the
/// session waits multiplex on (protocol §7): the reader enforces the session
/// flavor's message cap and reports framing violations explicitly, the
/// watcher reports worker death even when a descendant keeps the response
/// pipe open.
fn spawn_session_observers(
    process: &SecureSessionProcess,
    response_read: OwnedHandle,
    max_message_bytes: usize,
) -> io::Result<mpsc::Receiver<SessionEvent>> {
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
                // EOF before a response starts is the normal end of the
                // stream (worker exit); the process watcher reports it.
                return;
            }
            let length = u32::from_le_bytes(prefix) as usize;
            if length == 0 || length > max_message_bytes {
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
    Ok(receiver)
}

/// In-place discovery open request (issue #751): the ordered cluster by real
/// path plus the validated dependency search directories. No dependencies,
/// no sealed resources — the loader resolves the closure and the plug-in's
/// own directory holds its data files.
pub struct InPlaceDiscoverySessionOpenRequest<'a> {
    pub repository: &'a Path,
    pub plugins: Vec<ApprovedImageArtifact>,
    pub dependency_search_dirs: Vec<PathBuf>,
    /// The bounded module-enumeration capacity of the recorded audit.
    pub module_bound: u32,
    /// Per-inspect watchdog deadline (design §7).
    pub inspect_deadline: Duration,
    /// Per-launch environment inputs (issue #910), forwarded to the worker
    /// launch instead of the broker mutating its own environment.
    pub launch_environment: crate::secure_launch::LaunchEnvironment,
}

/// The outcome of an `inspect_plugin` exchange (design §4.2).
#[derive(Debug)]
pub enum InspectOutcome {
    /// The plugin loaded and inspected; `report` is the same JSON document
    /// the one-shot `--l2-params-only` path prints (minus the module audit,
    /// which the epoch/final report carries), so callers can consume it in
    /// the existing shape and A/B against the one-shot path directly.
    Inspected { report: Value },
    /// A parameter-local failure (the one-shot exit 12/20 equivalents): the
    /// session stays usable and whether to continue is the caller's
    /// decision. `error_kind` is the worker's structured cause
    /// (`entrypoint_unresolved` / `selector_error`); `report` carries a
    /// partial report when the worker produced one.
    InspectError {
        error_kind: String,
        report: Option<Value>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectDone {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    plugin_index: u32,
    request_index: u32,
    status: String,
    #[serde(default)]
    report: Option<Value>,
    #[serde(default)]
    error_kind: Option<String>,
}

pub struct DiscoverySession {
    process: Option<SecureSessionProcess>,
    collected: Option<CollectedExit>,
    transport: SessionTransport,
    receiver: mpsc::Receiver<SessionEvent>,
    process_exit_observed: bool,
    inspect_deadline: Duration,
    invalidation: Option<SessionInvalidation>,
    plugin_count: u32,
    next_request_index: u32,
    inspects_ok: u32,
    inspects_errored: u32,
    opened: Instant,
    /// The in-place manifest transport (issue #751): the document the worker
    /// read at launch, kept alive for the session. `None` on the sealed
    /// route, whose manifest lives inside the sealed tree.
    _in_place_transport: Option<crate::cluster_manifest::ClusterManifestTransport>,
}

impl DiscoverySession {
    /// In-place variant (issue #751): the cluster manifest (`cluster-manifest-v2`)
    /// names each plug-in by its real path and carries the dependency search
    /// directories; nothing is staged, no closure is walked, and the module
    /// audit is recorded rather than validated against a declared set.
    pub fn open_in_place(
        request: InPlaceDiscoverySessionOpenRequest<'_>,
    ) -> io::Result<DiscoverySession> {
        Self::open_impl(request)
    }

    fn open_impl(request: InPlaceDiscoverySessionOpenRequest<'_>) -> io::Result<DiscoverySession> {
        if request.inspect_deadline.is_zero() {
            return Err(invalid("discovery session inspect deadline is invalid"));
        }
        let (request_read, request_write) = inheritable_pipe(false)?;
        let (response_read, response_write) = inheritable_pipe(true)?;
        // Discovery carries no pixel transport; the inherited header-only
        // section keeps the launch boundary's session handle contract uniform
        // without giving the worker a shared pixel slot it must never use.
        let section_bytes = HEADER_BYTES;
        let mut security = inheritable_security();
        let section = OwnedHandle::new(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                &mut security,
                PAGE_READWRITE,
                0,
                section_bytes as u32,
                null(),
            )
        })?;
        let view_address = unsafe { MapViewOfFile(section.raw(), FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view_address.Value.is_null() {
            return Err(io::Error::last_os_error());
        }
        let transport = SessionTransport {
            request_write: Some(request_write),
            section,
            view: view_address.Value as *mut u8,
            section_bytes,
        };
        let args_before_plugin = vec!["--discovery-session-v1".to_owned()];
        let args_after_plugin: Vec<String> = Vec::new();
        let child_handles = SessionChildHandles {
            request_read: request_read.raw(),
            response_write: response_write.raw(),
            section: transport.section.raw(),
            layers: Vec::new(),
        };
        let dependency_search_dirs = request.dependency_search_dirs;
        let (process, plugin_count, in_place_transport) = {
            let dispatch = crate::secure_image_dispatch::SecureInPlaceClusterDispatch {
                repository: request.repository,
                worker_kind: WorkerKind::Render,
                plugins: request.plugins,
                dependency_search_dirs,
                positional_plugin: false,
                swap_payloads: None,
                module_bound: request.module_bound,
                args_before_plugin: &args_before_plugin,
                args_after_plugin: &args_after_plugin,
                launch_environment: request.launch_environment,
            };
            let launch = crate::secure_image_dispatch::dispatch_secure_in_place_cluster_session(
                dispatch,
                &child_handles,
            )?;
            (
                launch.process,
                launch.manifest.plugin_count(),
                Some(launch.transport),
            )
        };
        // The worker inherited its copies; dropping the broker's child-side
        // ends turns a worker exit into pipe EOF instead of a hang.
        drop(request_read);
        drop(response_write);
        let receiver =
            spawn_session_observers(&process, response_read, MAX_DISCOVERY_MESSAGE_BYTES)?;
        Ok(DiscoverySession {
            process: Some(process),
            collected: None,
            transport,
            receiver,
            process_exit_observed: false,
            inspect_deadline: request.inspect_deadline,
            invalidation: None,
            plugin_count: plugin_count as u32,
            next_request_index: 0,
            inspects_ok: 0,
            inspects_errored: 0,
            opened: Instant::now(),
            _in_place_transport: in_place_transport,
        })
    }

    pub fn invalidation(&self) -> Option<&SessionInvalidation> {
        self.invalidation.as_ref()
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
            "discovery session invalidated ({}): {}",
            stored.reason, stored.detail
        ))
    }

    fn await_response(&mut self) -> FrameWait {
        let deadline = Instant::now() + self.inspect_deadline;
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

    /// Inspects one manifest plugin (design §4.2): sends
    /// `{"v":1,"type":"inspect_plugin","plugin_index":N,"request_index":R}`
    /// and waits for `inspect_done` under the three-way wait. `request_index`
    /// is the 0-based serial the worker cross-checks; an out-of-manifest
    /// plugin index or an off-serial request index is a plain caller error
    /// rejected before anything is sent, while a protocol violation, worker
    /// death, or a missed deadline invalidates the whole session fail-closed.
    /// Inspecting the current plugin again (a re-inspect) is legal.
    pub fn inspect_plugin(
        &mut self,
        plugin_index: u32,
        request_index: u32,
    ) -> io::Result<InspectOutcome> {
        if let Some(invalidation) = &self.invalidation {
            return Err(invalid(format!(
                "discovery session is invalidated ({}): {}",
                invalidation.reason, invalidation.detail
            )));
        }
        if plugin_index >= self.plugin_count {
            return Err(invalid(
                "inspect plugin index is outside the cluster manifest",
            ));
        }
        if request_index != self.next_request_index {
            return Err(invalid(format!(
                "inspect request index {request_index} does not continue the serial {}",
                self.next_request_index
            )));
        }
        // Between exchanges, a queued process-death event fails the inspect
        // before anything is sent; a queued message with no exchange in
        // flight is a protocol violation.
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
        let message = format!(
            "{{\"v\":1,\"type\":\"inspect_plugin\",\"plugin_index\":{plugin_index},\"request_index\":{request_index}}}"
        );
        if !self.transport.send_message(&message) {
            return Err(self.invalidate(
                "request_pipe_closed",
                "the session request pipe rejected an inspect_plugin message".into(),
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let body = match self.await_response() {
            FrameWait::Message(body) => body,
            FrameWait::Deadline => {
                return Err(self.invalidate(
                    "inspect_deadline",
                    format!(
                        "request {request_index} exceeded the {}ms deadline",
                        self.inspect_deadline.as_millis()
                    ),
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
        let done: InspectDone = match serde_json::from_slice(&body) {
            Ok(done) => done,
            Err(error) => {
                return Err(self.invalidate(
                    "malformed_inspect_done",
                    format!("request {request_index} response did not parse strictly: {error}"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        if done.v != PROTOCOL_VERSION
            || done.kind != "inspect_done"
            || done.plugin_index != plugin_index
            || done.request_index != request_index
        {
            return Err(self.invalidate(
                "inspect_done_mismatch",
                format!(
                    "request {request_index} response carried v={} type={} plugin_index={} request_index={}",
                    done.v, done.kind, done.plugin_index, done.request_index
                ),
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        self.next_request_index += 1;
        match done.status.as_str() {
            "ok" => {
                if done.report.is_none() || done.error_kind.is_some() {
                    return Err(self.invalidate(
                        "malformed_inspect_done",
                        format!("request {request_index} ok response missed its report"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                self.inspects_ok += 1;
                Ok(InspectOutcome::Inspected {
                    report: done.report.expect("checked above"),
                })
            }
            // A parameter-local failure (design §4.2): the session stays
            // usable and the caller decides whether to continue. The worker
            // structures the cause as error_kind; a partial report may ride
            // along.
            "error" => {
                let error_kind = match done.error_kind.as_deref() {
                    // `identity_changed` (the bytes no longer match the
                    // manifest, the #309 state transition), `load_failed`,
                    // and `hash_unavailable` (the bytes could not be read at
                    // all) are in-place additions (issue #751); a sealed
                    // worker never emits them.
                    Some(
                        kind @ ("entrypoint_unresolved"
                        | "selector_error"
                        | "identity_changed"
                        | "load_failed"
                        | "hash_unavailable"),
                    ) => kind.to_owned(),
                    _ => {
                        return Err(self.invalidate(
                            "malformed_inspect_done",
                            format!(
                                "request {request_index} error response missed a known error_kind"
                            ),
                            POST_TERMINATION_COLLECT_TIMEOUT,
                        ));
                    }
                };
                self.inspects_errored += 1;
                Ok(InspectOutcome::InspectError {
                    error_kind,
                    report: done.report,
                })
            }
            other => Err(self.invalidate(
                "unknown_inspect_status",
                format!("request {request_index} reported status {other:?}"),
                POST_TERMINATION_COLLECT_TIMEOUT,
            )),
        }
    }

    /// Ends the session: sends `close`, drops the request pipe, collects the
    /// exit, validates the final report's module audit against the launch
    /// manifest's declared set (design §5), and returns a summary.
    pub fn close(mut self) -> Value {
        if self.invalidation.is_none() && self.process.is_some() {
            // The exit contract (design §7): a normal worker exit happens
            // only AFTER the broker's close handshake.
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
                        // Bounded raw stderr tail: a worker that dies before
                        // its final report (the discovery-session failure the
                        // close otherwise cannot attribute, design §7) still
                        // leaves its stage markers here.
                        "stderr_tail": result.stderr
                            .char_indices()
                            .rev()
                            .nth(4095)
                            .map_or(result.stderr.as_str(), |(index, _)| &result.stderr[index..]),
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
        // The final report's module audit is checked against the launch
        // manifest's declared set (design §5), replacing the one-shot
        // fixed-cap validator the cluster dispatch disabled at launch. Since
        // issue #730 the outcome is recorded on the close report instead of
        // invalidating the session.
        let module_audit_warning = match &collected {
            Some(CollectedExit {
                result: Some(result),
                ..
            }) if result.classification == crate::ExitClassification::Ok => {
                crate::worker_module_audit::observe_in_place_cluster_audit(
                    &result.stdout,
                    result.stdout_truncated,
                )
            }
            _ => None,
        };
        let session_clean = self.invalidation.is_none()
            && matches!(
                &collected,
                Some(CollectedExit { result: Some(result), .. })
                    if result.classification == crate::ExitClassification::Ok
            )
            && final_report
                .as_ref()
                .is_some_and(discovery_final_report_clean);
        json!({
            "stage": "discovery_session_close",
            "plugin_count": self.plugin_count,
            "inspects_ok": self.inspects_ok,
            "inspects_errored": self.inspects_errored,
            "invalidated": self.invalidation.is_some(),
            "invalidated_reason": self.invalidation.as_ref().map(|invalidation| json!({
                "reason": invalidation.reason,
                "detail": invalidation.detail,
            })),
            "module_audit_warning": module_audit_warning,
            "worker": worker,
            "final_report": final_report,
            "session_clean": session_clean,
        })
    }
}

/// A clean discovery session close requires the worker's final report to
/// agree: the session completed (the exit code is already gated separately).
/// The module audit itself is validated separately against the cluster
/// declaration (design §5). Missing keys fail closed.
fn discovery_final_report_clean(report: &Value) -> bool {
    report.get("stage") == Some(&json!("discovery_session"))
        && report.get("status") == Some(&json!("discovery_session_completed"))
}
