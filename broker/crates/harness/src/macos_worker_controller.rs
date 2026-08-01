//! macOS-only process and resource boundary for Apple Silicon guest workers.
//!
//! This is crash/resource containment, not a confidentiality sandbox. In
//! particular, neither a process group nor rlimits prevent filesystem or
//! network access by a hostile native carrier.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

const SESSION_PREFIX: &str = "aexcompat-macos-worker";
const TERM_GRACE: Duration = Duration::from_millis(150);
pub(crate) const MAX_STDOUT_BYTES: usize = 256 * 1024;
pub(crate) const MAX_STDERR_BYTES: usize = 128 * 1024;
pub(crate) const MAX_SESSION_FILES: usize = 4;
pub(crate) const MAX_SESSION_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_RESIDENT_BYTES: u64 = 1024 * 1024 * 1024;
pub(crate) const MAX_CHILD_PROCESSES: usize = 0;

#[repr(C)]
#[derive(Default)]
struct RusageInfoV0 {
    uuid: [u8; 16],
    user_time: u64,
    system_time: u64,
    package_idle_wakeups: u64,
    interrupt_wakeups: u64,
    pageins: u64,
    wired_size: u64,
    resident_size: u64,
    physical_footprint: u64,
    process_start_absolute_time: u64,
    process_exit_absolute_time: u64,
}

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pid_rusage(
        pid: libc::c_int,
        flavor: libc::c_int,
        buffer: *mut libc::c_void,
    ) -> libc::c_int;
    fn proc_listchildpids(
        pid: libc::pid_t,
        buffer: *mut libc::c_void,
        buffer_size: libc::c_int,
    ) -> libc::c_int;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SecurityTier {
    UnicornGuest,
    NativeCarrierTrustedOnly,
}

impl SecurityTier {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::UnicornGuest => "apple_silicon_unicorn_guest",
            Self::NativeCarrierTrustedOnly => "apple_silicon_native_carrier_trusted_only",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ResourceLimits {
    pub(crate) cpu_seconds: u64,
    pub(crate) open_files: u64,
    pub(crate) output_file_bytes: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu_seconds: 90,
            open_files: 32,
            output_file_bytes: 256 * 1024 * 1024,
        }
    }
}

pub(crate) struct WorkerSession {
    root: PathBuf,
}

