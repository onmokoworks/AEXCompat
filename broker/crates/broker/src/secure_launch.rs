use crate::ExitClassification;
use std::io;
use std::path::Path;
use std::time::Duration;

#[derive(Debug)]
pub struct SecureLaunchResult {
    pub classification: ExitClassification,
    pub exit_code: u32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// Kill evidence from Job Object accounting (see
    /// `windows_process::ProcessResult`): "timeout" or "memory_limit" when
    /// the cause of a dead worker is knowable, None otherwise.
    pub kill_reason: Option<&'static str>,
    pub worker_peak_commit_bytes: Option<u64>,
    pub peak_process_memory_bytes: Option<u64>,
    pub peak_job_memory_bytes: Option<u64>,
    pub process_memory_limit_bytes: u64,
    pub memory_limit_reached: bool,
    /// Windows the worker put on its private desktop (issue #351). Normally
    /// empty; carried through so the observation reaches the render/inspection
    /// diagnostics rather than stopping at the launch boundary.
    pub dismissed_windows: Vec<crate::worker_dialog::DismissedWindow>,
    /// Freshness observation from local worker admission (issue #729):
    /// `Some(reason)` when the admitted worker could not be confirmed current
    /// against `minihost/src` or its recorded build provenance. Recorded so
    /// the observation reaches diagnostics; it never blocks dispatch.
    /// Receipt-driven flows do not pass through local admission and stay
    /// `None`.
    pub worker_freshness_warning: Option<&'static str>,
    /// Module-audit observation (issue #730): `Some(reason)` when a required
    /// audit could not confirm that only known modules loaded (unknown
    /// modules, a missing or malformed report, truncated stdout). Recorded so
    /// the observation reaches diagnostics; it never fails the dispatch.
    /// `None` when the audit passed, was not required, or the exit was not
    /// clean (a dead worker's stdout proves nothing either way).
    pub module_audit_warning: Option<String>,
}

pub struct SecureLaunchRequest<'a> {
    pub worker_program: &'a Path,
    pub worker_expected_sha256: [u8; 32],
    pub worker_expected_size: u64,
    pub args_before_plugin: &'a [String],
    pub args_after_plugin: &'a [String],
    /// Repository root used by the Windows launch boundary to create and
    /// authenticate the optional broker-owned minidump handle (issue #18). The
    /// worker never receives this path or a dump-file handle.
    pub repository: &'a Path,
    pub require_module_audit: bool,
}

/// In-place variant of `secure_launch` (issue #751): the plug-in loads from
/// the path it actually lives at and its dependency closure resolves through
/// the worker's admitted search directories, so nothing is staged and no
/// sealed tree exists. The positional argv slot carries `plugin_path` verbatim.
pub fn secure_launch_in_place(
    plugin_path: &std::path::Path,
    request: SecureLaunchRequest<'_>,
    timeout: Option<Duration>,
    process_memory_limit: Option<usize>,
) -> io::Result<SecureLaunchResult> {
    let args = build_in_place_launch_args(plugin_path, &request)?;
    secure_launch_impl(
        request.worker_program,
        request.worker_expected_sha256,
        request.worker_expected_size,
        &args,
        request.require_module_audit,
        request.repository,
        timeout,
        process_memory_limit,
    )
}

/// Launches a worker whose protocol has no positional plug-in image (system
/// GPU probes and in-place cluster discovery). Issue #816 replaces the old
/// dummy plug-in guard with this explicit shape.
pub(crate) fn secure_launch_without_plugin(
    request: SecureLaunchRequest<'_>,
    timeout: Option<Duration>,
    process_memory_limit: Option<usize>,
) -> io::Result<SecureLaunchResult> {
    let mut args =
        Vec::with_capacity(request.args_before_plugin.len() + request.args_after_plugin.len());
    args.extend_from_slice(request.args_before_plugin);
    args.extend_from_slice(request.args_after_plugin);
    secure_launch_impl(
        request.worker_program,
        request.worker_expected_sha256,
        request.worker_expected_size,
        &args,
        request.require_module_audit,
        request.repository,
        timeout,
        process_memory_limit,
    )
}

fn build_in_place_launch_args(
    plugin_path: &std::path::Path,
    request: &SecureLaunchRequest<'_>,
) -> io::Result<Vec<String>> {
    if !plugin_path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "in-place plugin path must be absolute",
        ));
    }
    let plugin_argument = plugin_path
        .to_str()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "in-place plugin path must be UTF-8",
            )
        })?
        .to_owned();
    let mut args =
        Vec::with_capacity(request.args_before_plugin.len() + 1 + request.args_after_plugin.len());
    args.extend_from_slice(request.args_before_plugin);
    args.push(plugin_argument);
    args.extend_from_slice(request.args_after_plugin);
    Ok(args)
}

