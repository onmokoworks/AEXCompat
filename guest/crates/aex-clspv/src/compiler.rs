use std::fmt;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const FIXED_CLSPV_OPTIONS: [&str; 5] = [
    "-cl-std=CL1.2",
    "-cl-kernel-arg-info",
    "-spv-version=1.0",
    "-pod-ubo",
    "-cluster-pod-kernel-args=1",
];

const ALLOWED_GUEST_OPTIONS: [&str; 2] = ["-cl-single-precision-constant", "-cl-fast-relaxed-math"];

const SPIRV_MAGIC_LE: [u8; 4] = [0x03, 0x02, 0x23, 0x07];
const SPIRV_HEADER_BYTES: usize = 20;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompileLimits {
    pub max_source_bytes: usize,
    pub max_compiler_bytes: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
    pub max_output_bytes: usize,
    pub timeout: Duration,
    pub poll_interval: Duration,
}

impl Default for CompileLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 16 * 1024 * 1024,
            max_compiler_bytes: 512 * 1024 * 1024,
            max_stdout_bytes: 1024 * 1024,
            max_stderr_bytes: 4 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(5),
        }
    }
}

impl CompileLimits {
    fn validate(&self) -> Result<(), CompilerError> {
        if self.max_source_bytes == 0 {
            return Err(CompilerError::InvalidLimits(
                "max_source_bytes must be non-zero",
            ));
        }
        if self.max_compiler_bytes == 0 {
            return Err(CompilerError::InvalidLimits(
                "max_compiler_bytes must be non-zero",
            ));
        }
        if self.max_stdout_bytes == 0 {
            return Err(CompilerError::InvalidLimits(
                "max_stdout_bytes must be non-zero",
            ));
        }
        if self.max_stderr_bytes == 0 {
            return Err(CompilerError::InvalidLimits(
                "max_stderr_bytes must be non-zero",
            ));
        }
        if self.max_output_bytes < SPIRV_HEADER_BYTES {
            return Err(CompilerError::InvalidLimits(
                "max_output_bytes must fit a SPIR-V header",
            ));
        }
        for (name, value) in [
            ("max_source_bytes", self.max_source_bytes),
            ("max_stdout_bytes", self.max_stdout_bytes),
            ("max_stderr_bytes", self.max_stderr_bytes),
            ("max_output_bytes", self.max_output_bytes),
        ] {
            if u64::try_from(value).map_or(true, |value| value == u64::MAX) {
                return Err(CompilerError::InvalidLimits(match name {
                    "max_source_bytes" => "max_source_bytes is not representable safely",
                    "max_stdout_bytes" => "max_stdout_bytes is not representable safely",
                    "max_stderr_bytes" => "max_stderr_bytes is not representable safely",
                    _ => "max_output_bytes is not representable safely",
                }));
            }
        }
        if self.timeout.is_zero() {
            return Err(CompilerError::InvalidLimits("timeout must be non-zero"));
        }
        if self.poll_interval.is_zero() {
            return Err(CompilerError::InvalidLimits(
                "poll_interval must be non-zero",
            ));
        }
        Ok(())
    }
}

/// Provenance for a compiler artifact.
///
/// The digest covers the executable bytes only. Adjacent dynamic libraries and
/// resource files belong to the explicitly configured, trusted local toolchain.
/// The diagnostic path is deliberately excluded from equality and hashing.
#[derive(Clone, Debug)]
pub struct CompilerIdentity {
    // Diagnostic provenance only; cache identity is the executable digest below.
    executable_path: Option<PathBuf>,
    // This covers the executable bytes, not adjacent dylibs or compiler resources.
    binary_sha256: [u8; 32],
}

impl PartialEq for CompilerIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.binary_sha256 == other.binary_sha256
    }
}

impl Eq for CompilerIdentity {}

impl Hash for CompilerIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.binary_sha256.hash(state);
    }
}

impl CompilerIdentity {
    pub fn from_binary_sha256(binary_sha256: [u8; 32]) -> Self {
        Self::for_precompiled(binary_sha256)
    }

    pub fn for_precompiled(binary_sha256: [u8; 32]) -> Self {
        Self {
            executable_path: None,
            binary_sha256,
        }
    }

    pub fn for_precompiled_hex(binary_sha256: &str) -> Result<Self, CompilerError> {
        Ok(Self::for_precompiled(parse_sha256(binary_sha256)?))
    }

    pub fn executable_path(&self) -> Option<&Path> {
        self.executable_path.as_deref()
    }

    pub fn binary_sha256(&self) -> [u8; 32] {
        self.binary_sha256
    }

