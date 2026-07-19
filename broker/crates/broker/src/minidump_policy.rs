use std::env;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub const MINIDUMP_DIR_ENV: &str = "AEXCOMPAT_MINIDUMP_DIR";
pub const MINIDUMP_HANDLE_ENV: &str = "AEXCOMPAT_MINIDUMP_HANDLE";
pub const MINIDUMP_ACK_HANDLE_ENV: &str = "AEXCOMPAT_MINIDUMP_ACK_HANDLE";
pub const MAX_MINIDUMP_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_MINIDUMP_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_MINIDUMP_FILES: u64 = 16;
pub const MINIDUMP_POLICY_LOCK_TIMEOUT_MS: u64 = 5_000;

struct MinidumpDirectory {
    path: PathBuf,
    display: String,
    #[cfg(windows)]
    guard: DirectoryGuard,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
}

#[cfg(not(windows))]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn reject_reparse(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || is_reparse(&metadata) {
        return Err(invalid("minidump directory contains a reparse point"));
    }
    Ok(())
}

fn reject_dot_components(path: &Path) -> io::Result<()> {
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid("minidump directory traversal is forbidden"));
    }
    Ok(())
}

fn resolve_directory(repository: &Path, requested: &Path) -> io::Result<MinidumpDirectory> {
    if requested.as_os_str().is_empty() {
        return Err(invalid("minidump directory must not be empty"));
    }
    reject_dot_components(requested)?;
    let repository_root = fs::canonicalize(repository)?;
    let target_root = repository_root.join("target");
    let resolved = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        repository_root.join(requested)
    };
    if !resolved.starts_with(&target_root) {
        return Err(invalid(
            "minidump directory must stay under repository target",
        ));
    }
    fs::create_dir_all(&target_root)?;
    reject_reparse(&target_root)?;
    let relative_requested = resolved
        .strip_prefix(&target_root)
        .map_err(|_| invalid("minidump directory root mismatch"))?;
    let mut existing = target_root.clone();
    for component in relative_requested.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("minidump directory has an invalid component"));
        };
        existing.push(name);
        if existing.exists() {
            reject_reparse(&existing)?;
        } else {
            break;
        }
    }
    fs::create_dir_all(&resolved)?;
    let canonical = fs::canonicalize(&resolved)?;
    let canonical_target = fs::canonicalize(&target_root)?;
    reject_reparse(&canonical_target)?;
    if !canonical.starts_with(&canonical_target) {
        return Err(invalid(
            "minidump directory must stay under repository target",
        ));
    }
    let mut current = canonical_target.clone();
    let relative = canonical
        .strip_prefix(&canonical_target)
        .map_err(|_| invalid("minidump directory root mismatch"))?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("minidump directory has an invalid component"));
        };
        current.push(name);
        reject_reparse(&current)?;
    }
    let guard = harden_directory_acl(&canonical)?;
    let display = canonical
        .strip_prefix(canonical_target.parent().unwrap_or(&canonical_target))
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "target/(crash-dumps)".into());
    Ok(MinidumpDirectory {
        path: canonical,
        display,
        #[cfg(windows)]
        guard,
    })
}

#[cfg(windows)]
struct DirectoryGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl DirectoryGuard {
    fn try_clone(&self) -> io::Result<Self> {
        use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let process = unsafe { GetCurrentProcess() };
        let mut duplicate: HANDLE = std::ptr::null_mut();
        if unsafe {
            DuplicateHandle(
                process,
                self.0,
                process,
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(duplicate))
    }
}

#[cfg(windows)]
impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[cfg(windows)]
fn harden_directory_acl(directory: &Path) -> io::Result<DirectoryGuard> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::{
        SetKernelObjectSecurity, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
    };

    let wide: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let opened = final_path(handle)?;
        if !strip_extended_prefix(&opened)
            .as_os_str()
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(
                strip_extended_prefix(directory)
                    .as_os_str()
                    .to_string_lossy()
                    .trim_end_matches(['\\', '/']),
            )
        {
            return Err(invalid("minidump directory handle identity mismatch"));
        }
        let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0
            || information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(invalid(
                "minidump directory handle is not a plain directory",
            ));
        }
        let (_, descriptor) = protected_file_security()?;
        if unsafe {
            SetKernelObjectSecurity(
                handle,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor.0,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(DirectoryGuard(handle))
    })();
    if result.is_err() {
        unsafe { CloseHandle(handle) };
    }
    result
}

