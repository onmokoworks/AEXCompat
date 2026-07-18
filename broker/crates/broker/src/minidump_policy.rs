use std::env;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub const MINIDUMP_DIR_ENV: &str = "AEXCOMPAT_MINIDUMP_DIR";
pub const MINIDUMP_HANDLE_ENV: &str = "AEXCOMPAT_MINIDUMP_HANDLE";
pub const MAX_MINIDUMP_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_MINIDUMP_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_MINIDUMP_FILES: u64 = 16;
pub const MINIDUMP_POLICY_LOCK_TIMEOUT_MS: u64 = 5_000;

struct MinidumpDirectory {
    path: PathBuf,
    display: String,
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
    let display = canonical
        .strip_prefix(canonical_target.parent().unwrap_or(&canonical_target))
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "target/(crash-dumps)".into());
    Ok(MinidumpDirectory {
        path: canonical,
        display,
    })
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
    path: PathBuf,
}

#[cfg(windows)]
impl MinidumpLaunchFile {
    pub(crate) fn raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.dump_handle
    }
}

#[cfg(windows)]
impl Drop for MinidumpLaunchFile {
    fn drop(&mut self) {
        let mut size = 0i64;
        let empty = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileSizeEx(self.dump_handle, &mut size)
        } != 0
            && size == 0;
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.dump_handle);
        }
        if empty {
            let _ = fs::remove_file(&self.path);
        }
    }
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

    let path = directory.join(".aexcompat-minidump.lock");
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
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let name = entry.file_name();
        if metadata.is_file()
            && name.to_string_lossy().starts_with("crash-")
            && name.to_string_lossy().ends_with(".dmp")
        {
            files = files.saturating_add(1);
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    if files >= MAX_MINIDUMP_FILES
        || bytes > MAX_MINIDUMP_TOTAL_BYTES
        || bytes.saturating_add(MAX_MINIDUMP_FILE_BYTES) > MAX_MINIDUMP_TOTAL_BYTES
    {
        return Err(invalid("minidump directory capacity policy exceeded"));
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
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE,
    };

    // This lock covers only the budget scan and CREATE_NEW reservation. The
    // reservation handle itself remains alive until the worker exits.
    let _policy_lock = acquire_policy_lock(&directory.path)?;
    if let Err(error) = enforce_budget(&directory.path) {
        return Err(error);
    }
    let path = directory
        .path
        .join(format!("crash-{:032x}.dmp", rand::random::<u128>()));
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let dump_handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_WRITE,
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
    if let Err(error) = authenticate_file_handle(dump_handle, &directory.path, true) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(dump_handle) };
        return Err(error);
    }
    let mut flags = 0;
    if unsafe { windows_sys::Win32::Foundation::GetHandleInformation(dump_handle, &mut flags) } == 0
        || flags & HANDLE_FLAG_INHERIT == 0
    {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(dump_handle) };
        return Err(invalid("minidump file handle inheritance check failed"));
    }
    Ok(MinidumpLaunchFile { dump_handle, path })
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

    #[cfg(windows)]
    fn test_windows_directory() -> (PathBuf, MinidumpDirectory) {
        let repository = test_repository();
        let path = repository.join("target/crash-dumps");
        fs::create_dir_all(&path).expect("create dump directory");
        let path = fs::canonicalize(path).expect("canonical dump directory");
        let display = path.to_string_lossy().into_owned();
        (repository, MinidumpDirectory { path, display })
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
                    && entry.file_name().to_string_lossy().ends_with(".dmp")
            })
            .count();
        assert_eq!(retained, 0);
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
        let _ = fs::remove_dir_all(repository);
    }
}
