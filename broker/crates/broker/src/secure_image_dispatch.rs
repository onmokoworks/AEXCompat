use crate::runtime_module_policy::{AuthenticatedGpuModuleReport, RuntimeBackend};
use crate::sealed_load_tree::{LoadEntry, SealedLoadTree};
use crate::secure_launch::{secure_launch, SecureLaunchRequest, SecureLaunchResult};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerKind {
    L2,
    Render,
    Smart,
}

impl WorkerKind {
    fn repository_relative_program(self) -> &'static str {
        match self {
            Self::L2 => "target/minihost-build/aex_l2_worker.exe",
            Self::Render => "target/minihost-build/aex_render_worker.exe",
            Self::Smart => "target/minihost-build/aex_smart_worker.exe",
        }
    }
}

/// A trust decision made before dispatch. This type never derives trust from
/// the current contents of `path`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedImageArtifact {
    pub path: PathBuf,
    pub expected_sha256: [u8; 32],
    pub expected_size: u64,
}

pub struct SecureImageDispatch<'a> {
    pub repository: &'a Path,
    pub worker_kind: WorkerKind,
    pub plugin: ApprovedImageArtifact,
    pub dependencies: Vec<ApprovedImageArtifact>,
    pub args_before_plugin: &'a [String],
    pub args_after_plugin: &'a [String],
    pub timeout: Duration,
}

pub struct GpuRuntimeAuthorization<'a> {
    pub backend: RuntimeBackend,
    pub session_identity: [u8; 32],
    pub module_report: &'a AuthenticatedGpuModuleReport,
}

/// GPU-only dispatch boundary. CPU callers continue to use
/// `dispatch_secure_image` and do not require a runtime module policy.
pub fn dispatch_secure_gpu_image(
    input: SecureImageDispatch<'_>,
    authorization: GpuRuntimeAuthorization<'_>,
) -> io::Result<SecureLaunchResult> {
    if authorization.backend == RuntimeBackend::Cpu {
        return Err(invalid("GPU dispatch cannot use the CPU backend"));
    }
    authorization
        .module_report
        .authorize_dispatch(&authorization.session_identity, authorization.backend)?;
    dispatch_secure_image(input)
}

pub fn dispatch_secure_image(input: SecureImageDispatch<'_>) -> io::Result<SecureLaunchResult> {
    crate::trace_policy::validate_broker_trace_directory(input.repository)?;
    let worker_program = input
        .repository
        .join(input.worker_kind.repository_relative_program());
    let main = load_entry(input.plugin)?;
    let plugin_basename = main.relative_basename.clone();
    let dependencies = input
        .dependencies
        .into_iter()
        .map(load_entry)
        .collect::<io::Result<Vec<_>>>()?;
    let tree = SealedLoadTree::create(main, dependencies)?;
    let (worker_sha256, worker_size) = admit_local_worker(&worker_program)?;
    let request = SecureLaunchRequest {
        worker_program: &worker_program,
        worker_expected_sha256: worker_sha256,
        worker_expected_size: worker_size,
        plugin_basename: &plugin_basename,
        args_before_plugin: input.args_before_plugin,
        args_after_plugin: input.args_after_plugin,
        // Opt-in crash minidumps (issue #18) are injected uniformly in
        // secure_launch for every sealed dispatch.
        repository: input.repository,
        require_module_audit: true,
    };
    secure_launch(tree, request, input.timeout)
}

/// Admits the locally built worker by reading it exactly once. The returned
/// identity binds the staged copy that actually executes to the bytes observed
/// here; the build tree itself is the trust root, because anyone who can
/// replace the worker binary can equally rebuild the broker that dispatches
/// it. Receipt-driven flows keep supplying an externally pinned identity
/// through `secure_launch` and do not pass through this admission.
fn admit_local_worker(path: &Path) -> io::Result<([u8; 32], u64)> {
    let mut file = File::open(path).map_err(|error| {
        io::Error::new(error.kind(), "local worker binary is missing or unreadable")
    })?;
    let mut hasher = Sha256::new();
    let size = io::copy(&mut file, &mut hasher)?;
    if size == 0 {
        return Err(invalid("local worker binary is empty"));
    }
    Ok((hasher.finalize().into(), size))
}