    pub fn binary_sha256_hex(&self) -> String {
        hex_sha256(&self.binary_sha256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompileRequest {
    source: Vec<u8>,
    guest_options: Vec<String>,
}

impl CompileRequest {
    pub fn new(source: impl Into<Vec<u8>>, guest_options: Vec<String>) -> Self {
        Self {
            source: source.into(),
            guest_options,
        }
    }

    pub fn source(&self) -> &[u8] {
        &self.source
    }

    pub fn guest_options(&self) -> &[String] {
        &self.guest_options
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerOutput {
    spirv: Vec<u8>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    compiler_identity: CompilerIdentity,
    normalized_options: Vec<String>,
    invocation_argv: Vec<String>,
}

impl CompilerOutput {
    pub fn spirv(&self) -> &[u8] {
        &self.spirv
    }

    pub fn into_spirv(self) -> Vec<u8> {
        self.spirv
    }

    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    pub fn compiler_identity(&self) -> &CompilerIdentity {
        &self.compiler_identity
    }

    pub fn normalized_options(&self) -> &[String] {
        &self.normalized_options
    }

    /// Diagnostic only. Temporary input/output paths in this vector are not cache-key material.
    pub fn invocation_argv(&self) -> &[String] {
        &self.invocation_argv
    }
}

#[derive(Debug)]
pub enum CompilerError {
    InvalidLimits(&'static str),
    ExecutablePathNotAbsolute(PathBuf),
    ExecutableUnavailable {
        path: PathBuf,
        message: String,
    },
    ExecutableNotRegularFile(PathBuf),
    ExecutableNotRunnable(PathBuf),
    CompilerBinaryTooLarge {
        actual: u64,
        limit: u64,
    },
    InvalidSha256(String),
    CompilerIdentityMismatch {
        path: PathBuf,
        expected: [u8; 32],
        actual: [u8; 32],
    },
    SourceTooLarge {
        actual: usize,
        limit: usize,
    },
    UnsupportedGuestOption(String),
    DuplicateGuestOption(String),
    TemporaryDirectory(String),
    TemporaryIo {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    Spawn {
        path: PathBuf,
        message: String,
    },
    ProcessIo {
        operation: &'static str,
        message: String,
    },
    Timeout(Duration),
    StdoutTooLarge {
        actual: u64,
        limit: usize,
    },
    StderrTooLarge {
        actual: u64,
        limit: usize,
    },
    CompilerFailed {
        code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    OutputMissing(PathBuf),
    OutputTooLarge {
        actual: u64,
        limit: usize,
    },
    InvalidSpirv(String),
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits(message) => write!(f, "invalid compile limits: {message}"),
            Self::ExecutablePathNotAbsolute(path) => {
                write!(
                    f,
                    "clspv executable path must be absolute: {}",
                    path.display()
                )
            }
            Self::ExecutableUnavailable { path, message } => {
                write!(
                    f,
                    "cannot access clspv executable {}: {message}",
                    path.display()
                )
            }
            Self::ExecutableNotRegularFile(path) => {
                write!(
                    f,
                    "clspv executable is not a regular file: {}",
                    path.display()
                )
            }
            Self::ExecutableNotRunnable(path) => {
                write!(
                    f,
                    "clspv executable is not marked executable: {}",
                    path.display()
                )
            }
            Self::CompilerBinaryTooLarge { actual, limit } => {
                write!(f, "clspv executable is {actual} bytes; limit is {limit}")
            }
            Self::InvalidSha256(value) => write!(f, "invalid SHA-256 value: {value:?}"),
            Self::CompilerIdentityMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "clspv executable identity changed at {}: expected {}, got {}",
                path.display(),
                hex_sha256(expected),
                hex_sha256(actual)
            ),
            Self::SourceTooLarge { actual, limit } => {
                write!(f, "OpenCL source is {actual} bytes; limit is {limit}")
            }
            Self::UnsupportedGuestOption(option) => {
                write!(f, "unsupported OpenCL guest option: {option:?}")
            }
            Self::DuplicateGuestOption(option) => {
                write!(f, "duplicate OpenCL guest option: {option:?}")
            }
            Self::TemporaryDirectory(message) => {
                write!(
                    f,
                    "cannot create private clspv temporary directory: {message}"
                )
            }
            Self::TemporaryIo {
                operation,
                path,
                message,
            } => write!(f, "{operation} {} failed: {message}", path.display()),
            Self::Spawn { path, message } => {
                write!(f, "cannot spawn pinned clspv {}: {message}", path.display())
            }
            Self::ProcessIo { operation, message } => {
                write!(f, "clspv process {operation} failed: {message}")
            }
            Self::Timeout(timeout) => write!(f, "clspv exceeded timeout {timeout:?}"),
            Self::StdoutTooLarge { actual, limit } => {
                write!(f, "clspv stdout is {actual} bytes; limit is {limit}")
            }
            Self::StderrTooLarge { actual, limit } => {
                write!(f, "clspv stderr is {actual} bytes; limit is {limit}")
            }
            Self::CompilerFailed {
                code,
                stdout,
                stderr,
            } => write!(
                f,
                "clspv exited unsuccessfully (code {code:?}, stdout {} bytes, stderr {} bytes)",
                stdout.len(),
                stderr.len()
            ),
            Self::OutputMissing(path) => {
                write!(f, "clspv did not create output {}", path.display())
            }
            Self::OutputTooLarge { actual, limit } => {
                write!(
                    f,
                    "clspv output reached its {limit}-byte limit (observed {actual} bytes)"
                )
            }
            Self::InvalidSpirv(message) => write!(f, "invalid clspv output: {message}"),
        }
    }
}

impl std::error::Error for CompilerError {}

/// An explicitly configured clspv executable pinned by its byte digest.
///
/// The canonical toolchain path is trusted local configuration. It must not be
/// writable by the untrusted OpenCL source; the executable is rehashed directly
/// before every spawn.
#[derive(Clone, Debug)]
pub struct PinnedCompiler {
    // The configured toolchain path is trusted local configuration and must live
    // in storage not writable by the untrusted OpenCL source. It is rehashed
    // immediately before spawn; source/options never influence this pathname.
    executable_path: PathBuf,
    identity: CompilerIdentity,
    limits: CompileLimits,
}

impl PinnedCompiler {
    pub fn new(executable_path: impl AsRef<Path>) -> Result<Self, CompilerError> {
        Self::open(executable_path.as_ref(), None, CompileLimits::default())
    }

    pub fn with_expected_sha256(
        executable_path: impl AsRef<Path>,
        expected_sha256: [u8; 32],
    ) -> Result<Self, CompilerError> {
        Self::open(
            executable_path.as_ref(),
            Some(expected_sha256),
            CompileLimits::default(),
        )
    }

    pub fn with_expected_sha256_hex(
        executable_path: impl AsRef<Path>,
        expected_sha256: &str,
    ) -> Result<Self, CompilerError> {
        Self::with_expected_sha256(executable_path, parse_sha256(expected_sha256)?)
    }

    fn open(
        executable_path: &Path,
        expected_sha256: Option<[u8; 32]>,
        limits: CompileLimits,
    ) -> Result<Self, CompilerError> {
        limits.validate()?;
        if !executable_path.is_absolute() {
            return Err(CompilerError::ExecutablePathNotAbsolute(
                executable_path.to_path_buf(),
            ));
        }
        let executable_path = fs::canonicalize(executable_path).map_err(|error| {
            CompilerError::ExecutableUnavailable {
                path: executable_path.to_path_buf(),
                message: error.to_string(),
            }
        })?;
        validate_executable(&executable_path)?;
        let actual_sha256 = sha256_file(&executable_path, limits.max_compiler_bytes)?;
        if let Some(expected_sha256) = expected_sha256 {
            if expected_sha256 != actual_sha256 {
                return Err(CompilerError::CompilerIdentityMismatch {
                    path: executable_path,
                    expected: expected_sha256,
                    actual: actual_sha256,
                });
            }
        }
        let identity = CompilerIdentity {
            executable_path: Some(executable_path.clone()),
            binary_sha256: actual_sha256,
        };
        Ok(Self {
            executable_path,
            identity,
            limits,
        })
    }

    pub fn with_limits(mut self, limits: CompileLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn identity(&self) -> &CompilerIdentity {
        &self.identity
    }

    pub fn limits(&self) -> &CompileLimits {
        &self.limits
    }

    pub fn compile(&self, request: &CompileRequest) -> Result<CompilerOutput, CompilerError> {
        self.limits.validate()?;
        if request.source.len() > self.limits.max_source_bytes {
            return Err(CompilerError::SourceTooLarge {
                actual: request.source.len(),
                limit: self.limits.max_source_bytes,
            });
        }
        let normalized_guest_options = normalize_guest_options(&request.guest_options)?;
        let normalized_options = canonical_options_from_normalized_guest(&normalized_guest_options);
        self.verify_identity()?;

        let temp = tempfile::Builder::new()
            .prefix("aex-clspv-")
            .tempdir()
            .map_err(|error| CompilerError::TemporaryDirectory(error.to_string()))?;
        let source_path = temp.path().join("program.cl");
        let output_path = temp.path().join("program.spv");

        fs::write(&source_path, &request.source).map_err(|error| CompilerError::TemporaryIo {
            operation: "writing source",
            path: source_path.clone(),
            message: error.to_string(),
        })?;

        let mut argv = normalized_options.clone();
        argv.push("-o".to_owned());
        argv.push("program.spv".to_owned());
        argv.push("program.cl".to_owned());

        let mut command = Command::new(&self.executable_path);
        command
            .args(&argv)
            .current_dir(temp.path())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            let output_file_ceiling = self.limits.max_output_bytes as u64 + 1;
            // SAFETY: the closure performs only a direct setrlimit syscall before exec.
            unsafe {
                command.pre_exec(move || set_output_file_size_limit(output_file_ceiling));
            }
        }
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SYSTEMROOT") {
            command.env("SYSTEMROOT", system_root);
        }

        let mut child = command.spawn().map_err(|error| CompilerError::Spawn {
            path: self.executable_path.clone(),
            message: error.to_string(),
        })?;
        let stdout_pipe = match child.stdout.take() {
            Some(pipe) => pipe,
            None => {
                return terminate_with_error(
                    &mut child,
                    CompilerError::ProcessIo {
                        operation: "opening stdout pipe",
                        message: "spawned process has no stdout pipe".to_owned(),
                    },
                );
            }
        };
        let stderr_pipe = match child.stderr.take() {
            Some(pipe) => pipe,
            None => {
                return terminate_with_error(
                    &mut child,
                    CompilerError::ProcessIo {
                        operation: "opening stderr pipe",
                        message: "spawned process has no stderr pipe".to_owned(),
                    },
                );
            }
        };
        let stdout_reader = match BoundedCaptureReader::spawn(
            stdout_pipe,
            self.limits.max_stdout_bytes,
            CaptureKind::Stdout,
        ) {
            Ok(reader) => reader,
            Err(error) => return terminate_with_error(&mut child, error),
        };
        let stderr_reader = match BoundedCaptureReader::spawn(
            stderr_pipe,
            self.limits.max_stderr_bytes,
            CaptureKind::Stderr,
        ) {
            Ok(reader) => reader,
            Err(error) => {
                let result = terminate_with_error(&mut child, error);
                let _ = stdout_reader.finish();
                return result;
            }
        };
        let wait_result = self.wait_bounded(
            &mut child,
            stdout_reader.exceeded(),
            stderr_reader.exceeded(),
            &output_path,
        );
        let stdout_result = stdout_reader.finish();
        let stderr_result = stderr_reader.finish();
        let stdout = stdout_result?;
        let stderr = stderr_result?;
        let status = wait_result?;
        if !status.success() {
            if let Some(actual) = optional_file_len(&output_path)? {
                if actual >= self.limits.max_output_bytes as u64 {
                    return Err(CompilerError::OutputTooLarge {
                        actual,
                        limit: self.limits.max_output_bytes,
                    });
                }
            }
            return Err(CompilerError::CompilerFailed {
                code: status.code(),
                stdout,
                stderr,
            });
        }

        let output_metadata = match fs::metadata(&output_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CompilerError::OutputMissing(output_path));
            }
            Err(error) => {
                return Err(CompilerError::TemporaryIo {
                    operation: "statting output",
                    path: output_path,
                    message: error.to_string(),
                });
            }
        };
        if !output_metadata.is_file() {
            return Err(CompilerError::InvalidSpirv(
                "compiler output is not a regular file".to_owned(),
            ));
        }
        if output_metadata.len() > self.limits.max_output_bytes as u64 {
            return Err(CompilerError::OutputTooLarge {
                actual: output_metadata.len(),
                limit: self.limits.max_output_bytes,
            });
        }
        let spirv =
            read_file_limited(&output_path, self.limits.max_output_bytes).map_err(|error| {
                CompilerError::TemporaryIo {
                    operation: "reading output",
                    path: output_path.clone(),
                    message: error.to_string(),
                }
            })?;
        validate_spirv_container(&spirv)?;

        Ok(CompilerOutput {
            spirv,
            stdout,
            stderr,
            compiler_identity: self.identity.clone(),
            normalized_options,
            invocation_argv: argv,
        })
    }

    fn verify_identity(&self) -> Result<(), CompilerError> {
        validate_executable(&self.executable_path)?;
        let actual_sha256 = sha256_file(&self.executable_path, self.limits.max_compiler_bytes)?;
        if actual_sha256 != self.identity.binary_sha256 {
            return Err(CompilerError::CompilerIdentityMismatch {
                path: self.executable_path.clone(),
                expected: self.identity.binary_sha256,
                actual: actual_sha256,
            });
        }
        Ok(())
    }

    fn wait_bounded(
        &self,
        child: &mut Child,
        stdout_exceeded: &AtomicBool,
        stderr_exceeded: &AtomicBool,
        output_path: &Path,
    ) -> Result<ExitStatus, CompilerError> {
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    #[cfg(unix)]
                    kill_process_group(child).map_err(|error| CompilerError::ProcessIo {
                        operation: "terminating compiler helpers after leader exit",
                        message: error.to_string(),
                    })?;
                    return Ok(status);
                }
                Ok(None) => {}
                Err(error) => {
                    return terminate_with_error(
                        child,
                        CompilerError::ProcessIo {
                            operation: "try_wait",
                            message: error.to_string(),
                        },
                    );
                }
            }

            if stdout_exceeded.load(Ordering::Acquire) {
                return terminate_with_error(
                    child,
                    CompilerError::StdoutTooLarge {
                        actual: self.limits.max_stdout_bytes as u64 + 1,
                        limit: self.limits.max_stdout_bytes,
                    },
                );
            }
            if stderr_exceeded.load(Ordering::Acquire) {
                return terminate_with_error(
                    child,
                    CompilerError::StderrTooLarge {
                        actual: self.limits.max_stderr_bytes as u64 + 1,
                        limit: self.limits.max_stderr_bytes,
                    },
                );
            }
            let output_len = match optional_file_len(output_path) {
                Ok(length) => length,
                Err(error) => return terminate_with_error(child, error),
            };
            if let Some(output_len) = output_len {
                if output_len > self.limits.max_output_bytes as u64 {
                    return terminate_with_error(
                        child,
                        CompilerError::OutputTooLarge {
                            actual: output_len,
                            limit: self.limits.max_output_bytes,
                        },
                    );
                }
            }

            let elapsed = started.elapsed();
            if elapsed >= self.limits.timeout {
                return terminate_with_error(child, CompilerError::Timeout(self.limits.timeout));
            }
            let remaining = self.limits.timeout.saturating_sub(elapsed);
            thread::sleep(self.limits.poll_interval.min(remaining));
        }
    }
}

