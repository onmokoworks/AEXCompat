use crate::staging_trust;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

const MANIFEST_DOMAIN: &[u8] = b"AEXCompat sealed load tree manifest\0v1\0";
const ROOT_PREFIX: &str = "aexcompat-sealed-";
const STALE_ROOT_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug)]
pub struct LoadEntry {
    pub source: PathBuf,
    pub relative_basename: String,
    pub expected_sha256: [u8; 32],
    pub expected_size: u64,
}

/// A sealed data resource (issue #362,
/// docs/SEALED_DATA_RESOURCE_POLICY_2026-07-25.md): a non-module file staged
/// into `<root>/<subdir>/<basename>` with exactly the same authentication
/// strength as the DLL entries (size + SHA-256, reparse rejection, retained
/// read-only handle). Data resources are never loadable modules, so the
/// module audit does not classify them; a PE image staged this way would
/// fail the audit closed if anything loaded it (its parent is not the sealed
/// root itself).
#[derive(Clone, Debug)]
pub struct SealedResourceEntry {
    pub source: PathBuf,
    /// `<subdir>/<basename>` with `/` separators; each component follows the
    /// same Windows-safe basename rules as the flat entries. v1 admits
    /// exactly one subdirectory level.
    pub relative_path: String,
    pub expected_sha256: [u8; 32],
    pub expected_size: u64,
}

