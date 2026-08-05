use crate::runtime_module_policy::{AuthenticatedGpuModuleReport, RuntimeBackend};
use crate::sealed_load_tree::{LoadEntry, SealedLoadTree};
use crate::secure_launch::{SecureLaunchRequest, SecureLaunchResult, secure_launch};
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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
    /// In-place load mode (issue #751): non-empty switches the dispatch from
    /// sealed staging to loading the plug-in at `plugin.path` itself, with
    /// these directories admitted into the worker's DLL search set so the
    /// dependency closure resolves from where it actually lives. Mutually
    /// exclusive with `dependencies` (the closure is the loader's job here)
    /// and with sealed resources (the plug-in's own directory already holds
    /// its data files). On this route the broker does not re-read
    /// `plugin.expected_sha256`/`expected_size`; the identity record is the
    /// argv sha256 the worker verifies against the bytes it actually loads.
    pub dependency_search_dirs: Vec<PathBuf>,
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
    // The AEXRMA1 authorization manifest is staged beside the plug-in, which
    // an in-place launch (issue #751) has no staged directory for.
    if !input.dependency_search_dirs.is_empty() {
        return Err(invalid(
            "an in-place dispatch does not support GPU runtime authorization yet",
        ));
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
    // The AEXRMA1 authorization manifest is staged beside the plug-in, which
    // an in-place launch (issue #751) has no staged directory for.
    if !input.dependency_search_dirs.is_empty() {
        return Err(invalid(
            "an in-place dispatch does not support GPU runtime authorization yet",
        ));
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
    if !input.dependency_search_dirs.is_empty() {
        validate_in_place_input(&input.dependencies, true, &input.plugin)?;
        let joined = joined_dependency_search_dirs(&input.dependency_search_dirs)?;
        let admitted = admit_local_worker(input.repository, &worker_program)?;
        let mut args_after_plugin = input.args_after_plugin.to_vec();
        args_after_plugin.extend(["--dependency-dirs-v1".to_owned(), joined]);
        let request = SecureLaunchRequest {
            worker_program: &worker_program,
            worker_expected_sha256: admitted.sha256,
            worker_expected_size: admitted.size,
            plugin_basename: None,
            args_before_plugin: input.args_before_plugin,
            args_after_plugin: &args_after_plugin,
            repository: input.repository,
            require_module_audit: true,
        };
        let mut process = crate::secure_launch::secure_launch_session_in_place(
            Some(&input.plugin.path),
            request,
            session,
            desktop_policy,
        )?;
        process.record_worker_freshness_warning(admitted.freshness_warning);
        return Ok(process);
    }
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
    let admitted = admit_local_worker(repository, worker_program)?;
    let request = SecureLaunchRequest {
        worker_program,
        worker_expected_sha256: admitted.sha256,
        worker_expected_size: admitted.size,
        plugin_basename,
        args_before_plugin,
        args_after_plugin,
        repository,
        require_module_audit,
    };
    let mut process = match desktop_policy {
        crate::windows_process::WorkerDesktopPolicy::Dedicated => {
            crate::secure_launch::secure_launch_session(tree, request, session)
        }
        crate::windows_process::WorkerDesktopPolicy::Current => {
            crate::secure_launch::secure_launch_session_on_current_desktop(tree, request, session)
        }
    }?;
    process.record_worker_freshness_warning(admitted.freshness_warning);
    Ok(process)
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
    let staging_source =
        crate::cluster_manifest::ClusterManifestTransport::write(input.repository, &manifest)?;
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

/// In-place cluster session dispatch (issue #751, `cluster-manifest-v2`):
/// the manifest names each plug-in by its real absolute path plus SHA-256 and
/// carries the validated dependency search directories; nothing is staged.
/// The transport document is what the worker reads at launch (before any
/// plug-in code runs), so it rides the launch struct and must outlive the
/// session.
#[cfg(windows)]
pub struct SecureInPlaceClusterDispatch<'a> {
    pub repository: &'a Path,
    pub worker_kind: WorkerKind,
    /// The ordered cluster; `plugins[0]` is the launch plugin of a render
    /// session. Must be non-empty. Paths are the real, absolute plug-in
    /// locations.
    pub plugins: Vec<ApprovedImageArtifact>,
    /// Directories the worker admits into its DLL search set; validated and
    /// joined exactly like `--dependency-dirs-v1`.
    pub dependency_search_dirs: Vec<PathBuf>,
    /// Whether the positional argv image slot carries plugins[0]. Discovery
    /// sessions pass `false` and no plugin path rides argv at all.
    pub positional_plugin: bool,
    /// Render sessions carry the swap payloads for `plugins[1..]`; discovery
    /// sessions pass `None` so the manifest carries no `payload` keys.
    pub swap_payloads: Option<&'a [Option<String>]>,
    /// The bounded module-enumeration capacity the recorded audit uses.
    pub module_bound: u32,
    pub args_before_plugin: &'a [String],
    pub args_after_plugin: &'a [String],
}

/// A launched in-place cluster session: the process, the validated manifest
/// the caller cross-checks exchanges against, and the live transport document
/// the worker read at launch (the caller keeps it alive for the session).
#[cfg(windows)]
pub struct SecureInPlaceClusterSessionLaunch {
    pub process: crate::secure_launch::SecureSessionProcess,
    pub manifest: crate::cluster_manifest::ValidatedInPlaceClusterManifest,
    pub transport: crate::cluster_manifest::ClusterManifestTransport,
}

#[cfg(windows)]
pub fn dispatch_secure_in_place_cluster_session(
    input: SecureInPlaceClusterDispatch<'_>,
    session: &crate::windows_process::SessionChildHandles,
) -> io::Result<SecureInPlaceClusterSessionLaunch> {
    dispatch_secure_in_place_cluster_session_with_policy(
        input,
        session,
        crate::windows_process::WorkerDesktopPolicy::Dedicated,
    )
}

#[cfg(windows)]
pub(crate) fn dispatch_secure_in_place_cluster_session_with_policy(
    input: SecureInPlaceClusterDispatch<'_>,
    session: &crate::windows_process::SessionChildHandles,
    desktop_policy: crate::windows_process::WorkerDesktopPolicy,
) -> io::Result<SecureInPlaceClusterSessionLaunch> {
    crate::trace_policy::validate_broker_trace_directory(input.repository)?;
    let worker_program = input
        .repository
        .join(input.worker_kind.repository_relative_program());
    for plugin in &input.plugins {
        if !plugin.path.is_absolute() {
            return Err(invalid("in-place plugin path must be absolute"));
        }
    }
    // The same canonicalize + de-verbatim + bounds pipeline as the one-shot
    // in-place dispatch; the validated strings ride the manifest instead of
    // an argv value, so the ';' join constraint never applies here.
    let joined = joined_dependency_search_dirs(&input.dependency_search_dirs)?;
    let search_dirs: Vec<String> = joined.split(';').map(str::to_owned).collect();
    let manifest = crate::cluster_manifest::ValidatedInPlaceClusterManifest::from_approved(
        &input.plugins,
        &search_dirs,
        input.swap_payloads,
        input.module_bound,
    )?;
    let transport = manifest.write_transport(input.repository)?;
    let positional = if input.positional_plugin {
        Some(
            input
                .plugins
                .first()
                .ok_or_else(|| invalid("a cluster session requires at least one plugin"))?
                .path
                .as_path(),
        )
    } else {
        None
    };
    let mut args_after_plugin = input.args_after_plugin.to_vec();
    args_after_plugin.extend([
        "--cluster-manifest-v1".to_owned(),
        transport.path().to_string_lossy().into_owned(),
    ]);
    let admitted = admit_local_worker(input.repository, &worker_program)?;
    let request = SecureLaunchRequest {
        worker_program: &worker_program,
        worker_expected_sha256: admitted.sha256,
        worker_expected_size: admitted.size,
        plugin_basename: None,
        args_before_plugin: input.args_before_plugin,
        args_after_plugin: &args_after_plugin,
        repository: input.repository,
        // The in-place cluster audit is recorded at close
        // (`observe_in_place_cluster_audit`), not validated by the one-shot
        // validator at collection.
        require_module_audit: false,
    };
    let mut process = crate::secure_launch::secure_launch_session_in_place(
        positional,
        request,
        session,
        desktop_policy,
    )?;
    process.record_worker_freshness_warning(admitted.freshness_warning);
    Ok(SecureInPlaceClusterSessionLaunch {
        process,
        manifest,
        transport,
    })
}

pub fn dispatch_secure_image(input: SecureImageDispatch<'_>) -> io::Result<SecureLaunchResult> {
    dispatch_secure_image_impl(input, Vec::new(), None)
}

pub(crate) fn dispatch_secure_image_with_process_memory_limit(
    input: SecureImageDispatch<'_>,
    process_memory_limit: usize,
) -> io::Result<SecureLaunchResult> {
    dispatch_secure_image_impl(input, Vec::new(), Some(process_memory_limit))
}

/// Resource-carrying variant of `dispatch_secure_image` (issue #362): the
/// sealed tree also stages authenticated data resources into
/// `<root>/<subdir>/` (docs/SEALED_DATA_RESOURCE_POLICY_2026-07-25.md).
pub fn dispatch_secure_image_with_resources(
    input: SecureImageDispatch<'_>,
    resources: Vec<crate::sealed_load_tree::SealedResourceEntry>,
) -> io::Result<SecureLaunchResult> {
    dispatch_secure_image_impl(input, resources, None)
}

/// Validates and joins the in-place dependency search directories (issue
/// #751) into the `--dependency-dirs-v1` argv value: each directory must be
/// an absolute, canonicalizable directory whose canonical form does not
/// contain the `;` separator, and the bounded count matches the worker's
/// re-validation.
fn joined_dependency_search_dirs(dirs: &[PathBuf]) -> io::Result<String> {
    const MAX_SEARCH_DIRS: usize = 16;
    let mut joined: Vec<String> = Vec::with_capacity(dirs.len());
    for dir in dirs {
        if !dir.is_absolute() {
            return Err(invalid("dependency search directory must be absolute"));
        }
        let canonical = std::fs::canonicalize(dir).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("dependency search directory is unavailable: {error}"),
            )
        })?;
        if !canonical.is_dir() {
            return Err(invalid("dependency search directory is not a directory"));
        }
        // `canonicalize` returns verbatim `\\?\C:\...` paths on Windows; hand
        // the worker the plain form like every other path that crosses the
        // argv boundary (issue #231's aux-manifest lesson).
        let plain = strip_extended_prefix(&canonical);
        let text = plain
            .to_str()
            .ok_or_else(|| invalid("dependency search directory must be UTF-8"))?
            .to_owned();
        if text.contains(';') {
            return Err(invalid(
                "dependency search directory must not contain the ';' separator",
            ));
        }
        if !joined.iter().any(|seen| seen.eq_ignore_ascii_case(&text)) {
            joined.push(text);
        }
    }
    if joined.is_empty() {
        return Err(invalid("dependency search directories are empty"));
    }
    // Bounded after dedupe so a caller repeating one directory is not
    // rejected for a set the worker would accept.
    if joined.len() > MAX_SEARCH_DIRS {
        return Err(invalid("too many dependency search directories"));
    }
    let joined = joined.join(";");
    // The worker re-validates the joined value against the same bound; fail
    // here so an oversized launch never reaches it as an opaque worker exit.
    const MAX_JOINED_LENGTH: usize = 32768;
    if joined.len() > MAX_JOINED_LENGTH {
        return Err(invalid("dependency search directories are too long"));
    }
    Ok(joined)
}