fn normalize_guest_options(options: &[String]) -> Result<Vec<String>, CompilerError> {
    let mut selected = [false; ALLOWED_GUEST_OPTIONS.len()];
    for option in options {
        let Some(index) = ALLOWED_GUEST_OPTIONS
            .iter()
            .position(|allowed| option == allowed)
        else {
            return Err(CompilerError::UnsupportedGuestOption(option.clone()));
        };
        if selected[index] {
            return Err(CompilerError::DuplicateGuestOption(option.clone()));
        }
        selected[index] = true;
    }
    Ok(ALLOWED_GUEST_OPTIONS
        .iter()
        .enumerate()
        .filter(|(index, _)| selected[*index])
        .map(|(_, option)| (*option).to_owned())
        .collect())
}

fn canonical_options_from_guest(options: &[String]) -> Result<Vec<String>, CompilerError> {
    Ok(canonical_options_from_normalized_guest(
        &normalize_guest_options(options)?,
    ))
}

fn canonical_options_from_normalized_guest(options: &[String]) -> Vec<String> {
    let mut normalized = FIXED_CLSPV_OPTIONS
        .iter()
        .map(|option| (*option).to_owned())
        .collect::<Vec<_>>();
    normalized.extend(options.iter().cloned());
    normalized
}

pub(crate) fn normalize_options(options: &[String]) -> Result<Vec<String>, CompilerError> {
    if options.len() < FIXED_CLSPV_OPTIONS.len()
        || !options
            .iter()
            .zip(FIXED_CLSPV_OPTIONS)
            .all(|(actual, expected)| actual == expected)
    {
        return Err(CompilerError::UnsupportedGuestOption(
            "compiler option set is missing the exact fixed clspv prefix".to_owned(),
        ));
    }
    canonical_options_from_guest(&options[FIXED_CLSPV_OPTIONS.len()..])
}

