use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::path::{Component, Path, PathBuf};

const MANIFEST_DOMAIN: &[u8] = b"AEXCompat sealed load tree manifest\0v1\0";
const ROOT_PREFIX: &str = "aexcompat-sealed-";

#[derive(Clone, Debug)]
pub struct LoadEntry {
    pub source: PathBuf,
    pub relative_basename: String,
    pub expected_sha256: [u8; 32],
    pub expected_size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdditionalChildProtection {
    /// Manifest files are verified and held, but another principal can still add children.
    RequiresRestrictedWorkerAcl,
}

#[derive(Debug)]
pub struct SealedLoadTree {
    root: PathBuf,
    temp_parent: PathBuf,
    manifest_digest: [u8; 32],
    manifest_filenames: Vec<String>,
    handles: Option<Vec<File>>,
}

impl SealedLoadTree {
    pub fn create(main: LoadEntry, dependencies: Vec<LoadEntry>) -> io::Result<Self> {
        let temp_parent = fs::canonicalize(std::env::temp_dir())?;
        reject_reparse(&temp_parent)?;
        let root = create_random_root(&temp_parent)?;
        let result = Self::populate(root.clone(), temp_parent.clone(), main, dependencies);
        if result.is_err() {
            let _ = fs::remove_dir(&root);
        }
        result
    }

    fn populate(
        root: PathBuf,
        temp_parent: PathBuf,
        main: LoadEntry,
        mut dependencies: Vec<LoadEntry>,
    ) -> io::Result<Self> {
        dependencies.sort_by_key(|entry| entry.relative_basename.to_lowercase());
        let mut entries = Vec::with_capacity(dependencies.len() + 1);
        entries.push((0u8, main));
        entries.extend(dependencies.into_iter().map(|entry| (1u8, entry)));

        let mut names = HashSet::new();
        for (_, entry) in &entries {
            validate_basename(&entry.relative_basename)?;
            let folded = entry.relative_basename.to_lowercase();
            if !names.insert(folded) {
                return Err(invalid("duplicate or case-insensitive filename collision"));
            }
        }

        let mut handles = Vec::with_capacity(entries.len());
        let mut manifest_filenames = Vec::with_capacity(entries.len());
        let mut manifest = Sha256::new();
        manifest.update(MANIFEST_DOMAIN);
        manifest.update((entries.len() as u64).to_le_bytes());

        for (role, entry) in entries {
            let source_parent = entry
                .source
                .parent()
                .ok_or_else(|| invalid("source has no direct parent"))?;
            reject_reparse(source_parent)?;
            let canonical_parent = fs::canonicalize(source_parent)?;
            reject_reparse(&canonical_parent)?;
            let source_name = entry
                .source
                .file_name()
                .ok_or_else(|| invalid("source has no filename"))?;
            if source_name != entry.relative_basename.as_str() {
                return Err(invalid("source filename differs from relative basename"));
            }
            if entry.source.parent() != Some(source_parent) {
                return Err(invalid("source must be a direct child"));
            }

            reject_reparse(&entry.source)?;
            let mut source = open_source(&entry.source)?;
            validate_regular_unique(&source)?;
            let (size, digest) = hash_file(&mut source)?;
            if size != entry.expected_size {
                return Err(invalid("source size mismatch"));
            }
            if digest != entry.expected_sha256 {
                return Err(invalid("source SHA-256 mismatch"));
            }
            source.rewind()?;

            let destination = root.join(&entry.relative_basename);
            if destination.parent() != Some(root.as_path()) {
                return Err(invalid("destination must be a direct child"));
            }
            let mut output = create_destination(&destination)?;
            io::copy(&mut source, &mut output)?;
            output.flush()?;
            output.sync_all()?;
            reject_reparse(&destination)?;
            validate_regular_unique(&output)?;
            let mut verify = output.try_clone()?;
            let (copied_size, copied_digest) = hash_file(&mut verify)?;
            if copied_size != entry.expected_size || copied_digest != entry.expected_sha256 {
                return Err(invalid("destination verification failed"));
            }
            drop(verify);
            drop(output);

            // A retained write-capable handle conflicts with the Windows image loader,
            // whose reopen does not share writes. Reopen read-only and authenticate the
            // exact path again before retaining the no-write/no-delete-share handle.
            let mut retained = open_source(&destination)?;
            validate_regular_unique(&retained)?;
            let (retained_size, retained_digest) = hash_file(&mut retained)?;
            if retained_size != entry.expected_size || retained_digest != entry.expected_sha256 {
                return Err(invalid("retained destination verification failed"));
            }

            manifest.update([role]);
            manifest.update((entry.relative_basename.len() as u64).to_le_bytes());
            manifest.update(entry.relative_basename.as_bytes());
            manifest.update(entry.expected_size.to_le_bytes());
            manifest.update(entry.expected_sha256);
            manifest_filenames.push(entry.relative_basename);
            handles.push(retained);
        }

        Ok(Self {
            root,
            temp_parent,
            manifest_digest: manifest.finalize().into(),
            manifest_filenames,
            handles: Some(handles),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }

    /// Returns the validated direct-child basenames authenticated by the manifest.
    pub fn manifest_basenames(&self) -> &[String] {
        &self.manifest_filenames
    }

    /// Resolves the authenticated main plugin without allowing callers to
    /// substitute another manifest member or an arbitrary path.
    pub fn plugin_path(&self, plugin_basename: &str) -> io::Result<PathBuf> {
        validate_basename(plugin_basename)?;
        if self.manifest_filenames.first().map(String::as_str) != Some(plugin_basename) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "plugin basename is not the authenticated main manifest entry",
            ));
        }
        Ok(self.root.join(plugin_basename))
    }

