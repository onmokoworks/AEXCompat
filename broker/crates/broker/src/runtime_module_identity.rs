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

const CATALOG_ENUMERATION_SUCCESS: u32 = 0;
const CATALOG_ENUMERATION_EXHAUSTED: u32 = 1168; // ERROR_NOT_FOUND

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawCatalogEnumeration<H> {
    next: Option<H>,
    previous_retained: Option<H>,
    last_error: u32,
}

trait CatalogEnumerationBackend {
    type Handle: Copy + Eq;

    fn enumerate(
        &mut self,
        previous: Option<Self::Handle>,
        member_hash: &[u8],
    ) -> RawCatalogEnumeration<Self::Handle>;
    fn release(&mut self, handle: Self::Handle);
}

struct CatalogEnumeration<B: CatalogEnumerationBackend> {
    backend: B,
    current: Option<B::Handle>,
}

impl<B: CatalogEnumerationBackend> CatalogEnumeration<B> {
    fn new(backend: B) -> Self {
        Self {
            backend,
            current: None,
        }
    }

    fn advance(&mut self, member_hash: &[u8]) -> Result<Option<B::Handle>, u32> {
        let previous = self.current.take();
        let outcome = self.backend.enumerate(previous, member_hash);
        if let Some(retained) = outcome.previous_retained {
            self.backend.release(retained);
        }
        self.current = outcome.next;
        if self.current.is_some() {
            return Ok(self.current);
        }
        if matches!(
            outcome.last_error,
            CATALOG_ENUMERATION_SUCCESS | CATALOG_ENUMERATION_EXHAUSTED
        ) {
            Ok(None)
        } else {
            Err(outcome.last_error)
        }
    }
}