fn validate_spirv_container(spirv: &[u8]) -> Result<(), CompilerError> {
    if spirv.len() < SPIRV_HEADER_BYTES {
        return Err(CompilerError::InvalidSpirv(format!(
            "output is {} bytes; a SPIR-V header needs {SPIRV_HEADER_BYTES}",
            spirv.len()
        )));
    }
    if spirv.len() % 4 != 0 {
        return Err(CompilerError::InvalidSpirv(format!(
            "output size {} is not word-aligned",
            spirv.len()
        )));
    }
    if spirv[..4] != SPIRV_MAGIC_LE {
        return Err(CompilerError::InvalidSpirv(
            "little-endian SPIR-V magic is missing".to_owned(),
        ));
    }
    Ok(())
}

fn validate_executable(path: &Path) -> Result<(), CompilerError> {
    let metadata = fs::metadata(path).map_err(|error| CompilerError::ExecutableUnavailable {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(CompilerError::ExecutableNotRegularFile(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(CompilerError::ExecutableNotRunnable(path.to_path_buf()));
        }
    }
    Ok(())
}

fn optional_file_len(path: &Path) -> Result<Option<u64>, CompilerError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CompilerError::TemporaryIo {
            operation: "statting output",
            path: path.to_path_buf(),
            message: error.to_string(),
        }),
    }
}