/// Strip the Windows `\\?\` (or `\\?\UNC\`) extended-length prefix; the same
/// de-verbatim rule as `types_and_transport::strip_extended_prefix` and the
/// minidump/trace policies (issue #231). Identity on plain paths.
fn strip_extended_prefix(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// The shared precondition of both in-place dispatch shapes (issue #751):
/// the closure is the loader's job, so approved dependency artifacts and
/// sealed resources cannot ride an in-place launch.
fn validate_in_place_input(
    dependencies: &[ApprovedImageArtifact],
    resources_empty: bool,
    plugin: &ApprovedImageArtifact,
) -> io::Result<()> {
    if !dependencies.is_empty() {
        return Err(invalid(
            "an in-place dispatch resolves dependencies by search directory, not by staged artifact",
        ));
    }
    if !resources_empty {
        return Err(invalid(
            "an in-place dispatch reads data resources from the plugin's own directory",
        ));
    }
    if !plugin.path.is_absolute() {
        return Err(invalid("in-place plugin path must be absolute"));
    }
    Ok(())
}

fn dispatch_secure_image_impl(
    input: SecureImageDispatch<'_>,
    resources: Vec<crate::sealed_load_tree::SealedResourceEntry>,
    process_memory_limit: Option<usize>,
) -> io::Result<SecureLaunchResult> {
    crate::trace_policy::validate_broker_trace_directory(input.repository)?;
    let worker_program = input
        .repository
        .join(input.worker_kind.repository_relative_program());
    if !input.dependency_search_dirs.is_empty() {
        validate_in_place_input(&input.dependencies, resources.is_empty(), &input.plugin)?;
        let joined = joined_dependency_search_dirs(&input.dependency_search_dirs)?;
        let admitted = admit_local_worker(input.repository, &worker_program)?;
        let mut args_after_plugin = input.args_after_plugin.to_vec();
        args_after_plugin.extend(["--dependency-dirs-v1".to_owned(), joined]);
        let request = SecureLaunchRequest {
            worker_program: &worker_program,
            worker_expected_sha256: admitted.sha256,
            worker_expected_size: admitted.size,
            plugin_basename: None,
            args_before_plugin: input.args_before_plugin,
            args_after_plugin: &args_after_plugin,
            repository: input.repository,
            // Recorded, never enforced (issue #730/#751): the worker emits
            // the loaded-module record and an unconfirmable list rides the
            // result as a warning.
            require_module_audit: true,
        };
        let mut result = crate::secure_launch::secure_launch_in_place(
            &input.plugin.path,
            request,
            input.timeout,
            process_memory_limit,
        )?;
        result.worker_freshness_warning = admitted.freshness_warning;
        return Ok(result);
    }
    let main = load_entry(input.plugin)?;
    let plugin_basename = main.relative_basename.clone();
    let dependencies = input
        .dependencies
        .into_iter()
        .map(load_entry)
        .collect::<io::Result<Vec<_>>>()?;
    let tree = SealedLoadTree::create_with_resources(main, dependencies, resources)?;
    let admitted = admit_local_worker(input.repository, &worker_program)?;
    let request = SecureLaunchRequest {
        worker_program: &worker_program,
        worker_expected_sha256: admitted.sha256,
        worker_expected_size: admitted.size,
        plugin_basename: Some(&plugin_basename),
        args_before_plugin: input.args_before_plugin,
        args_after_plugin: input.args_after_plugin,
        // The repository is carried to the Windows launch boundary so the
        // optional minidump file handle is created there for every dispatch.
        repository: input.repository,
        require_module_audit: true,
    };
    let mut result = if let Some(limit) = process_memory_limit {
        crate::secure_launch::secure_launch_with_process_memory_limit(
            tree,
            request,
            input.timeout,
            limit,
        )
    } else {
        secure_launch(tree, request, input.timeout)
    }?;
    result.worker_freshness_warning = admitted.freshness_warning;
    Ok(result)
}

/// The locally built worker as admitted for one launch: its identity, and the
/// freshness observation that rides the dispatch result as a warning.
struct AdmittedLocalWorker {
    sha256: [u8; 32],
    size: u64,
    /// `Some(reason)` when the worker could not be confirmed current
    /// (issue #729). Recorded, never enforced.
    freshness_warning: Option<&'static str>,
}

/// Admits the locally built worker by reading it exactly once. The returned
/// identity binds the staged copy that actually executes to the bytes observed
/// here; the build tree itself is the trust root, because anyone who can
/// replace the worker binary can equally rebuild the broker that dispatches
/// it. Receipt-driven flows keep supplying an externally pinned identity
/// through `secure_launch` and do not pass through this admission.
fn admit_local_worker(repository: &Path, path: &Path) -> io::Result<AdmittedLocalWorker> {
    let bytes = std::fs::read(path).map_err(|error| {
        io::Error::new(error.kind(), "local worker binary is missing or unreadable")
    })?;
    if bytes.is_empty() {
        return Err(invalid("local worker binary is empty"));
    }
    // Read once: the provenance the freshness decision needs is in the same
    // bytes the identity is taken from, so recognizing it costs no extra I/O and
    // cannot describe a different file than the one admitted.
    let freshness_warning =
        local_worker_freshness_warning(repository, path, WorkerProvenance::find(&bytes));
    Ok(AdmittedLocalWorker {
        sha256: Sha256::digest(&bytes).into(),
        size: bytes.len() as u64,
        freshness_warning,
    })
}

/// What a worker records about the tree it was built from (issue #649).
/// `minihost/src/worker_build_provenance.cpp` writes the marker this parses.
#[derive(Clone, PartialEq, Eq, Debug)]
struct WorkerProvenance {
    revision: String,
    /// Tracked files differed from that commit when the worker was configured.
    dirty: bool,
}

impl WorkerProvenance {
    const MARKER: &'static str = "AEXCOMPAT-WORKER-PROVENANCE-V1";

    /// How far past the marker the record may run. A 12-character revision and a
    /// one-character flag need well under this; the bound is what keeps a stray
    /// byte pattern from pulling the binary into a scan.
    const MAX_RECORD_BYTES: usize = 256;

    /// Finds the marker in a worker's bytes, or `None` for a worker built before
    /// this existed. Scans the raw bytes rather than parsing the executable: the
    /// marker is a plain literal, and a parser would be a second thing to keep in
    /// step with the linker.
    fn find(bytes: &[u8]) -> Option<Self> {
        let marker = Self::MARKER.as_bytes();
        let start = bytes
            .windows(marker.len())
            .position(|window| window == marker)?;
        // Bounded so a byte pattern that merely happens to start like the marker
        // scans a fixed amount rather than the rest of the executable.
        let window = &bytes[start..(start + Self::MAX_RECORD_BYTES).min(bytes.len())];
        // Cut on the bytes, then decode only the record. Decoding the whole
        // window first would throw the record away whenever the linker happened
        // to place non-UTF-8 data after it — which is decided by layout, so the
        // worker would identify itself or not depending on unrelated code.
        let terminator = b" END";
        let end = window
            .windows(terminator.len())
            .position(|candidate| candidate == terminator)?;
        let record = std::str::from_utf8(&window[..end]).ok()?;
        Some(Self {
            revision: field(record, " rev=")?.to_owned(),
            dirty: field(record, " dirty=")? != "0",
        })
    }

    /// Whether this identifies a specific tree. A worker built without git, or
    /// from modified sources, records what it can but cannot be matched by it.
    fn identifies_a_tree(&self) -> bool {
        !self.dirty && self.revision != "unknown" && !self.revision.is_empty()
    }
}

/// The value after `key` in a provenance record, up to the next space.
fn field<'a>(record: &'a str, key: &str) -> Option<&'a str> {
    record
        .split_once(key)
        .map(|(_, rest)| rest.split(' ').next().unwrap_or(rest))
}

