use std::cell::RefCell;
use std::env;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

const TRACE_ROOT: &str = "target/worker-traces";

#[derive(Clone)]
struct TraceConfig {
    root: PathBuf,
    directory: PathBuf,
    configured_value: PathBuf,
}

thread_local! {
    static CONFIG: RefCell<Option<TraceConfig>> = const { RefCell::new(None) };
}

fn set_config(value: Option<TraceConfig>) {
    CONFIG.with(|config| *config.borrow_mut() = value);
}

fn get_config() -> Option<TraceConfig> {
    CONFIG.with(|config| config.borrow().clone())
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
        return Err(invalid("trace directory contains a reparse point"));
    }
    Ok(())
}

fn reject_dot_components(path: &Path) -> io::Result<()> {
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid("trace directory traversal is forbidden"));
    }
    Ok(())
}

/// Register the optional broker-owned trace directory. Workers never receive
/// this path; each launch receives only a broker-created file handle.
pub fn validate_broker_trace_directory(repository: &Path) -> io::Result<Option<PathBuf>> {
    validate_configured_trace_directory(
        repository,
        env::var_os("AEX_INSTRUMENT_TRACE_DIR").map(PathBuf::from),
    )
}

fn validate_configured_trace_directory(
    repository: &Path,
    configured: Option<PathBuf>,
) -> io::Result<Option<PathBuf>> {
    set_config(None);
    let Some(configured) = configured else {
        return Ok(None);
    };
    if !configured.is_absolute() {
        return Err(invalid("AEX_INSTRUMENT_TRACE_DIR must be absolute"));
    }
    reject_dot_components(&configured)?;

    let root = repository.join(TRACE_ROOT);
    fs::create_dir_all(&root)?;
    reject_reparse(&root)?;
    let root = fs::canonicalize(&root)?;

    fs::create_dir_all(&configured)?;
    reject_reparse(&configured)?;
    let resolved = fs::canonicalize(&configured)?;
    reject_reparse(&resolved)?;
    if !resolved.is_dir() || !resolved.starts_with(&root) {
        return Err(invalid("trace directory is outside the broker-owned root"));
    }

    let relative = resolved
        .strip_prefix(&root)
        .map_err(|_| invalid("trace directory root mismatch"))?;
    let mut current = root.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("trace directory has an invalid component"));
        };
        current.push(name);
        reject_reparse(&current)?;
    }
    set_config(Some(TraceConfig {
        root,
        directory: resolved.clone(),
        configured_value: configured,
    }));
    Ok(Some(resolved))
}

#[cfg(windows)]
pub struct TraceLaunchFile {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl TraceLaunchFile {
    pub(crate) fn raw(&self) -> windows_sys::Win32::Foundation::HANDLE {
        self.handle
    }
}

#[cfg(windows)]
impl Drop for TraceLaunchFile {
    fn drop(&mut self) {
        if !self.handle.is_null()
            && self.handle != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE
        {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
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

/// Create one unique trace file and authenticate the opened handle's final
/// path. The handle remains alive through process creation, closing the path
/// replacement race before the worker can write any trace bytes.
#[cfg(windows)]
pub(crate) fn create_trace_file_for_launch() -> io::Result<Option<TraceLaunchFile>> {
    let Some(config) = get_config() else {
        return Ok(None);
    };
    if env::var_os("AEX_INSTRUMENT_TRACE_DIR").map(PathBuf::from)
        != Some(config.configured_value.clone())
    {
        return Err(invalid("trace environment changed after policy validation"));
    }
    create_trace_file(&config).map(Some)
}

#[cfg(windows)]
fn create_trace_file(config: &TraceConfig) -> io::Result<TraceLaunchFile> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE,
        GetFinalPathNameByHandleW,
    };

    // Recheck every component immediately before the create-new operation.
    reject_reparse(&config.root)?;
    let relative = config
        .directory
        .strip_prefix(&config.root)
        .map_err(|_| invalid("trace directory root mismatch"))?;
    let mut current = config.root.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("trace directory has an invalid component"));
        };
        current.push(name);
        reject_reparse(&current)?;
    }

    let path = config
        .directory
        .join(format!("host-trace-{:032x}.jsonl", rand::random::<u128>()));
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let handle = unsafe {
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
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let launch = TraceLaunchFile { handle };

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
    let final_path = strip_extended_prefix(Path::new(&std::ffi::OsString::from_wide(
        &buffer[..written as usize],
    )));
    let final_parent = final_path
        .parent()
        .ok_or_else(|| invalid("trace file final path has no parent"))?;
    let normalized_directory = strip_extended_prefix(&config.directory);
    let normalized_root = strip_extended_prefix(&config.root);
    let parent_text = final_parent.as_os_str().to_string_lossy();
    let directory_text = normalized_directory.as_os_str().to_string_lossy();
    let final_text = final_path.as_os_str().to_string_lossy();
    let root_text = normalized_root.as_os_str().to_string_lossy();
    if !parent_text.eq_ignore_ascii_case(&directory_text)
        || !final_text
            .get(..root_text.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&root_text))
    {
        return Err(invalid("trace file handle escaped broker-owned root"));
    }
    // CreateFileW inherited the flag through SECURITY_ATTRIBUTES; assert it
    // explicitly so future creation changes fail closed.
    let mut flags = 0;
    if unsafe { windows_sys::Win32::Foundation::GetHandleInformation(handle, &mut flags) } == 0
        || flags & HANDLE_FLAG_INHERIT == 0
    {
        return Err(invalid("trace file handle is not inheritable"));
    }
    Ok(launch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> PathBuf {
        env::temp_dir().join(format!(
            "aexcompat-trace-policy-{:032x}",
            rand::random::<u128>()
        ))
    }

    #[test]
    fn absent_environment_keeps_trace_disabled() {
        let root = repository();
        assert_eq!(
            validate_configured_trace_directory(&root, None).unwrap(),
            None
        );
    }

    #[test]
    fn accepts_only_a_child_of_the_broker_root() {
        let root = repository();
        let allowed = root.join(TRACE_ROOT).join("run");
        assert_eq!(
            validate_configured_trace_directory(&root, Some(allowed.clone())).unwrap(),
            Some(fs::canonicalize(&allowed).unwrap())
        );
        #[cfg(windows)]
        create_trace_file(&get_config().unwrap()).unwrap();
        set_config(None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_absolute_paths_outside_the_broker_root() {
        let root = repository();
        let outside = env::temp_dir().join(format!(
            "aexcompat-trace-outside-{:032x}",
            rand::random::<u128>()
        ));
        assert!(validate_configured_trace_directory(&root, Some(outside.clone())).is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(windows)]
    #[test]
    fn rejects_directory_replacement_after_policy_validation() {
        use std::os::windows::fs::symlink_dir;

        let root = repository();
        let allowed = root.join(TRACE_ROOT).join("run");
        let moved = root.join(TRACE_ROOT).join("run-original");
        let outside = env::temp_dir().join(format!(
            "aexcompat-trace-race-outside-{:032x}",
            rand::random::<u128>()
        ));
        validate_configured_trace_directory(&root, Some(allowed.clone())).unwrap();
        let trace_config = get_config().unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::rename(&allowed, &moved).unwrap();
        symlink_dir(&outside, &allowed).unwrap();
        assert!(create_trace_file(&trace_config).is_err());
        set_config(None);
        fs::remove_dir(&allowed).unwrap();
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
