use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const ROOT_PREFIX: &str = "aexcompat-trusted-worker-";
const WORKER_BASENAME: &str = "trusted-worker.exe";
const CLEANUP_RETRY_COUNT: usize = 20;
const CLEANUP_RETRY_DELAY: Duration = Duration::from_millis(10);
const POPULATION_LOCK_TIMEOUT_MS: u32 = 10_000;
const CONTENT_ROOT_SLOT_COUNT: usize = 8;

/// Owns an authenticated copy of a trusted worker executable. The staging
/// root is derived from the worker and executable-relative asset contents, so
/// identical launches retain one stable Windows application identity instead
/// of accumulating path-keyed Firewall policy entries. Keep this value alive
/// until the worker process has exited.
#[derive(Debug)]
pub struct TrustedWorkerStage {
    root: PathBuf,
    temp_parent: PathBuf,
    stage_identity: [u8; 32],
    worker: PathBuf,
    handles: Option<(File, File, File)>,
    auxiliary_handles: Vec<File>,
    auxiliary_directory_handles: Vec<File>,
    auxiliary_files: Vec<PathBuf>,
    auxiliary_dirs: Vec<PathBuf>,
}

impl TrustedWorkerStage {
    pub fn create(
        source: &Path,
        expected_sha256: [u8; 32],
        expected_size: u64,
    ) -> io::Result<Self> {
        Self::create_with_assets(source, expected_sha256, expected_size, &[])
    }

    pub fn create_with_assets(
        source: &Path,
        expected_sha256: [u8; 32],
        expected_size: u64,
        auxiliary_assets: &[(PathBuf, PathBuf)],
    ) -> io::Result<Self> {
        validate_exe(source)?;
        let temp_parent = fs::canonicalize(std::env::temp_dir())?;
        reject_reparse_path(&temp_parent)?;
        let assets = inspect_auxiliary_assets(auxiliary_assets)
            .map_err(|error| stage_context("inspect auxiliary assets", error))?;
        let stage_identity = stage_identity(expected_sha256, expected_size, &assets);
        let _population_lock = acquire_population_lock(stage_identity)
            .map_err(|error| stage_context("acquire population lock", error))?;
        for slot in 0..CONTENT_ROOT_SLOT_COUNT {
            let (root, root_handle) = create_content_root(&temp_parent, stage_identity, slot)
                .map_err(|error| stage_context("open content root", error))?;
            if stage_tree_has_unexpected_entry(&root, &assets)? {
                continue;
            }
            let result = Self::populate(
                root.clone(),
                root_handle,
                temp_parent.clone(),
                source,
                expected_sha256,
                expected_size,
                stage_identity,
                &assets,
            );
            if result.is_ok() {
                return result;
            }
            let poisoned = stage_tree_has_unexpected_entry(&root, &assets).unwrap_or(true);
            cleanup_known_stage_files(&root, &assets);
            if !poisoned {
                return result;
            }
        }
        Err(invalid("trusted worker stage slots are unavailable"))
    }

