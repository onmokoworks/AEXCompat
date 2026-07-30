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
    Embedded,
    Catalog,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeModuleIdentityEvidence {
    pub canonical_path: PathBuf,
    pub size: u64,
    pub sha256: [u8; 32],
    pub pe_machine: PeMachine,
    pub file_identity: FileIdentity,
    pub authenticode: AuthenticodeEvidence,
    /// SHA-256 of the catalog that authoritatively contains this module.
    /// Embedded signatures and non-unique/incomplete catalog enumeration have
    /// no driver-package binding evidence.
    pub signing_catalog_sha256: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityEvidenceErrorKind {
    InvalidPath,
    UnsafeFile,
    InvalidPe,
    Unsupported,
    UntrustedSignature,
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

pub fn require_verified_authenticode(
    evidence: &RuntimeModuleIdentityEvidence,
) -> Result<(), IdentityEvidenceError> {
    match evidence.authenticode {
        AuthenticodeEvidence::Embedded | AuthenticodeEvidence::Catalog => Ok(()),
    }
}

#[cfg(windows)]
fn capture_impl(path: &Path) -> Result<RuntimeModuleIdentityEvidence, IdentityEvidenceError> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, GetFileInformationByHandle,
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

    verify_open_file_identity(path, &canonical_path, &file, &info)?;
    let (authenticode, signing_catalog_sha256) = verify_authenticode(&file, &canonical_path)?;
    verify_open_file_identity(path, &canonical_path, &file, &info)?;
    Ok(RuntimeModuleIdentityEvidence {
        canonical_path,
        size,
        sha256,
        pe_machine,
        file_identity: FileIdentity {
            volume_serial_number: info.dwVolumeSerialNumber,
            file_index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        },
        authenticode,
        signing_catalog_sha256,
    })
}

#[cfg(windows)]
fn verify_open_file_identity(
    input_path: &Path,
    canonical_path: &Path,
    file: &File,
    original: &windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION,
) -> Result<(), IdentityEvidenceError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut current: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut current) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    if fs::canonicalize(input_path)? != canonical_path
        || current.dwVolumeSerialNumber != original.dwVolumeSerialNumber
        || current.nFileIndexHigh != original.nFileIndexHigh
        || current.nFileIndexLow != original.nFileIndexLow
        || current.nFileSizeHigh != original.nFileSizeHigh
        || current.nFileSizeLow != original.nFileSizeLow
    {
        return Err(error(
            IdentityEvidenceErrorKind::UnsafeFile,
            "runtime module path or open file identity changed during evidence capture",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn verify_authenticode(
    file: &File,
    canonical_path: &Path,
) -> Result<(AuthenticodeEvidence, Option<[u8; 32]>), IdentityEvidenceError> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Security::Cryptography::Catalog::{
        CATALOG_INFO, CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2,
        CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
        CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext,
    };
    use windows_sys::Win32::Security::WinTrust::{
        WINTRUST_CATALOG_INFO, WINTRUST_DATA_0, WINTRUST_FILE_INFO, WTD_CHOICE_CATALOG,
        WTD_CHOICE_FILE,
    };

    let path_wide = wide_null(canonical_path.as_os_str());
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path_wide.as_ptr(),
        hFile: file.as_raw_handle() as _,
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut embedded_data = trust_data(WTD_CHOICE_FILE);
    embedded_data.Anonymous = WINTRUST_DATA_0 {
        pFile: &mut file_info,
    };
    if verify_and_close(&mut embedded_data) == 0 {
        return Ok((AuthenticodeEvidence::Embedded, None));
    }

    struct CatalogAdmin(isize);
    impl Drop for CatalogAdmin {
        fn drop(&mut self) {
            unsafe { CryptCATAdminReleaseContext(self.0, 0) };
        }
    }
    struct CatalogEnumeration {
        admin: isize,
        current: isize,
    }
    impl CatalogEnumeration {
        fn advance(&mut self, hash: &[u8]) {
            let mut previous = std::mem::replace(&mut self.current, 0);
            self.current = unsafe {
                CryptCATAdminEnumCatalogFromHash(
                    self.admin,
                    hash.as_ptr(),
                    hash.len() as u32,
                    0,
                    &mut previous,
                )
            };
        }
    }
    impl Drop for CatalogEnumeration {
        fn drop(&mut self) {
            if self.current != 0 {
                unsafe { CryptCATAdminReleaseCatalogContext(self.admin, self.current, 0) };
            }
        }
    }

    let mut admin = 0isize;
    if unsafe {
        CryptCATAdminAcquireContext2(
            &mut admin,
            std::ptr::null(),
            windows_sys::core::w!("SHA256"),
            std::ptr::null(),
            0,
        )
    } == 0
    {
        return Err(untrusted("failed to acquire the Windows catalog context"));
    }
    let admin = CatalogAdmin(admin);
    let mut hash_len = 0u32;
    let handle = file.as_raw_handle() as _;
    if unsafe {
        CryptCATAdminCalcHashFromFileHandle2(
            admin.0,
            handle,
            &mut hash_len,
            std::ptr::null_mut(),
            0,
        )
    } == 0
        || hash_len == 0
    {
        return Err(untrusted("failed to size the catalog member hash"));
    }
    let mut hash = vec![0u8; hash_len as usize];
    if unsafe {
        CryptCATAdminCalcHashFromFileHandle2(admin.0, handle, &mut hash_len, hash.as_mut_ptr(), 0)
    } == 0
    {
        return Err(untrusted("failed to calculate the catalog member hash"));
    }
    hash.truncate(hash_len as usize);
    let first_catalog = unsafe {
        CryptCATAdminEnumCatalogFromHash(admin.0, hash.as_ptr(), hash_len, 0, std::ptr::null_mut())
    };
    if first_catalog == 0 {
        return Err(untrusted(
            "no verified embedded or catalog signature was found",
        ));
    }
    let mut catalogs = CatalogEnumeration {
        admin: admin.0,
        current: first_catalog,
    };
    let member_tag = hash
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let member_tag_wide: Vec<u16> = member_tag.encode_utf16().chain(Some(0)).collect();
    let mut verified_catalogs = Vec::new();
    let mut incomplete = false;
    let mut visited_catalogs = 0usize;
    const MAX_CATALOG_MEMBERSHIPS: usize = 256;
    while catalogs.current != 0 {
        if visited_catalogs >= MAX_CATALOG_MEMBERSHIPS {
            incomplete = true;
            break;
        }
        visited_catalogs += 1;
        let mut catalog_path: CATALOG_INFO = unsafe { std::mem::zeroed() };
        catalog_path.cbStruct = std::mem::size_of::<CATALOG_INFO>() as u32;
        if unsafe { CryptCATCatalogInfoFromContext(catalogs.current, &mut catalog_path, 0) } == 0 {
            incomplete = true;
            catalogs.advance(&hash);
            continue;
        }
        let mut catalog_info = WINTRUST_CATALOG_INFO {
            cbStruct: std::mem::size_of::<WINTRUST_CATALOG_INFO>() as u32,
            dwCatalogVersion: 0,
            pcwszCatalogFilePath: catalog_path.wszCatalogFile.as_ptr(),
            pcwszMemberTag: member_tag_wide.as_ptr(),
            pcwszMemberFilePath: path_wide.as_ptr(),
            hMemberFile: handle,
            pbCalculatedFileHash: hash.as_mut_ptr(),
            cbCalculatedFileHash: hash_len,
            pcCatalogContext: std::ptr::null_mut(),
            hCatAdmin: admin.0,
        };
        let mut catalog_data = trust_data(WTD_CHOICE_CATALOG);
        catalog_data.Anonymous = WINTRUST_DATA_0 {
            pCatalog: &mut catalog_info,
        };
        if verify_and_close(&mut catalog_data) == 0 {
            let catalog_path = PathBuf::from(std::ffi::OsString::from_wide(nul_slice(
                &catalog_path.wszCatalogFile,
            )));
            verified_catalogs.push(hash_regular_non_reparse_file(&catalog_path)?);
        } else {
            incomplete = true;
        }
        catalogs.advance(&hash);
    }
    match verified_catalogs.as_slice() {
        [] => Err(untrusted("catalog member signature verification failed")),
        [catalog_sha256] if !incomplete => {
            Ok((AuthenticodeEvidence::Catalog, Some(*catalog_sha256)))
        }
        _ => Ok((AuthenticodeEvidence::Catalog, None)),
    }
}

#[cfg(windows)]
fn hash_regular_non_reparse_file(path: &Path) -> Result<[u8; 32], IdentityEvidenceError> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    validate_input_path(path)?;
    reject_reparse_components(path)?;
    let canonical = fs::canonicalize(path)?;
    reject_reparse_components(&canonical)?;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(canonical)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(error(
            IdentityEvidenceErrorKind::UnsafeFile,
            "signing catalog must be a regular non-reparse file",
        ));
    }
    let mut hash = Sha256::new();
    io::copy(&mut file, &mut hash)?;
    Ok(hash.finalize().into())
}

