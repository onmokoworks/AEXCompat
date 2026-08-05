use crate::ExitClassification;
use crate::sealed_load_tree::SealedLoadTree;
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
}

pub struct SecureLaunchRequest<'a> {
    pub worker_program: &'a Path,
    pub worker_expected_sha256: [u8; 32],
    pub worker_expected_size: u64,
    /// Basename of the plugin image carried as the positional argv slot.
    /// `None` launches without a positional plugin: cluster discovery sessions
    /// (issue #405) select plugins by manifest index at runtime, so no plugin
    /// path rides their argv at all.
    pub plugin_basename: Option<&'a str>,
    pub args_before_plugin: &'a [String],
    pub args_after_plugin: &'a [String],
    /// Repository root used by the Windows launch boundary to create and
    /// authenticate the optional broker-owned minidump handle (issue #18). The
    /// worker never receives this path or a dump-file handle.
    pub repository: &'a Path,
    pub require_module_audit: bool,
}

fn build_launch_args(
    tree: &SealedLoadTree,
    request: &SecureLaunchRequest<'_>,
) -> io::Result<Vec<String>> {
    let plugin_path = request
        .plugin_basename
        .map(|basename| tree.plugin_path(basename))
        .transpose()?;
    let mut args =
        Vec::with_capacity(request.args_before_plugin.len() + 1 + request.args_after_plugin.len());
    args.extend_from_slice(request.args_before_plugin);
    if let Some(plugin_path) = plugin_path {
        args.push(plugin_path.into_os_string().to_string_lossy().into_owned());
    }
    args.extend_from_slice(request.args_after_plugin);
    Ok(args)
}

/// Launches a trusted external worker with an authenticated sealed plugin.
///
/// The tree is owned by this call so its verified file handles and protected
/// root remain alive until the isolated process has exited.
pub fn secure_launch(
    tree: SealedLoadTree,
    request: SecureLaunchRequest<'_>,
    timeout: Option<Duration>,
) -> io::Result<SecureLaunchResult> {
    let args = build_launch_args(&tree, &request)?;
    secure_launch_impl(
        tree,
        request.worker_program,
        request.worker_expected_sha256,
        request.worker_expected_size,
        &args,
        request.require_module_audit,
        request.repository,
        timeout,
        None,
    )
}

pub(crate) fn secure_launch_with_process_memory_limit(
    tree: SealedLoadTree,
    request: SecureLaunchRequest<'_>,
    timeout: Option<Duration>,
    process_memory_limit: usize,
) -> io::Result<SecureLaunchResult> {
    let args = build_launch_args(&tree, &request)?;
    secure_launch_impl(
        tree,
        request.worker_program,
        request.worker_expected_sha256,
        request.worker_expected_size,
        &args,
        request.require_module_audit,
        request.repository,
        timeout,
        Some(process_memory_limit),
    )
}