#[cfg(not(windows))]
fn harden_directory_acl(_directory: &Path) -> io::Result<()> {
    Ok(())
}

fn configured_directory(repository: &Path) -> io::Result<Option<MinidumpDirectory>> {
    env::var_os(MINIDUMP_DIR_ENV)
        .map(|value| resolve_directory(repository, Path::new(value.as_os_str())))
        .transpose()
}

pub(crate) fn configured_directory_display(repository: &Path) -> io::Result<Option<String>> {
    configured_directory(repository).map(|directory| directory.map(|value| value.display))
}

#[cfg(windows)]
pub(crate) struct MinidumpLaunchFile {
    dump_handle: windows_sys::Win32::Foundation::HANDLE,
    worker_handle: windows_sys::Win32::Foundation::HANDLE,
    ack_handle: windows_sys::Win32::Foundation::HANDLE,
    final_path: PathBuf,
    _directory_guard: DirectoryGuard,
    cancel_reader: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reader: Option<std::thread::JoinHandle<MinidumpCapture>>,
}

#[cfg(windows)]
impl MinidumpLaunchFile {
    pub(crate) fn raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.worker_handle
    }

    pub(crate) fn ack_raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.ack_handle
    }

    pub(crate) fn close_worker_handles(&mut self) {
        unsafe {
            if !self.worker_handle.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.worker_handle);
            }
            if !self.ack_handle.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.ack_handle);
            }
        }
        self.worker_handle = std::ptr::null_mut();
        self.ack_handle = std::ptr::null_mut();
    }
}

#[cfg(windows)]
impl Drop for MinidumpLaunchFile {
    fn drop(&mut self) {
        self.close_worker_handles();
        // The reader uses PeekNamedPipe rather than a blocking ReadFile, so a
        // retained/duplicated writer cannot hold this join open indefinitely.
        // It drains bytes already buffered before honoring cancellation.
        self.cancel_reader
            .store(true, std::sync::atomic::Ordering::Release);
        let capture = self
            .reader
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default();
        let publish = capture.bytes > 0 && !capture.overflow && !capture.write_failed;
        let published = publish
            && rename_file_by_handle(self.dump_handle, &self.final_path).is_ok()
            && authenticate_file_handle(
                self.dump_handle,
                self.final_path.parent().unwrap_or(Path::new(".")),
                false,
            )
            .is_ok();
        if !published {
            // Delete the object through the still-authenticated handle. The
            // handle points at either the reservation or its renamed final
            // name, so no close/path-delete substitution window exists.
            let _ = mark_delete_by_handle(self.dump_handle);
        }
        unsafe {
            if !self.dump_handle.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.dump_handle);
            }
        }
    }
}

#[cfg(windows)]
#[derive(Default)]
struct MinidumpCapture {
    bytes: u64,
    overflow: bool,
    write_failed: bool,
}

#[cfg(windows)]
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

#[cfg(windows)]
fn final_path(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;

    let needed = unsafe { GetFinalPathNameByHandleW(handle, std::ptr::null_mut(), 0, 0) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u16; needed as usize + 1];
    let written =
        unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if written == 0 || written as usize >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(strip_extended_prefix(Path::new(
        &std::ffi::OsString::from_wide(&buffer[..written as usize]),
    )))
}

#[cfg(windows)]
fn authenticate_file_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
    directory: &Path,
    require_inherit: bool,
) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
    };

    let final_path = final_path(handle)?;
    let final_parent = final_path
        .parent()
        .ok_or_else(|| invalid("minidump file final path has no parent"))?;
    if !strip_extended_prefix(final_parent)
        .as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&strip_extended_prefix(directory).to_string_lossy())
    {
        return Err(invalid("minidump file escaped broker-owned directory"));
    }
    let mut flags = 0;
    if unsafe { GetHandleInformation(handle, &mut flags) } == 0
        || (require_inherit && flags & HANDLE_FLAG_INHERIT == 0)
    {
        return Err(invalid(
            "minidump file handle is not authenticated/inheritable",
        ));
    }
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || information.nNumberOfLinks != 1
    {
        return Err(invalid(
            "minidump file handle is reparse-backed or multiply linked",
        ));
    }
    Ok(())
}