    fn populate(
        root: PathBuf,
        root_handle: File,
        temp_parent: PathBuf,
        source_path: &Path,
        expected_sha256: [u8; 32],
        expected_size: u64,
        stage_identity: [u8; 32],
        assets: &[InspectedAsset],
    ) -> io::Result<Self> {
        let mut source = open_source_no_reparse(source_path)
            .map_err(|error| stage_context("open worker source", error))?;
        validate_regular_no_reparse(&source)?;
        let (source_size, source_hash) = hash_file(&mut source)?;
        if source_size != expected_size || source_hash != expected_sha256 {
            return Err(invalid("trusted worker source hash or size mismatch"));
        }
        source.rewind()?;

        let worker = root.join(WORKER_BASENAME);
        let staged_handle = open_or_populate_file(
            &worker,
            &mut source,
            expected_sha256,
            expected_size,
            "trusted worker staged copy verification failed",
        )
        .map_err(|error| stage_context("populate worker", error))?;

        let mut auxiliary_files = Vec::with_capacity(assets.len());
        let mut auxiliary_dirs = Vec::new();
        let mut auxiliary_handles = Vec::with_capacity(assets.len());
        let mut auxiliary_directory_handles = Vec::new();
        for asset in assets {
            let destination = root.join(&asset.relative);
            let mut cursor = root.clone();
            for component in asset
                .relative
                .parent()
                .into_iter()
                .flat_map(Path::components)
            {
                cursor.push(component.as_os_str());
                match fs::create_dir(&cursor) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                reject_reparse_path(&cursor)?;
                let directory_handle = open_root_no_reparse(&cursor)?;
                validate_directory_no_reparse(&directory_handle)?;
                auxiliary_dirs.push(cursor.clone());
                auxiliary_directory_handles.push(directory_handle);
            }
            let mut source_file = open_source_no_reparse(&asset.source)
                .map_err(|error| stage_context("open auxiliary source", error))?;
            validate_regular_no_reparse(&source_file)?;
            let handle = open_or_populate_file(
                &destination,
                &mut source_file,
                asset.sha256,
                asset.size,
                "trusted worker auxiliary staged copy verification failed",
            )
            .map_err(|error| stage_context("populate auxiliary asset", error))?;
            auxiliary_files.push(destination);
            auxiliary_handles.push(handle);
        }

        validate_exact_stage_tree(&root, assets)
            .map_err(|error| stage_context("validate exact stage tree", error))?;
        Ok(Self {
            root,
            temp_parent,
            stage_identity,
            worker,
            handles: Some((source, staged_handle, root_handle)),
            auxiliary_handles,
            auxiliary_directory_handles,
            auxiliary_files,
            auxiliary_dirs,
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
        self.auxiliary_handles.clear();
        self.auxiliary_directory_handles.clear();
        let safe_name = self
            .root
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|v| v.starts_with(ROOT_PREFIX));
        let safe_parent = self.root.parent() == Some(self.temp_parent.as_path());
        if safe_name && safe_parent && reject_reparse_path(&self.root).is_ok() {
            let Ok(_population_lock) = acquire_population_lock(self.stage_identity) else {
                return;
            };
            for file in self.auxiliary_files.iter().rev() {
                let _ = fs::remove_file(file);
            }
            for directory in self.auxiliary_dirs.iter().rev() {
                let _ = fs::remove_dir(directory);
            }
            // Windows can signal the process before the image section has
            // released the staged executable. Retry only this owned file/root
            // pair for a short bounded interval; never broaden cleanup to a
            // recursive delete or an unvalidated path.
            for attempt in 0..CLEANUP_RETRY_COUNT {
                let _ = fs::remove_file(&self.worker);
                match fs::remove_dir(&self.root) {
                    Ok(()) => break,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                    Err(_) if attempt + 1 < CLEANUP_RETRY_COUNT => {
                        thread::sleep(CLEANUP_RETRY_DELAY);
                    }
                    Err(_) => break,
                }
            }
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

#[derive(Debug)]
struct InspectedAsset {
    source: PathBuf,
    relative: PathBuf,
    size: u64,
    sha256: [u8; 32],
}

fn inspect_auxiliary_assets(assets: &[(PathBuf, PathBuf)]) -> io::Result<Vec<InspectedAsset>> {
    let mut inspected = Vec::with_capacity(assets.len());
    for (source, relative) in assets {
        validate_relative_asset_path(relative)?;
        let mut source_file = open_source_no_reparse(source)?;
        validate_regular_no_reparse(&source_file)?;
        let (size, sha256) = hash_file(&mut source_file)?;
        inspected.push(InspectedAsset {
            source: source.clone(),
            relative: relative.clone(),
            size,
            sha256,
        });
    }
    inspected.sort_by(|left, right| {
        path_identity_bytes(&left.relative).cmp(&path_identity_bytes(&right.relative))
    });
    for pair in inspected.windows(2) {
        if path_identity_bytes(&pair[0].relative) == path_identity_bytes(&pair[1].relative) {
            return Err(invalid("duplicate staged auxiliary path"));
        }
    }
    Ok(inspected)
}

fn validate_relative_asset_path(relative: &Path) -> io::Result<()> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(invalid("staged auxiliary path must stay relative"));
    }
    Ok(())
}

fn stage_identity(
    worker_sha256: [u8; 32],
    worker_size: u64,
    assets: &[InspectedAsset],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"aexcompat-trusted-worker-stage-v1\0");
    hash.update(worker_sha256);
    hash.update(worker_size.to_le_bytes());
    for asset in assets {
        let relative = path_identity_bytes(&asset.relative);
        hash.update((relative.len() as u64).to_le_bytes());
        hash.update(relative);
        hash.update(asset.sha256);
        hash.update(asset.size.to_le_bytes());
    }
    hash.finalize().into()
}

fn path_identity_bytes(path: &Path) -> Vec<u8> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().to_lowercase()),
            std::path::Component::CurDir => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
        .into_bytes()
}

