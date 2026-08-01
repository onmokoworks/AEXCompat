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

const SESSION_PREFIX: &str = "aexcompat-macos-worker";
const TERM_GRACE: Duration = Duration::from_millis(150);
pub(crate) const MAX_STDOUT_BYTES: usize = 256 * 1024;
pub(crate) const MAX_STDERR_BYTES: usize = 128 * 1024;

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
        Ok(destination)
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
        if let Err(error) = fs::remove_dir_all(&self.root)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!(
                "aexcompat macOS worker cleanup failed for {}: {error}",
                self.root.display()
            );
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
            None if started.elapsed() < deadline => thread::sleep(Duration::from_millis(5)),
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

pub(crate) fn run_staged_setup(
    worker: &Path,
    plugin: &Path,
    tier: SecurityTier,
    deadline: Duration,
) -> Result<BoundedOutput, String> {
    let session = WorkerSession::create()?;
    let staged_worker = session.stage_file(worker, "worker")?;
    let staged_plugin = session.stage_file(plugin, &staged_name("plugin", plugin))?;
    let mut command = session.command(&staged_worker, tier, ResourceLimits::default());
    command.arg("setup").arg(staged_plugin);
    wait_bounded(
        command
            .spawn()
            .map_err(|error| format!("macos_worker_launch: {error}"))?,
        deadline,
    )
}

fn read_bounded(mut reader: impl Read, limit: usize, stream: &str) -> Result<Vec<u8>, String> {
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
    if wait_for_exit(child, TERM_GRACE)? {
        return Ok(());
    }
    signal_group(pid, libc::SIGKILL)?;
    child
        .wait()
        .map(|_| ())
        .map_err(|error| format!("macos_worker_residual_process: reap group {pid}: {error}"))
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
            .arg("pwd; env | sort; python_fd=3; if test -e /dev/fd/$python_fd; then exit 91; fi");
        let output = wait_bounded(command.spawn().unwrap(), Duration::from_secs(2)).unwrap();
        assert!(output.status.success(), "{:?}", output);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.lines().next().unwrap().contains(SESSION_PREFIX));
        assert!(text.contains("AEXCOMPAT_MACOS_SECURITY_TIER=apple_silicon_unicorn_guest"));
        assert!(text.contains("TMPDIR="));
        assert!(!text.contains("HOME="));
        assert!(!text.contains("SSH_"));
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
            .arg("sleep 30 & echo $! > child-started; wait");
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
    fn stdout_flood_is_classified() {
        let session = WorkerSession::create().unwrap();
        let shell = fixture_shell(&session);
        let mut command = session.command(
            &shell,
            SecurityTier::UnicornGuest,
            ResourceLimits::default(),
        );
        command.arg("-c").arg(format!(
            "dd if=/dev/zero bs={} count=1 2>/dev/null",
            MAX_STDOUT_BYTES + 1
        ));
        let error = wait_bounded(command.spawn().unwrap(), Duration::from_secs(2)).unwrap_err();
        assert!(error.contains("macos_worker_output_limit"), "{error}");
    }
}