#[cfg(windows)]
fn trust_data(choice: u32) -> windows_sys::Win32::Security::WinTrust::WINTRUST_DATA {
    use windows_sys::Win32::Security::WinTrust::*;
    WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: std::ptr::null_mut(),
        pSIPClientData: std::ptr::null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: choice,
        Anonymous: WINTRUST_DATA_0 {
            pFile: std::ptr::null_mut(),
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: std::ptr::null_mut(),
        pwszURLReference: std::ptr::null_mut(),
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
        dwUIContext: WTD_UICONTEXT_EXECUTE,
        pSignatureSettings: std::ptr::null_mut(),
    }
}

#[cfg(windows)]
fn verify_and_close(data: &mut windows_sys::Win32::Security::WinTrust::WINTRUST_DATA) -> i32 {
    use windows_sys::Win32::Security::WinTrust::{
        WINTRUST_ACTION_GENERIC_VERIFY_V2, WTD_STATEACTION_CLOSE, WinVerifyTrust,
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, data as *mut _ as _) };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe { WinVerifyTrust(std::ptr::null_mut(), &mut action, data as *mut _ as _) };
    status
}

#[cfg(windows)]
fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn nul_slice(value: &[u16]) -> &[u16] {
    &value[..value
        .iter()
        .position(|word| *word == 0)
        .unwrap_or(value.len())]
}

fn untrusted(message: &str) -> IdentityEvidenceError {
    error(IdentityEvidenceErrorKind::UntrustedSignature, message)
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
