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
    #[allow(dead_code)] // retained for diagnostics and path-policy tests; I/O is guard-relative
    path: PathBuf,
    display: String,
    #[cfg(windows)]
    guard: std::sync::Arc<DirectoryGuard>,
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
struct DirectoryGuard(usize);

#[cfg(windows)]
impl DirectoryGuard {
    fn raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.0 as windows_sys::Win32::Foundation::HANDLE
    }
}

#[cfg(windows)]
impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(
                self.0 as windows_sys::Win32::Foundation::HANDLE,
            )
        };
    }
}

#[cfg(windows)]
fn harden_directory_acl(directory: &Path) -> io::Result<std::sync::Arc<DirectoryGuard>> {
    use std::os::windows::ffi::OsStrExt;
    use std::sync::Arc;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetKernelObjectSecurity,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
    };

    let wide: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
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
        Ok(Arc::new(DirectoryGuard(handle as usize)))
    })();
    if result.is_err() {
        unsafe { CloseHandle(handle) };
    }
    result
}

#[cfg(windows)]
#[repr(C)]
struct NtUnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[cfg(windows)]
#[repr(C)]
struct NtObjectAttributes {
    length: u32,
    root_directory: windows_sys::Win32::Foundation::HANDLE,
    object_name: *mut NtUnicodeString,
    attributes: u32,
    security_descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
    security_quality_of_service: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[repr(C)]
union NtIoStatusValue {
    status: i32,
    pointer: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[repr(C)]
struct NtIoStatusBlock {
    value: NtIoStatusValue,
    information: usize,
}

#[cfg(windows)]
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        file_handle: *mut windows_sys::Win32::Foundation::HANDLE,
        desired_access: u32,
        object_attributes: *mut NtObjectAttributes,
        io_status_block: *mut NtIoStatusBlock,
        allocation_size: *const i64,
        file_attributes: u32,
        share_access: u32,
        create_disposition: u32,
        create_options: u32,
        ea_buffer: *const std::ffi::c_void,
        ea_length: u32,
    ) -> i32;
    fn NtQueryDirectoryFile(
        file_handle: windows_sys::Win32::Foundation::HANDLE,
        event: windows_sys::Win32::Foundation::HANDLE,
        apc_routine: *const std::ffi::c_void,
        apc_context: *const std::ffi::c_void,
        io_status_block: *mut NtIoStatusBlock,
        file_information: *mut std::ffi::c_void,
        length: u32,
        file_information_class: u32,
        return_single_entry: u8,
        file_name: *const NtUnicodeString,
        restart_scan: u8,
    ) -> i32;
    fn NtSetInformationFile(
        file_handle: windows_sys::Win32::Foundation::HANDLE,
        io_status_block: *mut NtIoStatusBlock,
        file_information: *mut std::ffi::c_void,
        length: u32,
        file_information_class: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

#[cfg(windows)]
fn nt_error(status: i32) -> io::Error {
    io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
}

#[cfg(windows)]
fn open_relative(
    directory: &DirectoryGuard,
    name: &std::ffi::OsStr,
    desired_access: u32,
    share_access: u32,
    disposition: u32,
    options: u32,
    attributes: u32,
    security_descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
) -> io::Result<windows_sys::Win32::Foundation::HANDLE> {
    use std::os::windows::ffi::OsStrExt;

    let mut name: Vec<u16> = name.encode_wide().collect();
    let byte_len = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .filter(|length| *length <= u16::MAX as usize)
        .ok_or_else(|| invalid("minidump relative name is too long"))? as u16;
    let mut unicode = NtUnicodeString {
        length: byte_len,
        maximum_length: byte_len,
        buffer: name.as_mut_ptr(),
    };
    let mut object = NtObjectAttributes {
        length: std::mem::size_of::<NtObjectAttributes>() as u32,
        root_directory: directory.raw(),
        object_name: &mut unicode,
        // OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE
        attributes: 0x40 | 0x1000,
        security_descriptor,
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut io_status = NtIoStatusBlock {
        value: NtIoStatusValue { status: 0 },
        information: 0,
    };
    let mut handle = std::ptr::null_mut();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &mut object,
            &mut io_status,
            std::ptr::null(),
            attributes,
            share_access,
            disposition,
            options,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        Err(nt_error(status))
    } else {
        Ok(handle)
    }
}

#[cfg(windows)]
struct DirectoryEntry {
    name: std::ffi::OsString,
    size: u64,
    attributes: u32,
}

#[cfg(windows)]
fn directory_entries(directory: &DirectoryGuard) -> io::Result<Vec<DirectoryEntry>> {
    use std::os::windows::ffi::OsStringExt;

    const FILE_ID_BOTH_DIRECTORY_INFORMATION: u32 = 37;
    const STATUS_NO_MORE_FILES: i32 = 0x80000006u32 as i32;
    const NAME_OFFSET: usize = 104;
    let mut result = Vec::new();
    let mut restart = 1u8;
    loop {
        let mut buffer = vec![0u8; 64 * 1024];
        let mut io_status = NtIoStatusBlock {
            value: NtIoStatusValue { status: 0 },
            information: 0,
        };
        let status = unsafe {
            NtQueryDirectoryFile(
                directory.raw(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                &mut io_status,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                FILE_ID_BOTH_DIRECTORY_INFORMATION,
                0,
                std::ptr::null(),
                restart,
            )
        };
        restart = 0;
        if status == STATUS_NO_MORE_FILES {
            break;
        }
        if status < 0 {
            return Err(nt_error(status));
        }
        let used = io_status.information.min(buffer.len());
        let mut offset = 0usize;
        loop {
            if offset
                .checked_add(NAME_OFFSET)
                .map_or(true, |end| end > used)
            {
                return Err(invalid("malformed minidump directory enumeration"));
            }
            let read_u32 = |at: usize| u32::from_ne_bytes(buffer[at..at + 4].try_into().unwrap());
            let read_i64 = |at: usize| i64::from_ne_bytes(buffer[at..at + 8].try_into().unwrap());
            let next = read_u32(offset) as usize;
            let name_bytes = read_u32(offset + 60) as usize;
            if name_bytes % 2 != 0
                || offset
                    .checked_add(NAME_OFFSET + name_bytes)
                    .map_or(true, |end| end > used)
            {
                return Err(invalid("malformed minidump directory entry name"));
            }
            let words: Vec<u16> = buffer[offset + NAME_OFFSET..offset + NAME_OFFSET + name_bytes]
                .chunks_exact(2)
                .map(|bytes| u16::from_ne_bytes([bytes[0], bytes[1]]))
                .collect();
            result.push(DirectoryEntry {
                name: std::ffi::OsString::from_wide(&words),
                size: read_i64(offset + 40).max(0) as u64,
                attributes: read_u32(offset + 56),
            });
            if next == 0 {
                break;
            }
            if next < NAME_OFFSET || offset.checked_add(next).map_or(true, |end| end >= used) {
                return Err(invalid("malformed minidump directory entry chain"));
            }
            offset += next;
        }
    }
    Ok(result)
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
    final_name: std::ffi::OsString,
    _directory_guard: std::sync::Arc<DirectoryGuard>,
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
        let publish = capture.bytes > 0
            && capture.producer_complete
            && !capture.overflow
            && !capture.write_failed;
        let rename_result = if publish {
            rename_file_by_handle(self.dump_handle, &self._directory_guard, &self.final_name)
        } else {
            Err(invalid("empty or rejected minidump capture"))
        };
        let published =
            rename_result.is_ok() && authenticate_plain_file(self.dump_handle, false).is_ok();
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
    producer_complete: bool,
    overflow: bool,
    write_failed: bool,
}

#[cfg(windows)]
const MINIDUMP_COMPLETION_MARKER: &[u8; 16] = b"AEXDUMP-COMPLETE";

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
fn authenticate_plain_file(
    handle: windows_sys::Win32::Foundation::HANDLE,
    require_inherit: bool,
) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, GetFileInformationByHandle,
    };

    let mut flags = 0;
    if unsafe { GetHandleInformation(handle, &mut flags) } == 0
        || (require_inherit && flags & HANDLE_FLAG_INHERIT == 0)
    {
        return Err(invalid(
            "minidump file handle is not authenticated/inheritable",
        ));
    }
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(handle, &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || information.nNumberOfLinks != 1
    {
        return Err(invalid(format!(
            "minidump file handle is reparse-backed or multiply linked (attributes={:#x}, links={})",
            information.dwFileAttributes, information.nNumberOfLinks
        )));
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
fn harden_file_acl(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<()> {
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetKernelObjectSecurity,
    };

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
    Ok(())
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
    // Keep the last marker-sized suffix out of the dump file until EOF.  A
    // nonempty prefix is not proof that MiniDumpWriteDump succeeded: DbgHelp
    // can fail after emitting bytes, and a dying worker can close the pipe at
    // any point.  Only the writer appends this marker after a successful
    // MiniDumpWriteDump return.
    let mut pending = Vec::<u8>::with_capacity(64 * 1024 + MINIDUMP_COMPLETION_MARKER.len());
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
        pending.extend_from_slice(&buffer[..count]);
        let flush_count = pending
            .len()
            .saturating_sub(MINIDUMP_COMPLETION_MARKER.len());
        if capture.bytes > MAX_MINIDUMP_FILE_BYTES - flush_count as u64 {
            capture.overflow = true;
            // Closing the read side makes an over-budget producer fail fast;
            // draining attacker-controlled bytes would only burn broker CPU.
            break;
        }
        let mut offset = 0usize;
        while offset < flush_count {
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    dump_handle,
                    pending[offset..flush_count].as_ptr(),
                    (flush_count - offset) as u32,
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
        if capture.write_failed {
            break;
        }
        capture.bytes += flush_count as u64;
        pending.drain(..flush_count);
    }
    capture.producer_complete = !capture.overflow
        && !capture.write_failed
        && pending.as_slice() == MINIDUMP_COMPLETION_MARKER;
    let mut ack = [0u8; 9];
    ack[..8].copy_from_slice(&capture.bytes.to_le_bytes());
    ack[8] = u8::from(!capture.producer_complete || capture.overflow || capture.write_failed);
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
    directory: &DirectoryGuard,
    destination: &std::ffi::OsStr,
) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_RENAME_INFO;

    let name: Vec<u16> = destination.encode_wide().collect();
    let header = std::mem::size_of::<FILE_RENAME_INFO>();
    let bytes = header
        .checked_add(name.len().saturating_sub(1) * std::mem::size_of::<u16>())
        .ok_or_else(|| invalid("minidump final path is too long"))?;
    let mut storage = vec![0u8; bytes];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = 0;
        (*info).RootDirectory = directory.raw();
        (*info).FileNameLength = (name.len() * std::mem::size_of::<u16>()) as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        let mut io_status = NtIoStatusBlock {
            value: NtIoStatusValue { status: 0 },
            information: 0,
        };
        let status = NtSetInformationFile(
            handle,
            &mut io_status,
            info.cast(),
            bytes as u32,
            10, // FileRenameInformation
        );
        if status < 0 {
            return Err(nt_error(status));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn mark_delete_by_handle(handle: windows_sys::Win32::Foundation::HANDLE) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
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
fn acquire_policy_lock(directory: &DirectoryGuard) -> io::Result<PolicyLock> {
    acquire_policy_lock_with_timeout(
        directory,
        std::time::Duration::from_millis(MINIDUMP_POLICY_LOCK_TIMEOUT_MS),
    )
}

#[cfg(windows)]
fn acquire_policy_lock_with_timeout(
    directory: &DirectoryGuard,
    timeout: std::time::Duration,
) -> io::Result<PolicyLock> {
    use std::thread::sleep;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE,
    };
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_ATTRIBUTE_HIDDEN};

    let deadline = Instant::now() + timeout;
    loop {
        // FILE_OPEN_IF, FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT |
        // FILE_DELETE_ON_CLOSE | FILE_OPEN_REPARSE_POINT.
        match open_relative(
            directory,
            std::ffi::OsStr::new("aexcompat-minidump.lock"),
            GENERIC_READ | GENERIC_WRITE | DELETE | 0x0010_0000,
            0,
            3,
            0x40 | 0x20 | 0x1000 | 0x0020_0000,
            FILE_ATTRIBUTE_HIDDEN,
            std::ptr::null_mut(),
        ) {
            Ok(handle) => {
                if let Err(error) = authenticate_plain_file(handle, false) {
                    unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
                    return Err(error);
                }
                return Ok(PolicyLock(handle));
            }
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(code)
                        if code == ERROR_SHARING_VIOLATION as i32
                            || code == ERROR_ACCESS_DENIED as i32
                ) => {}
            Err(error) => return Err(error),
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

#[cfg(not(windows))]
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
fn enforce_budget(directory: &DirectoryGuard) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut in_flight = 0u64;
    for entry in directory_entries(directory)? {
        if entry.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            continue;
        }
        let name = entry.name.to_string_lossy();
        if !name.starts_with("crash-") {
            continue;
        }
        if name.ends_with(".dmp") {
            files = files.saturating_add(1);
            bytes = bytes.saturating_add(entry.size);
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
fn remove_stale_reservations(directory: &DirectoryGuard) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SHARING_VIOLATION};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_READ_ATTRIBUTES,
    };

    for entry in directory_entries(directory)? {
        if entry.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            continue;
        }
        let name_os = entry.name;
        let name = name_os.to_string_lossy();
        if !name.starts_with("crash-") || !name.ends_with(".dmp.part") {
            continue;
        }
        match open_relative(
            directory,
            &name_os,
            FILE_READ_ATTRIBUTES | DELETE | 0x0010_0000,
            0,
            1,
            0x40 | 0x20 | 0x0020_0000,
            0,
            std::ptr::null_mut(),
        ) {
            Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => {
                // An active reservation is held with share mode zero. Leave
                // it in the in-flight budget.
                continue;
            }
            Err(error) => return Err(error),
            Ok(handle) => {
                let result = authenticate_plain_file(handle, false)
                    .and_then(|_| mark_delete_by_handle(handle));
                unsafe { CloseHandle(handle) };
                result?;
            }
        }
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
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, WRITE_DAC,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;

    // This lock covers only the budget scan and CREATE_NEW reservation. The
    // reservation handle itself remains alive until the worker exits.
    let directory_guard = std::sync::Arc::clone(&directory.guard);
    let _policy_lock = acquire_policy_lock(&directory_guard)?;
    remove_stale_reservations(&directory_guard)?;
    if let Err(error) = enforce_budget(&directory_guard) {
        return Err(error);
    }
    let nonce = format!("crash-{:032x}", rand::random::<u128>());
    let reservation_name = std::ffi::OsString::from(format!("{nonce}.dmp.part"));
    let final_name = std::ffi::OsString::from(format!("{nonce}.dmp"));
    let (_, security_descriptor) = protected_file_security()?;
    let dump_handle = open_relative(
        &directory_guard,
        &reservation_name,
        FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE | WRITE_DAC | 0x0010_0000,
        0,
        2,
        0x40 | 0x20 | 0x0020_0000,
        FILE_ATTRIBUTE_NORMAL,
        security_descriptor.0,
    )
    .map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("relative reservation create: {error}"),
        )
    })?;
    if let Err(error) = harden_file_acl(dump_handle) {
        let _ = mark_delete_by_handle(dump_handle);
        unsafe { CloseHandle(dump_handle) };
        return Err(io::Error::new(
            error.kind(),
            format!("relative reservation ACL: {error}"),
        ));
    }
    if let Err(error) = authenticate_plain_file(dump_handle, false) {
        let _ = mark_delete_by_handle(dump_handle);
        unsafe { CloseHandle(dump_handle) };
        return Err(io::Error::new(
            error.kind(),
            format!("relative reservation authentication: {error}"),
        ));
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
        final_name,
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

    #[cfg(windows)]
    fn enforce_budget_path(path: &Path) -> io::Result<()> {
        let canonical = fs::canonicalize(path)?;
        let guard = harden_directory_acl(&canonical)?;
        enforce_budget(&guard)
    }

    #[cfg(not(windows))]
    fn enforce_budget_path(path: &Path) -> io::Result<()> {
        enforce_budget(path)
    }

    #[test]
    fn directory_resolution_stays_under_target() {
        let repository = test_repository();
        let resolved = resolve_directory(&repository, Path::new("target/crash-dumps"))
            .expect("relative directory should resolve");
        assert!(
            resolved.path.starts_with(
                fs::canonicalize(repository.join("target")).expect("canonical target")
            )
        );
        let concurrent = resolve_directory(&repository, Path::new("target/crash-dumps"))
            .expect("concurrent directory guards should coexist");
        assert_eq!(concurrent.path, resolved.path);
        #[cfg(windows)]
        {
            let renamed = repository.join("target/renamed-crash-dumps");
            fs::rename(&resolved.path, &renamed).expect("guard permits cross-process coexistence");
            let reservation = create_minidump_file_in_directory(&resolved)
                .expect("handle-relative launch survives directory rename");
            drop(reservation);
        }
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
        enforce_budget_path(&directory).expect("one small dump should fit");
        fs::write(
            directory.join("crash-full.dmp"),
            vec![0u8; (MAX_MINIDUMP_TOTAL_BYTES - MAX_MINIDUMP_FILE_BYTES) as usize],
        )
        .expect("write near-cap dump");
        assert!(enforce_budget_path(&directory).is_err());
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
        assert!(enforce_budget_path(&directory).is_err());
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
        enforce_budget_path(&directory).expect("three maximum reservations should fit");
        fs::write(directory.join("crash-3.dmp.part"), []).expect("write reservation");
        assert!(enforce_budget_path(&directory).is_err());
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
        let final_path = directory.path.join(&reservation.final_name);
        let bytes = b"MDMP-test";
        let transport: Vec<u8> = bytes
            .iter()
            .copied()
            .chain(MINIDUMP_COMPLETION_MARKER.iter().copied())
            .collect();
        let mut written = 0u32;
        assert_ne!(
            unsafe {
                WriteFile(
                    reservation.raw(),
                    transport.as_ptr(),
                    transport.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(written as usize, transport.len());
        reservation.close_worker_handles();
        drop(reservation);
        assert_eq!(fs::read(&final_path).expect("published dump"), bytes);
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    #[test]
    fn partial_capture_without_completion_marker_is_deleted() {
        use windows_sys::Win32::Storage::FileSystem::WriteFile;

        let (repository, directory) = test_windows_directory();
        let mut reservation =
            create_minidump_file_in_directory(&directory).expect("create reservation");
        let final_path = directory.path.join(&reservation.final_name);
        let partial = b"MDMP-incomplete-prefix";
        let mut written = 0u32;
        assert_ne!(
            unsafe {
                WriteFile(
                    reservation.raw(),
                    partial.as_ptr(),
                    partial.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(written as usize, partial.len());
        reservation.close_worker_handles();
        drop(reservation);
        assert!(!final_path.exists());
        assert_eq!(
            fs::read_dir(&directory.path)
                .expect("read dump directory")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("crash-"))
                .count(),
            0
        );
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    #[test]
    fn retained_writer_cannot_block_reservation_drop() {
        use std::time::{Duration, Instant};
        use windows_sys::Win32::Foundation::{
            CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE,
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
        let first = acquire_policy_lock(&directory.guard).expect("first lock");
        let guard = std::sync::Arc::clone(&directory.guard);
        let contender = thread::spawn(move || {
            let lock = acquire_policy_lock(&guard).expect("contender should wait");
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
        let first = acquire_policy_lock(&directory.guard).expect("first lock");
        let error = acquire_policy_lock_with_timeout(
            &directory.guard,
            std::time::Duration::from_millis(50),
        )
        .err()
        .expect("held policy lock should time out");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        drop(first);
        drop(directory);
        let _ = fs::remove_dir_all(repository);
    }

    #[cfg(windows)]
    #[test]
    fn directory_guards_coexist_across_processes() {
        use std::process::Command;
        use std::thread;
        use std::time::{Duration, Instant};

        const CHILD_REPOSITORY: &str = "AEXCOMPAT_TEST_GUARD_REPOSITORY";
        const CHILD_READY: &str = "AEXCOMPAT_TEST_GUARD_READY";
        if let (Some(repository), Some(ready)) =
            (env::var_os(CHILD_REPOSITORY), env::var_os(CHILD_READY))
        {
            let repository = PathBuf::from(repository);
            let directory = resolve_directory(&repository, Path::new("target/crash-dumps"))
                .expect("child guard");
            fs::write(ready, []).expect("signal child guard");
            thread::sleep(Duration::from_secs(3));
            drop(directory);
            return;
        }

        let repository = test_repository();
        let ready = repository.join("guard-ready");
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("minidump_policy::tests::directory_guards_coexist_across_processes")
            .arg("--nocapture")
            .env(CHILD_REPOSITORY, &repository)
            .env(CHILD_READY, &ready)
            .spawn()
            .expect("spawn guard child");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "child guard did not become ready");
        let directory = resolve_directory(&repository, Path::new("target/crash-dumps"))
            .expect("parent guard must coexist with child guard");
        let reservation = create_minidump_file_in_directory(&directory)
            .expect("parent capture must work while child guard is alive");
        drop(reservation);
        drop(directory);
        assert!(child.wait().expect("wait guard child").success());
        let _ = fs::remove_dir_all(repository);
    }
}