fn load_entry(artifact: ApprovedImageArtifact) -> io::Result<LoadEntry> {
    if artifact.expected_size == 0 {
        return Err(invalid("approved artifact size must be nonzero"));
    }
    let basename = artifact
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| invalid("approved artifact path must have a UTF-8 filename"))?;
    Ok(LoadEntry {
        source: artifact.path.clone(),
        relative_basename: basename.to_owned(),
        expected_sha256: artifact.expected_sha256,
        expected_size: artifact.expected_size,
    })
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    fn artifact(root: &Path, name: &str, bytes: &[u8]) -> ApprovedImageArtifact {
        let path = root.join(name);
        fs::write(&path, bytes).unwrap();
        ApprovedImageArtifact {
            path,
            expected_sha256: Sha256::digest(bytes).into(),
            expected_size: bytes.len() as u64,
        }
    }

    #[test]
    fn worker_kind_uses_only_fixed_repository_paths() {
        assert_eq!(
            WorkerKind::L2.repository_relative_program(),
            "target/minihost-build/aex_l2_worker.exe"
        );
        assert_eq!(
            WorkerKind::Render.repository_relative_program(),
            "target/minihost-build/aex_render_worker.exe"
        );
        assert_eq!(
            WorkerKind::Smart.repository_relative_program(),
            "target/minihost-build/aex_smart_worker.exe"
        );
    }

    #[test]
    fn rejects_missing_local_worker_after_sealing_the_plugin() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-secure-image-dispatch-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let plugin = artifact(&root, "plugin.plugin", b"plugin");

        let error = dispatch_secure_image(SecureImageDispatch {
            repository: &root,
            worker_kind: WorkerKind::Render,
            plugin,
            dependencies: vec![],
            args_before_plugin: &[],
            args_after_plugin: &[],
            timeout: Duration::from_secs(1),
        })
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "local worker binary is missing or unreadable"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_empty_local_worker_before_launch() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-secure-image-dispatch-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let plugin = artifact(&root, "plugin.plugin", b"plugin");
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        fs::create_dir_all(worker.parent().unwrap()).unwrap();
        fs::write(&worker, b"").unwrap();

        let error = dispatch_secure_image(SecureImageDispatch {
            repository: &root,
            worker_kind: WorkerKind::Render,
            plugin,
            dependencies: vec![],
            args_before_plugin: &[],
            args_after_plugin: &[],
            timeout: Duration::from_secs(1),
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "local worker binary is empty");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dependencies_are_authenticated_as_part_of_the_sealed_tree() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-secure-image-dispatch-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let plugin = artifact(&root, "plugin.plugin", b"plugin");
        let mut dependency = artifact(&root, "helper.dll", b"approved dependency");
        dependency.expected_sha256 = [0; 32];

        let error = dispatch_secure_image(SecureImageDispatch {
            repository: &root,
            worker_kind: WorkerKind::Smart,
            plugin,
            dependencies: vec![dependency],
            args_before_plugin: &["--before".into()],
            args_after_plugin: &["--after".into()],
            timeout: Duration::from_secs(1),
        })
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gpu_dispatch_rejects_cpu_backend_and_session_mismatch_before_launch() {
        let report = AuthenticatedGpuModuleReport::test_only(
            [0x11; 32],
            RuntimeBackend::Cuda,
            SystemTime::now() + Duration::from_secs(60),
        );
        let make_input = || SecureImageDispatch {
            repository: Path::new("repository-that-does-not-exist"),
            worker_kind: WorkerKind::Smart,
            plugin: ApprovedImageArtifact {
                path: PathBuf::from("plugin.plugin"),
                expected_sha256: [0; 32],
                expected_size: 1,
            },
            dependencies: vec![],
            args_before_plugin: &[],
            args_after_plugin: &[],
            timeout: Duration::from_secs(1),
        };

        let cpu = dispatch_secure_gpu_image(
            make_input(),
            GpuRuntimeAuthorization {
                backend: RuntimeBackend::Cpu,
                session_identity: [0x11; 32],
                module_report: &report,
            },
        )
        .unwrap_err();
        assert_eq!(cpu.to_string(), "GPU dispatch cannot use the CPU backend");

        let wrong_session = dispatch_secure_gpu_image(
            make_input(),
            GpuRuntimeAuthorization {
                backend: RuntimeBackend::Cuda,
                session_identity: [0x22; 32],
                module_report: &report,
            },
        )
        .unwrap_err();
        assert_eq!(
            wrong_session.to_string(),
            "runtime module report session identity mismatch"
        );
    }
}
