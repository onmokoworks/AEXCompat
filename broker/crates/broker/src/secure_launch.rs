use crate::sealed_load_tree::SealedLoadTree;
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
    pub peak_process_memory_bytes: Option<u64>,
    pub peak_job_memory_bytes: Option<u64>,
    pub process_memory_limit_bytes: u64,
    pub memory_limit_reached: bool,
}

pub struct SecureLaunchRequest<'a> {
    pub worker_program: &'a Path,
    pub worker_expected_sha256: [u8; 32],
    pub worker_expected_size: u64,
    pub plugin_basename: &'a str,
    pub args_before_plugin: &'a [String],
    pub args_after_plugin: &'a [String],
    pub require_module_audit: bool,
}

/// Launches a trusted external worker with an authenticated sealed plugin.
///
/// The tree is owned by this call so its verified file handles and protected
/// root remain alive until the isolated process has exited.
pub fn secure_launch(
    tree: SealedLoadTree,
    request: SecureLaunchRequest<'_>,
    timeout: Duration,
) -> io::Result<SecureLaunchResult> {
    let plugin_path = tree.plugin_path(request.plugin_basename)?;
    let mut args =
        Vec::with_capacity(request.args_before_plugin.len() + 1 + request.args_after_plugin.len());
    args.extend_from_slice(request.args_before_plugin);
    args.push(plugin_path.into_os_string().to_string_lossy().into_owned());
    args.extend_from_slice(request.args_after_plugin);
    secure_launch_impl(
        tree,
        request.worker_program,
        request.worker_expected_sha256,
        request.worker_expected_size,
        &args,
        request.require_module_audit,
        timeout,
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
    timeout: Duration,
) -> io::Result<SecureLaunchResult> {
    use crate::restricted_worker_acl::{protect_sealed_load_tree, RestrictedWorkerSid};
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
    let result = crate::windows_process::run_isolated_with_restricted_token(
        worker_stage.worker_path(),
        args,
        timeout,
        &token,
        worker_stage.root(),
    )
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
        peak_process_memory_bytes: result.peak_process_memory_bytes,
        peak_job_memory_bytes: result.peak_job_memory_bytes,
        process_memory_limit_bytes: result.process_memory_limit_bytes,
        memory_limit_reached: result.memory_limit_reached,
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
    _timeout: Duration,
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
                plugin_basename: target,
                args_before_plugin: &[],
                args_after_plugin: &[],
                require_module_audit: false,
            };
            let error = secure_launch(tree, request, Duration::from_secs(1)).unwrap_err();
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
            plugin_basename: "worker.exe",
            args_before_plugin: &[],
            args_after_plugin: &[],
            require_module_audit: false,
        };
        let error = secure_launch(tree, request, Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        fs::remove_dir_all(source).unwrap();
    }
}