/// This broker's own build revision, and whether it was built from modified
/// sources. See `build.rs`.
fn broker_provenance() -> WorkerProvenance {
    WorkerProvenance {
        revision: env!("AEXCOMPAT_BUILD_REVISION").to_owned(),
        dirty: env!("AEXCOMPAT_BUILD_DIRTY") != "0",
    }
}

/// A local minihost worker carries source-side logic (notably module-audit
/// classification), so a stale build can mislabel observations made for
/// current broker sources. Since issue #729 that observation is a recorded
/// warning on the dispatch result, not a launch gate: the observation is
/// bound to the worker hash either way, a wrong pairing disqualifies the
/// result at comparison time, and stale-*reuse* protection belongs to cache
/// keys (the multifilter `BuildFingerprint` pattern), not to dispatch.
///
/// The source mtimes decide the verdict whenever they are readable. They are
/// the only signal that sees a developer's uncommitted edit: a revision
/// describes the tree the same way before and after one, so a matching
/// revision *instead* would miss precisely the stale build this looks for.
///
/// The recorded revisions answer the one question mtimes cannot: a worker with no
/// repository around it. Shipped beside the plugin there is no `minihost/src` to
/// be newer than anything (issue #649). Both sides decide their revision at
/// build time, so a bundle built from one tree recognizes itself with nothing
/// else present. A dirty or unknown revision names no tree and is never
/// treated as a match.
fn local_worker_freshness_warning(
    repository: &Path,
    worker: &Path,
    provenance: Option<WorkerProvenance>,
) -> Option<&'static str> {
    local_worker_freshness_warning_against(repository, worker, provenance, broker_provenance())
}