#[cfg(windows)]
fn secure_launch_impl(
    worker_program: &Path,
    worker_expected_sha256: [u8; 32],
    worker_expected_size: u64,
    args: &[String],
    require_module_audit: bool,
    repository: &Path,
    timeout: Option<Duration>,
    process_memory_limit: Option<usize>,
) -> io::Result<SecureLaunchResult> {
    use crate::trusted_worker_stage::TrustedWorkerStage;

    let worker_stage =
        TrustedWorkerStage::create(worker_program, worker_expected_sha256, worker_expected_size)
            .map_err(|error| stage_error("trusted worker staging", error))?;
    // Never expose the repository root as a worker CWD. The native sidecar
    // loader pins relative access to <cwd>/image-transport, which is the same
    // broker-owned <repository>/target/image-transport boundary as before.
    let worker_cwd = repository.join("target");
    std::fs::create_dir_all(&worker_cwd)
        .map_err(|error| stage_error("worker cwd creation", error))?;
    let result = if let Some(limit) = process_memory_limit {
        crate::windows_process::run_isolated_staged_with_memory_limit(
            worker_stage.worker_path(),
            args,
            timeout,
            &worker_cwd,
            repository,
            limit,
        )
    } else {
        crate::windows_process::run_isolated_staged(
            worker_stage.worker_path(),
            args,
            timeout,
            &worker_cwd,
            // Repository root for the launch-boundary minidump handle (issue #18).
            repository,
        )
    }
    .map_err(|error| stage_error("staged process launch", error))?;
    // Recorded, not enforced (issue #730): an audit that cannot confirm the
    // module list rides the result as a warning, and the dispatch stands.
    let module_audit_warning = (require_module_audit
        && result.classification == ExitClassification::Ok)
        .then(|| {
            crate::worker_module_audit::observe_required_worker_audit(
                &result.stdout,
                result.stdout_truncated,
            )
        })
        .flatten();
    Ok(SecureLaunchResult {
        classification: result.classification,
        exit_code: result.exit_code,
        stdout: result.stdout,
        stderr: result.stderr,
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
        kill_reason: result.kill_reason,
        worker_peak_commit_bytes: result.worker_peak_commit_bytes,
        peak_process_memory_bytes: result.peak_process_memory_bytes,
        peak_job_memory_bytes: result.peak_job_memory_bytes,
        process_memory_limit_bytes: result.process_memory_limit_bytes,
        memory_limit_reached: result.memory_limit_reached,
        dismissed_windows: result.dismissed_windows,
        worker_freshness_warning: None,
        module_audit_warning,
    })
}

/// An isolated worker launched for a resident render session. The authenticated
/// worker copy is held for the whole session; dropping without `finish`
/// terminates it through the job's kill-on-close limit.
#[cfg(windows)]
pub struct SecureSessionProcess {
    launched: Option<crate::windows_process::LaunchedIsolatedProcess>,
    require_module_audit: bool,
    /// See `SecureLaunchResult::worker_freshness_warning`; recorded by the
    /// dispatch that admitted the worker and carried into `finish`'s result.
    worker_freshness_warning: Option<&'static str>,
    _stage: crate::trusted_worker_stage::TrustedWorkerStage,
}

#[cfg(windows)]
impl SecureSessionProcess {
    /// Records the freshness observation from local worker admission so it
    /// reaches the `finish` result (issue #729).
    pub(crate) fn record_worker_freshness_warning(&mut self, warning: Option<&'static str>) {
        self.worker_freshness_warning = warning;
    }

    /// Terminates the whole job now (frame-deadline watchdog path). The
    /// collected result still comes from a later `finish` call.
    pub fn terminate_job(&self) -> io::Result<()> {
        self.launched
            .as_ref()
            .expect("session process not collected")
            .terminate_job()
    }

    /// See `LaunchedIsolatedProcess::duplicated_process_handle`.
    pub fn duplicated_process_handle(&self) -> io::Result<usize> {
        self.launched
            .as_ref()
            .expect("session process not collected")
            .duplicated_process_handle()
    }

    /// See `LaunchedIsolatedProcess::has_exited`.
    pub fn has_exited(&self) -> bool {
        self.launched
            .as_ref()
            .expect("session process not collected")
            .has_exited()
    }

    /// Waits up to `timeout` for the worker to exit and collects stdout,
    /// stderr, and job accounting; `None` waits indefinitely (issue #354).
    /// Applies the same module-audit validation as the one-shot
    /// `secure_launch` when the exit classified as ok.
    pub fn finish(mut self, timeout: Option<Duration>) -> io::Result<SecureLaunchResult> {
        let result = self
            .launched
            .take()
            .expect("session process already collected")
            .wait_and_collect(timeout)
            .map_err(|error| stage_error("session worker collection", error))?;
        // Same recording boundary as the one-shot path (issue #730).
        let module_audit_warning = (self.require_module_audit
            && result.classification == ExitClassification::Ok)
            .then(|| {
                crate::worker_module_audit::observe_required_worker_audit(
                    &result.stdout,
                    result.stdout_truncated,
                )
            })
            .flatten();
        Ok(SecureLaunchResult {
            classification: result.classification,
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
            kill_reason: result.kill_reason,
            worker_peak_commit_bytes: result.worker_peak_commit_bytes,
            peak_process_memory_bytes: result.peak_process_memory_bytes,
            peak_job_memory_bytes: result.peak_job_memory_bytes,
            process_memory_limit_bytes: result.process_memory_limit_bytes,
            memory_limit_reached: result.memory_limit_reached,
            dismissed_windows: result.dismissed_windows,
            worker_freshness_warning: self.worker_freshness_warning,
            module_audit_warning,
        })
    }
}