/// Validates a resource relative path and returns `(subdir, basename)`.
pub fn split_resource_relative_path(relative_path: &str) -> io::Result<(&str, &str)> {
    let Some((subdir, basename)) = relative_path.split_once('/') else {
        return Err(invalid("resource path must be <subdir>/<basename>"));
    };
    if basename.contains('/') {
        return Err(invalid(
            "resource path must have exactly one subdirectory level",
        ));
    }
    validate_basename(subdir)?;
    validate_basename(basename)?;
    Ok((subdir, basename))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdditionalChildProtection {
    /// Manifest files are verified and held, but another principal can still add children.
    RequiresRestrictedWorkerAcl,
}

/// How the entries of a sealed load tree were staged into the root.
#[derive(Clone, Copy, Debug, Default)]
pub struct StagingStats {
    /// Entries staged by hard-linking the source into the sealed root.
    pub hard_linked: u64,
    /// Entries staged by copying after the hard link failed.
    pub copied: u64,
    /// Whether the stale-root sweep before staging completed without error.
    pub stale_cleanup_ok: bool,
}

#[derive(Debug)]
pub struct SealedLoadTree {
    root: PathBuf,
    temp_parent: PathBuf,
    manifest_digest: [u8; 32],
    manifest_filenames: Vec<String>,
    /// Ordered plugin basenames (manifest entries whose role is a plugin);
    /// `plugin_filenames[0]` is the main plugin `plugin_path` resolves. A
    /// single-plugin tree holds exactly one entry; a cluster tree (issue
    /// #405) holds the whole ordered cluster.
    plugin_filenames: Vec<String>,
    handles: Option<Vec<File>>,
}

impl SealedLoadTree {
    pub fn create(main: LoadEntry, dependencies: Vec<LoadEntry>) -> io::Result<Self> {
        Self::create_with_stats(main, dependencies).map(|(tree, _)| tree)
    }

    /// Cluster variant (issue #405): stages the whole ordered plugin cluster
    /// plus the shared dependency closure into one sealed root, so staging,
    /// hashing, and the ACL happen once per cluster instead of once per
    /// plugin. `plugins[0]` keeps the main-plugin manifest role, so a
    /// single-entry cluster tree is byte-identical (manifest digest included)
    /// to what `create` builds.
    pub fn create_cluster(
        plugins: Vec<LoadEntry>,
        dependencies: Vec<LoadEntry>,
    ) -> io::Result<Self> {
        Self::create_cluster_with_stats(plugins, dependencies).map(|(tree, _)| tree)
    }

    /// Like [`Self::create`], but also reports how each entry was staged.
    pub fn create_with_stats(
        main: LoadEntry,
        dependencies: Vec<LoadEntry>,
    ) -> io::Result<(Self, StagingStats)> {
        Self::create_cluster_with_stats(vec![main], dependencies)
    }

    /// Cluster variant of [`Self::create_with_stats`] (issue #405): the whole
    /// ordered cluster is staged under one staging-statistics report.
    pub fn create_cluster_with_stats(
        plugins: Vec<LoadEntry>,
        dependencies: Vec<LoadEntry>,
    ) -> io::Result<(Self, StagingStats)> {
        Self::create_cluster_with_resources(plugins, dependencies, Vec::new())
    }

    /// Like [`Self::create`], but also seals data resources (issue #362):
    /// authenticated non-module files staged into `<root>/<subdir>/`.
    pub fn create_with_resources(
        main: LoadEntry,
        dependencies: Vec<LoadEntry>,
        resources: Vec<SealedResourceEntry>,
    ) -> io::Result<Self> {
        Self::create_cluster_with_resources(vec![main], dependencies, resources)
            .map(|(tree, _)| tree)
    }

    /// Cluster variant of [`Self::create_with_resources`], retaining the
    /// staging-statistics report.
    pub fn create_cluster_with_resources(
        plugins: Vec<LoadEntry>,
        dependencies: Vec<LoadEntry>,
        resources: Vec<SealedResourceEntry>,
    ) -> io::Result<(Self, StagingStats)> {
        let temp_parent = fs::canonicalize(std::env::temp_dir())?;
        reject_reparse(&temp_parent)?;
        Self::create_at_with_stats(&temp_parent, plugins, dependencies, resources)
    }

    #[cfg(test)]
    fn create_at(
        temp_parent: &Path,
        plugins: Vec<LoadEntry>,
        dependencies: Vec<LoadEntry>,
    ) -> io::Result<Self> {
        Self::create_at_with_stats(temp_parent, plugins, dependencies, Vec::new())
            .map(|(tree, _)| tree)
    }

    fn create_at_with_stats(
        temp_parent: &Path,
        plugins: Vec<LoadEntry>,
        dependencies: Vec<LoadEntry>,
        resources: Vec<SealedResourceEntry>,
    ) -> io::Result<(Self, StagingStats)> {
        let temp_parent = fs::canonicalize(temp_parent)?;
        reject_reparse(&temp_parent)?;
        let mut stats = StagingStats {
            stale_cleanup_ok: cleanup_stale_roots(&temp_parent, SystemTime::now(), STALE_ROOT_AGE)
                .is_ok(),
            ..StagingStats::default()
        };
        let root = create_random_root(&temp_parent)?;
        let result = Self::populate(
            root.clone(),
            temp_parent.clone(),
            plugins,
            dependencies,
            resources,
        );
        if result.is_err() {
            let _ = remove_owned_root_impl(&root, &temp_parent, true);
        }
        result.map(|(tree, populate_stats)| {
            stats.hard_linked = populate_stats.hard_linked;
            stats.copied = populate_stats.copied;
            (tree, stats)
        })
    }

    fn populate(
        root: PathBuf,
        temp_parent: PathBuf,
        plugins: Vec<LoadEntry>,
        mut dependencies: Vec<LoadEntry>,
        resources: Vec<SealedResourceEntry>,
    ) -> io::Result<(Self, StagingStats)> {
        let mut plugins = plugins.into_iter();
        let Some(main) = plugins.next() else {
            return Err(invalid("a sealed load tree requires at least one plugin"));
        };
        dependencies.sort_by_key(|entry| entry.relative_basename.to_lowercase());
        let entry_count = dependencies.len() + plugins.len() + 1 + resources.len();
        let mut entries = Vec::with_capacity(entry_count);
        // Manifest roles: 0 = main plugin (byte-compatible with the original
        // single-plugin layout), 2 = additional cluster plugin, 1 = dependency.
        entries.push((0u8, main));
        entries.extend(plugins.map(|entry| (2u8, entry)));
        entries.extend(dependencies.into_iter().map(|entry| (1u8, entry)));

        let mut names = HashSet::new();
        let mut plugin_filenames = Vec::new();
        for (role, entry) in &entries {
            validate_basename(&entry.relative_basename)?;
            let folded = entry.relative_basename.to_lowercase();
            if !names.insert(folded) {
                return Err(invalid("duplicate or case-insensitive filename collision"));
            }
            if *role != 1 {
                plugin_filenames.push(entry.relative_basename.clone());
            }
        }

        let mut handles = Vec::with_capacity(entry_count);
        let mut manifest_filenames = Vec::with_capacity(entry_count);
        let mut stats = StagingStats::default();
        let mut manifest = Sha256::new();
        manifest.update(MANIFEST_DOMAIN);
        manifest.update((entry_count as u64).to_le_bytes());

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

            let destination = root.join(&entry.relative_basename);
            if destination.parent() != Some(root.as_path()) {
                return Err(invalid("destination must be a direct child"));
            }
            let retained = stage_and_retain(
                &entry.source,
                entry.expected_sha256,
                entry.expected_size,
                &destination,
                &mut stats,
            )?;

            manifest.update([role]);
            manifest.update((entry.relative_basename.len() as u64).to_le_bytes());
            manifest.update(entry.relative_basename.as_bytes());
            manifest.update(entry.expected_size.to_le_bytes());
            manifest.update(entry.expected_sha256);
            manifest_filenames.push(entry.relative_basename);
            handles.push(retained);
        }

        // Data resources (role 3, issue #362): authenticated non-module files
        // staged into `<root>/<subdir>/<basename>` with the same strength.
        let mut resource_names = HashSet::new();
        let mut resource_subdirs = HashSet::new();
        for resource in resources {
            let (subdir, basename) = split_resource_relative_path(&resource.relative_path)?;
            if !resource_names.insert(resource.relative_path.to_lowercase()) {
                return Err(invalid(
                    "duplicate or case-insensitive resource path collision",
                ));
            }
            // The subdirectory name itself enters the flat namespace: a DLL
            // or plugin must not share it, or one could shadow the other.
            if resource_subdirs.insert(subdir.to_lowercase())
                && !names.insert(subdir.to_lowercase())
            {
                return Err(invalid(
                    "resource subdirectory collides with a flat manifest entry",
                ));
            }
            let source_parent = resource
                .source
                .parent()
                .ok_or_else(|| invalid("resource source has no direct parent"))?;
            reject_reparse(source_parent)?;
            let canonical_parent = fs::canonicalize(source_parent)?;
            reject_reparse(&canonical_parent)?;
            let source_name = resource
                .source
                .file_name()
                .ok_or_else(|| invalid("resource source has no filename"))?;
            if source_name != basename {
                return Err(invalid(
                    "resource source filename differs from its relative basename",
                ));
            }
            if resource.source.parent() != Some(source_parent) {
                return Err(invalid("resource source must be a direct child"));
            }

            let subdir_path = root.join(subdir);
            if subdir_path.parent() != Some(root.as_path()) {
                return Err(invalid("resource subdirectory must be a direct child"));
            }
            if !subdir_path.exists() {
                fs::create_dir(&subdir_path)?;
            }
            if !subdir_path.is_dir() {
                return Err(invalid("resource subdirectory is not a directory"));
            }
            reject_reparse(&subdir_path)?;
            let destination = subdir_path.join(basename);
            if destination.parent() != Some(subdir_path.as_path()) {
                return Err(invalid("resource destination must be a direct child"));
            }
            let retained = stage_and_retain(
                &resource.source,
                resource.expected_sha256,
                resource.expected_size,
                &destination,
                &mut stats,
            )?;

            manifest.update([3u8]);
            manifest.update((resource.relative_path.len() as u64).to_le_bytes());
            manifest.update(resource.relative_path.as_bytes());
            manifest.update(resource.expected_size.to_le_bytes());
            manifest.update(resource.expected_sha256);
            manifest_filenames.push(resource.relative_path);
            handles.push(retained);
        }

        Ok((
            Self {
                root,
                temp_parent,
                manifest_digest: manifest.finalize().into(),
                manifest_filenames,
                plugin_filenames,
                handles: Some(handles),
            },
            stats,
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest_digest(&self) -> [u8; 32] {
        self.manifest_digest
    }

    /// Returns the validated manifest entry names authenticated by the
    /// manifest (flat basenames for images, `<subdir>/<basename>` relative
    /// paths for data resources).
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

    /// Cluster plugin resolution (issue #405): like `plugin_path`, but accepts
    /// any manifest entry whose role is a plugin — every cluster member, not
    /// just the main one. Dependencies stay unresolvable through this API: a
    /// caller handed a sealed root can never turn a dependency basename into
    /// a plugin path.
    pub fn cluster_plugin_path(&self, plugin_basename: &str) -> io::Result<PathBuf> {
        validate_basename(plugin_basename)?;
        if !self
            .plugin_filenames
            .iter()
            .any(|name| name == plugin_basename)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "plugin basename is not an authenticated cluster plugin entry",
            ));
        }
        Ok(self.root.join(plugin_basename))
    }

    /// The ordered plugin basenames of the cluster (`[0]` is the main plugin).
    pub fn cluster_plugin_basenames(&self) -> &[String] {
        &self.plugin_filenames
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
            let mut subdirs = HashSet::new();
            for name in &self.manifest_filenames {
                if let Ok((subdir, basename)) = split_resource_relative_path(name) {
                    // Data resource (issue #362): remove the staged file, then
                    // its subdirectory after every staged file is gone.
                    let subdir_path = self.root.join(subdir);
                    if subdir_path.parent() == Some(self.root.as_path()) {
                        let path = subdir_path.join(basename);
                        if path.parent() == Some(subdir_path.as_path()) {
                            let _ = fs::remove_file(path);
                        }
                        subdirs.insert(subdir.to_lowercase());
                    }
                } else if validate_basename(name).is_ok() {
                    let path = self.root.join(name);
                    if path.parent() == Some(self.root.as_path()) {
                        let _ = fs::remove_file(path);
                    }
                }
            }
            for subdir in subdirs {
                let _ = fs::remove_dir(self.root.join(subdir));
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

fn cleanup_stale_roots(parent: &Path, now: SystemTime, age: Duration) -> io::Result<()> {
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let root = entry.path();
        if !is_owned_root(&root, parent) {
            continue;
        }
        let modified = match fs::symlink_metadata(&root).and_then(|metadata| metadata.modified()) {
            Ok(modified) => modified,
            Err(_) => continue,
        };
        // Require both the root and every direct child to be stale. A creator
        // can expose a fresh non-empty root between create_dir and population;
        // keeping it until observed entries age out avoids deleting a live or
        // incomplete tree. remove_owned_root retains the fail-closed checks.
        let root_old_enough = now.duration_since(modified).unwrap_or_default() >= age;
        if !root_old_enough {
            continue;
        }

        let children = match fs::read_dir(&root) {
            Ok(children) => children,
            Err(_) => continue,
        };
        let mut all_children_stale = true;
        for child in children {
            let child = match child {
                Ok(child) => child,
                Err(_) => {
                    all_children_stale = false;
                    break;
                }
            };
            let child_modified =
                match fs::symlink_metadata(child.path()).and_then(|metadata| metadata.modified()) {
                    Ok(modified) => modified,
                    Err(_) => {
                        all_children_stale = false;
                        break;
                    }
                };
            if now.duration_since(child_modified).unwrap_or_default() < age {
                all_children_stale = false;
                break;
            }
        }
        if all_children_stale {
            let _ = remove_owned_root(&root, parent);
        }
    }
    Ok(())
}

fn remove_owned_root(root: &Path, parent: &Path) -> io::Result<bool> {
    remove_owned_root_impl(root, parent, false)
}

/// `allow_subdirs` is set only for the create-time failure cleanup, which
/// removes exactly the tree it just populated (data-resource subdirectories
/// included, issue #362). The stale sweep keeps the conservative
/// never-recurse behavior for content it did not create.
fn remove_owned_root_impl(root: &Path, parent: &Path, allow_subdirs: bool) -> io::Result<bool> {
    if !is_owned_root(root, parent) {
        return Ok(false);
    }

    let mut children = Vec::new();
    let mut subdirs = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Ok(false);
        }
        if metadata.file_type().is_dir() {
            if !allow_subdirs {
                return Ok(false);
            }
            // Data-resource subdirectories: only plain directories holding
            // only plain files are removed; anything else fails closed and
            // leaves the root in place.
            for sub_entry in fs::read_dir(&child)? {
                let sub_entry = sub_entry?;
                let sub_metadata = fs::symlink_metadata(sub_entry.path())?;
                if sub_metadata.file_type().is_symlink()
                    || is_reparse(&sub_metadata)
                    || !sub_metadata.file_type().is_file()
                {
                    return Ok(false);
                }
            }
            subdirs.push(child);
            continue;
        }
        if !metadata.file_type().is_file() {
            return Ok(false);
        }
        let mut file = match open_source(&child) {
            Ok(file) => file,
            Err(_) => return Ok(false),
        };
        if validate_regular_staged(&file).is_err() {
            return Ok(false);
        }
        file.rewind()?;
        children.push(child);
    }

    for child in children {
        if fs::remove_file(child).is_err() {
            return Ok(false);
        }
    }
    for subdir in subdirs {
        if fs::remove_dir_all(subdir).is_err() {
            return Ok(false);
        }
    }
    Ok(fs::remove_dir(root).is_ok())
}