/// The decision, with the broker's own side passed in so it can be exercised
/// from a tree that does not identify itself — which is every working tree with
/// an uncommitted change, i.e. the normal one.
fn local_worker_freshness_warning_against(
    repository: &Path,
    worker: &Path,
    provenance: Option<WorkerProvenance>,
    broker: WorkerProvenance,
) -> Option<&'static str> {
    let source_root = repository.join("minihost").join("src");
    let source_modified = match newest_source_modified(&source_root) {
        Ok(modified) => modified,
        // Only "there is no source tree here" hands the verdict to the recorded
        // revisions. Every other failure — the planted-symlink refusal, a file
        // locked mid-walk — happens *inside* a checkout whose sources exist, and
        // treating those as "no sources" would let a stale local build dodge
        // the very comparison that spots it.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return freshness_warning_without_sources(provenance, broker);
        }
        Err(_) => return Some("metadata_unavailable"),
    };
    let worker_modified = match std::fs::metadata(worker).and_then(|metadata| metadata.modified()) {
        Ok(modified) => modified,
        Err(_) => return Some("metadata_unavailable"),
    };
    if source_modified > worker_modified {
        return Some("source_newer_than_worker");
    }
    None
}

/// The worker's sources are not here to be compared against, so the recorded
/// revisions are all there is. Every outcome below was a flat rejection before
/// issue #729 demoted the gate to a warning.
fn freshness_warning_without_sources(
    provenance: Option<WorkerProvenance>,
    broker: WorkerProvenance,
) -> Option<&'static str> {
    let Some(provenance) = provenance else {
        return Some("worker_provenance_unavailable");
    };
    if !provenance.identifies_a_tree() || !broker.identifies_a_tree() {
        // Built from modified or unidentifiable sources. Nothing here can tell
        // whether the two halves belong together, and there are no mtimes to
        // fall back on.
        return Some("worker_provenance_indeterminate");
    }
    if provenance.revision != broker.revision {
        return Some("worker_revision_mismatch");
    }
    None
}