#[derive(Clone, Copy)]
enum CaptureKind {
    Stdout,
    Stderr,
}

impl CaptureKind {
    fn label(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }

    fn too_large(self, actual: u64, limit: usize) -> CompilerError {
        match self {
            CaptureKind::Stdout => CompilerError::StdoutTooLarge { actual, limit },
            CaptureKind::Stderr => CompilerError::StderrTooLarge { actual, limit },
        }
    }
}

struct BoundedCapture {
    bytes: Vec<u8>,
    total_bytes: u64,
}

struct BoundedCaptureReader {
    kind: CaptureKind,
    limit: usize,
    exceeded: Arc<AtomicBool>,
    handle: thread::JoinHandle<io::Result<BoundedCapture>>,
}

impl BoundedCaptureReader {
    fn spawn<R>(mut pipe: R, limit: usize, kind: CaptureKind) -> Result<Self, CompilerError>
    where
        R: Read + Send + 'static,
    {
        let exceeded = Arc::new(AtomicBool::new(false));
        let reader_exceeded = Arc::clone(&exceeded);
        let handle = thread::Builder::new()
            .name(format!("clspv-{}-reader", kind.label()))
            .spawn(move || {
                let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
                let mut total_bytes = 0_u64;
                let mut buffer = [0_u8; 16 * 1024];
                loop {
                    let count = pipe.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    total_bytes = total_bytes.saturating_add(count as u64);
                    if bytes.len() < limit {
                        let retained = count.min(limit - bytes.len());
                        bytes.extend_from_slice(&buffer[..retained]);
                    }
                    if total_bytes > limit as u64 {
                        reader_exceeded.store(true, Ordering::Release);
                    }
                }
                Ok(BoundedCapture { bytes, total_bytes })
            })
            .map_err(|error| CompilerError::ProcessIo {
                operation: "starting output reader",
                message: error.to_string(),
            })?;
        Ok(Self {
            kind,
            limit,
            exceeded,
            handle,
        })
    }