fn is_owned_root(root: &Path, parent: &Path) -> bool {
    let Some(name) = root.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(suffix) = name.strip_prefix(ROOT_PREFIX) else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        && root.parent() == Some(parent)
        && reject_reparse(root).is_ok()
        && fs::canonicalize(root).ok().is_some_and(|path| path == root)
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

/// Stages one authenticated entry into `destination` and returns the
/// retained read-only, no-delete-share handle. The source is re-hashed
/// before staging, the staged bytes are re-verified (hard-link first, copy
/// with post-copy verification on failure), and the retained handle is
/// re-hashed — identical strength for DLL entries and data resources.
fn stage_and_retain(
    source_path: &Path,
    expected_sha256: [u8; 32],
    expected_size: u64,
    destination: &Path,
    stats: &mut StagingStats,
) -> io::Result<File> {
    reject_reparse(source_path)?;
    let mut source = open_source(source_path)?;
    validate_regular_unique(&source)?;
    let (size, digest) = staging_trust::hash_file_trusted(&mut source, hash_file)?;
    if size != expected_size {
        return Err(invalid("source size mismatch"));
    }
    if digest != expected_sha256 {
        return Err(invalid("source SHA-256 mismatch"));
    }
    source.rewind()?;

    let staged_by_hard_link = fs::hard_link(source_path, destination).is_ok();
    if staged_by_hard_link {
        stats.hard_linked += 1;
    } else {
        stats.copied += 1;
        let mut output = create_destination(destination)?;
        io::copy(&mut source, &mut output)?;
        output.flush()?;
        output.sync_all()?;
        reject_reparse(destination)?;
        validate_regular_unique(&output)?;
        let mut verify = output.try_clone()?;
        let (copied_size, copied_digest) = hash_file(&mut verify)?;
        if copied_size != expected_size || copied_digest != expected_sha256 {
            return Err(invalid("destination verification failed"));
        }
        drop(verify);
        drop(output);
    }
    reject_reparse(destination)?;

    // A retained write-capable handle conflicts with the Windows image loader,
    // whose reopen does not share writes. Reopen read-only and authenticate the
    // exact path again before retaining the no-write/no-delete-share handle.
    let mut retained = open_source(destination)?;
    if staged_by_hard_link {
        validate_regular_staged(&retained)?;
    } else {
        validate_regular_unique(&retained)?;
    }
    let (retained_size, retained_digest) =
        staging_trust::hash_file_trusted(&mut retained, hash_file)?;
    if retained_size != expected_size || retained_digest != expected_sha256 {
        return Err(invalid("retained destination verification failed"));
    }
    Ok(retained)
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
fn validate_regular_staged(file: &File) -> io::Result<()> {
    use std::mem::zeroed;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
        || info.nNumberOfLinks == 0
        || !file.metadata()?.is_file()
    {
        return Err(invalid("file must be regular and non-reparse"));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_regular_staged(file: &File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() == 0 {
        return Err(invalid("file must be regular and have a link"));
    }
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn validate_regular_staged(file: &File) -> io::Result<()> {
    if !file.metadata()?.is_file() {
        return Err(invalid("file must be regular"));
    }
    Ok(())
}

#[cfg(windows)]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    validate_regular_staged(file)?;
    use std::mem::zeroed;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.nNumberOfLinks != 1 {
        return Err(invalid("file must have one link"));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    validate_regular_staged(file)?;
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if metadata.nlink() != 1 {
        return Err(invalid("file must have one link"));
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

    fn resource_fixture(dir: &Path, relative_path: &str, bytes: &[u8]) -> SealedResourceEntry {
        let (_, basename) = relative_path.rsplit_once('/').unwrap();
        let path = dir.join(basename);
        fs::write(&path, bytes).unwrap();
        SealedResourceEntry {
            source: path,
            relative_path: relative_path.to_owned(),
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

    #[cfg(windows)]
    fn file_identity(path: &Path) -> (u32, u64) {
        use std::mem::zeroed;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let file = File::open(path).unwrap();
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
        assert_ne!(ok, 0);
        (
            info.dwVolumeSerialNumber,
            ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
        )
    }

    #[cfg(unix)]
    fn file_identity(path: &Path) -> (u64, u64) {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path).unwrap();
        (metadata.dev(), metadata.ino())
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
    fn stages_a_same_volume_source_as_a_hard_link() {
        let parent = source_dir();
        let main = fixture(&parent, "main.plugin", b"main");
        let source_path = main.source.clone();
        let tree = SealedLoadTree::create_at(&parent, vec![main], vec![]).unwrap();
        let staged_path = tree.root().join("main.plugin");

        assert_eq!(file_identity(&source_path), file_identity(&staged_path));
        assert_eq!(fs::read(staged_path).unwrap(), b"main");
        drop(tree);
        assert!(source_path.is_file());
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn create_with_stats_reports_staging_counts() {
        let source = source_dir();
        let main = fixture(&source, "main.plugin", b"main");
        let dependency = fixture(&source, "helper.dll", b"dependency");
        let (tree, stats) = SealedLoadTree::create_with_stats(main, vec![dependency]).unwrap();
        assert_eq!(stats.hard_linked + stats.copied, 2);
        assert!(stats.stale_cleanup_ok);
        drop(tree);
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn stale_cleanup_removes_hard_linked_children_without_removing_source() {
        let parent = fs::canonicalize(source_dir()).unwrap();
        let source = parent.join("source.dll");
        fs::write(&source, b"source").unwrap();
        let root = parent.join(format!("{ROOT_PREFIX}{:032x}", 4u128));
        fs::create_dir(&root).unwrap();
        fs::hard_link(&source, root.join("source.dll")).unwrap();

        cleanup_stale_roots(
            &parent,
            SystemTime::now() + Duration::from_secs(24 * 60 * 60),
            Duration::from_secs(24 * 60 * 60),
        )
        .unwrap();

        assert!(!root.exists());
        assert_eq!(fs::read(source).unwrap(), b"source");
        fs::remove_dir_all(parent).unwrap();
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

    #[test]
    fn failed_population_removes_partial_owned_root() {
        let parent = source_dir();
        let source = parent.join("source");
        fs::create_dir(&source).unwrap();
        let main = fixture(&source, "main.plugin", b"main");
        let mut dependency = fixture(&source, "helper.dll", b"dependency");
        dependency.expected_sha256 = [9; 32];
        assert!(SealedLoadTree::create_at(&parent, vec![main], vec![dependency]).is_err());

        let parent_leftovers = fs::read_dir(&parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(ROOT_PREFIX))
            })
            .count();
        assert_eq!(parent_leftovers, 0, "partial sealed roots were left behind");
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn stale_cleanup_removes_stale_unlocked_owned_roots() {
        let parent = fs::canonicalize(source_dir()).unwrap();
        let root = parent.join(format!("{ROOT_PREFIX}{:032x}", 1u128));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("partial.dll"), b"partial").unwrap();

        cleanup_stale_roots(
            &parent,
            SystemTime::now() + Duration::from_secs(24 * 60 * 60),
            Duration::from_secs(24 * 60 * 60),
        )
        .unwrap();
        assert!(!root.exists());
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn stale_cleanup_keeps_fresh_non_empty_owned_root() {
        let parent = fs::canonicalize(source_dir()).unwrap();
        let root = parent.join(format!("{ROOT_PREFIX}{:032x}", 5u128));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("partial.dll"), b"partial").unwrap();

        cleanup_stale_roots(
            &parent,
            SystemTime::now(),
            Duration::from_secs(24 * 60 * 60),
        )
        .unwrap();
        assert!(root.exists());
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn stale_cleanup_keeps_a_fresh_empty_owned_root() {
        let parent = fs::canonicalize(source_dir()).unwrap();
        let root = parent.join(format!("{ROOT_PREFIX}{:032x}", 3u128));
        fs::create_dir(&root).unwrap();

        cleanup_stale_roots(
            &parent,
            SystemTime::now(),
            Duration::from_secs(24 * 60 * 60),
        )
        .unwrap();
        assert!(root.exists());
        fs::remove_dir(root).unwrap();
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn stale_cleanup_does_not_recurse_into_unexpected_children() {
        let parent = fs::canonicalize(source_dir()).unwrap();
        let root = parent.join(format!("{ROOT_PREFIX}{:032x}", 2u128));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("unexpected")).unwrap();

        cleanup_stale_roots(
            &parent,
            SystemTime::now() + Duration::from_secs(2 * 24 * 60 * 60),
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(root.exists());
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn trusted_profile_restages_unchanged_sources() {
        crate::staging_trust::set_enabled_override_for_testing(Some(true));
        // A private parent keeps this test's sweeps out of the shared temp root.
        let parent = source_dir();
        let main = fixture(&parent, "main.plugin", b"main");
        let dependency = fixture(&parent, "helper.dll", b"dependency");
        let first =
            SealedLoadTree::create_at(&parent, vec![main.clone()], vec![dependency.clone()])
                .unwrap();
        drop(first);

        let second = SealedLoadTree::create_at(&parent, vec![main], vec![dependency]).unwrap();
        assert_eq!(
            second.manifest_basenames(),
            &["main.plugin".to_owned(), "helper.dll".to_owned()]
        );
        assert_eq!(
            fs::read(second.root().join("helper.dll")).unwrap(),
            b"dependency"
        );
        drop(second);
        crate::staging_trust::set_enabled_override_for_testing(None);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn trusted_profile_still_fails_closed_on_a_modified_source() {
        crate::staging_trust::set_enabled_override_for_testing(Some(true));
        let parent = source_dir();
        let main = fixture(&parent, "main.plugin", b"main");
        let first = SealedLoadTree::create_at(&parent, vec![main.clone()], vec![]).unwrap();
        drop(first);

        // Different length, so the cache miss cannot hinge on mtime granularity.
        fs::write(&main.source, b"main-with-more-bytes").unwrap();
        let error = SealedLoadTree::create_at(&parent, vec![main], vec![]).unwrap_err();
        assert_eq!(error.to_string(), "source size mismatch");
        crate::staging_trust::set_enabled_override_for_testing(None);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn cluster_tree_stages_every_plugin_and_resolves_them_by_basename() {
        let source = source_dir();
        let alpha = fixture(&source, "alpha.plugin", b"alpha");
        let beta = fixture(&source, "beta.plugin", b"beta");
        let dependency = fixture(&source, "helper.dll", b"dependency");
        let tree = SealedLoadTree::create_cluster(vec![alpha, beta], vec![dependency]).unwrap();
        assert_eq!(
            fs::read(tree.root().join("alpha.plugin")).unwrap(),
            b"alpha"
        );
        assert_eq!(fs::read(tree.root().join("beta.plugin")).unwrap(), b"beta");
        assert_eq!(
            tree.cluster_plugin_basenames(),
            &["alpha.plugin".to_owned(), "beta.plugin".to_owned()]
        );
        // Every cluster plugin resolves; a dependency or an unknown basename
        // never resolves as a plugin, and the main-plugin API keeps accepting
        // only the first entry.
        assert_eq!(
            tree.cluster_plugin_path("beta.plugin").unwrap(),
            tree.root().join("beta.plugin")
        );
        assert_eq!(
            tree.cluster_plugin_path("helper.dll").unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            tree.cluster_plugin_path("unknown.plugin")
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            tree.plugin_path("beta.plugin").unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            tree.plugin_path("alpha.plugin").unwrap(),
            tree.root().join("alpha.plugin")
        );
        drop(tree);
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn single_plugin_cluster_tree_matches_create() {
        let source = source_dir();
        let main = fixture(&source, "main.plugin", b"main");
        let dependency = fixture(&source, "helper.dll", b"dependency");
        let plain = SealedLoadTree::create(main.clone(), vec![dependency.clone()]).unwrap();
        let plain_digest = plain.manifest_digest();
        drop(plain);
        let clustered = SealedLoadTree::create_cluster(vec![main], vec![dependency]).unwrap();
        assert_eq!(plain_digest, clustered.manifest_digest());
        assert_eq!(
            clustered.cluster_plugin_basenames(),
            &["main.plugin".to_owned()]
        );
        drop(clustered);
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn cluster_tree_rejects_an_empty_plugin_list() {
        let source = source_dir();
        let dependency = fixture(&source, "helper.dll", b"dependency");
        assert!(SealedLoadTree::create_cluster(vec![], vec![dependency]).is_err());
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn resources_stage_into_one_subdirectory_with_the_same_authentication() {
        let source = source_dir();
        let main = fixture(&source, "main.plugin", b"main");
        let grain = resource_fixture(&source, "Film Stocks/100T.grain", b"grain-bytes");
        let plain = SealedLoadTree::create(main.clone(), vec![]).unwrap();
        let plain_digest = plain.manifest_digest();
        drop(plain);
        let tree = SealedLoadTree::create_with_resources(main, vec![], vec![grain]).unwrap();
        // A resource-carrying tree digests differently (role 3 + path).
        assert_ne!(plain_digest, tree.manifest_digest());
        let staged = tree.root().join("Film Stocks").join("100T.grain");
        assert_eq!(fs::read(&staged).unwrap(), b"grain-bytes");
        assert_eq!(
            tree.manifest_basenames(),
            &[
                "main.plugin".to_owned(),
                "Film Stocks/100T.grain".to_owned()
            ]
        );
        let root = tree.root().to_path_buf();
        drop(tree);
        // Drop removes the staged file, the subdirectory, and the root.
        assert!(
            !root.exists(),
            "drop removes the whole tree including subdirs"
        );
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn resource_paths_reject_traversal_nesting_and_collisions() {
        let source = source_dir();
        for bad in [
            "../evil.grain",
            "a/b/c.grain",
            "Film Stocks",
            "Film Stocks/",
            "/abs.grain",
        ] {
            assert!(
                split_resource_relative_path(bad).is_err(),
                "{bad} must be rejected"
            );
        }
        // The subdirectory name must not collide with a flat manifest entry.
        let main = fixture(&source, "main.plugin", b"main");
        let collision = resource_fixture(&source, "main.plugin/x.grain", b"x");
        assert!(SealedLoadTree::create_with_resources(main, vec![], vec![collision]).is_err());
        // Duplicate resource paths fail closed, case-insensitively.
        let main = fixture(&source, "main.plugin", b"main");
        let first = resource_fixture(&source, "Film Stocks/a.grain", b"a");
        let second = resource_fixture(&source, "Film Stocks/A.grain", b"a2");
        assert!(SealedLoadTree::create_with_resources(main, vec![], vec![first, second]).is_err());
        // The staged basename must match the source file name.
        let main = fixture(&source, "main.plugin", b"main");
        let mut mismatched = resource_fixture(&source, "Film Stocks/other.grain", b"other");
        mismatched.source = source.join("main.plugin");
        assert!(SealedLoadTree::create_with_resources(main, vec![], vec![mismatched]).is_err());
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