#[cfg(windows)]
struct ProtectedSecurityDescriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

#[cfg(windows)]
impl Drop for ProtectedSecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0.cast());
        }
    }
}

#[cfg(windows)]
fn protected_file_security() -> io::Result<(
    windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    ProtectedSecurityDescriptor,
)> {
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };

    // Protected DACL: full access only for LocalSystem and the object owner.
    // Crash dumps can contain arbitrary process memory and must not inherit a
    // repository ACL that grants read access to other local users.
    let sddl: Vec<u16> = "D:P(A;;FA;;;SY)(A;;FA;;;OW)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let guard = ProtectedSecurityDescriptor(descriptor);
    let attributes = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    Ok((attributes, guard))
}

#[cfg(windows)]
fn copy_minidump_pipe(
    read_value: usize,
    dump_value: usize,
    ack_value: usize,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> MinidumpCapture {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    let read_handle = read_value as HANDLE;
    let dump_handle = dump_value as HANDLE;
    let ack_handle = ack_value as HANDLE;
    let mut capture = MinidumpCapture::default();
    loop {
        let mut available = 0u32;
        let peeked = unsafe {
            PeekNamedPipe(
                read_handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if peeked == 0 {
            break;
        }
        if available == 0 {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }
        let mut buffer = [0u8; 64 * 1024];
        let mut read = 0u32;
        let requested = available.min(buffer.len() as u32);
        let ok = unsafe {
            ReadFile(
                read_handle,
                buffer.as_mut_ptr(),
                requested,
                &mut read,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || read == 0 {
            break;
        }
        let count = read as usize;
        if capture.bytes > MAX_MINIDUMP_FILE_BYTES - count as u64 {
            capture.overflow = true;
            // Closing the read side makes an over-budget producer fail fast;
            // draining attacker-controlled bytes would only burn broker CPU.
            break;
        }
        let mut offset = 0usize;
        while offset < count {
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    dump_handle,
                    buffer[offset..count].as_ptr(),
                    (count - offset) as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || written == 0 {
                capture.write_failed = true;
                break;
            }
            offset += written as usize;
        }
        if !capture.write_failed {
            capture.bytes += count as u64;
        }
    }
    let mut ack = [0u8; 9];
    ack[..8].copy_from_slice(&capture.bytes.to_le_bytes());
    ack[8] = u8::from(capture.overflow || capture.write_failed);
    let mut sent = 0u32;
    unsafe {
        WriteFile(
            ack_handle,
            ack.as_ptr(),
            ack.len() as u32,
            &mut sent,
            std::ptr::null_mut(),
        );
        CloseHandle(read_handle);
        CloseHandle(ack_handle);
    }
    capture
}

#[cfg(windows)]
fn rename_file_by_handle(
    handle: windows_sys::Win32::Foundation::HANDLE,
    destination: &Path,
) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO,
    };

    let name: Vec<u16> = destination.as_os_str().encode_wide().collect();
    let header = std::mem::size_of::<FILE_RENAME_INFO>();
    let bytes = header
        .checked_add(name.len().saturating_sub(1) * std::mem::size_of::<u16>())
        .ok_or_else(|| invalid("minidump final path is too long"))?;
    let mut storage = vec![0u8; bytes];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = 0;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength = (name.len() * std::mem::size_of::<u16>()) as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        if SetFileInformationByHandle(handle, FileRenameInfo, info.cast(), bytes as u32) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn mark_delete_by_handle(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
    };

    let disposition = FILE_DISPOSITION_INFO { DeleteFile: 1 };
    if unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
struct PolicyLock(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for PolicyLock {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[cfg(windows)]
fn acquire_policy_lock(directory: &Path) -> io::Result<PolicyLock> {
    acquire_policy_lock_with_timeout(
        directory,
        std::time::Duration::from_millis(MINIDUMP_POLICY_LOCK_TIMEOUT_MS),
    )
}

#[cfg(windows)]
fn acquire_policy_lock_with_timeout(
    directory: &Path,
    timeout: std::time::Duration,
) -> io::Result<PolicyLock> {
    use std::os::windows::ffi::OsStrExt;
    use std::thread::sleep;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_ATTRIBUTE_HIDDEN, FILE_FLAG_DELETE_ON_CLOSE,
        FILE_FLAG_OPEN_REPARSE_POINT, OPEN_ALWAYS,
    };

    let path = directory.join("aexcompat-minidump.lock");
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 0,
    };
    let deadline = Instant::now() + timeout;
    loop {
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE | DELETE,
                0,
                &security,
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_HIDDEN | FILE_FLAG_DELETE_ON_CLOSE | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            if let Err(error) = authenticate_file_handle(handle, directory, false) {
                unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
                return Err(error);
            }
            return Ok(PolicyLock(handle));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_SHARING_VIOLATION as i32) {
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "minidump policy lock acquisition timed out",
            ));
        }
        sleep(Duration::from_millis(25));
    }
}

fn enforce_budget(directory: &Path) -> io::Result<()> {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut in_flight = 0u64;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !metadata.is_file() || !name.starts_with("crash-") {
            continue;
        }
        if name.ends_with(".dmp") {
            files = files.saturating_add(1);
            bytes = bytes.saturating_add(metadata.len());
        } else if name.ends_with(".dmp.part") {
            files = files.saturating_add(1);
            in_flight = in_flight.saturating_add(1);
        }
    }
    if files >= MAX_MINIDUMP_FILES
        || bytes > MAX_MINIDUMP_TOTAL_BYTES
        || bytes
            .saturating_add(in_flight.saturating_mul(MAX_MINIDUMP_FILE_BYTES))
            .saturating_add(MAX_MINIDUMP_FILE_BYTES)
            > MAX_MINIDUMP_TOTAL_BYTES
    {
        return Err(invalid("minidump directory capacity policy exceeded"));
    }
    Ok(())
}