#[cfg(windows)]
fn secure_launch_impl(
    tree: SealedLoadTree,
    worker_program: &Path,
    worker_expected_sha256: [u8; 32],
    worker_expected_size: u64,
    args: &[String],
    require_module_audit: bool,
    repository: &Path,
    timeout: Option<Duration>,
    process_memory_limit: Option<usize>,
) -> io::Result<SecureLaunchResult> {
    use crate::restricted_worker_acl::{RestrictedWorkerSid, protect_sealed_load_tree};
    use crate::restricted_worker_token::create_restricted_worker_token;
    use crate::trusted_worker_stage::TrustedWorkerStage;

    let worker_sid = RestrictedWorkerSid::generate();
    let token = create_restricted_worker_token(&worker_sid)
        .map_err(|error| stage_error("restricted token creation", error))?;
    if !token
        .contains_restricting_sid(&worker_sid)
        .map_err(|error| stage_error("restricted token verification", error))?
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "restricted token does not contain the generated worker SID",
        ));
    }
    protect_sealed_load_tree(tree.root(), tree.manifest_basenames(), &worker_sid)
        .map_err(|error| stage_error("sealed tree ACL application", error))?;
    let worker_stage = TrustedWorkerStage::create(
        worker_program,
        worker_expected_sha256,
        worker_expected_size,
        &worker_sid,
    )
    .map_err(|error| stage_error("trusted worker staging", error))?;
    // Never expose the repository root as a worker CWD. The native sidecar
    // loader pins relative access to <cwd>/image-transport, which is the same
    // broker-owned <repository>/target/image-transport boundary as before.
    let worker_cwd = repository.join("target");
    std::fs::create_dir_all(&worker_cwd)
        .map_err(|error| stage_error("worker cwd creation", error))?;
    let result = if let Some(limit) = process_memory_limit {
        crate::windows_process::run_isolated_with_restricted_token_and_memory_limit(
            worker_stage.worker_path(),
            args,
            timeout,
            &token,
            &worker_cwd,
            repository,
            limit,
        )
    } else {
        crate::windows_process::run_isolated_with_restricted_token(
            worker_stage.worker_path(),
            args,
            timeout,
            &token,
            &worker_cwd,
            // Repository root for the launch-boundary minidump handle (issue #18).
            repository,
        )
    }
    .map_err(|error| stage_error("restricted process launch", error))?;
    if require_module_audit && result.classification == ExitClassification::Ok {
        crate::worker_module_audit::validate_required_worker_audit(
            &result.stdout,
            result.stdout_truncated,
        )
        .map_err(|error| stage_error("worker module audit validation", error))?;
    }
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
    })
}

/// A sealed worker launched for a resident render session. Every trust
/// artifact whose lifetime the one-shot path scoped to a single dispatch
/// (sealed tree with its verified handles and ACL, staged worker copy,
/// restricted token) is held here for the whole session; dropping without
/// `finish` terminates the worker through the job's kill-on-close limit.
#[cfg(windows)]
pub struct SecureSessionProcess {
    launched: Option<crate::windows_process::LaunchedIsolatedProcess>,
    require_module_audit: bool,
    /// See `SecureLaunchResult::worker_freshness_warning`; recorded by the
    /// dispatch that admitted the worker and carried into `finish`'s result.
    worker_freshness_warning: Option<&'static str>,
    _tree: SealedLoadTree,
    _stage: crate::trusted_worker_stage::TrustedWorkerStage,
    _token: crate::restricted_worker_token::RestrictedWorkerToken,
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
        if self.require_module_audit && result.classification == ExitClassification::Ok {
            crate::worker_module_audit::validate_required_worker_audit(
                &result.stdout,
                result.stdout_truncated,
            )
            .map_err(|error| stage_error("worker module audit validation", error))?;
        }
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
        })
    }
}

