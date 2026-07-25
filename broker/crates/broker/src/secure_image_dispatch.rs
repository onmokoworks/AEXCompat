use crate::runtime_module_policy::{AuthenticatedGpuModuleReport, RuntimeBackend};
use crate::sealed_load_tree::{LoadEntry, SealedLoadTree};
use crate::secure_launch::{SecureLaunchRequest, SecureLaunchResult, secure_launch};
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
    pub timeout: Option<Duration>,
}

pub struct GpuRuntimeAuthorization<'a> {
    pub backend: RuntimeBackend,
    pub session_identity: [u8; 32],
    pub module_report: &'a AuthenticatedGpuModuleReport,
}

/// GPU-only session dispatch boundary, mirroring `dispatch_secure_gpu_image`
/// for resident sessions: the authenticated runtime module report authorizes
/// the backend before the session worker launches. CPU session callers
/// continue to use `dispatch_secure_image_session` without a policy.
#[cfg(windows)]
pub fn dispatch_secure_gpu_image_session(
    input: SecureImageDispatch<'_>,
    authorization: GpuRuntimeAuthorization<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<crate::secure_launch::SecureSessionProcess> {
    if authorization.backend == RuntimeBackend::Cpu {
        return Err(invalid("GPU dispatch cannot use the CPU backend"));
    }
    authorization
        .module_report
        .authorize_dispatch(&authorization.session_identity, authorization.backend)?;
    dispatch_secure_image_session(input, session)
}

#[cfg(windows)]
pub(crate) fn dispatch_secure_gpu_image_session_on_current_desktop(
    input: SecureImageDispatch<'_>,
    authorization: GpuRuntimeAuthorization<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<crate::secure_launch::SecureSessionProcess> {
    if authorization.backend == RuntimeBackend::Cpu {
        return Err(invalid("GPU dispatch cannot use the CPU backend"));
    }
    authorization
        .module_report
        .authorize_dispatch(&authorization.session_identity, authorization.backend)?;
    dispatch_secure_image_session_on_current_desktop(input, session)
}

/// Session variant of `dispatch_secure_image`: the same admission pipeline
/// (sealed plugin tree, dependency authentication, local worker admission),
/// but the worker keeps running with the inherited session transport.
/// `input.timeout` is unused here: a session is bounded per frame by the
/// caller's deadline, not per launch.
#[cfg(windows)]
pub fn dispatch_secure_image_session(
    input: SecureImageDispatch<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<crate::secure_launch::SecureSessionProcess> {
    dispatch_secure_image_session_with_policy(
        input,
        session,
        crate::windows_process::WorkerDesktopPolicy::Dedicated,
    )
}

#[cfg(windows)]
pub(crate) fn dispatch_secure_image_session_on_current_desktop(
    input: SecureImageDispatch<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<crate::secure_launch::SecureSessionProcess> {
    dispatch_secure_image_session_with_policy(
        input,
        session,
        crate::windows_process::WorkerDesktopPolicy::Current,
    )
}

#[cfg(windows)]
fn dispatch_secure_image_session_with_policy(
    input: SecureImageDispatch<'_>,
    session: &crate::windows_process::SessionChildHandles,
    desktop_policy: crate::windows_process::WorkerDesktopPolicy,
) -> io::Result<crate::secure_launch::SecureSessionProcess> {
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
    launch_session_with_admission(
        &worker_program,
        tree,
        Some(&plugin_basename),
        input.args_before_plugin,
        input.args_after_plugin,
        input.repository,
        true,
        session,
        desktop_policy,
    )
}

/// The shared session launch boundary for the single-plugin and cluster
/// session dispatchers: admits the local worker binary exactly once per
/// launch (the staged copy must still match the admitted bytes) and hands
/// the sealed tree to the desktop-policy-aware session launch.
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn launch_session_with_admission(
    worker_program: &Path,
    tree: SealedLoadTree,
    plugin_basename: Option<&str>,
    args_before_plugin: &[String],
    args_after_plugin: &[String],
    repository: &Path,
    require_module_audit: bool,
    session: &crate::windows_process::SessionChildHandles,
    desktop_policy: crate::windows_process::WorkerDesktopPolicy,
) -> io::Result<crate::secure_launch::SecureSessionProcess> {
    let (worker_sha256, worker_size) = admit_local_worker(worker_program)?;
    let request = SecureLaunchRequest {
        worker_program,
        worker_expected_sha256: worker_sha256,
        worker_expected_size: worker_size,
        plugin_basename,
        args_before_plugin,
        args_after_plugin,
        repository,
        require_module_audit,
    };
    match desktop_policy {
        crate::windows_process::WorkerDesktopPolicy::Dedicated => {
            crate::secure_launch::secure_launch_session(tree, request, session)
        }
        crate::windows_process::WorkerDesktopPolicy::Current => {
            crate::secure_launch::secure_launch_session_on_current_desktop(tree, request, session)
        }
    }
}

/// Cluster session dispatch input (issue #405): an ordered plugin cluster
/// sharing one dependency closure, staged into a single sealed load tree so
/// staging, hashing, and the ACL happen once per cluster.
pub struct SecureClusterImageDispatch<'a> {
    pub repository: &'a Path,
    pub worker_kind: WorkerKind,
    /// The ordered cluster; `plugins[0]` is the launch plugin of a render
    /// session. Must be non-empty.
    pub plugins: Vec<ApprovedImageArtifact>,
    /// The shared closure, authenticated and staged once with the plugins.
    pub dependencies: Vec<ApprovedImageArtifact>,
    /// Authenticated data resources staged into `<root>/<subdir>/` (issue
    /// #362); not modules, so they ride no manifest entry and no audit
    /// declared set — the sealed tree stages them with the same strength.
    pub sealed_resources: Vec<crate::sealed_load_tree::SealedResourceEntry>,
    /// Whether the positional argv image slot carries plugins[0]. Render
    /// sessions pass `true` (the positional contract names plugins[0]);
    /// discovery sessions pass `false` and no plugin path rides argv at all.
    pub positional_plugin: bool,
    /// Render sessions carry the swap payloads for `plugins[1..]` (parallel
    /// to `plugins`; the entry for `plugins[0]` is ignored because the launch
    /// argv payload wins, design §2.2). Discovery sessions pass `None` so the
    /// manifest carries no `payload` keys at all.
    pub swap_payloads: Option<&'a [Option<String>]>,
    /// The declared module bound the session's module audit is validated
    /// against at close (design §5).
    pub module_bound: u32,
    pub args_before_plugin: &'a [String],
    pub args_after_plugin: &'a [String],
}

/// A launched cluster session plus the launch-authenticated manifest (the
/// swap and audit reference the caller validates the session against). The
/// manifest document itself is staged inside the sealed root; its lifetime
/// is the tree's.
#[cfg(windows)]
pub struct SecureClusterSessionLaunch {
    pub process: crate::secure_launch::SecureSessionProcess,
    pub manifest: crate::cluster_manifest::ValidatedClusterManifest,
}

/// Cluster variant of `dispatch_secure_image_session` (issue #405,
/// docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md): the whole plugin cluster,
/// the shared closure, and the launch-authenticated `cluster-manifest-v1`
/// document are sealed into one tree, and the worker launches with the
/// staged manifest path on argv as `--cluster-manifest-v1 <path>` (design
/// §2.3: the manifest sits directly inside the sealed root, which is what
/// the worker pins against) plus the inherited session transport. The
/// module audit is validated at session close against the manifest's
/// declared set (`worker_module_audit::validate_cluster_worker_audit`), so
/// the one-shot validator stays disabled here.
#[cfg(windows)]
pub fn dispatch_secure_cluster_image_session(
    input: SecureClusterImageDispatch<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<SecureClusterSessionLaunch> {
    dispatch_secure_cluster_image_session_with_policy(
        input,
        session,
        crate::windows_process::WorkerDesktopPolicy::Dedicated,
    )
}

#[cfg(windows)]
pub(crate) fn dispatch_secure_cluster_image_session_on_current_desktop(
    input: SecureClusterImageDispatch<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<SecureClusterSessionLaunch> {
    dispatch_secure_cluster_image_session_with_policy(
        input,
        session,
        crate::windows_process::WorkerDesktopPolicy::Current,
    )
}

#[cfg(windows)]
fn dispatch_secure_cluster_image_session_with_policy(
    input: SecureClusterImageDispatch<'_>,
    session: &crate::windows_process::SessionChildHandles,
    desktop_policy: crate::windows_process::WorkerDesktopPolicy,
) -> io::Result<SecureClusterSessionLaunch> {
    crate::trace_policy::validate_broker_trace_directory(input.repository)?;
    let worker_program = input
        .repository
        .join(input.worker_kind.repository_relative_program());
    // The manifest is built from the same broker-approved artifacts that are
    // staged below, so the declared basenames/hashes and the sealed tree can
    // never diverge; any bound or shape violation fails the launch here
    // (fail-closed, the caller falls back to the per-plugin path).
    let manifest = crate::cluster_manifest::ValidatedClusterManifest::from_approved(
        &input.plugins,
        &input.dependencies,
        input.swap_payloads,
        input.module_bound,
    )?;
    // The manifest document is staged into the sealed root like every other
    // entry (design §2.3): written to a broker-owned staging source, copied
    // and hash-verified by the tree, then the source is removed. The worker
    // receives only the staged path inside the sealed root.
    let staging_source = crate::cluster_manifest::ClusterManifestTransport::write(
        input.repository,
        &manifest,
    )?;
    let manifest_bytes = std::fs::read(staging_source.path())?;
    let manifest_entry = LoadEntry {
        source: staging_source.path().to_path_buf(),
        relative_basename: crate::cluster_manifest::CLUSTER_MANIFEST_SEALED_BASENAME.to_owned(),
        expected_sha256: Sha256::digest(&manifest_bytes).into(),
        expected_size: manifest_bytes.len() as u64,
    };
    let positional_basename = if input.positional_plugin {
        Some(
            input
                .plugins
                .first()
                .ok_or_else(|| invalid("a cluster session requires at least one plugin"))?
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid("plugin path must have a UTF-8 basename"))?,
        )
    } else {
        None
    };
    let plugin_entries = input
        .plugins
        .into_iter()
        .map(load_entry)
        .collect::<io::Result<Vec<_>>>()?;
    let mut dependencies = input
        .dependencies
        .into_iter()
        .map(load_entry)
        .collect::<io::Result<Vec<_>>>()?;
    // The manifest rides the tree as a non-plugin entry: staged, hashed, and
    // ACL'd with the closure, but never resolvable as a plugin path.
    dependencies.push(manifest_entry);
    let tree = SealedLoadTree::create_cluster_with_resources(
        plugin_entries,
        dependencies,
        input.sealed_resources,
    )?
    .0;
    drop(staging_source);
    let staged_manifest = tree
        .root()
        .join(crate::cluster_manifest::CLUSTER_MANIFEST_SEALED_BASENAME);
    // The manifest option pair rides argv's tail, behind every other
    // auxiliary option pair, so the worker's auxiliary-option peeling sees it
    // last; render sessions keep their positional contract untouched.
    let mut args_after_plugin = input.args_after_plugin.to_vec();
    args_after_plugin.extend([
        "--cluster-manifest-v1".to_owned(),
        staged_manifest.to_string_lossy().into_owned(),
    ]);
    let process = launch_session_with_admission(
        &worker_program,
        tree,
        positional_basename.as_deref(),
        input.args_before_plugin,
        &args_after_plugin,
        input.repository,
        // Cluster sessions follow the declared-set audit model (design §5);
        // the broker validates the final report at close with
        // validate_cluster_worker_audit instead of the one-shot validator.
        false,
        session,
        desktop_policy,
    )?;
    Ok(SecureClusterSessionLaunch { process, manifest })
}

pub fn dispatch_secure_image(input: SecureImageDispatch<'_>) -> io::Result<SecureLaunchResult> {
    dispatch_secure_image_with_resources(input, Vec::new())
}

/// Resource-carrying variant of `dispatch_secure_image` (issue #362): the
/// sealed tree also stages authenticated data resources into
/// `<root>/<subdir>/` (docs/SEALED_DATA_RESOURCE_POLICY_2026-07-25.md).
pub fn dispatch_secure_image_with_resources(
    input: SecureImageDispatch<'_>,
    resources: Vec<crate::sealed_load_tree::SealedResourceEntry>,
) -> io::Result<SecureLaunchResult> {
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
    let tree = SealedLoadTree::create_with_resources(main, dependencies, resources)?;
    let (worker_sha256, worker_size) = admit_local_worker(&worker_program)?;
    let request = SecureLaunchRequest {
        worker_program: &worker_program,
        worker_expected_sha256: worker_sha256,
        worker_expected_size: worker_size,
        plugin_basename: Some(&plugin_basename),
        args_before_plugin: input.args_before_plugin,
        args_after_plugin: input.args_after_plugin,
        // The repository is carried to the Windows launch boundary so the
        // optional minidump file handle is created there for every dispatch.
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
            timeout: Some(Duration::from_secs(1)),
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
            timeout: Some(Duration::from_secs(1)),
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
            timeout: Some(Duration::from_secs(1)),
        })
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        fs::remove_dir_all(root).unwrap();
    }

    /// Both guards run before the session transport is touched, so a
    /// handle-free `SessionChildHandles` reaches neither. #365 deleted the
    /// one-shot `dispatch_secure_gpu_image` this used to drive; the session
    /// boundary carries the identical checks, so the coverage moved rather
    /// than going away with the transport.
    #[cfg(windows)]
    #[test]
    fn gpu_session_dispatch_rejects_cpu_backend_and_session_mismatch_before_launch() {
        let handles = crate::windows_process::SessionChildHandles {
            request_read: std::ptr::null_mut(),
            response_write: std::ptr::null_mut(),
            section: std::ptr::null_mut(),
            layers: Vec::new(),
        };
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
            timeout: Some(Duration::from_secs(1)),
        };

        let Err(cpu) = dispatch_secure_gpu_image_session(
            make_input(),
            GpuRuntimeAuthorization {
                backend: RuntimeBackend::Cpu,
                session_identity: [0x11; 32],
                module_report: &report,
            },
            &handles,
        ) else {
            panic!("a CPU backend must not reach the GPU session dispatch");
        };
        assert_eq!(cpu.to_string(), "GPU dispatch cannot use the CPU backend");

        let Err(wrong_session) = dispatch_secure_gpu_image_session(
            make_input(),
            GpuRuntimeAuthorization {
                backend: RuntimeBackend::Cuda,
                session_identity: [0x22; 32],
                module_report: &report,
            },
            &handles,
        ) else {
            panic!("a foreign session identity must not reach the GPU session dispatch");
        };
        assert_eq!(
            wrong_session.to_string(),
            "runtime module report session identity mismatch"
        );
    }
}