fn create_content_root(
    parent: &Path,
    identity: [u8; 32],
    slot: usize,
) -> io::Result<(PathBuf, File)> {
    let identity = identity
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let suffix = if slot == 0 {
        String::new()
    } else {
        format!("-slot{slot}")
    };
    let path = parent.join(format!("{ROOT_PREFIX}{identity}{suffix}"));
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    reject_reparse_path(&path)?;
    let canonical = fs::canonicalize(path)?;
    reject_reparse_path(&canonical)?;
    if !canonical.is_dir() || canonical.parent() != Some(parent) {
        return Err(invalid("trusted worker stage root is invalid"));
    }
    let handle = open_root_no_reparse(&canonical)?;
    validate_directory_no_reparse(&handle)?;
    Ok((canonical, handle))
}

fn stage_tree_has_unexpected_entry(root: &Path, assets: &[InspectedAsset]) -> io::Result<bool> {
    let mut expected = BTreeSet::from([path_identity_bytes(Path::new(WORKER_BASENAME))]);
    for asset in assets {
        expected.insert(path_identity_bytes(&asset.relative));
        if let Some(parent) = asset.relative.parent() {
            let mut cursor = PathBuf::new();
            for component in parent.components() {
                if let std::path::Component::Normal(value) = component {
                    cursor.push(value);
                    expected.insert(path_identity_bytes(&cursor));
                }
            }
        }
    }
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if reject_reparse_path(&path).is_err() {
                return Ok(true);
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid("trusted worker stage entry escaped its root"))?;
            if !expected.contains(&path_identity_bytes(relative)) {
                return Ok(true);
            }
            if entry.file_type()?.is_dir() {
                directories.push(path);
            }
        }
    }
    Ok(false)
}

fn open_or_populate_file(
    destination: &Path,
    source: &mut File,
    expected_sha256: [u8; 32],
    expected_size: u64,
    mismatch_message: &'static str,
) -> io::Result<File> {
    let mut destination_file = match create_destination(destination) {
        Ok(mut file) => {
            source.rewind()?;
            io::copy(source, &mut file)?;
            file.flush()?;
            file.sync_all()?;
            file
        }
        Err(error) if is_existing_staged_file(&error) => open_staged_hold(destination)?,
        Err(error) => return Err(error),
    };
    validate_regular_no_reparse(&destination_file)?;
    let (size, sha256) = hash_file(&mut destination_file)?;
    if size != expected_size || sha256 != expected_sha256 {
        return Err(invalid(mismatch_message));
    }
    drop(destination_file);
    open_staged_hold(destination)
}

fn is_existing_staged_file(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::AlreadyExists
        || matches!(error.raw_os_error(), Some(32 | 33 | 80 | 183))
}

fn cleanup_known_stage_files(root: &Path, assets: &[InspectedAsset]) {
    for asset in assets.iter().rev() {
        let _ = fs::remove_file(root.join(&asset.relative));
    }
    let mut directories = assets
        .iter()
        .filter_map(|asset| asset.relative.parent())
        .flat_map(|parent| {
            let mut cursor = root.to_owned();
            parent.components().map(move |component| {
                cursor.push(component.as_os_str());
                cursor.clone()
            })
        })
        .collect::<Vec<_>>();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    directories.dedup();
    for directory in directories {
        let _ = fs::remove_dir(directory);
    }
    let _ = fs::remove_file(root.join(WORKER_BASENAME));
    let _ = fs::remove_dir(root);
}