impl<B: CatalogEnumerationBackend> Drop for CatalogEnumeration<B> {
    fn drop(&mut self) {
        if let Some(current) = self.current.take() {
            self.backend.release(current);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StableCatalogIdentity {
    volume_serial_number: u32,
    file_index: u64,
    size: u64,
    last_write_time: u64,
}

fn require_stable_catalog_evidence(
    expected: StableCatalogIdentity,
    observed: StableCatalogIdentity,
    digest_before_trust: [u8; 32],
    digest_after_trust: [u8; 32],
) -> Result<(), IdentityEvidenceError> {
    if expected != observed || digest_before_trust != digest_after_trust {
        return Err(error(
            IdentityEvidenceErrorKind::UnsafeFile,
            "signing catalog identity or content changed during trust verification",
        ));
    }
    Ok(())
}

fn unique_verified_catalog_digest(
    verified_catalogs: &[[u8; 32]],
    enumeration_incomplete: bool,
) -> Option<[u8; 32]> {
    match verified_catalogs {
        [catalog_sha256] if !enumeration_incomplete => Some(*catalog_sha256),
        _ => None,
    }
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
struct WindowsCatalogEnumerationBackend {
    admin: isize,
}

#[cfg(windows)]
impl CatalogEnumerationBackend for WindowsCatalogEnumerationBackend {
    type Handle = isize;

    fn enumerate(
        &mut self,
        previous: Option<Self::Handle>,
        member_hash: &[u8],
    ) -> RawCatalogEnumeration<Self::Handle> {
        use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
        use windows_sys::Win32::Security::Cryptography::Catalog::CryptCATAdminEnumCatalogFromHash;

        let mut previous_raw = previous.unwrap_or(0);
        unsafe { SetLastError(CATALOG_ENUMERATION_SUCCESS) };
        let next = unsafe {
            CryptCATAdminEnumCatalogFromHash(
                self.admin,
                member_hash.as_ptr(),
                member_hash.len() as u32,
                0,
                if previous.is_some() {
                    &mut previous_raw
                } else {
                    std::ptr::null_mut()
                },
            )
        };
        let last_error = unsafe { GetLastError() };
        RawCatalogEnumeration {
            next: (next != 0).then_some(next),
            previous_retained: (previous_raw != 0).then_some(previous_raw),
            last_error,
        }
    }

    fn release(&mut self, handle: Self::Handle) {
        use windows_sys::Win32::Security::Cryptography::Catalog::CryptCATAdminReleaseCatalogContext;
        unsafe {
            CryptCATAdminReleaseCatalogContext(self.admin, handle, 0);
        }
    }
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
    let mut catalogs = CatalogEnumeration::new(WindowsCatalogEnumerationBackend { admin: admin.0 });
    let mut current_catalog = catalogs.advance(&hash).map_err(|last_error| {
        error(
            IdentityEvidenceErrorKind::UntrustedSignature,
            format!("catalog enumeration failed with Win32 error {last_error}"),
        )
    })?;
    if current_catalog.is_none() {
        return Err(untrusted(
            "no verified embedded or catalog signature was found",
        ));
    }
    let member_tag = hash
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let member_tag_wide: Vec<u16> = member_tag.encode_utf16().chain(Some(0)).collect();
    let mut verified_catalogs = Vec::new();
    let mut incomplete = false;
    let mut visited_catalogs = 0usize;
    const MAX_CATALOG_MEMBERSHIPS: usize = 256;
    while let Some(catalog_handle) = current_catalog {
        if visited_catalogs >= MAX_CATALOG_MEMBERSHIPS {
            incomplete = true;
            break;
        }
        visited_catalogs += 1;
        let mut catalog_path: CATALOG_INFO = unsafe { std::mem::zeroed() };
        catalog_path.cbStruct = std::mem::size_of::<CATALOG_INFO>() as u32;
        if unsafe { CryptCATCatalogInfoFromContext(catalog_handle, &mut catalog_path, 0) } == 0 {
            incomplete = true;
            current_catalog = match catalogs.advance(&hash) {
                Ok(next) => next,
                Err(_) => {
                    incomplete = true;
                    None
                }
            };
            continue;
        }
        let catalog_path = PathBuf::from(std::ffi::OsString::from_wide(nul_slice(
            &catalog_path.wszCatalogFile,
        )));
        match verified_catalog_digest(&catalog_path, |trusted_catalog_path| {
            let trusted_catalog_path_wide = wide_null(trusted_catalog_path.as_os_str());
            let mut catalog_info = WINTRUST_CATALOG_INFO {
                cbStruct: std::mem::size_of::<WINTRUST_CATALOG_INFO>() as u32,
                dwCatalogVersion: 0,
                pcwszCatalogFilePath: trusted_catalog_path_wide.as_ptr(),
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
            verify_and_close(&mut catalog_data) == 0
        })? {
            Some(digest) => verified_catalogs.push(digest),
            None => incomplete = true,
        }
        current_catalog = match catalogs.advance(&hash) {
            Ok(next) => next,
            Err(_) => {
                incomplete = true;
                None
            }
        };
    }
    if verified_catalogs.is_empty() {
        Err(untrusted("catalog member signature verification failed"))
    } else {
        Ok((
            AuthenticodeEvidence::Catalog,
            unique_verified_catalog_digest(&verified_catalogs, incomplete),
        ))
    }
}

#[cfg(windows)]
struct OpenedCatalog {
    canonical_path: PathBuf,
    file: File,
    opened_identity: StableCatalogIdentity,
}

#[cfg(windows)]
impl OpenedCatalog {
    fn open(path: &Path) -> Result<Self, IdentityEvidenceError> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };

        validate_input_path(path)?;
        reject_reparse_components(path)?;
        let canonical_path = fs::canonicalize(path)?;
        reject_reparse_components(&canonical_path)?;
        let file = OpenOptions::new()
            .read(true)
            // Excluding write/delete sharing prevents path replacement while
            // WinVerifyTrust opens the same catalog by canonical path.
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&canonical_path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(error(
                IdentityEvidenceErrorKind::UnsafeFile,
                "signing catalog must be a regular non-reparse file",
            ));
        }
        let opened_identity = observe_catalog_identity(&file)?;
        Ok(Self {
            canonical_path,
            file,
            opened_identity,
        })
    }

    fn hash_and_observe(
        &mut self,
        source_path: &Path,
    ) -> Result<([u8; 32], StableCatalogIdentity), IdentityEvidenceError> {
        self.verify_path(source_path)?;
        let before = observe_catalog_identity(&self.file)?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        io::copy(&mut self.file, &mut hash)?;
        let after = observe_catalog_identity(&self.file)?;
        require_stable_catalog_evidence(before, after, [0; 32], [0; 32])?;
        Ok((hash.finalize().into(), after))
    }

    fn verify_path(&self, source_path: &Path) -> Result<(), IdentityEvidenceError> {
        validate_input_path(source_path)?;
        reject_reparse_components(source_path)?;
        if fs::canonicalize(source_path)? != self.canonical_path {
            return Err(error(
                IdentityEvidenceErrorKind::UnsafeFile,
                "signing catalog path changed during trust verification",
            ));
        }
        reject_reparse_components(&self.canonical_path)?;
        Ok(())
    }
}

#[cfg(windows)]
fn observe_catalog_identity(file: &File) -> Result<StableCatalogIdentity, IdentityEvidenceError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(StableCatalogIdentity {
        volume_serial_number: info.dwVolumeSerialNumber,
        file_index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        size: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        last_write_time: (u64::from(info.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(info.ftLastWriteTime.dwLowDateTime),
    })
}

#[cfg(windows)]
fn verified_catalog_digest(
    catalog_path: &Path,
    verify: impl FnOnce(&Path) -> bool,
) -> Result<Option<[u8; 32]>, IdentityEvidenceError> {
    let mut catalog = OpenedCatalog::open(catalog_path)?;
    let (digest_before, identity_before) = catalog.hash_and_observe(catalog_path)?;
    require_stable_catalog_evidence(
        catalog.opened_identity,
        identity_before,
        digest_before,
        digest_before,
    )?;
    if !verify(&catalog.canonical_path) {
        return Ok(None);
    }
    let (digest_after, identity_after) = catalog.hash_and_observe(catalog_path)?;
    require_stable_catalog_evidence(identity_before, identity_after, digest_before, digest_after)?;
    Ok(Some(digest_after))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    #[derive(Default)]
    struct MockCatalogState {
        releases: Vec<u32>,
        consumed_by_enumeration: Vec<u32>,
    }

    struct MockCatalogBackend {
        outcomes: VecDeque<(Option<u32>, RawCatalogEnumeration<u32>)>,
        state: Rc<RefCell<MockCatalogState>>,
    }

    impl CatalogEnumerationBackend for MockCatalogBackend {
        type Handle = u32;

        fn enumerate(
            &mut self,
            previous: Option<Self::Handle>,
            _: &[u8],
        ) -> RawCatalogEnumeration<Self::Handle> {
            let (expected_previous, outcome) =
                self.outcomes.pop_front().expect("unexpected enumeration");
            assert_eq!(previous, expected_previous);
            if let Some(previous) = previous {
                if outcome.previous_retained.is_none() {
                    self.state
                        .borrow_mut()
                        .consumed_by_enumeration
                        .push(previous);
                }
            }
            outcome
        }

        fn release(&mut self, handle: Self::Handle) {
            self.state.borrow_mut().releases.push(handle);
        }
    }

    fn mock_catalogs(
        outcomes: impl IntoIterator<Item = (Option<u32>, RawCatalogEnumeration<u32>)>,
    ) -> (
        CatalogEnumeration<MockCatalogBackend>,
        Rc<RefCell<MockCatalogState>>,
    ) {
        let state = Rc::new(RefCell::new(MockCatalogState::default()));
        (
            CatalogEnumeration::new(MockCatalogBackend {
                outcomes: outcomes.into_iter().collect(),
                state: state.clone(),
            }),
            state,
        )
    }

    #[test]
    fn catalog_enumeration_error_after_one_success_is_not_normal_completion() {
        let (mut catalogs, state) = mock_catalogs([
            (
                None,
                RawCatalogEnumeration {
                    next: Some(11),
                    previous_retained: None,
                    last_error: CATALOG_ENUMERATION_SUCCESS,
                },
            ),
            (
                Some(11),
                RawCatalogEnumeration {
                    next: None,
                    previous_retained: Some(11),
                    last_error: 5,
                },
            ),
        ]);
        assert_eq!(catalogs.advance(&[1, 2]), Ok(Some(11)));
        assert_eq!(catalogs.advance(&[1, 2]), Err(5));
        assert_eq!(unique_verified_catalog_digest(&[[9; 32]], true), None);
        drop(catalogs);
        assert_eq!(state.borrow().releases, vec![11]);
        assert!(state.borrow().consumed_by_enumeration.is_empty());
    }

    #[test]
    fn catalog_contexts_are_released_exactly_once_on_end_advance_and_drop() {
        let (mut normal, normal_state) = mock_catalogs([
            (
                None,
                RawCatalogEnumeration {
                    next: Some(21),
                    previous_retained: None,
                    last_error: CATALOG_ENUMERATION_SUCCESS,
                },
            ),
            (
                Some(21),
                RawCatalogEnumeration {
                    next: None,
                    previous_retained: Some(21),
                    last_error: CATALOG_ENUMERATION_EXHAUSTED,
                },
            ),
        ]);
        assert_eq!(normal.advance(&[]), Ok(Some(21)));
        assert_eq!(normal.advance(&[]), Ok(None));
        drop(normal);
        assert_eq!(normal_state.borrow().releases, vec![21]);

        let (mut advancing, advancing_state) = mock_catalogs([
            (
                None,
                RawCatalogEnumeration {
                    next: Some(31),
                    previous_retained: None,
                    last_error: CATALOG_ENUMERATION_SUCCESS,
                },
            ),
            (
                Some(31),
                RawCatalogEnumeration {
                    next: Some(32),
                    previous_retained: None,
                    last_error: CATALOG_ENUMERATION_SUCCESS,
                },
            ),
        ]);
        assert_eq!(advancing.advance(&[]), Ok(Some(31)));
        assert_eq!(advancing.advance(&[]), Ok(Some(32)));
        drop(advancing);
        assert_eq!(advancing_state.borrow().consumed_by_enumeration, vec![31]);
        assert_eq!(advancing_state.borrow().releases, vec![32]);

        let (mut early, early_state) = mock_catalogs([(
            None,
            RawCatalogEnumeration {
                next: Some(41),
                previous_retained: None,
                last_error: CATALOG_ENUMERATION_SUCCESS,
            },
        )]);
        assert_eq!(early.advance(&[]), Ok(Some(41)));
        drop(early);
        assert_eq!(early_state.borrow().releases, vec![41]);
    }

    #[test]
    fn catalog_identity_or_content_mismatch_is_fail_closed() {
        let identity = StableCatalogIdentity {
            volume_serial_number: 1,
            file_index: 2,
            size: 3,
            last_write_time: 4,
        };
        require_stable_catalog_evidence(identity, identity, [5; 32], [5; 32]).unwrap();

        let mut changed_identity = identity;
        changed_identity.file_index += 1;
        assert_eq!(
            require_stable_catalog_evidence(identity, changed_identity, [5; 32], [5; 32])
                .unwrap_err()
                .kind,
            IdentityEvidenceErrorKind::UnsafeFile
        );
        assert_eq!(
            require_stable_catalog_evidence(identity, identity, [5; 32], [6; 32])
                .unwrap_err()
                .kind,
            IdentityEvidenceErrorKind::UnsafeFile
        );
    }

    #[cfg(windows)]
    #[test]
    fn stable_catalog_handle_blocks_replacement_during_trust_callback() {
        use sha2::{Digest, Sha256};
        use std::cell::Cell;
        use std::fs;

        let path = std::env::temp_dir().join(format!(
            "aexcompat-stable-catalog-{}-{}.cat",
            std::process::id(),
            rand::random::<u64>()
        ));
        let contents = b"signed catalog fixture";
        fs::write(&path, contents).unwrap();
        let replacement_succeeded = Cell::new(false);
        let digest = verified_catalog_digest(&path, |trusted_path| {
            replacement_succeeded.set(fs::write(trusted_path, b"replacement").is_ok());
            true
        })
        .unwrap()
        .unwrap();
        fs::remove_file(path).unwrap();

        assert!(!replacement_succeeded.get());
        assert_eq!(digest.as_slice(), Sha256::digest(contents).as_slice());
    }
}