    /// Child insertion prevention requires the future restricted worker SID/DACL boundary.
    pub fn additional_child_protection(&self) -> AdditionalChildProtection {
        AdditionalChildProtection::RequiresRestrictedWorkerAcl
    }
}

impl Drop for SealedLoadTree {
    fn drop(&mut self) {
        drop(self.handles.take());
        let safe_name = self
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(ROOT_PREFIX));
        let safe_parent = self.root.parent() == Some(self.temp_parent.as_path());
        let verified_root = fs::canonicalize(&self.root)
            .ok()
            .is_some_and(|path| path == self.root);
        if safe_name && safe_parent && verified_root && reject_reparse(&self.root).is_ok() {
            for name in &self.manifest_filenames {
                if validate_basename(name).is_ok() {
                    let path = self.root.join(name);
                    if path.parent() == Some(self.root.as_path()) {
                        let _ = fs::remove_file(path);
                    }
                }
            }
            let _ = fs::remove_dir(&self.root);
        }
    }
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
        "could not allocate a random sealed root",
    ))
}

fn validate_basename(name: &str) -> io::Result<()> {
    if name.is_empty() || Path::new(name).is_absolute() {
        return Err(invalid("filename must be a non-empty relative basename"));
    }
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(invalid(
            "filename must not contain directories or dot components",
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(invalid("filename must not contain separators"));
    }
    Ok(())
}

fn reject_reparse(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || is_reparse(&metadata) {
        return Err(invalid("reparse points are not allowed"));
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
}

#[cfg(not(windows))]
fn is_reparse(_: &fs::Metadata) -> bool {
    false
}

fn hash_file(file: &mut File) -> io::Result<(u64, [u8; 32])> {
    file.rewind()?;
    let mut hash = Sha256::new();
    let size = io::copy(file, &mut hash)?;
    Ok((size, hash.finalize().into()))
}

#[cfg(windows)]
fn open_source(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(windows))]
fn open_source(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[cfg(windows)]
fn create_destination(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
        .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(windows))]
fn create_destination(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(windows)]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    use std::mem::zeroed;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
        || info.nNumberOfLinks != 1
        || !file.metadata()?.is_file()
    {
        return Err(invalid(
            "file must be regular, non-reparse, and have one link",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(invalid("file must be regular and have one link"));
    }
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    if !file.metadata()?.is_file() {
        return Err(invalid("file must be regular"));
    }
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &Path, name: &str, bytes: &[u8]) -> LoadEntry {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        LoadEntry {
            source: path,
            relative_basename: name.to_owned(),
            expected_sha256: Sha256::digest(bytes).into(),
            expected_size: bytes.len() as u64,
        }
    }

    fn source_dir() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("aexcompat-source-{:032x}", rand::random::<u128>()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn seals_main_and_dependencies_and_excludes_unknown_files() {
        let source = source_dir();
        let main = fixture(&source, "main.plugin", b"main");
        let dependency = fixture(&source, "helper.dll", b"dependency");
        fs::write(source.join("unknown.dll"), b"unknown").unwrap();
        let tree = SealedLoadTree::create(main, vec![dependency]).unwrap();
        assert_eq!(fs::read(tree.root().join("main.plugin")).unwrap(), b"main");
        assert_eq!(
            fs::read(tree.root().join("helper.dll")).unwrap(),
            b"dependency"
        );
        assert!(!tree.root().join("unknown.dll").exists());
        assert_ne!(tree.manifest_digest(), [0; 32]);
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn rejects_hash_mismatch() {
        let source = source_dir();
        let mut main = fixture(&source, "main.plugin", b"main");
        main.expected_sha256 = [7; 32];
        assert!(SealedLoadTree::create(main, vec![]).is_err());
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn rejects_case_insensitive_collision() {
        let source = source_dir();
        let main = fixture(&source, "main.plugin", b"main");
        let dependency = fixture(&source, "MAIN.PLUGIN", b"dependency");
        assert!(SealedLoadTree::create(main, vec![dependency]).is_err());
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn dependency_order_does_not_change_manifest_digest() {
        let source = source_dir();
        let main = fixture(&source, "main.plugin", b"main");
        let alpha = fixture(&source, "alpha.dll", b"alpha");
        let zulu = fixture(&source, "zulu.dll", b"zulu");
        let first =
            SealedLoadTree::create(main.clone(), vec![zulu.clone(), alpha.clone()]).unwrap();
        let first_digest = first.manifest_digest();
        drop(first);
        let second = SealedLoadTree::create(main, vec![alpha, zulu]).unwrap();
        assert_eq!(first_digest, second.manifest_digest());
        drop(second);
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn drop_cleans_the_root() {
        let source = source_dir();
        let root = {
            let tree =
                SealedLoadTree::create(fixture(&source, "main.plugin", b"main"), vec![]).unwrap();
            tree.root().to_owned()
        };
        assert!(!root.exists());
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn plugin_path_only_resolves_the_main_manifest_entry() {
        let source = source_dir();
        let main = fixture(&source, "main.plugin", b"main");
        let dependency = fixture(&source, "helper.dll", b"dependency");
        let tree = SealedLoadTree::create(main, vec![dependency]).unwrap();
        assert_eq!(
            tree.plugin_path("main.plugin").unwrap(),
            tree.root().join("main.plugin")
        );
        assert_eq!(
            tree.plugin_path("helper.dll").unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            tree.plugin_path("../main.plugin").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        drop(tree);
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn drop_is_non_recursive_and_leaves_unknown_children() {
        let source = source_dir();
        let tree =
            SealedLoadTree::create(fixture(&source, "main.plugin", b"main"), vec![]).unwrap();
        let root = tree.root().to_owned();
        let unknown = root.join("unknown.dll");
        fs::write(&unknown, b"unknown").unwrap();
        assert_eq!(
            tree.additional_child_protection(),
            AdditionalChildProtection::RequiresRestrictedWorkerAcl
        );
        drop(tree);
        assert!(root.is_dir());
        assert!(unknown.is_file());
        assert!(!root.join("main.plugin").exists());
        fs::remove_file(unknown).unwrap();
        fs::remove_dir(root).unwrap();
        fs::remove_dir_all(source).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rejects_reparse_source() {
        use std::os::windows::fs::symlink_file;
        let source = source_dir();
        let target = fixture(&source, "target.plugin", b"main");
        let link = source.join("main.plugin");
        symlink_file(&target.source, &link).unwrap();
        let entry = LoadEntry {
            source: link,
            relative_basename: "main.plugin".into(),
            ..target
        };
        assert!(SealedLoadTree::create(entry, vec![]).is_err());
        fs::remove_dir_all(source).unwrap();
    }
}