fn validate_exact_stage_tree(root: &Path, assets: &[InspectedAsset]) -> io::Result<()> {
    let mut expected_files = BTreeSet::from([path_identity_bytes(Path::new(WORKER_BASENAME))]);
    let mut expected_directories = BTreeSet::new();
    for asset in assets {
        expected_files.insert(path_identity_bytes(&asset.relative));
        if let Some(parent) = asset.relative.parent() {
            let mut cursor = PathBuf::new();
            for component in parent.components() {
                if let std::path::Component::Normal(value) = component {
                    cursor.push(value);
                    expected_directories.insert(path_identity_bytes(&cursor));
                }
            }
        }
    }

    let expected_entry_count = expected_files.len() + expected_directories.len();
    let mut directories = vec![root.to_owned()];
    let mut observed_entry_count = 0usize;
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            observed_entry_count += 1;
            if observed_entry_count > expected_entry_count {
                return Err(invalid("trusted worker stage contains an unexpected entry"));
            }
            let path = entry.path();
            reject_reparse_path(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid("trusted worker stage entry escaped its root"))?;
            let identity = path_identity_bytes(relative);
            let file_type = entry.file_type()?;
            if file_type.is_file() {
                if !expected_files.remove(&identity) {
                    return Err(invalid("trusted worker stage contains an unexpected file"));
                }
            } else if file_type.is_dir() {
                if !expected_directories.remove(&identity) {
                    return Err(invalid(
                        "trusted worker stage contains an unexpected directory",
                    ));
                }
                directories.push(path);
            } else {
                return Err(invalid(
                    "trusted worker stage contains an unsupported entry",
                ));
            }
        }
    }
    if !expected_files.is_empty() || !expected_directories.is_empty() {
        return Err(invalid("trusted worker stage manifest is incomplete"));
    }
    Ok(())
}

fn hash_file(file: &mut File) -> io::Result<(u64, [u8; 32])> {
    file.rewind()?;
    let mut hash = Sha256::new();
    let size = io::copy(file, &mut hash)?;
    Ok((size, hash.finalize().into()))
}

#[cfg(windows)]
#[derive(Debug)]
struct PopulationLock {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Drop for PopulationLock {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::ReleaseMutex;
        unsafe {
            let _ = ReleaseMutex(self.handle);
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
fn acquire_population_lock(identity: [u8; 32]) -> io::Result<PopulationLock> {
    use windows_sys::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
    const WAIT_ABANDONED_0: u32 = 0x0000_0080;
    const WAIT_OBJECT_0: u32 = 0x0000_0000;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;

    let identity = identity
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut name = format!("Local\\AEXCompatTrustedWorkerStage-{identity}")
        .encode_utf16()
        .collect::<Vec<_>>();
    name.push(0);
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    match unsafe { WaitForSingleObject(handle, POPULATION_LOCK_TIMEOUT_MS) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED_0 => Ok(PopulationLock { handle }),
        WAIT_TIMEOUT => {
            unsafe {
                let _ = windows_sys::Win32::Foundation::CloseHandle(handle);
            }
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "trusted worker population lock timed out",
            ))
        }
        _ => {
            let error = io::Error::last_os_error();
            unsafe {
                let _ = windows_sys::Win32::Foundation::CloseHandle(handle);
            }
            Err(error)
        }
    }
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
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
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
fn validate_directory_no_reparse(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        GetFileInformationByHandle,
    };
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(invalid(
            "trusted worker stage directory must be non-reparse",
        ));
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
#[derive(Debug)]
struct PopulationLock;
#[cfg(not(windows))]
fn acquire_population_lock(_: [u8; 32]) -> io::Result<PopulationLock> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "trusted worker staging is only available on Windows",
    ))
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
fn validate_directory_no_reparse(_: &File) -> io::Result<()> {
    unreachable!()
}
#[cfg(not(windows))]
fn reject_reparse_path(_: &Path) -> io::Result<()> {
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn stage_context(stage: &'static str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{stage}: {error}"))
}