/// In-place session variant (issue #751): mirrors `secure_launch_in_place`
/// for resident sessions. No sealed tree exists; the positional argv slot
/// carries the real plugin path — or nothing at all for cluster discovery
/// sessions, whose plug-ins are selected by manifest index at runtime.
#[cfg(windows)]
pub(crate) fn secure_launch_session_in_place(
    plugin_path: Option<&std::path::Path>,
    request: SecureLaunchRequest<'_>,
    session: &crate::windows_process::SessionChildHandles,
    desktop_policy: crate::windows_process::WorkerDesktopPolicy,
) -> io::Result<SecureSessionProcess> {
    let args = match plugin_path {
        Some(plugin_path) => build_in_place_launch_args(plugin_path, &request)?,
        None => {
            let mut args = Vec::with_capacity(
                request.args_before_plugin.len() + request.args_after_plugin.len(),
            );
            args.extend_from_slice(request.args_before_plugin);
            args.extend_from_slice(request.args_after_plugin);
            args
        }
    };
    secure_launch_session_impl(args, request, session, desktop_policy)
}

#[cfg(windows)]
fn secure_launch_session_impl(
    args: Vec<String>,
    request: SecureLaunchRequest<'_>,
    session: &crate::windows_process::SessionChildHandles,
    desktop_policy: crate::windows_process::WorkerDesktopPolicy,
) -> io::Result<SecureSessionProcess> {
    use crate::trusted_worker_stage::TrustedWorkerStage;

    let worker_stage = TrustedWorkerStage::create(
        request.worker_program,
        request.worker_expected_sha256,
        request.worker_expected_size,
    )
    .map_err(|error| stage_error("trusted worker staging", error))?;
    // Session workers share the same non-root CWD and transport boundary as
    // one-shot workers.
    let worker_cwd = request.repository.join("target");
    std::fs::create_dir_all(&worker_cwd)
        .map_err(|error| stage_error("session worker cwd creation", error))?;
    let launched = match desktop_policy {
        crate::windows_process::WorkerDesktopPolicy::Dedicated => {
            crate::windows_process::launch_isolated_session_staged(
                worker_stage.worker_path(),
                &args,
                &worker_cwd,
                session,
                // Repository root for the launch-boundary minidump handle (issue #18/#224).
                request.repository,
            )
        }
        crate::windows_process::WorkerDesktopPolicy::Current => {
            crate::windows_process::launch_isolated_session_on_current_desktop(
                worker_stage.worker_path(),
                &args,
                &worker_cwd,
                session,
                request.repository,
            )
        }
    }
    .map_err(|error| stage_error("staged session launch", error))?;
    Ok(SecureSessionProcess {
        launched: Some(launched),
        require_module_audit: request.require_module_audit,
        worker_freshness_warning: None,
        _stage: worker_stage,
    })
}

#[cfg(windows)]
fn stage_error(stage: &'static str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{stage} failed: {error}"))
}

#[cfg(not(windows))]
fn secure_launch_impl(
    _worker_program: &Path,
    _worker_expected_sha256: [u8; 32],
    _worker_expected_size: u64,
    _args: &[String],
    _require_module_audit: bool,
    _repository: &Path,
    _timeout: Option<Duration>,
    _process_memory_limit: Option<usize>,
) -> io::Result<SecureLaunchResult> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure worker launch is only available on Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn request<'a>(before: &'a [String], after: &'a [String]) -> SecureLaunchRequest<'a> {
        SecureLaunchRequest {
            worker_program: Path::new("worker.exe"),
            worker_expected_sha256: [1; 32],
            worker_expected_size: 1,
            args_before_plugin: before,
            args_after_plugin: after,
            repository: Path::new("."),
            require_module_audit: false,
        }
    }

    #[test]
    fn in_place_args_require_an_absolute_plugin_path() {
        let before = ["--render".to_owned()];
        let after = ["hash".to_owned()];
        assert!(
            build_in_place_launch_args(Path::new("relative.aex"), &request(&before, &after))
                .is_err()
        );
    }

    #[test]
    fn in_place_args_carry_the_real_plugin_path_positionally() {
        let before = ["--render".to_owned()];
        let after = ["hash".to_owned()];
        let plugin = if cfg!(windows) {
            PathBuf::from(r"C:\effects\a.aex")
        } else {
            PathBuf::from("/effects/a.aex")
        };
        let args = build_in_place_launch_args(&plugin, &request(&before, &after)).unwrap();
        assert_eq!(args, ["--render", plugin.to_str().unwrap(), "hash"]);
    }
}
