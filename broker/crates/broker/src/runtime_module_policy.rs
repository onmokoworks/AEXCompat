use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_RUNTIME_MODULES: usize = 128;
const PATH_TOKEN_DOMAIN: &[u8] = b"AEXCompat runtime module path token\0v1\0";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeBackend {
    Cpu,
    Cuda,
    Opencl,
    Directx,
    Opengl,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct PolicyDto {
    schema_version: u32,
    expires: String,
    modules: Vec<ModuleDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ModuleDto {
    path: PathBuf,
    basename: String,
    sha256: String,
    size: u64,
    backend: RuntimeBackend,
    #[serde(default)]
    signer_thumbprint: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeModule {
    pub path: PathBuf,
    pub basename: String,
    pub sha256: [u8; 32],
    pub size: u64,
    pub backend: RuntimeBackend,
    pub signer_thumbprint: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RuntimeModulePolicy {
    expires: SystemTime,
    modules: Vec<RuntimeModule>,
}

impl RuntimeModulePolicy {
    pub fn expires(&self) -> SystemTime {
        self.expires
    }
    pub fn modules(&self) -> &[RuntimeModule] {
        &self.modules
    }
}

pub fn parse_and_validate(json: &[u8]) -> io::Result<RuntimeModulePolicy> {
    parse_and_validate_at(json, SystemTime::now())
}

pub fn parse_and_validate_at(json: &[u8], now: SystemTime) -> io::Result<RuntimeModulePolicy> {
    let dto: PolicyDto = serde_json::from_slice(json).map_err(|e| invalid(e.to_string()))?;
    if dto.schema_version != 1 {
        return Err(invalid("unsupported runtime module policy schema"));
    }
    if dto.modules.len() > MAX_RUNTIME_MODULES {
        return Err(invalid("runtime module limit exceeded"));
    }
    let expires = parse_utc_rfc3339(&dto.expires)?;
    if expires <= now {
        return Err(invalid("runtime module policy is expired"));
    }
    let mut paths = HashSet::new();
    let mut names = HashSet::new();
    let mut modules = Vec::with_capacity(dto.modules.len());
    for module in dto.modules {
        validate_basename(&module.basename)?;
        validate_optional(
            "signer thumbprint",
            module.signer_thumbprint.as_deref(),
            128,
        )?;
        validate_optional("version", module.version.as_deref(), 128)?;
        if module.size == 0 {
            return Err(invalid("runtime module size must be nonzero"));
        }
        let canonical = canonical_exact(&module.path)?;
        if canonical.file_name().and_then(|v| v.to_str()) != Some(&module.basename) {
            return Err(invalid(
                "runtime module path does not end with its basename",
            ));
        }
        if !paths.insert(fold_path(&canonical)) {
            return Err(invalid("runtime module path collision"));
        }
        if !names.insert(module.basename.to_lowercase()) {
            return Err(invalid("runtime module basename collision"));
        }
        let sha256 = decode_sha256(&module.sha256)?;
        authenticate_file(&canonical, module.size, &sha256)?;
        modules.push(RuntimeModule {
            path: canonical,
            basename: module.basename,
            sha256,
            size: module.size,
            backend: module.backend,
            signer_thumbprint: module.signer_thumbprint,
            version: module.version,
        });
    }
    Ok(RuntimeModulePolicy { expires, modules })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ModuleClassification {
    Policy,
    System32,
    Sealed,
    Trusted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClassifiedModuleReport {
    pub classification: ModuleClassification,
    pub basename: String,
    pub path_token: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedClassifiedModule {
    pub path: PathBuf,
    pub basename: String,
    pub sha256: [u8; 32],
    pub size: u64,
}

pub struct WorkerModuleValidation<'a> {
    pub policy: &'a RuntimeModulePolicy,
    pub sealed: &'a [ApprovedClassifiedModule],
    pub trusted: &'a [ApprovedClassifiedModule],
    pub system32: &'a Path,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GpuWorkerModuleReportDto {
    session_identity: String,
    backend: RuntimeBackend,
    modules: Vec<ClassifiedModuleReport>,
}

/// Proof that a GPU worker module report was authenticated against a policy
/// and bound to one broker-created render session.
#[derive(Clone, Debug)]
pub struct AuthenticatedGpuModuleReport {
    session_identity: [u8; 32],
    backend: RuntimeBackend,
    policy_expires: SystemTime,
}

impl AuthenticatedGpuModuleReport {
    #[cfg(test)]
    pub(crate) fn test_only(
        session_identity: [u8; 32],
        backend: RuntimeBackend,
        policy_expires: SystemTime,
    ) -> Self {
        Self {
            session_identity,
            backend,
            policy_expires,
        }
    }

    pub fn authorize_dispatch(
        &self,
        session_identity: &[u8; 32],
        backend: RuntimeBackend,
    ) -> io::Result<()> {
        self.authorize_dispatch_at(session_identity, backend, SystemTime::now())
    }

    pub fn authorize_dispatch_at(
        &self,
        session_identity: &[u8; 32],
        backend: RuntimeBackend,
        now: SystemTime,
    ) -> io::Result<()> {
        if self.policy_expires <= now {
            return Err(invalid("runtime module policy is expired"));
        }
        if self.backend != backend {
            return Err(invalid("runtime module report backend mismatch"));
        }
        if !constant_time_eq(&self.session_identity, session_identity) {
            return Err(invalid("runtime module report session identity mismatch"));
        }
        Ok(())
    }
}

/// Authenticates the report emitted by a GPU worker. The returned value is the
/// only report form accepted by secure GPU dispatch.
pub fn authenticate_gpu_worker_report(
    json: &[u8],
    expected_session_identity: &[u8; 32],
    expected_backend: RuntimeBackend,
    context: WorkerModuleValidation<'_>,
) -> io::Result<AuthenticatedGpuModuleReport> {
    authenticate_gpu_worker_report_at(
        json,
        expected_session_identity,
        expected_backend,
        context,
        SystemTime::now(),
    )
}

pub fn authenticate_gpu_worker_report_at(
    json: &[u8],
    expected_session_identity: &[u8; 32],
    expected_backend: RuntimeBackend,
    context: WorkerModuleValidation<'_>,
    now: SystemTime,
) -> io::Result<AuthenticatedGpuModuleReport> {
    if expected_backend == RuntimeBackend::Cpu {
        return Err(invalid(
            "GPU module policy cannot authorize the CPU backend",
        ));
    }
    if expected_session_identity.iter().all(|byte| *byte == 0) {
        return Err(invalid("runtime module session identity must be nonzero"));
    }
    if context.policy.expires <= now {
        return Err(invalid("runtime module policy is expired"));
    }
    let report: GpuWorkerModuleReportDto =
        serde_json::from_slice(json).map_err(|e| invalid(e.to_string()))?;
    let reported_session = decode_sha256(&report.session_identity)?;
    if !constant_time_eq(&reported_session, expected_session_identity) {
        return Err(invalid("runtime module report session identity mismatch"));
    }
    if report.backend != expected_backend {
        return Err(invalid("runtime module report backend mismatch"));
    }
    let policy_expires = context.policy.expires;
    validate_worker_reports_at(report.modules, context, now, Some(expected_backend))?;
    Ok(AuthenticatedGpuModuleReport {
        session_identity: reported_session,
        backend: report.backend,
        policy_expires,
    })
}

pub fn validate_worker_report(json: &[u8], context: WorkerModuleValidation<'_>) -> io::Result<()> {
    validate_worker_report_at(json, context, SystemTime::now())
}

fn validate_worker_report_at(
    json: &[u8],
    context: WorkerModuleValidation<'_>,
    now: SystemTime,
) -> io::Result<()> {
    let reports: Vec<ClassifiedModuleReport> =
        serde_json::from_slice(json).map_err(|e| invalid(e.to_string()))?;
    validate_worker_reports_at(reports, context, now, None)
}

fn validate_worker_reports_at(
    reports: Vec<ClassifiedModuleReport>,
    context: WorkerModuleValidation<'_>,
    now: SystemTime,
    policy_backend: Option<RuntimeBackend>,
) -> io::Result<()> {
    if context.policy.expires <= now {
        return Err(invalid("runtime module policy is expired"));
    }
    if reports.len() > MAX_RUNTIME_MODULES {
        return Err(invalid("worker module report limit exceeded"));
    }
    let system32 = canonical_exact(context.system32)?;
    let mut seen = HashSet::new();
    for report in reports {
        validate_basename(&report.basename)?;
        let digest = decode_sha256(&report.sha256)?;
        let candidates: Vec<ApprovedClassifiedModule> = match report.classification {
            ModuleClassification::Policy => context
                .policy
                .modules
                .iter()
                .filter(|module| policy_backend.is_none_or(|backend| module.backend == backend))
                .map(|m| ApprovedClassifiedModule {
                    path: m.path.clone(),
                    basename: m.basename.clone(),
                    sha256: m.sha256,
                    size: m.size,
                })
                .collect(),
            ModuleClassification::Sealed => context.sealed.to_vec(),
            ModuleClassification::Trusted => context.trusted.to_vec(),
            ModuleClassification::System32 => {
                let path = system32.join(&report.basename);
                let canonical = canonical_exact(&path)?;
                if canonical.parent() != Some(system32.as_path()) {
                    return Err(invalid("System32 module is not a direct child"));
                }
                vec![ApprovedClassifiedModule {
                    path: canonical,
                    basename: report.basename.clone(),
                    sha256: digest,
                    size: report.size,
                }]
            }
        };
        let mut matched = None;
        for candidate in &candidates {
            let canonical = canonical_exact(&candidate.path)?;
            if candidate.basename.eq_ignore_ascii_case(&report.basename)
                && candidate.sha256 == digest
                && candidate.size == report.size
                && path_token(&canonical) == report.path_token
            {
                matched = Some(candidate);
                break;
            }
        }
        let module = matched.ok_or_else(|| invalid("classified worker module is not approved"))?;
        let key = format!("{:?}:{}", report.classification, report.path_token);
        if !seen.insert(key) {
            return Err(invalid("duplicate classified worker module"));
        }
        authenticate_file(&module.path, module.size, &module.sha256)?;
    }
    Ok(())
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

pub fn path_token(path: &Path) -> String {
    let mut hash = Sha256::new();
    hash.update(PATH_TOKEN_DOMAIN);
    hash.update(fold_path(path).as_bytes());
    hex(&hash.finalize())
}

fn canonical_exact(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid(
            "runtime module path must be canonical and absolute",
        ));
    }
    reject_reparse_components(path)?;
    let canonical = fs::canonicalize(path)?;
    if fold_path(path) != fold_path(&canonical) {
        return Err(invalid("runtime module path is not canonical"));
    }
    Ok(canonical)
}

fn validate_basename(name: &str) -> io::Result<()> {
    let mut c = Path::new(name).components();
    if name.is_empty()
        || !matches!(c.next(), Some(Component::Normal(_)))
        || c.next().is_some()
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || name.ends_with(['.', ' '])
    {
        return Err(invalid("runtime module basename is not Windows-safe"));
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', ' '])
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || matches!(
            stem.as_bytes(),
            [b'C', b'O', b'M', b'1'..=b'9'] | [b'L', b'P', b'T', b'1'..=b'9']
        )
    {
        return Err(invalid("runtime module basename is a reserved device name"));
    }
    Ok(())
}

fn validate_optional(kind: &str, value: Option<&str>, max: usize) -> io::Result<()> {
    if let Some(v) = value {
        if v.is_empty() || v.len() > max || v.chars().any(char::is_control) {
            return Err(invalid(format!("invalid {kind}")));
        }
    }
    Ok(())
}

fn authenticate_file(path: &Path, size: u64, digest: &[u8; 32]) -> io::Result<()> {
    reject_reparse_components(path)?;
    let mut file = open_source(path)?;
    validate_regular_unique(&file)?;
    if file.metadata()?.len() != size {
        return Err(invalid("runtime module size changed"));
    }
    let mut hash = Sha256::new();
    io::copy(&mut file, &mut hash)?;
    if hash.finalize().as_slice() != digest {
        return Err(invalid("runtime module hash changed"));
    }
    Ok(())
}

fn reject_reparse_components(path: &Path) -> io::Result<()> {
    for p in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        let m = fs::symlink_metadata(p)?;
        if m.file_type().is_symlink() || is_reparse(&m) {
            return Err(invalid("runtime module path contains a reparse point"));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse(m: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT != 0
}
#[cfg(not(windows))]
fn is_reparse(_: &fs::Metadata) -> bool {
    false
}
#[cfg(windows)]
fn open_source(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}
#[cfg(not(windows))]
fn open_source(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[cfg(windows)]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    use std::{mem::zeroed, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, GetFileInformationByHandle,
    };
    let mut i: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut i) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if i.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || i.nNumberOfLinks != 1
        || !file.metadata()?.is_file()
    {
        return Err(invalid("runtime module must be regular and single-link"));
    }
    Ok(())
}
#[cfg(unix)]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    if !m.is_file() || m.nlink() != 1 {
        return Err(invalid("runtime module must be regular and single-link"));
    }
    Ok(())
}
#[cfg(not(any(windows, unix)))]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    if !file.metadata()?.is_file() {
        return Err(invalid("runtime module must be regular"));
    }
    Ok(())
}

fn decode_sha256(v: &str) -> io::Result<[u8; 32]> {
    if v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("SHA-256 must be 64 hexadecimal characters"));
    }
    let mut out = [0; 32];
    for (o, p) in out.iter_mut().zip(v.as_bytes().chunks_exact(2)) {
        *o = u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16)
            .map_err(|_| invalid("invalid SHA-256"))?;
    }
    Ok(out)
}
fn fold_path(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn parse_utc_rfc3339(value: &str) -> io::Result<SystemTime> {
    if value.len() != 20
        || &value[4..5] != "-"
        || &value[7..8] != "-"
        || &value[10..11] != "T"
        || &value[13..14] != ":"
        || &value[16..17] != ":"
        || &value[19..] != "Z"
    {
        return Err(invalid("expires must be RFC 3339 UTC seconds"));
    }
    let n = |a, b| {
        value[a..b]
            .parse::<i64>()
            .map_err(|_| invalid("invalid expires"))
    };
    let (y, mo, d, h, mi, s) = (
        n(0, 4)?,
        n(5, 7)?,
        n(8, 10)?,
        n(11, 13)?,
        n(14, 16)?,
        n(17, 19)?,
    );
    if y < 1970 || !(1..=12).contains(&mo) || h > 23 || mi > 59 || s > 59 {
        return Err(invalid("invalid expires"));
    }
    let leap = |year: i64| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let md = [
        31,
        if leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if d < 1 || d > md[(mo - 1) as usize] {
        return Err(invalid("invalid expires"));
    }
    let days = (1970..y)
        .map(|yr| if leap(yr) { 366 } else { 365 })
        .sum::<i64>()
        + md[..(mo - 1) as usize].iter().sum::<i64>()
        + d
        - 1;
    UNIX_EPOCH
        .checked_add(Duration::from_secs(
            (days * 86400 + h * 3600 + mi * 60 + s) as u64,
        ))
        .ok_or_else(|| invalid("expires out of range"))
}
