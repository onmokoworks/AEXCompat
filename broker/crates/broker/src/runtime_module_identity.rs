use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeMachine {
    I386,
    Amd64,
    Arm,
    Arm64,
    Unknown(u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileIdentity {
    pub volume_serial_number: u32,
    pub file_index: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticodeEvidence {
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeModuleIdentityEvidence {
    pub canonical_path: PathBuf,
    pub size: u64,
    pub sha256: [u8; 32],
    pub pe_machine: PeMachine,
    pub file_identity: FileIdentity,
    pub authenticode: AuthenticodeEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityEvidenceErrorKind {
    InvalidPath,
    UnsafeFile,
    InvalidPe,
    Unsupported,
    Io,
}

#[derive(Debug)]
pub struct IdentityEvidenceError {
    pub kind: IdentityEvidenceErrorKind,
    message: String,
}

impl std::fmt::Display for IdentityEvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for IdentityEvidenceError {}

impl From<io::Error> for IdentityEvidenceError {
    fn from(value: io::Error) -> Self {
        Self {
            kind: IdentityEvidenceErrorKind::Io,
            message: value.to_string(),
        }
    }
}

pub fn capture_runtime_module_identity(
    path: &Path,
) -> Result<RuntimeModuleIdentityEvidence, IdentityEvidenceError> {
    capture_impl(path)
}

/// Fails closed until full WinVerifyTrust policy and chain evidence is implemented.
pub fn require_verified_authenticode(
    evidence: &RuntimeModuleIdentityEvidence,
) -> Result<(), IdentityEvidenceError> {
    match evidence.authenticode {
        AuthenticodeEvidence::Unsupported => Err(error(
            IdentityEvidenceErrorKind::Unsupported,
            "Authenticode verification is unsupported",
        )),
    }
}

#[cfg(windows)]
fn capture_impl(path: &Path) -> Result<RuntimeModuleIdentityEvidence, IdentityEvidenceError> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    };

    validate_input_path(path)?;
    reject_reparse_components(path)?;
    let canonical_path = fs::canonicalize(path)?;
    reject_reparse_components(&canonical_path)?;

    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&canonical_path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(error(
            IdentityEvidenceErrorKind::UnsafeFile,
            "runtime module must be a regular non-reparse file",
        ));
    }

    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    if info.nNumberOfLinks != 1 {
        return Err(error(
            IdentityEvidenceErrorKind::UnsafeFile,
            "runtime module must have exactly one hard link",
        ));
    }
    let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
    let pe_machine = read_pe_machine(&mut file, size)?;
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    io::copy(&mut file, &mut hash)?;
    let sha256: [u8; 32] = hash.finalize().into();

    // Re-resolve while the no-write/no-delete-share handle is alive to detect namespace replacement.
    if fs::canonicalize(path)? != canonical_path {
        return Err(error(
            IdentityEvidenceErrorKind::UnsafeFile,
            "runtime module path changed during evidence capture",
        ));
    }
    Ok(RuntimeModuleIdentityEvidence {
        canonical_path,
        size,
        sha256,
        pe_machine,
        file_identity: FileIdentity {
            volume_serial_number: info.dwVolumeSerialNumber,
            file_index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        },
        authenticode: AuthenticodeEvidence::Unsupported,
    })
}

#[cfg(not(windows))]
fn capture_impl(_: &Path) -> Result<RuntimeModuleIdentityEvidence, IdentityEvidenceError> {
    Err(error(
        IdentityEvidenceErrorKind::Unsupported,
        "runtime module identity evidence is only supported on Windows",
    ))
}

fn validate_input_path(path: &Path) -> Result<(), IdentityEvidenceError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(error(
            IdentityEvidenceErrorKind::InvalidPath,
            "runtime module path must be absolute without dot components",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn reject_reparse_components(path: &Path) -> Result<(), IdentityEvidenceError> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    for component in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        let metadata = fs::symlink_metadata(component)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(error(
                IdentityEvidenceErrorKind::UnsafeFile,
                "runtime module path contains a reparse point",
            ));
        }
    }
    Ok(())
}

fn read_pe_machine(file: &mut File, size: u64) -> Result<PeMachine, IdentityEvidenceError> {
    if size < 64 {
        return Err(invalid_pe("PE file is shorter than the DOS header"));
    }
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        return Err(invalid_pe("PE DOS signature is missing"));
    }
    let offset = u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()) as u64;
    let header_end = offset
        .checked_add(6)
        .filter(|end| *end <= size)
        .ok_or_else(|| invalid_pe("PE header offset is outside the file"))?;
    debug_assert!(header_end <= size);
    file.seek(SeekFrom::Start(offset))?;
    let mut header = [0u8; 6];
    file.read_exact(&mut header)?;
    if &header[..4] != b"PE\0\0" {
        return Err(invalid_pe("PE signature is missing"));
    }
    Ok(match u16::from_le_bytes([header[4], header[5]]) {
        0x014c => PeMachine::I386,
        0x8664 => PeMachine::Amd64,
        0x01c0 | 0x01c2 | 0x01c4 => PeMachine::Arm,
        0xaa64 => PeMachine::Arm64,
        other => PeMachine::Unknown(other),
    })
}

fn invalid_pe(message: &str) -> IdentityEvidenceError {
    error(IdentityEvidenceErrorKind::InvalidPe, message)
}

fn error(kind: IdentityEvidenceErrorKind, message: impl Into<String>) -> IdentityEvidenceError {
    IdentityEvidenceError {
        kind,
        message: message.into(),
    }
}