fn newest_source_modified(root: &Path) -> io::Result<SystemTime> {
    let mut newest = None;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "minihost source metadata is indeterminate",
                ));
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let modified = entry.metadata()?.modified()?;
                newest =
                    Some(newest.map_or(modified, |previous: SystemTime| previous.max(modified)));
            }
        }
    }
    newest.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "minihost source metadata is indeterminate",
        )
    })
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
    use std::time::{Duration as StdDuration, SystemTime};

    fn artifact(root: &Path, name: &str, bytes: &[u8]) -> ApprovedImageArtifact {
        let path = root.join(name);
        fs::write(&path, bytes).unwrap();
        ApprovedImageArtifact {
            path,
            expected_sha256: Sha256::digest(bytes).into(),
            expected_size: bytes.len() as u64,
        }
    }

    fn set_modified(path: &Path, time: SystemTime) {
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(time))
            .unwrap();
    }

    fn freshness_fixture(root: &Path, worker_newer: bool) -> PathBuf {
        let source = root.join("minihost/src/worker.cpp");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"source").unwrap();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        fs::create_dir_all(worker.parent().unwrap()).unwrap();
        fs::write(&worker, b"worker").unwrap();
        let now = SystemTime::now();
        let older = now - StdDuration::from_secs(60);
        if worker_newer {
            set_modified(&source, older);
            set_modified(&worker, now);
        } else {
            set_modified(&source, now);
            set_modified(&worker, older);
        }
        worker
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
            dependency_search_dirs: Vec::new(),
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

    // --- in-place dispatch (issue #751) --------------------------------------

    #[test]
    fn in_place_dispatch_rejects_staged_dependencies_and_relative_plugins() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-secure-image-dispatch-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let plugin = artifact(&root, "plugin.plugin", b"plugin");
        let dependency = artifact(&root, "dependency.dll", b"dependency");

        // Staged dependency artifacts and search directories describe two
        // different owners of closure resolution; carrying both is a caller
        // bug and must not silently prefer either.
        let error = dispatch_secure_image(SecureImageDispatch {
            repository: &root,
            worker_kind: WorkerKind::Render,
            plugin: plugin.clone(),
            dependencies: vec![dependency],
            dependency_search_dirs: vec![root.clone()],
            args_before_plugin: &[],
            args_after_plugin: &[],
            timeout: Some(Duration::from_secs(1)),
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("in-place dispatch"));

        let error = dispatch_secure_image(SecureImageDispatch {
            repository: &root,
            worker_kind: WorkerKind::Render,
            plugin: ApprovedImageArtifact {
                path: PathBuf::from("relative.plugin"),
                expected_sha256: plugin.expected_sha256,
                expected_size: plugin.expected_size,
            },
            dependencies: vec![],
            dependency_search_dirs: vec![root.clone()],
            args_before_plugin: &[],
            args_after_plugin: &[],
            timeout: Some(Duration::from_secs(1)),
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "in-place plugin path must be absolute");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn in_place_search_dirs_are_canonicalized_bounded_and_separator_free() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-search-dirs-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();

        // Relative and missing directories fail closed before any launch.
        let error = joined_dependency_search_dirs(&[PathBuf::from("relative")]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(joined_dependency_search_dirs(&[root.join("missing")]).is_err());

        // A directory whose name carries the separator cannot be encoded.
        let hostile = root.join("with;separator");
        fs::create_dir(&hostile).unwrap();
        let error = joined_dependency_search_dirs(&[hostile]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("separator"));

        // The bounded count matches the worker's re-validation, judged after
        // dedupe so only genuinely distinct directories count.
        let too_many: Vec<PathBuf> = (0..17)
            .map(|index| {
                let dir = root.join(format!("dir-{index}"));
                fs::create_dir(&dir).unwrap();
                dir
            })
            .collect();
        assert!(joined_dependency_search_dirs(&too_many).is_err());
        assert!(joined_dependency_search_dirs(&too_many[..16]).is_ok());

        // Case-insensitive duplicates collapse to one canonical entry, and the
        // worker receives the plain (de-verbatim) form (issue #231).
        let joined = joined_dependency_search_dirs(&[root.clone(), root.clone()]).unwrap();
        assert!(
            !joined.contains(';'),
            "one directory, no separator: {joined}"
        );
        assert!(
            !joined.starts_with(r"\\?\"),
            "worker paths are de-verbatim: {joined}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn in_place_dispatch_rejects_sealed_resources() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-secure-image-dispatch-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let plugin = artifact(&root, "plugin.plugin", b"plugin");
        let resource = root.join("Film Stocks");
        fs::create_dir(&resource).unwrap();
        let resource_file = resource.join("stock.grain");
        fs::write(&resource_file, b"grain").unwrap();
        let entry = crate::sealed_load_tree::SealedResourceEntry {
            source: resource_file,
            relative_path: "Film Stocks/stock.grain".into(),
            expected_sha256: Sha256::digest(b"grain").into(),
            expected_size: 5,
        };
        let error = dispatch_secure_image_with_resources(
            SecureImageDispatch {
                repository: &root,
                worker_kind: WorkerKind::L2,
                plugin,
                dependencies: vec![],
                dependency_search_dirs: vec![root.clone()],
                args_before_plugin: &[],
                args_after_plugin: &[],
                timeout: Some(Duration::from_secs(1)),
            },
            vec![entry],
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("data resources"));
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
        let source = root.join("minihost/src/worker.cpp");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"source").unwrap();
        let now = SystemTime::now();
        set_modified(&source, now - StdDuration::from_secs(60));
        set_modified(&worker, now);

        let error = dispatch_secure_image(SecureImageDispatch {
            repository: &root,
            worker_kind: WorkerKind::Render,
            plugin,
            dependencies: vec![],
            dependency_search_dirs: Vec::new(),
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
    fn current_local_worker_is_admitted_without_a_warning() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-worker-freshness-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let worker = freshness_fixture(&root, true);
        let admitted = admit_local_worker(&root, &worker).expect("current worker is admitted");
        assert_eq!(admitted.size, 6);
        assert_eq!(admitted.freshness_warning, None);
        fs::remove_dir_all(root).unwrap();
    }

    /// The gate this used to be rejected the dispatch outright; since issue
    /// #729 a stale worker launches, and the observation rides the result as
    /// a recorded warning instead.
    #[test]
    fn stale_local_worker_is_admitted_with_a_warning() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-worker-freshness-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let worker = freshness_fixture(&root, false);
        let admitted =
            admit_local_worker(&root, &worker).expect("a stale worker still launches (issue #729)");
        assert_eq!(admitted.freshness_warning, Some("source_newer_than_worker"));
        fs::remove_dir_all(root).unwrap();
    }

    /// A worker with neither sources to compare against nor a recorded build is
    /// unidentifiable. The reason names which of the two is missing, because
    /// they are fixed differently: build the worker from a tree that records
    /// its revision, or dispatch from a checkout.
    #[test]
    fn an_unidentifiable_worker_is_admitted_with_a_warning() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-worker-freshness-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        fs::create_dir_all(worker.parent().unwrap()).unwrap();
        fs::write(&worker, b"worker").unwrap();
        let admitted = admit_local_worker(&root, &worker)
            .expect("missing source metadata warns instead of blocking (issue #729)");
        assert_eq!(
            admitted.freshness_warning,
            Some("worker_provenance_unavailable")
        );
        fs::remove_dir_all(root).unwrap();
    }

    // --- build provenance (issue #649) ---------------------------------------

    /// The marker `minihost/src/worker_build_provenance.cpp` emits, as it appears
    /// in a built worker.
    fn provenance_bytes(revision: &str, dirty: bool) -> Vec<u8> {
        let dirty = u8::from(dirty);
        format!(
            "...binary noise...AEXCOMPAT-WORKER-PROVENANCE-V1 rev={revision} \
             dirty={dirty} END...more noise..."
        )
        .into_bytes()
    }

    fn identified(revision: &str) -> WorkerProvenance {
        WorkerProvenance {
            revision: revision.to_owned(),
            dirty: false,
        }
    }

    /// A worker with no repository around it is exactly what issue #649 is
    /// about: shipped beside the plugin, it has no `minihost/src` to be judged
    /// against. Matching build revisions answer it without any of that.
    ///
    /// The broker's side is injected rather than read from `env!`, so this runs
    /// in a working tree with uncommitted changes — the normal one, where the
    /// real broker identifies no tree and the test would otherwise assert
    /// nothing while reporting `ok`.
    #[test]
    fn a_worker_built_from_this_revision_carries_no_warning_without_sources() {
        assert_eq!(
            local_worker_freshness_warning_against(
                Path::new(r"C:\no\such\repository"),
                Path::new(r"C:\no\such\worker.exe"),
                Some(identified("0123456789ab")),
                identified("0123456789ab"),
            ),
            None,
            "a worker built from this revision needs no sources"
        );
    }

    /// A bundle whose halves came from different commits is flagged. There is
    /// no mtime relationship that would reveal it, so this is the only thing
    /// that can.
    #[test]
    fn a_worker_built_from_another_revision_warns_without_sources() {
        assert_eq!(
            local_worker_freshness_warning_against(
                Path::new(r"C:\no\such\repository"),
                Path::new(r"C:\no\such\worker.exe"),
                Some(identified("0000deadbeef")),
                identified("0123456789ab"),
            ),
            Some("worker_revision_mismatch")
        );
    }

    /// Either half built from modified or unidentifiable sources says nothing
    /// about whether they belong together, and without sources there is nothing
    /// to fall back on.
    #[test]
    fn an_unidentifiable_half_warns_without_sources() {
        let dirty = WorkerProvenance {
            revision: "0123456789ab".to_owned(),
            dirty: true,
        };
        for (worker, broker) in [
            (dirty.clone(), identified("0123456789ab")),
            (identified("0123456789ab"), dirty.clone()),
            (identified("unknown"), identified("unknown")),
        ] {
            assert_eq!(
                local_worker_freshness_warning_against(
                    Path::new(r"C:\no\such\repository"),
                    Path::new(r"C:\no\such\worker.exe"),
                    Some(worker.clone()),
                    broker.clone(),
                ),
                Some("worker_provenance_indeterminate"),
                "{worker:?} vs {broker:?}"
            );
        }
    }

    /// With the sources present the mtimes decide, whatever the revisions say.
    /// They are the only signal that sees an uncommitted edit — a revision reads
    /// the same before and after one — so letting a matching revision stand in
    /// for them would miss exactly the stale build this looks for.
    #[test]
    fn matching_revisions_do_not_excuse_a_worker_older_than_its_sources() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-worker-freshness-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let worker = freshness_fixture(&root, false);

        assert_eq!(
            local_worker_freshness_warning_against(
                &root,
                &worker,
                Some(identified("0123456789ab")),
                identified("0123456789ab"),
            ),
            Some("source_newer_than_worker"),
            "a worker older than its sources is stale however it identifies"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// ...and a revision that disagrees does not condemn a worker the sources
    /// vouch for either. A developer who commits without reconfiguring cmake has
    /// a current worker with a stale recorded revision; flagging it would tag
    /// every dispatch until someone re-ran configure.
    #[test]
    fn a_mismatched_revision_does_not_condemn_a_worker_newer_than_its_sources() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-worker-freshness-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let worker = freshness_fixture(&root, true);

        assert_eq!(
            local_worker_freshness_warning_against(
                &root,
                &worker,
                Some(identified("0000deadbeef")),
                identified("0123456789ab"),
            ),
            None,
            "the sources are here and they say the worker is current"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// A worker built with no git around it records `unknown`, which names no
    /// tree. Treating that as a match would admit anything.
    #[test]
    fn an_unknown_revision_identifies_nothing() {
        for revision in ["unknown", ""] {
            let provenance = WorkerProvenance {
                revision: revision.to_owned(),
                dirty: false,
            };
            assert!(!provenance.identifies_a_tree(), "{provenance:?}");
        }
        assert!(
            !WorkerProvenance {
                revision: "0123456789ab".to_owned(),
                dirty: true,
            }
            .identifies_a_tree(),
            "a dirty build identifies no tree either"
        );
        assert!(
            WorkerProvenance {
                revision: "0123456789ab".to_owned(),
                dirty: false,
            }
            .identifies_a_tree()
        );
    }

    /// Parsed out of the surrounding binary, not out of a tidy line.
    #[test]
    fn provenance_is_recovered_from_the_bytes_around_it() {
        assert_eq!(
            WorkerProvenance::find(&provenance_bytes("0123456789ab", false)),
            Some(identified("0123456789ab"))
        );
        assert_eq!(
            WorkerProvenance::find(&provenance_bytes("0123456789ab", true)),
            Some(WorkerProvenance {
                revision: "0123456789ab".to_owned(),
                dirty: true,
            })
        );
        assert_eq!(
            WorkerProvenance::find(b"a worker built before this existed"),
            None
        );
    }

    /// What follows the record in `.rdata` is whatever the linker put there:
    /// UTF-16 literals, doubles, pointers. Decoding the whole window before
    /// cutting at ` END` therefore discards a perfectly good record depending on
    /// unrelated code layout, and the worker stops identifying itself for reasons
    /// nobody can see.
    #[test]
    fn binary_noise_after_the_record_does_not_discard_it() {
        let mut bytes = b"AEXCOMPAT-WORKER-PROVENANCE-V1 rev=0123456789ab dirty=0 END".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x80, 0xC0]);
        bytes.extend_from_slice(&[0x41; 200]);

        assert_eq!(
            WorkerProvenance::find(&bytes),
            Some(identified("0123456789ab"))
        );
    }

    /// A record that never terminates inside the window is not a record. The
    /// bound is what stops a stray byte pattern from pulling the binary in.
    #[test]
    fn an_unterminated_record_is_not_accepted() {
        let mut bytes = b"AEXCOMPAT-WORKER-PROVENANCE-V1 rev=0123456789ab dirty=0 ".to_vec();
        bytes.extend_from_slice(&[0x41; WorkerProvenance::MAX_RECORD_BYTES]);
        bytes.extend_from_slice(b" END");

        assert_eq!(WorkerProvenance::find(&bytes), None);
        // ...and neither is a marker with nothing after it at all.
        assert_eq!(
            WorkerProvenance::find(WorkerProvenance::MARKER.as_bytes()),
            None
        );
        assert_eq!(
            WorkerProvenance::find(b"AEXCOMPAT-WORKER-PROVENANCE-V1 dirty=0 END"),
            None,
            "a record with no revision names no tree"
        );
    }

    /// The producer is C++ and the parser is Rust, with nothing but agreement
    /// between them. Renaming a token would leave every distributed bundle
    /// reporting `worker_provenance_unavailable` while both test suites stay
    /// green, so pin the tokens against the file that emits them.
    #[test]
    fn the_worker_side_emits_the_record_this_parses() {
        let source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../minihost/src/worker_build_provenance.cpp"),
        )
        .expect("the worker-side producer is part of this contract");

        for token in [WorkerProvenance::MARKER, " rev=", " dirty=", " END"] {
            assert!(
                source.contains(&format!("\"{token}")) || source.contains(&format!("{token}\"")),
                "{token:?} is not emitted by worker_build_provenance.cpp"
            );
        }
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
            dependency_search_dirs: Vec::new(),
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
            dependency_search_dirs: Vec::new(),
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
