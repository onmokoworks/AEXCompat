use std::env;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

const TRACE_ROOT: &str = "target/worker-traces";

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

/// Validate the optional trace destination before a worker is launched.
///
/// The broker owns `target/worker-traces`; a worker never gets an arbitrary
/// path supplied by the caller. When the variable is absent, no trace
/// environment is introduced and the normal worker path is unchanged.
pub fn validate_broker_trace_directory(repository: &Path) -> io::Result<Option<PathBuf>> {
    let Some(value) = env::var_os("AEX_INSTRUMENT_TRACE_DIR") else {
        return Ok(None);
    };
    let configured = PathBuf::from(value);
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
    Ok(Some(resolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn repository() -> PathBuf {
        env::temp_dir().join(format!(
            "aexcompat-trace-policy-{:032x}",
            rand::random::<u128>()
        ))
    }

    #[test]
    fn absent_environment_keeps_trace_disabled() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { env::remove_var("AEX_INSTRUMENT_TRACE_DIR") };
        let root = repository();
        assert_eq!(validate_broker_trace_directory(&root).unwrap(), None);
    }

    #[test]
    fn accepts_only_a_child_of_the_broker_root() {
        let _lock = ENV_LOCK.lock().unwrap();
        let root = repository();
        let allowed = root.join(TRACE_ROOT).join("run");
        unsafe { env::set_var("AEX_INSTRUMENT_TRACE_DIR", &allowed) };
        assert_eq!(
            validate_broker_trace_directory(&root).unwrap(),
            Some(fs::canonicalize(&allowed).unwrap())
        );
        unsafe { env::remove_var("AEX_INSTRUMENT_TRACE_DIR") };
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_absolute_paths_outside_the_broker_root() {
        let _lock = ENV_LOCK.lock().unwrap();
        let root = repository();
        let outside = env::temp_dir().join(format!(
            "aexcompat-trace-outside-{:032x}",
            rand::random::<u128>()
        ));
        unsafe { env::set_var("AEX_INSTRUMENT_TRACE_DIR", &outside) };
        assert!(validate_broker_trace_directory(&root).is_err());
        unsafe { env::remove_var("AEX_INSTRUMENT_TRACE_DIR") };
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