#[cfg(windows)]
fn remove_stale_reservations(directory: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_SHARING_VIOLATION, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, OPEN_EXISTING,
    };

    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("crash-") || !name.ends_with(".dmp.part") {
            continue;
        }
        let path = entry.path();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES | DELETE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) {
                // An active reservation is held with share mode zero. Leave
                // it in the in-flight budget.
                continue;
            }
            return Err(error);
        }
        let result = authenticate_file_handle(handle, directory, false)
            .and_then(|_| mark_delete_by_handle(handle));
        unsafe { CloseHandle(handle) };
        result?;
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn create_minidump_file_for_launch(
    repository: &Path,
) -> io::Result<Option<MinidumpLaunchFile>> {
    let Some(directory) = configured_directory(repository)? else {
        return Ok(None);
    };
    create_minidump_file_in_directory(&directory).map(Some)
}

#[cfg(windows)]
fn create_minidump_file_in_directory(
    directory: &MinidumpDirectory,
) -> io::Result<MinidumpLaunchFile> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, CREATE_NEW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;

    // This lock covers only the budget scan and CREATE_NEW reservation. The
    // reservation handle itself remains alive until the worker exits.
    let _policy_lock = acquire_policy_lock(&directory.path)?;
    let directory_guard = directory.guard.try_clone()?;
    remove_stale_reservations(&directory.path)?;
    if let Err(error) = enforce_budget(&directory.path) {
        return Err(error);
    }
    let reservation_path = directory
        .path
        .join(format!("crash-{:032x}.dmp.part", rand::random::<u128>()));
    let final_path = reservation_path.with_extension("dmp");
    let wide: Vec<u16> = reservation_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let (mut security, _security_descriptor) = protected_file_security()?;
    let dump_handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_WRITE | DELETE,
            0,
            &mut security,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if dump_handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    if let Err(error) = authenticate_file_handle(dump_handle, &directory.path, false) {
        let _ = mark_delete_by_handle(dump_handle);
        unsafe { CloseHandle(dump_handle) };
        return Err(error);
    }
    let mut dump_read: HANDLE = std::ptr::null_mut();
    let mut dump_write: HANDLE = std::ptr::null_mut();
    let mut ack_read: HANDLE = std::ptr::null_mut();
    let mut ack_write: HANDLE = std::ptr::null_mut();
    let pipe_security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let pipes_ok = unsafe {
        CreatePipe(&mut dump_read, &mut dump_write, &pipe_security, 0) != 0
            && CreatePipe(&mut ack_read, &mut ack_write, &pipe_security, 0) != 0
            && SetHandleInformation(dump_read, HANDLE_FLAG_INHERIT, 0) != 0
            && SetHandleInformation(ack_write, HANDLE_FLAG_INHERIT, 0) != 0
    };
    if !pipes_ok {
        let error = io::Error::last_os_error();
        let _ = mark_delete_by_handle(dump_handle);
        unsafe {
            if !dump_read.is_null() {
                CloseHandle(dump_read);
            }
            if !dump_write.is_null() {
                CloseHandle(dump_write);
            }
            if !ack_read.is_null() {
                CloseHandle(ack_read);
            }
            if !ack_write.is_null() {
                CloseHandle(ack_write);
            }
            CloseHandle(dump_handle);
        }
        return Err(error);
    }
    let read_value = dump_read as usize;
    let dump_value = dump_handle as usize;
    let ack_value = ack_write as usize;
    let cancel_reader = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader_cancel = std::sync::Arc::clone(&cancel_reader);
    let reader = std::thread::spawn(move || {
        copy_minidump_pipe(read_value, dump_value, ack_value, reader_cancel)
    });
    Ok(MinidumpLaunchFile {
        dump_handle,
        worker_handle: dump_write,
        ack_handle: ack_read,
        final_path,
        _directory_guard: directory_guard,
        cancel_reader,
        reader: Some(reader),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_repository() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let repository = std::env::temp_dir().join(format!("aexcompat-minidump-{nonce}"));
        fs::create_dir_all(repository.join("target")).expect("create test target");
        repository
    }

    #[test]
    fn directory_resolution_stays_under_target() {
        let repository = test_repository();
        let resolved = resolve_directory(&repository, Path::new("target/crash-dumps"))
            .expect("relative directory should resolve");
        assert!(resolved
            .path
            .starts_with(fs::canonicalize(repository.join("target")).expect("canonical target")));
        assert!(resolve_directory(&repository, Path::new("../outside")).is_err());
        assert!(resolve_directory(&repository, Path::new("target/../outside")).is_err());
        let _ = fs::remove_dir_all(repository);
    }

    #[test]
    fn budget_reserves_one_maximum_size_dump() {
        let repository = test_repository();
        let directory = repository.join("target/crash-dumps");
        fs::create_dir_all(&directory).expect("create dump directory");
        fs::write(directory.join("crash-existing.dmp"), [0u8; 1]).expect("write dump");
        enforce_budget(&directory).expect("one small dump should fit");
        fs::write(
            directory.join("crash-full.dmp"),
            vec![0u8; (MAX_MINIDUMP_TOTAL_BYTES - MAX_MINIDUMP_FILE_BYTES) as usize],
        )
        .expect("write near-cap dump");
        assert!(enforce_budget(&directory).is_err());
        let _ = fs::remove_dir_all(repository);
    }

    #[test]
    fn budget_rejects_too_many_dump_files() {
        let repository = test_repository();
        let directory = repository.join("target/crash-dumps");
        fs::create_dir_all(&directory).expect("create dump directory");
        for index in 0..MAX_MINIDUMP_FILES {
            fs::write(directory.join(format!("crash-{index}.dmp")), [0u8; 1]).expect("write dump");
        }
        assert!(enforce_budget(&directory).is_err());
        let _ = fs::remove_dir_all(repository);
    }

    #[test]
    fn budget_reserves_max_bytes_for_in_flight_parts() {
        let repository = test_repository();
        let directory = repository.join("target/crash-dumps");
        fs::create_dir_all(&directory).expect("create dump directory");
        for index in 0..3 {
            fs::write(directory.join(format!("crash-{index}.dmp.part")), [])
                .expect("write reservation");
        }
        enforce_budget(&directory).expect("three maximum reservations should fit");
        fs::write(directory.join("crash-3.dmp.part"), []).expect("write reservation");
        assert!(enforce_budget(&directory).is_err());
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    fn test_windows_directory() -> (PathBuf, MinidumpDirectory) {
        let repository = test_repository();
        let path = repository.join("target/crash-dumps");
        fs::create_dir_all(&path).expect("create dump directory");
        let path = fs::canonicalize(path).expect("canonical dump directory");
        let display = path.to_string_lossy().into_owned();
        let guard = harden_directory_acl(&path).expect("harden dump directory");
        (
            repository,
            MinidumpDirectory {
                path,
                display,
                guard,
            },
        )
    }

    #[cfg(windows)]
    #[test]
    fn dropped_empty_reservations_do_not_consume_file_cap() {
        let (repository, directory) = test_windows_directory();
        for _ in 0..(MAX_MINIDUMP_FILES + 4) {
            let reservation = create_minidump_file_in_directory(&directory)
                .expect("reservation should be created");
            drop(reservation);
        }
        let retained = fs::read_dir(&directory.path)
            .expect("read dump directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_string_lossy().starts_with("crash-")
                    && (entry.file_name().to_string_lossy().ends_with(".dmp")
                        || entry.file_name().to_string_lossy().ends_with(".dmp.part"))
            })
            .count();
        assert_eq!(retained, 0);
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    #[test]
    fn stale_parts_are_removed_before_budgeting() {
        let (repository, directory) = test_windows_directory();
        let stale = directory.path.join("crash-stale.dmp.part");
        fs::write(&stale, []).expect("write stale reservation");
        let reservation = create_minidump_file_in_directory(&directory)
            .expect("stale reservation must not block a launch");
        assert!(!stale.exists());
        drop(reservation);
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    #[test]
    fn completed_capture_is_published_by_its_open_handle() {
        use windows_sys::Win32::Storage::FileSystem::WriteFile;

        let (repository, directory) = test_windows_directory();
        let mut reservation =
            create_minidump_file_in_directory(&directory).expect("create reservation");
        let final_path = reservation.final_path.clone();
        let bytes = b"MDMP-test";
        let mut written = 0u32;
        assert_ne!(
            unsafe {
                WriteFile(
                    reservation.raw(),
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(written as usize, bytes.len());
        reservation.close_worker_handles();
        drop(reservation);
        assert_eq!(fs::read(&final_path).expect("published dump"), bytes);
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    #[test]
    fn retained_writer_cannot_block_reservation_drop() {
        use std::time::{Duration, Instant};
        use windows_sys::Win32::Foundation::{
            CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let (repository, directory) = test_windows_directory();
        let reservation =
            create_minidump_file_in_directory(&directory).expect("create reservation");
        let mut retained: HANDLE = std::ptr::null_mut();
        let process = unsafe { GetCurrentProcess() };
        assert_ne!(
            unsafe {
                DuplicateHandle(
                    process,
                    reservation.raw(),
                    process,
                    &mut retained,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            },
            0
        );
        let started = Instant::now();
        drop(reservation);
        assert!(started.elapsed() < Duration::from_secs(1));
        unsafe { CloseHandle(retained) };
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    #[test]
    fn policy_lock_waits_for_a_concurrent_reservation() {
        use std::thread;
        use std::time::Duration;

        let (repository, directory) = test_windows_directory();
        let first = acquire_policy_lock(&directory.path).expect("first lock");
        let path = directory.path.clone();
        let contender = thread::spawn(move || {
            let lock = acquire_policy_lock(&path).expect("contender should wait");
            drop(lock);
        });
        thread::sleep(Duration::from_millis(100));
        drop(first);
        contender.join().expect("contender thread");
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    #[test]
    fn policy_lock_timeout_has_an_explicit_error_kind() {
        let (repository, directory) = test_windows_directory();
        let first = acquire_policy_lock(&directory.path).expect("first lock");
        let error =
            acquire_policy_lock_with_timeout(&directory.path, std::time::Duration::from_millis(50))
                .err()
                .expect("held policy lock should time out");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        drop(first);
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }
}