    fn exceeded(&self) -> &AtomicBool {
        &self.exceeded
    }

    fn finish(self) -> Result<Vec<u8>, CompilerError> {
        let capture = self
            .handle
            .join()
            .map_err(|_| CompilerError::ProcessIo {
                operation: "joining output reader",
                message: format!("{} reader panicked", self.kind.label()),
            })?
            .map_err(|error| CompilerError::ProcessIo {
                operation: "reading output pipe",
                message: format!("{}: {error}", self.kind.label()),
            })?;
        if capture.total_bytes > self.limit as u64 {
            return Err(self.kind.too_large(capture.total_bytes, self.limit));
        }
        Ok(capture.bytes)
    }
}

fn read_file_limited(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    file.take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file grew beyond its validated bound",
        ));
    }
    Ok(bytes)
}

fn terminate_and_reap(child: &mut Child) -> Result<(), CompilerError> {
    #[cfg(unix)]
    let kill_result = kill_process_group(child);
    #[cfg(not(unix))]
    let kill_result = child.kill();
    let wait_result = child.wait();
    if let Err(error) = kill_result {
        if error.kind() != io::ErrorKind::InvalidInput {
            return Err(CompilerError::ProcessIo {
                operation: "kill",
                message: error.to_string(),
            });
        }
    }
    wait_result
        .map(|_| ())
        .map_err(|error| CompilerError::ProcessIo {
            operation: "reap",
            message: error.to_string(),
        })
}

