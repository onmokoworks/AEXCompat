use crate::restricted_worker_acl::{protect_sealed_load_tree, RestrictedWorkerSid};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};

const ROOT_PREFIX: &str = "aexcompat-trusted-worker-";
const WORKER_BASENAME: &str = "trusted-worker.exe";

/// Owns an authenticated, read/execute-only copy of a trusted worker executable.
/// Keep this value alive until the worker process has exited.
#[derive(Debug)]
pub struct TrustedWorkerStage {
    root: PathBuf,
    temp_parent: PathBuf,
    worker: PathBuf,
    handles: Option<(File, File, File)>,
}

impl TrustedWorkerStage {
    pub fn create(
        source: &Path,
        expected_sha256: [u8; 32],
        expected_size: u64,
        worker_sid: &RestrictedWorkerSid,
    ) -> io::Result<Self> {
        validate_exe(source)?;
        let temp_parent = fs::canonicalize(std::env::temp_dir())?;
        reject_reparse_path(&temp_parent)?;
        let root = create_random_root(&temp_parent)?;
        let result = Self::populate(
            root.clone(),
            temp_parent.clone(),
            source,
            expected_sha256,
            expected_size,
            worker_sid,
        );
        if result.is_err() {
            let _ = fs::remove_file(root.join(WORKER_BASENAME));
            let _ = fs::remove_dir(&root);
        }
        result
    }

    fn populate(
        root: PathBuf,
        temp_parent: PathBuf,
        source_path: &Path,
        expected_sha256: [u8; 32],
        expected_size: u64,
        worker_sid: &RestrictedWorkerSid,
    ) -> io::Result<Self> {
        let mut source = open_source_no_reparse(source_path)?;
        validate_regular_no_reparse(&source)?;
        let (source_size, source_hash) = hash_file(&mut source)?;
        if source_size != expected_size || source_hash != expected_sha256 {
            return Err(invalid("trusted worker source hash or size mismatch"));
        }
        source.rewind()?;

        let worker = root.join(WORKER_BASENAME);
        let mut destination = create_destination(&worker)?;
        io::copy(&mut source, &mut destination)?;
        destination.flush()?;
        destination.sync_all()?;
        validate_regular_no_reparse(&destination)?;
        let (copied_size, copied_hash) = hash_file(&mut destination)?;
        if copied_size != expected_size || copied_hash != expected_sha256 {
            return Err(invalid("trusted worker staged copy verification failed"));
        }

        let root_handle = open_root_no_reparse(&root)?;
        drop(destination);
        protect_sealed_load_tree(&root, &[WORKER_BASENAME], worker_sid)?;
        let staged_handle = open_staged_hold(&worker)?;
        Ok(Self {
            root,
            temp_parent,
            worker,
            handles: Some((source, staged_handle, root_handle)),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn worker_path(&self) -> &Path {
        &self.worker
    }
}

impl Drop for TrustedWorkerStage {
    fn drop(&mut self) {
        drop(self.handles.take());
        let safe_name = self
            .root
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|v| v.starts_with(ROOT_PREFIX));
        let safe_parent = self.root.parent() == Some(self.temp_parent.as_path());
        if safe_name && safe_parent && reject_reparse_path(&self.root).is_ok() {
            let _ = fs::remove_file(&self.worker);
            let _ = fs::remove_dir(&self.root);
        }
    }
}

fn validate_exe(path: &Path) -> io::Result<()> {
    if !path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
    {
        return Err(invalid("trusted worker source must be an EXE"));
    }
    Ok(())
}

fn create_random_root(parent: &Path) -> io::Result<PathBuf> {
    for _ in 0..128 {
        let path = parent.join(format!("{ROOT_PREFIX}{:032x}", rand::random::<u128>()));
        match fs::create_dir(&path) {
            Ok(()) => return fs::canonicalize(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate trusted worker staging root",
    ))
}

fn hash_file(file: &mut File) -> io::Result<(u64, [u8; 32])> {
    file.rewind()?;
    let mut hash = Sha256::new();
    let size = io::copy(file, &mut hash)?;
    Ok((size, hash.finalize().into()))
}

#[cfg(windows)]
fn open_source_no_reparse(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn create_destination(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn open_root_no_reparse(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn open_staged_hold(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn validate_regular_no_reparse(file: &File) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(invalid("trusted worker must be a regular non-reparse file"));
    }
    Ok(())
}

#[cfg(windows)]
fn reject_reparse_path(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    if fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(invalid("reparse points are not allowed"));
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_source_no_reparse(_: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "trusted worker staging is only available on Windows",
    ))
}
#[cfg(not(windows))]
fn create_destination(_: &Path) -> io::Result<File> {
    unreachable!()
}
#[cfg(not(windows))]
fn open_root_no_reparse(_: &Path) -> io::Result<File> {
    unreachable!()
}
#[cfg(not(windows))]
fn open_staged_hold(_: &Path) -> io::Result<File> {
    unreachable!()
}
#[cfg(not(windows))]
fn validate_regular_no_reparse(_: &File) -> io::Result<()> {
    unreachable!()
}
#[cfg(not(windows))]
fn reject_reparse_path(_: &Path) -> io::Result<()> {
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