impl WorkerSession {
    pub(crate) fn create() -> Result<Self, String> {
        let base = std::env::temp_dir();
        for attempt in 0..64u32 {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let root = base.join(format!(
                "{SESSION_PREFIX}-{}-{nonce:032x}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => {
                    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(
                        |error| format!("secure macOS worker session permissions: {error}"),
                    )?;
                    return Ok(Self { root });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("create macOS worker session: {error}")),
            }
        }
        Err("could not allocate a unique macOS worker session".into())
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn stage_file(&self, source: &Path, name: &str) -> Result<PathBuf, String> {
        if name.is_empty()
            || name == "."
            || name == ".."
            || Path::new(name).components().count() != 1
        {
            return Err(format!("invalid staged file name: {name:?}"));
        }
        let metadata = fs::symlink_metadata(source)
            .map_err(|error| format!("inspect staged source {}: {error}", source.display()))?;
        if !metadata.file_type().is_file() {
            return Err(format!(
                "staged source is not a regular file: {}",
                source.display()
            ));
        }
        let destination = self.root.join(name);
        fs::copy(source, &destination).map_err(|error| {
            format!(
                "stage {} as {}: {error}",
                source.display(),
                destination.display()
            )
        })?;
        fs::set_permissions(&destination, metadata.permissions())
            .map_err(|error| format!("preserve staged file permissions: {error}"))?;
        let source_digest = hash_file(source)?;
        let destination_digest = hash_file(&destination)?;
        if source_digest != destination_digest {
            let _ = fs::remove_file(&destination);
            return Err(format!(
                "macos_worker_stage_identity: copied bytes differ for {}",
                source.display()
            ));
        }
        Ok(destination)
    }

    pub(crate) fn audit_tree(&self) -> Result<(), String> {
        self.audit_tree_with_limits(MAX_SESSION_FILES, MAX_SESSION_BYTES)
    }

    fn audit_tree_with_limits(&self, max_files: usize, max_bytes: u64) -> Result<(), String> {
        let mut count = 0usize;
        let mut bytes = 0u64;
        for entry in fs::read_dir(&self.root)
            .map_err(|error| format!("macos_worker_cleanup: enumerate session: {error}"))?
        {
            let entry = entry
                .map_err(|error| format!("macos_worker_cleanup: read session entry: {error}"))?;
            let metadata = entry.metadata().map_err(|error| {
                format!(
                    "macos_worker_cleanup: inspect session entry {}: {error}",
                    entry.path().display()
                )
            })?;
            if !metadata.is_file() {
                return Err(format!(
                    "macos_worker_artifact_limit: unexpected non-file {}",
                    entry.path().display()
                ));
            }
            count = count
                .checked_add(1)
                .ok_or_else(|| "macos_worker_artifact_limit: file count overflow".to_string())?;
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or_else(|| "macos_worker_artifact_limit: byte count overflow".to_string())?;
        }
        if count > max_files || bytes > max_bytes {
            return Err(format!(
                "macos_worker_artifact_limit: session has {count} files/{bytes} bytes; limits are {max_files}/{max_bytes}"
            ));
        }
        Ok(())
    }

    pub(crate) fn cleanup(&mut self) -> Result<(), String> {
        if self.root.as_os_str().is_empty() {
            return Ok(());
        }
        match fs::remove_dir_all(&self.root) {
            Ok(()) => {
                self.root.clear();
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.root.clear();
                Ok(())
            }
            Err(error) => Err(format!(
                "macos_worker_cleanup: remove session {}: {error}",
                self.root.display()
            )),
        }
    }

    pub(crate) fn command(
        &self,
        staged_worker: &Path,
        tier: SecurityTier,
        limits: ResourceLimits,
    ) -> Command {
        let mut command = Command::new(staged_worker);
        command
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("TMPDIR", &self.root)
            .env("AEXCOMPAT_MACOS_SECURITY_TIER", tier.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        install_resource_limits(&mut command, limits);
        command
    }
}

fn hash_file(path: &Path) -> Result<[u8; 32], String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("open staged identity source {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("hash staged file {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().into())
}

fn install_resource_limits(command: &mut Command, limits: ResourceLimits) {
    // SAFETY: this closure performs only async-signal-safe libc calls and
    // returns an io::Error without allocation in the child-before-exec path.
    unsafe {
        command.pre_exec(move || {
            // RLIMIT_RSS is declared by macOS but setrlimit returns EINVAL on
            // supported releases; memory therefore needs broker-side resident
            // accounting. RLIMIT_AS also cannot safely be lowered after the
            // GUI has mapped its frameworks.
            apply_limit(libc::RLIMIT_CPU, limits.cpu_seconds)?;
            apply_limit(libc::RLIMIT_NOFILE, limits.open_files)?;
            apply_limit(libc::RLIMIT_FSIZE, limits.output_file_bytes)?;
            Ok(())
        });
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("aexcompat macOS worker cleanup failed: {error}");
        }
    }
}

unsafe fn apply_limit(resource: libc::c_int, value: u64) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    // SAFETY: `limit` is initialized and points to storage valid for this call.
    if unsafe { libc::setrlimit(resource, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(Debug)]
pub(crate) struct BoundedOutput {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

pub(crate) fn wait_bounded(mut child: Child, deadline: Duration) -> Result<BoundedOutput, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or("worker stdout pipe is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("worker stderr pipe is unavailable")?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAX_STDOUT_BYTES, "stdout"));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_STDERR_BYTES, "stderr"));
    let started = Instant::now();
    let status = loop {
        match child
            .try_wait()
            .map_err(|error| format!("poll macOS worker: {error}"))?
        {
            Some(status) => break status,
            None if started.elapsed() < deadline => {
                if let Err(error) = audit_process(child.id()) {
                    terminate_process_group(&mut child)?;
                    return Err(error);
                }
                thread::sleep(Duration::from_millis(5));
            }
            None => {
                terminate_process_group(&mut child)?;
                return Err(format!(
                    "macos_worker_timeout: process group exceeded {} ms",
                    deadline.as_millis()
                ));
            }
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "macos_worker_output_limit: stdout reader panicked".to_string())??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "macos_worker_output_limit: stderr reader panicked".to_string())??;
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
    })
}

pub(crate) fn audit_process(pid: u32) -> Result<(), String> {
    let mut usage = RusageInfoV0::default();
    // SAFETY: usage is a correctly sized writable RUSAGE_INFO_V0 buffer.
    let usage_result = unsafe {
        proc_pid_rusage(
            pid as libc::c_int,
            0,
            (&mut usage as *mut RusageInfoV0).cast(),
        )
    };
    if usage_result != 0 {
        return Err(format!(
            "macos_worker_resource_accounting: proc_pid_rusage({pid}): {}",
            io::Error::last_os_error()
        ));
    }
    let observed_memory = usage.resident_size.max(usage.physical_footprint);
    if observed_memory > MAX_RESIDENT_BYTES {
        return Err(format!(
            "macos_worker_memory_limit: pid {pid} uses {observed_memory} bytes; limit is {MAX_RESIDENT_BYTES}"
        ));
    }
    let mut children = [0 as libc::pid_t; MAX_CHILD_PROCESSES + 1];
    // SAFETY: children is a writable pid_t array and its byte length is exact.
    let child_count = unsafe {
        proc_listchildpids(
            pid as libc::pid_t,
            children.as_mut_ptr().cast(),
            std::mem::size_of_val(&children) as libc::c_int,
        )
    };
    if child_count < 0 {
        return Err(format!(
            "macos_worker_resource_accounting: proc_listchildpids({pid}): {}",
            io::Error::last_os_error()
        ));
    }
    if child_count as usize > MAX_CHILD_PROCESSES {
        return Err(format!(
            "macos_worker_child_limit: pid {pid} has at least {child_count} child processes; limit is {MAX_CHILD_PROCESSES}"
        ));
    }
    Ok(())
}

pub(crate) fn run_staged_setup(
    worker: &Path,
    plugin: &Path,
    tier: SecurityTier,
    deadline: Duration,
) -> Result<BoundedOutput, String> {
    let mut session = WorkerSession::create()?;
    let staged_worker = session.stage_file(worker, "worker")?;
    let staged_plugin = session.stage_file(plugin, &staged_name("plugin", plugin))?;
    let mut command = session.command(&staged_worker, tier, ResourceLimits::default());
    command.arg("setup").arg(staged_plugin);
    let result = wait_bounded(
        command
            .spawn()
            .map_err(|error| format!("macos_worker_launch: {error}"))?,
        deadline,
    );
    let audit = session.audit_tree_with_limits(2, MAX_SESSION_BYTES);
    let cleanup = session.cleanup();
    match (result, audit, cleanup) {
        (Ok(output), Ok(()), Ok(())) => Ok(output),
        (Err(error), Ok(()), Ok(())) => Err(error),
        (Ok(_), Err(audit), Ok(())) => Err(audit),
        (Ok(_), Ok(()), Err(cleanup)) => Err(cleanup),
        (result, audit, cleanup) => {
            let mut errors = Vec::new();
            if let Err(error) = result {
                errors.push(error);
            }
            if let Err(error) = audit {
                errors.push(error);
            }
            if let Err(error) = cleanup {
                errors.push(error);
            }
            Err(errors.join("; "))
        }
    }
}

pub(crate) fn read_bounded(
    mut reader: impl Read,
    limit: usize,
    stream: &str,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("read worker {stream}: {error}"))?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > limit {
            return Err(format!(
                "macos_worker_output_limit: {stream} exceeded {limit} bytes"
            ));
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

pub(crate) fn terminate_process_group(child: &mut Child) -> Result<(), String> {
    let pid = child.id() as libc::pid_t;
    signal_group(pid, libc::SIGTERM)?;
    let leader_reaped = wait_for_exit(child, TERM_GRACE)?;
    if leader_reaped && !process_group_exists(pid)? {
        return Ok(());
    }
    signal_group(pid, libc::SIGKILL)?;
    if !leader_reaped {
        child.wait().map_err(|error| {
            format!("macos_worker_residual_process: reap leader {pid}: {error}")
        })?;
    }
    let started = Instant::now();
    while process_group_exists(pid)? {
        if started.elapsed() >= Duration::from_secs(2) {
            return Err(format!(
                "macos_worker_residual_process: process group {pid} remains after SIGKILL"
            ));
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

fn process_group_exists(pid: libc::pid_t) -> Result<bool, String> {
    // SAFETY: signal 0 performs an existence/permission probe without delivery.
    let result = unsafe { libc::kill(-pid, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(format!("probe macOS worker process group {pid}: {error}")),
    }
}

fn signal_group(pid: libc::pid_t, signal: libc::c_int) -> Result<(), String> {
    // SAFETY: a negative, validated child pid addresses only its process group.
    let result = unsafe { libc::kill(-pid, signal) };
    if result == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(format!("signal macOS worker process group {pid}: {error}"))
        }
    }
}

fn wait_for_exit(child: &mut Child, budget: Duration) -> Result<bool, String> {
    let started = Instant::now();
    loop {
        match child
            .try_wait()
            .map_err(|error| format!("reap macOS worker: {error}"))?
        {
            Some(_) => return Ok(true),
            None if started.elapsed() < budget => thread::sleep(Duration::from_millis(5)),
            None => return Ok(false),
        }
    }
}

pub(crate) fn staged_name(prefix: &str, source: &Path) -> String {
    let suffix = source
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("payload")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("{prefix}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_shell(_session: &WorkerSession) -> PathBuf {
        // macOS platform binaries are subject to path-sensitive signature
        // enforcement and cannot be copied as staging fixtures. Production
        // AEXCompat workers are project-built artifacts and are staged.
        PathBuf::from("/bin/sh")
    }

    #[test]
    fn session_is_private_and_removed_on_drop() {
        let root = {
            let session = WorkerSession::create().unwrap();
            assert_eq!(
                fs::metadata(session.root()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            session.root().to_path_buf()
        };
        assert!(!root.exists());
    }

    #[test]
    fn cleanup_failure_is_structurally_classified() {
        let mut session = WorkerSession::create().unwrap();
        let root = session.root().to_path_buf();
        fs::remove_dir(&root).unwrap();
        fs::write(&root, b"not a directory").unwrap();
        let error = session.cleanup().unwrap_err();
        assert!(error.contains("macos_worker_cleanup"), "{error}");
        fs::remove_file(root).unwrap();
    }

    #[test]
    fn staged_release_worker_executes_from_private_session() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let worker = repository.join("guest/target/release/aex-guest-worker");
        if !worker.is_file() {
            eprintln!("skipping staged worker smoke test until the guest Release build exists");
            return;
        }
        let fixture = WorkerSession::create().unwrap();
        let malformed = fixture.root().join("malformed.aex");
        fs::write(&malformed, b"not a PE image").unwrap();
        let output = run_staged_setup(
            &worker,
            &malformed,
            SecurityTier::UnicornGuest,
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("invalid PE"));
    }

    #[test]
    fn staged_x86_64_worker_executes_under_rosetta_when_built() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let worker = repository.join("guest/target/x86_64-apple-darwin/release/aex-guest-worker");
        if !worker.is_file() {
            eprintln!("skipping staged x86_64 smoke test until the native carrier exists");
            return;
        }
        let fixture = WorkerSession::create().unwrap();
        let malformed = fixture.root().join("malformed.aex");
        fs::write(&malformed, b"not a PE image").unwrap();
        let output = run_staged_setup(
            &worker,
            &malformed,
            SecurityTier::NativeCarrierTrustedOnly,
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("invalid PE"));
    }

    #[test]
    fn command_has_session_cwd_minimal_environment_and_fd_allowlist() {
        let session = WorkerSession::create().unwrap();
        let shell = fixture_shell(&session);
        let mut command = session.command(
            &shell,
            SecurityTier::UnicornGuest,
            ResourceLimits::default(),
        );
        command
            .arg("-c")
            .arg("pwd; printf 'tier=%s\\ntmp=%s\\nhome=%s\\nssh=%s\\n' \"$AEXCOMPAT_MACOS_SECURITY_TIER\" \"$TMPDIR\" \"$HOME\" \"$SSH_AUTH_SOCK\"; python_fd=3; if test -e /dev/fd/$python_fd; then exit 91; fi");
        let output = wait_bounded(command.spawn().unwrap(), Duration::from_secs(2)).unwrap();
        assert!(output.status.success(), "{:?}", output);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.lines().next().unwrap().contains(SESSION_PREFIX));
        assert!(text.contains("tier=apple_silicon_unicorn_guest"));
        assert!(text.contains("tmp="));
        assert!(text.contains("home=\n"));
        assert!(text.contains("ssh=\n"));
    }

    #[test]
    fn timeout_kills_the_worker_process_group_and_reaps_leader() {
        let session = WorkerSession::create().unwrap();
        let shell = fixture_shell(&session);
        let marker = session.root().join("child-started");
        let mut command = session.command(
            &shell,
            SecurityTier::NativeCarrierTrustedOnly,
            ResourceLimits::default(),
        );
        command
            .arg("-c")
            .arg("echo $$ > child-started; while :; do :; done");
        let child = command.spawn().unwrap();
        let pid = child.id();
        let error = wait_bounded(child, Duration::from_millis(100)).unwrap_err();
        assert!(error.contains("macos_worker_timeout"), "{error}");
        assert!(marker.is_file());
        // SAFETY: signal 0 only probes existence of the exact former leader.
        let probe = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(probe, -1, "worker leader {pid} still exists");
    }

    #[test]
    fn child_spawn_is_classified_and_contained() {
        let session = WorkerSession::create().unwrap();
        let shell = fixture_shell(&session);
        let mut command = session.command(
            &shell,
            SecurityTier::NativeCarrierTrustedOnly,
            ResourceLimits::default(),
        );
        let marker = session.root().join("child-pid");
        command
            .arg("-c")
            .arg("sleep 30 & echo $! > child-pid; wait");
        let child = command.spawn().unwrap();
        let pid = child.id();
        let error = wait_bounded(child, Duration::from_secs(2)).unwrap_err();
        assert!(error.contains("macos_worker_child_limit"), "{error}");
        let child_pid = fs::read_to_string(marker)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        // SAFETY: signal 0 only probes existence of the exact former leader.
        let probe = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(probe, -1, "worker leader {pid} still exists");
        // SAFETY: signal 0 only probes existence of the exact recorded child.
        let child_probe = unsafe { libc::kill(child_pid, 0) };
        assert_eq!(child_probe, -1, "worker child {child_pid} still exists");
    }

    #[test]
    fn stdout_flood_is_classified() {
        let session = WorkerSession::create().unwrap();
        let shell = fixture_shell(&session);
        let mut command = session.command(
            &shell,
            SecurityTier::UnicornGuest,
            ResourceLimits::default(),
        );
        command
            .arg("-c")
            .arg(format!("printf '%*s' {} ''", MAX_STDOUT_BYTES + 1));
        let error = wait_bounded(command.spawn().unwrap(), Duration::from_secs(2)).unwrap_err();
        assert!(error.contains("macos_worker_output_limit"), "{error}");
    }
}