#[cfg(unix)]
fn kill_process_group(child: &mut Child) -> io::Result<()> {
    const SIGKILL: i32 = 9;
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }

    let process_group = i32::try_from(child.id())
        .map_err(|_| io::Error::other("child PID does not fit a Unix process-group ID"))?;
    // The child was placed in a fresh process group before spawn. A negative PID
    // targets that whole group, including compiler helpers inheriting the group.
    let result = unsafe { kill(-process_group, SIGKILL) };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(3) {
        // ESRCH is an expected race when the child exits between try_wait and kill.
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(unix)]
fn set_output_file_size_limit(limit: u64) -> io::Result<()> {
    use std::os::raw::{c_int, c_ulong};

    const RLIMIT_FSIZE: c_int = 1;

    #[repr(C)]
    struct RLimit {
        current: c_ulong,
        maximum: c_ulong,
    }

    unsafe extern "C" {
        fn setrlimit(resource: c_int, limit: *const RLimit) -> c_int;
    }

    let limit = c_ulong::try_from(limit)
        .map_err(|_| io::Error::other("output limit does not fit Unix rlim_t"))?;
    let limit = RLimit {
        current: limit,
        maximum: limit,
    };
    // SAFETY: `limit` points to a live C-compatible rlimit value for this syscall.
    if unsafe { setrlimit(RLIMIT_FSIZE, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn terminate_with_error<T>(child: &mut Child, error: CompilerError) -> Result<T, CompilerError> {
    terminate_and_reap(child)?;
    Err(error)
}

fn sha256_file(path: &Path, max_bytes: u64) -> Result<[u8; 32], CompilerError> {
    let metadata = fs::metadata(path).map_err(|error| CompilerError::ExecutableUnavailable {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if metadata.len() > max_bytes {
        return Err(CompilerError::CompilerBinaryTooLarge {
            actual: metadata.len(),
            limit: max_bytes,
        });
    }
    let mut file = File::open(path).map_err(|error| CompilerError::ExecutableUnavailable {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read =
            file.read(&mut buffer)
                .map_err(|error| CompilerError::ExecutableUnavailable {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                })?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(CompilerError::CompilerBinaryTooLarge {
                actual: total,
                limit: max_bytes,
            });
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn parse_sha256(value: &str) -> Result<[u8; 32], CompilerError> {
    if value.len() != 64 || !value.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(CompilerError::InvalidSha256(value.to_owned()));
    }
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| CompilerError::InvalidSha256(value.to_owned()))?;
    }
    Ok(bytes)
}

fn hex_sha256(value: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in value {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vector() {
        let digest: [u8; 32] = Sha256::digest(b"abc").into();
        assert_eq!(
            hex_sha256(&digest),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn precompiled_identity_is_path_independent() {
        let digest = [0x5a; 32];
        let identity = CompilerIdentity::for_precompiled(digest);
        assert_eq!(identity.binary_sha256(), digest);
        assert_eq!(identity.executable_path(), None);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        #[test]
        fn invokes_fixed_argv_and_canonicalizes_allowed_options() {
            let fixture = FakeCompiler::new(
                r#"
printf '%s\n' "$@" >&2
out=
previous=
for argument in "$@"; do
  if [ "$previous" = "-o" ]; then out=$argument; fi
  previous=$argument
done
printf '\003\002\043\007\000\000\001\000\000\000\000\000\001\000\000\000\000\000\000\000' > "$out"
"#,
            );
            let compiler = PinnedCompiler::new(&fixture.path).unwrap();
            let request = CompileRequest::new(
                b"__kernel void k(void) {}".to_vec(),
                vec![
                    "-cl-fast-relaxed-math".to_owned(),
                    "-cl-single-precision-constant".to_owned(),
                ],
            );
            let output = compiler.compile(&request).unwrap();
            assert_eq!(output.spirv(), minimal_spirv());
            assert_eq!(
                output.normalized_options(),
                canonical_options_from_guest(&[
                    "-cl-fast-relaxed-math".to_owned(),
                    "-cl-single-precision-constant".to_owned(),
                ])
                .unwrap()
            );
            let stderr = String::from_utf8(output.stderr().to_vec()).unwrap();
            let arguments = stderr.lines().collect::<Vec<_>>();
            assert_eq!(&arguments[..5], FIXED_CLSPV_OPTIONS);
            assert_eq!(
                &arguments[5..7],
                ["-cl-single-precision-constant", "-cl-fast-relaxed-math"]
            );
            assert_eq!(arguments[7], "-o");
            assert_eq!(arguments[8], "program.spv");
            assert_eq!(arguments[9], "program.cl");
        }

        #[test]
        fn rejects_unknown_and_duplicate_options_before_invocation() {
            let fixture = FakeCompiler::new("exit 99");
            let compiler = PinnedCompiler::new(&fixture.path).unwrap();
            let unknown = compiler.compile(&CompileRequest::new(
                Vec::new(),
                vec!["-I/tmp/injected".to_owned()],
            ));
            assert!(matches!(
                unknown,
                Err(CompilerError::UnsupportedGuestOption(option))
                    if option == "-I/tmp/injected"
            ));
            let duplicate = compiler.compile(&CompileRequest::new(
                Vec::new(),
                vec![
                    "-cl-fast-relaxed-math".to_owned(),
                    "-cl-fast-relaxed-math".to_owned(),
                ],
            ));
            assert!(matches!(
                duplicate,
                Err(CompilerError::DuplicateGuestOption(option))
                    if option == "-cl-fast-relaxed-math"
            ));
        }

        #[test]
        fn kills_the_compiler_process_group_and_reaps_on_timeout() {
            let marker_directory = tempfile::tempdir().unwrap();
            let marker = marker_directory.path().join("descendant-escaped");
            let fixture = FakeCompiler::new(&format!(
                "(sleep 1; printf escaped > \"{}\") &\nwhile :; do :; done",
                marker.display()
            ));
            let limits = CompileLimits {
                timeout: Duration::from_millis(75),
                poll_interval: Duration::from_millis(2),
                ..CompileLimits::default()
            };
            let compiler = PinnedCompiler::new(&fixture.path)
                .unwrap()
                .with_limits(limits);
            let started = Instant::now();
            let result = compiler.compile(&CompileRequest::new(Vec::new(), Vec::new()));
            assert!(matches!(result, Err(CompilerError::Timeout(_))));
            assert!(started.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(1100));
            assert!(
                !marker.exists(),
                "a compiler descendant escaped the terminated process group"
            );
        }

        #[test]
        fn kills_helpers_that_outlive_a_successful_compiler_leader() {
            let marker_directory = tempfile::tempdir().unwrap();
            let marker = marker_directory.path().join("helper-escaped");
            let fixture = FakeCompiler::new(&format!(
                r#"
(sleep 1; printf escaped > "{}") &
out=
previous=
for argument in "$@"; do
  if [ "$previous" = "-o" ]; then out=$argument; fi
  previous=$argument
done
printf '\003\002\043\007\000\000\001\000\000\000\000\000\001\000\000\000\000\000\000\000' > "$out"
"#,
                marker.display()
            ));
            let compiler = PinnedCompiler::new(&fixture.path).unwrap();
            let output = compiler
                .compile(&CompileRequest::new(Vec::new(), Vec::new()))
                .unwrap();
            assert_eq!(output.spirv(), minimal_spirv());
            thread::sleep(Duration::from_millis(1100));
            assert!(
                !marker.exists(),
                "a helper inherited the pipe and escaped after compiler success"
            );
        }

        #[test]
        fn rejects_output_over_bound() {
            let fixture = FakeCompiler::new(
                r#"
out=
previous=
for argument in "$@"; do
  if [ "$previous" = "-o" ]; then out=$argument; fi
  previous=$argument
done
dd if=/dev/zero of="$out" bs=64 count=1 2>/dev/null
"#,
            );
            let limits = CompileLimits {
                max_output_bytes: 32,
                ..CompileLimits::default()
            };
            let compiler = PinnedCompiler::new(&fixture.path)
                .unwrap()
                .with_limits(limits);
            assert!(matches!(
                compiler.compile(&CompileRequest::new(Vec::new(), Vec::new())),
                Err(CompilerError::OutputTooLarge {
                    actual,
                    limit: 32
                }) if actual >= 32
            ));
        }

        #[test]
        fn rejects_stdout_over_bound() {
            let fixture = FakeCompiler::new(
                r#"
out=
previous=
for argument in "$@"; do
  if [ "$previous" = "-o" ]; then out=$argument; fi
  previous=$argument
done
dd if=/dev/zero bs=64 count=1 2>/dev/null
printf '\003\002\043\007\000\000\001\000\000\000\000\000\001\000\000\000\000\000\000\000' > "$out"
"#,
            );
            let limits = CompileLimits {
                max_stdout_bytes: 32,
                ..CompileLimits::default()
            };
            let compiler = PinnedCompiler::new(&fixture.path)
                .unwrap()
                .with_limits(limits);
            assert!(matches!(
                compiler.compile(&CompileRequest::new(Vec::new(), Vec::new())),
                Err(CompilerError::StdoutTooLarge {
                    actual: 64,
                    limit: 32
                })
            ));
        }

        #[test]
        fn refuses_a_compiler_binary_changed_after_pinning() {
            let fixture = FakeCompiler::new("exit 0");
            let compiler = PinnedCompiler::new(&fixture.path).unwrap();
            fs::write(&fixture.path, "#!/bin/sh\nexit 1\n").unwrap();
            let mut permissions = fs::metadata(&fixture.path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&fixture.path, permissions).unwrap();
            assert!(matches!(
                compiler.compile(&CompileRequest::new(Vec::new(), Vec::new())),
                Err(CompilerError::CompilerIdentityMismatch { .. })
            ));
        }

        struct FakeCompiler {
            _directory: tempfile::TempDir,
            path: PathBuf,
        }

        impl FakeCompiler {
            fn new(body: &str) -> Self {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("fake-clspv");
                fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
                let mut permissions = fs::metadata(&path).unwrap().permissions();
                permissions.set_mode(0o700);
                fs::set_permissions(&path, permissions).unwrap();
                Self {
                    _directory: directory,
                    path,
                }
            }
        }
    }

    fn minimal_spirv() -> Vec<u8> {
        vec![
            0x03, 0x02, 0x23, 0x07, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]
    }
}