/// Session variant of `secure_launch`: identical trust pipeline (restricted
/// token, sealed tree ACL, trusted worker staging), but the worker is left
/// running with the inherited session transport and returned to the caller
/// instead of being awaited.
#[cfg(windows)]
pub fn secure_launch_session(
    tree: SealedLoadTree,
    request: SecureLaunchRequest<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<SecureSessionProcess> {
    secure_launch_session_with_desktop_policy(
        tree,
        request,
        session,
        crate::windows_process::WorkerDesktopPolicy::Dedicated,
    )
}

#[cfg(windows)]
pub(crate) fn secure_launch_session_on_current_desktop(
    tree: SealedLoadTree,
    request: SecureLaunchRequest<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<SecureSessionProcess> {
    secure_launch_session_with_desktop_policy(
        tree,
        request,
        session,
        crate::windows_process::WorkerDesktopPolicy::Current,
    )
}

#[cfg(windows)]
fn secure_launch_session_with_desktop_policy(
    tree: SealedLoadTree,
    request: SecureLaunchRequest<'_>,
    session: &crate::windows_process::SessionChildHandles,
    desktop_policy: crate::windows_process::WorkerDesktopPolicy,
) -> io::Result<SecureSessionProcess> {
    use crate::restricted_worker_acl::{RestrictedWorkerSid, protect_sealed_load_tree};
    use crate::restricted_worker_token::create_restricted_worker_token;
    use crate::trusted_worker_stage::TrustedWorkerStage;

    let args = build_launch_args(&tree, &request)?;
    let worker_sid = RestrictedWorkerSid::generate();
    let token = create_restricted_worker_token(&worker_sid)
        .map_err(|error| stage_error("restricted token creation", error))?;
    if !token
        .contains_restricting_sid(&worker_sid)
        .map_err(|error| stage_error("restricted token verification", error))?
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "restricted token does not contain the generated worker SID",
        ));
    }
    protect_sealed_load_tree(tree.root(), tree.manifest_basenames(), &worker_sid)
        .map_err(|error| stage_error("sealed tree ACL application", error))?;
    let worker_stage = TrustedWorkerStage::create(
        request.worker_program,
        request.worker_expected_sha256,
        request.worker_expected_size,
        &worker_sid,
    )
    .map_err(|error| stage_error("trusted worker staging", error))?;
    // Session workers share the same non-root CWD and transport boundary as
    // one-shot workers.
    let worker_cwd = request.repository.join("target");
    std::fs::create_dir_all(&worker_cwd)
        .map_err(|error| stage_error("session worker cwd creation", error))?;
    let launched = match desktop_policy {
        crate::windows_process::WorkerDesktopPolicy::Dedicated => {
            crate::windows_process::launch_isolated_session_with_restricted_token(
                worker_stage.worker_path(),
                &args,
                &token,
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
                &token,
                &worker_cwd,
                session,
                request.repository,
            )
        }
    }
    .map_err(|error| stage_error("restricted session launch", error))?;
    Ok(SecureSessionProcess {
        launched: Some(launched),
        require_module_audit: request.require_module_audit,
        worker_freshness_warning: None,
        _tree: tree,
        _stage: worker_stage,
        _token: token,
    })
}

#[cfg(windows)]
fn stage_error(stage: &'static str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{stage} failed: {error}"))
}

#[cfg(not(windows))]
fn secure_launch_impl(
    _tree: SealedLoadTree,
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
        "secure sealed-tree launch is only available on Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sealed_load_tree::LoadEntry;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;

    fn tree() -> (SealedLoadTree, PathBuf) {
        let source = std::env::temp_dir().join(format!(
            "aexcompat-secure-launch-source-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&source).unwrap();
        let bytes = b"not an executable";
        let path = source.join("worker.exe");
        fs::write(&path, bytes).unwrap();
        let entry = LoadEntry {
            source: path,
            relative_basename: "worker.exe".into(),
            expected_sha256: Sha256::digest(bytes).into(),
            expected_size: bytes.len() as u64,
        };
        (SealedLoadTree::create(entry, vec![]).unwrap(), source)
    }

    #[test]
    fn rejects_non_manifest_and_non_basename_targets_before_launch() {
        for (target, kind) in [
            ("other.exe", io::ErrorKind::PermissionDenied),
            ("subdir/worker.exe", io::ErrorKind::InvalidInput),
        ] {
            let (tree, source) = tree();
            let request = SecureLaunchRequest {
                worker_program: Path::new("trusted-worker.exe"),
                worker_expected_sha256: [0; 32],
                worker_expected_size: 0,
                plugin_basename: Some(target),
                args_before_plugin: &[],
                args_after_plugin: &[],
                repository: Path::new("."),
                require_module_audit: false,
            };
            let error = secure_launch(tree, request, Some(Duration::from_secs(1))).unwrap_err();
            assert_eq!(error.kind(), kind);
            fs::remove_dir_all(source).unwrap();
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_fails_closed_without_fallback() {
        let (tree, source) = tree();
        let request = SecureLaunchRequest {
            worker_program: Path::new("trusted-worker.exe"),
            worker_expected_sha256: [0; 32],
            worker_expected_size: 0,
            plugin_basename: Some("worker.exe"),
            args_before_plugin: &[],
            args_after_plugin: &[],
            repository: Path::new("."),
            require_module_audit: false,
        };
        let error = secure_launch(tree, request, Some(Duration::from_secs(1))).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        fs::remove_dir_all(source).unwrap();
    }
}
