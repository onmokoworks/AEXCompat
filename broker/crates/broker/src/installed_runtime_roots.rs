//! Bounded discovery of runtime DLL directories below Windows-registered installs.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const MAX_INSTALL_ROOTS: usize = 256;
const MAX_VISITED_DIRS: usize = 4096;
const MAX_DEPTH: usize = 3;
const MAX_MATCHES: usize = 15;
const MAX_INDEXED_FILES: usize = 262_144;
const MAX_INDEX_METADATA_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
struct IndexedRuntimeFile {
    path: PathBuf,
    directory: PathBuf,
    install_root: PathBuf,
    install_root_key: String,
    len: u64,
}

/// One immutable, bounded view of the registered install forest.
///
/// Building the view is the only operation that enumerates the forest.
/// Association and dependency lookup are basename-indexed afterwards, so a
/// corpus containing many relocated AEX copies does not repeat the same walk.
#[derive(Debug)]
pub struct RegisteredRuntimeIndex {
    by_basename: HashMap<String, Vec<IndexedRuntimeFile>>,
    install_roots: Vec<PathBuf>,
    snapshot_id: String,
    visited_directories: usize,
    truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegisteredRuntimeLookup {
    Found(BTreeMap<String, Vec<PathBuf>>),
    IndexTruncated,
}

pub fn unique_runtime_roots(found: BTreeMap<String, Vec<PathBuf>>, limit: usize) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = found.into_values().flatten().collect();
    roots.sort_by_key(|path| path_key(path));
    roots.dedup_by(|left, right| path_key(left) == path_key(right));
    roots.truncate(limit);
    roots
}

impl RegisteredRuntimeIndex {
    pub fn from_install_roots(install_roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self::from_install_roots_with_limits(
            install_roots,
            MAX_INSTALL_ROOTS,
            MAX_VISITED_DIRS,
            MAX_INDEXED_FILES,
            MAX_INDEX_METADATA_BYTES,
        )
    }

    fn from_install_roots_with_limits(
        install_roots: impl IntoIterator<Item = PathBuf>,
        max_install_roots: usize,
        max_visited_dirs: usize,
        max_indexed_files: usize,
        max_metadata_bytes: usize,
    ) -> Self {
        let mut roots = Vec::new();
        let mut seen_roots = HashSet::new();
        let mut truncated = false;
        for (index, root) in install_roots.into_iter().enumerate() {
            if index == max_install_roots {
                truncated = true;
                break;
            }
            let Ok(root) = root.canonicalize() else {
                continue;
            };
            if !root.is_dir() {
                continue;
            }
            let key = path_key(&root);
            if seen_roots.insert(key.clone()) {
                roots.push((root, key));
            }
        }
        roots.sort_by(|left, right| left.1.cmp(&right.1));

        let mut by_basename: HashMap<String, Vec<IndexedRuntimeFile>> = HashMap::new();
        let mut snapshot_records = Vec::new();
        let mut visited_directories = 0usize;
        let mut indexed_files = 0usize;
        let mut metadata_bytes = 0usize;
        let install_roots: Vec<PathBuf> = roots.iter().map(|(root, _)| root.clone()).collect();
        for (root, root_key) in roots {
            let mut queue = VecDeque::from([(root.clone(), 0usize)]);
            let mut visited_for_root = 0usize;
            while let Some((directory, depth)) = queue.pop_front() {
                if visited_for_root == max_visited_dirs {
                    truncated = true;
                    break;
                }
                visited_for_root += 1;
                visited_directories += 1;
                let Ok(entries) = std::fs::read_dir(&directory) else {
                    continue;
                };
                let mut children = Vec::new();
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if !path
                            .extension()
                            .and_then(|value| value.to_str())
                            .is_some_and(|value| {
                                value.eq_ignore_ascii_case("dll")
                                    || value.eq_ignore_ascii_case("aex")
                            })
                        {
                            continue;
                        }
                        let Some(basename) = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(str::to_ascii_lowercase)
                        else {
                            continue;
                        };
                        let Ok(metadata) = path.metadata() else {
                            continue;
                        };
                        let modified = metadata
                            .modified()
                            .ok()
                            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|value| (value.as_secs(), value.subsec_nanos()))
                            .unwrap_or_default();
                        let directory_key = path_key(&directory);
                        let record = format!(
                            "{root_key}\0{directory_key}\0{basename}\0{}\0{}:{}",
                            metadata.len(),
                            modified.0,
                            modified.1
                        );
                        if indexed_files == max_indexed_files
                            || metadata_bytes.saturating_add(record.len()) > max_metadata_bytes
                        {
                            truncated = true;
                            break;
                        }
                        indexed_files += 1;
                        metadata_bytes += record.len();
                        snapshot_records.push(record);
                        by_basename
                            .entry(basename)
                            .or_default()
                            .push(IndexedRuntimeFile {
                                path,
                                directory: directory.clone(),
                                install_root: root.clone(),
                                install_root_key: root_key.clone(),
                                len: metadata.len(),
                            });
                    } else if depth < MAX_DEPTH && path.is_dir() {
                        children.push(path);
                    }
                }
                if truncated {
                    break;
                }
                if depth < MAX_DEPTH {
                    children.sort_by_key(|path| path_key(path));
                    queue.extend(children.into_iter().map(|child| (child, depth + 1)));
                }
            }
            if truncated {
                break;
            }
        }
        for files in by_basename.values_mut() {
            files.sort_by_key(|file| path_key(&file.directory));
        }
        snapshot_records.sort();
        let mut snapshot = Sha256::new();
        for record in snapshot_records {
            snapshot.update(record.as_bytes());
            snapshot.update([0]);
        }
        Self {
            by_basename,
            install_roots,
            snapshot_id: hex_lower(&snapshot.finalize()),
            visited_directories,
            truncated,
        }
    }

    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    pub fn visited_directories(&self) -> usize {
        self.visited_directories
    }

    pub fn associated_install_roots(&self, plugin: &Path) -> Vec<PathBuf> {
        let Ok(plugin_bytes) = std::fs::read(plugin) else {
            return Vec::new();
        };
        let Some(plugin_name) = plugin
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_ascii_lowercase)
        else {
            return Vec::new();
        };
        let expected = Sha256::digest(&plugin_bytes);
        let expected_len = plugin_bytes.len() as u64;
        let mut associated = BTreeMap::new();
        for candidate in self
            .by_basename
            .get(&plugin_name)
            .into_iter()
            .flatten()
            .filter(|candidate| candidate.len == expected_len)
        {
            if std::fs::read(&candidate.path).is_ok_and(|bytes| Sha256::digest(bytes) == expected) {
                associated
                    .entry(candidate.install_root_key.clone())
                    .or_insert_with(|| candidate.install_root.clone());
            }
        }
        associated.into_values().collect()
    }

    pub fn matching_roots_by_basename(
        &self,
        unresolved_basenames: &[String],
        associated_install_roots: &[PathBuf],
    ) -> RegisteredRuntimeLookup {
        if self.truncated {
            return RegisteredRuntimeLookup::IndexTruncated;
        }
        let associated: HashSet<String> = associated_install_roots
            .iter()
            .map(|root| path_key(root))
            .collect();
        let mut result = BTreeMap::new();
        for basename in normalized_basenames(unresolved_basenames) {
            let mut directories: Vec<PathBuf> = self
                .by_basename
                .get(&basename)
                .into_iter()
                .flatten()
                .filter(|candidate| associated.contains(&candidate.install_root_key))
                .map(|candidate| candidate.directory.clone())
                .collect();
            directories.sort_by_key(|path| path_key(path));
            directories.dedup_by(|left, right| path_key(left) == path_key(right));
            result.insert(basename, directories);
        }
        RegisteredRuntimeLookup::Found(result)
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn normalized_basenames(unresolved_basenames: &[String]) -> Vec<String> {
    let mut wanted: Vec<String> = unresolved_basenames
        .iter()
        .filter(|name| Path::new(name).file_name().and_then(|value| value.to_str()) == Some(name))
        .map(|name| name.to_ascii_lowercase())
        .collect();
    wanted.sort();
    wanted.dedup();
    wanted
}

#[derive(Default)]
struct LaunchRuntimeIndex(OnceLock<Arc<RegisteredRuntimeIndex>>);

impl LaunchRuntimeIndex {
    fn get_or_init_with(
        &self,
        build: impl FnOnce() -> RegisteredRuntimeIndex,
    ) -> Arc<RegisteredRuntimeIndex> {
        Arc::clone(self.0.get_or_init(|| Arc::new(build())))
    }

    fn matching_with(
        &self,
        unresolved_basenames: &[String],
        build: impl FnOnce() -> RegisteredRuntimeIndex,
    ) -> RegisteredRuntimeLookup {
        let index = self.get_or_init_with(build);
        index.matching_roots_by_basename(unresolved_basenames, &index.install_roots)
    }
}

fn registered_runtime_index() -> Arc<RegisteredRuntimeIndex> {
    // The registered forest is a process-launch snapshot. Re-probing nested
    // mtimes would itself repeat the expensive tree walk and cannot be made
    // race-free; a new shipping host process creates the next snapshot.
    launch_runtime_index().get_or_init_with(|| {
        RegisteredRuntimeIndex::from_install_roots(registered_install_locations())
    })
}

fn launch_runtime_index() -> &'static LaunchRuntimeIndex {
    static INDEX: LaunchRuntimeIndex = LaunchRuntimeIndex(OnceLock::new());
    &INDEX
}

pub fn registered_runtime_snapshot_id() -> String {
    registered_runtime_index().snapshot_id().to_owned()
}

pub fn matching_registered_runtime_roots(
    unresolved_basenames: &[String],
) -> RegisteredRuntimeLookup {
    launch_runtime_index().matching_with(unresolved_basenames, || {
        RegisteredRuntimeIndex::from_install_roots(registered_install_locations())
    })
}

/// Restrict runtime lookup to registered install trees that contain a byte-
/// identical copy of the selected plug-in. This associates a relocated host
/// copy with its own installer without guessing from vendor names or paths.
pub fn matching_registered_runtime_roots_for_plugin(
    plugin: &Path,
    unresolved_basenames: &[String],
) -> RegisteredRuntimeLookup {
    let associated = associated_registered_install_roots(plugin);
    matching_registered_runtime_roots_by_basename(unresolved_basenames, &associated)
}

pub fn associated_registered_install_roots(plugin: &Path) -> Vec<PathBuf> {
    registered_runtime_index().associated_install_roots(plugin)
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct DependencySetCacheKey {
    snapshot_id: String,
    associated_roots: Vec<String>,
    basenames: Vec<String>,
}

const MAX_DEPENDENCY_SET_CACHE_ENTRIES: usize = 1024;

#[derive(Default)]
struct DependencySetCache {
    entries: HashMap<DependencySetCacheKey, RegisteredRuntimeLookup>,
    insertion_order: VecDeque<DependencySetCacheKey>,
}

impl DependencySetCache {
    fn get(&self, key: &DependencySetCacheKey) -> Option<RegisteredRuntimeLookup> {
        self.entries.get(key).cloned()
    }

    fn insert(
        &mut self,
        key: DependencySetCacheKey,
        value: RegisteredRuntimeLookup,
    ) -> RegisteredRuntimeLookup {
        if let Some(existing) = self.entries.get(&key) {
            return existing.clone();
        }
        while self.entries.len() >= MAX_DEPENDENCY_SET_CACHE_ENTRIES {
            let Some(oldest) = self.insertion_order.pop_front() else {
                self.entries.clear();
                break;
            };
            self.entries.remove(&oldest);
        }
        self.insertion_order.push_back(key.clone());
        self.entries.insert(key, value.clone());
        value
    }
}

pub fn matching_registered_runtime_roots_by_basename(
    unresolved_basenames: &[String],
    associated_install_roots: &[PathBuf],
) -> RegisteredRuntimeLookup {
    static CACHE: OnceLock<Mutex<DependencySetCache>> = OnceLock::new();
    let index = registered_runtime_index();
    let mut associated_roots: Vec<String> = associated_install_roots
        .iter()
        .map(|root| path_key(root))
        .collect();
    associated_roots.sort();
    associated_roots.dedup();
    let key = DependencySetCacheKey {
        snapshot_id: index.snapshot_id().to_owned(),
        associated_roots,
        basenames: normalized_basenames(unresolved_basenames),
    };
    let cache = CACHE.get_or_init(|| Mutex::new(DependencySetCache::default()));
    if let Some(found) = cache
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&key)
    {
        return found;
    }
    let found = index.matching_roots_by_basename(unresolved_basenames, associated_install_roots);
    cache
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(key, found)
}

pub fn matching_runtime_roots(
    unresolved_basenames: &[String],
    install_roots: impl IntoIterator<Item = PathBuf>,
) -> Vec<PathBuf> {
    let wanted = unresolved_basenames
        .iter()
        .filter(|name| Path::new(name).file_name().and_then(|value| value.to_str()) == Some(name))
        .map(|name| name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut queue = VecDeque::new();
    let mut seen = HashSet::new();
    for root in install_roots.into_iter().take(MAX_INSTALL_ROOTS) {
        if root.is_absolute() && root.is_dir() {
            queue.push_back((root, 0usize));
        }
    }
    let mut matches = Vec::new();
    while let Some((directory, depth)) = queue.pop_front() {
        if seen.len() >= MAX_VISITED_DIRS || matches.len() >= MAX_MATCHES {
            break;
        }
        let key = directory.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut contains_wanted = false;
        let mut children = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| wanted.contains(&name.to_ascii_lowercase()))
            {
                contains_wanted = true;
            } else if depth < MAX_DEPTH && path.is_dir() {
                children.push(path);
            }
        }
        if contains_wanted {
            if let Ok(canonical) = directory.canonicalize() {
                matches.push(canonical);
            }
        }
        if depth < MAX_DEPTH {
            children.sort();
            queue.extend(children.into_iter().map(|child| (child, depth + 1)));
        }
    }
    matches.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    matches.dedup_by(|left, right| {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    });
    matches.truncate(MAX_MATCHES);
    matches
}

#[cfg(windows)]
fn registered_install_locations() -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
        REG_SZ, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW,
    };
    use windows::core::{PCWSTR, PWSTR};

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
    let uninstall: Vec<u16> = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let install_location: Vec<u16> = "InstallLocation".encode_utf16().chain(Some(0)).collect();
    let mut result = Vec::new();
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let mut parent = HKEY::default();
            if !unsafe {
                RegOpenKeyExW(
                    hive,
                    PCWSTR(uninstall.as_ptr()),
                    0,
                    KEY_READ | view,
                    &mut parent,
                )
            }
            .is_ok()
            {
                continue;
            }
            let parent = Key(parent);
            for index in 0..2048u32 {
                let mut name = vec![0u16; 512];
                let mut len = 511u32;
                let status = unsafe {
                    RegEnumKeyExW(
                        parent.0,
                        index,
                        PWSTR(name.as_mut_ptr()),
                        &mut len,
                        None,
                        PWSTR::null(),
                        None,
                        None,
                    )
                };
                if status == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if !status.is_ok() {
                    continue;
                }
                name.truncate(len as usize);
                name.push(0);
                let mut child = HKEY::default();
                if !unsafe {
                    RegOpenKeyExW(
                        parent.0,
                        PCWSTR(name.as_ptr()),
                        0,
                        KEY_READ | view,
                        &mut child,
                    )
                }
                .is_ok()
                {
                    continue;
                }
                let child = Key(child);
                let mut kind = windows::Win32::System::Registry::REG_VALUE_TYPE::default();
                let mut data = vec![0u16; 4096];
                let mut bytes = (data.len() * std::mem::size_of::<u16>()) as u32;
                let status = unsafe {
                    RegQueryValueExW(
                        child.0,
                        PCWSTR(install_location.as_ptr()),
                        None,
                        Some(&mut kind),
                        Some(data.as_mut_ptr().cast::<u8>()),
                        Some(&mut bytes),
                    )
                };
                if status.is_ok() && kind == REG_SZ && bytes >= 2 && bytes % 2 == 0 {
                    let words = &data[..(bytes as usize / 2).min(data.len())];
                    let end = words
                        .iter()
                        .position(|word| *word == 0)
                        .unwrap_or(words.len());
                    let path = PathBuf::from(std::ffi::OsString::from_wide(&words[..end]));
                    if path.is_absolute() && path.is_dir() {
                        result.push(path);
                    }
                }
            }
        }
    }
    result
}

#[cfg(not(windows))]
fn registered_install_locations() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(lookup: RegisteredRuntimeLookup) -> BTreeMap<String, Vec<PathBuf>> {
        match lookup {
            RegisteredRuntimeLookup::Found(found) => found,
            RegisteredRuntimeLookup::IndexTruncated => panic!("fixture index truncated"),
        }
    }

    #[test]
    fn finds_only_exact_unresolved_dll_parents_within_the_bounded_depth() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-runtime-roots-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let match_dir = root.join("lib64").join("runtime");
        let unrelated = root.join("other");
        let too_deep = root.join("a").join("b").join("c").join("d");
        std::fs::create_dir_all(&match_dir).unwrap();
        std::fs::create_dir_all(&unrelated).unwrap();
        std::fs::create_dir_all(&too_deep).unwrap();
        std::fs::write(root.join("parent.dll"), b"fixture").unwrap();
        std::fs::write(match_dir.join("libmmd.dll"), b"fixture").unwrap();
        std::fs::write(unrelated.join("different.dll"), b"fixture").unwrap();
        std::fs::write(too_deep.join("libmmd.dll"), b"fixture").unwrap();

        let matches =
            matching_runtime_roots(&["parent.dll".into(), "LIBMMD.DLL".into()], [root.clone()]);
        assert_eq!(
            matches,
            vec![
                root.canonicalize().unwrap(),
                match_dir.canonicalize().unwrap()
            ]
        );
        assert!(matching_runtime_roots(&[r"..\libmmd.dll".into()], [root.clone()]).is_empty());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn one_index_serves_association_and_repeated_dependency_sets_without_rescanning() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-runtime-index-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let install = root.join("registered");
        let plugin_dir = install.join("plugins");
        let runtime = install.join("runtime");
        let relocated = root.join("relocated");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::create_dir_all(&relocated).unwrap();
        let plugin_bytes = b"byte-identical-aex-fixture";
        std::fs::write(plugin_dir.join("effect.aex"), plugin_bytes).unwrap();
        let selected = relocated.join("effect.aex");
        std::fs::write(&selected, plugin_bytes).unwrap();
        std::fs::write(runtime.join("alpha.dll"), b"alpha").unwrap();
        std::fs::write(runtime.join("beta.dll"), b"beta").unwrap();

        let index = RegisteredRuntimeIndex::from_install_roots([install.clone()]);
        let directories_walked = index.visited_directories();
        let associated = index.associated_install_roots(&selected);
        assert_eq!(associated, vec![install.canonicalize().unwrap()]);

        let wanted = vec!["BETA.DLL".to_owned(), "alpha.dll".to_owned()];
        let first = found(index.matching_roots_by_basename(&wanted, &associated));
        let second = found(index.matching_roots_by_basename(&wanted, &associated));
        assert_eq!(first, second);
        assert_eq!(first["alpha.dll"], vec![runtime.canonicalize().unwrap()]);
        assert_eq!(first["beta.dll"], vec![runtime.canonicalize().unwrap()]);
        assert_eq!(index.visited_directories(), directories_walked);

        let late = install.join("late");
        std::fs::create_dir_all(&late).unwrap();
        std::fs::write(late.join("gamma.dll"), b"gamma").unwrap();
        assert!(
            found(index.matching_roots_by_basename(&["gamma.dll".into()], &associated))["gamma.dll"]
                .is_empty()
        );
        let rebuilt = RegisteredRuntimeIndex::from_install_roots([install]);
        assert_eq!(
            found(rebuilt.matching_roots_by_basename(&["gamma.dll".into()], &associated))["gamma.dll"],
            vec![late.canonicalize().unwrap()]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dependency_set_cache_is_bounded_and_keys_snapshot_roots_and_basenames() {
        fn key(snapshot: &str, root: &str, basename: &str) -> DependencySetCacheKey {
            DependencySetCacheKey {
                snapshot_id: snapshot.into(),
                associated_roots: vec![root.into()],
                basenames: vec![basename.into()],
            }
        }

        let mut cache = DependencySetCache::default();
        let value = BTreeMap::from([("alpha.dll".into(), vec![PathBuf::from("runtime")])]);
        cache.insert(
            key("snapshot-a", "root-a", "alpha.dll"),
            RegisteredRuntimeLookup::Found(value.clone()),
        );
        assert_eq!(
            cache.get(&key("snapshot-a", "root-a", "alpha.dll")),
            Some(RegisteredRuntimeLookup::Found(value))
        );
        assert!(
            cache
                .get(&key("snapshot-b", "root-a", "alpha.dll"))
                .is_none()
        );
        assert!(
            cache
                .get(&key("snapshot-a", "root-b", "alpha.dll"))
                .is_none()
        );
        assert!(
            cache
                .get(&key("snapshot-a", "root-a", "beta.dll"))
                .is_none()
        );

        for index in 0..=MAX_DEPENDENCY_SET_CACHE_ENTRIES {
            cache.insert(
                key("snapshot", "root", &format!("dependency-{index}.dll")),
                RegisteredRuntimeLookup::Found(BTreeMap::new()),
            );
        }
        assert_eq!(cache.entries.len(), MAX_DEPENDENCY_SET_CACHE_ENTRIES);
        assert_eq!(
            cache.insertion_order.len(),
            MAX_DEPENDENCY_SET_CACHE_ENTRIES
        );
    }

    #[test]
    fn incomplete_index_fails_closed_instead_of_returning_partial_candidates() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-runtime-index-cap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("first.dll"), b"first").unwrap();
        std::fs::write(root.join("second.dll"), b"second").unwrap();
        let index = RegisteredRuntimeIndex::from_install_roots_with_limits(
            [root.clone()],
            MAX_INSTALL_ROOTS,
            MAX_VISITED_DIRS,
            1,
            MAX_INDEX_METADATA_BYTES,
        );
        assert_eq!(
            index.matching_roots_by_basename(&["first.dll".into()], &[root.clone()]),
            RegisteredRuntimeLookup::IndexTruncated
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn launch_facade_production_lookup_builds_the_registered_tree_index_once() {
        let facade = LaunchRuntimeIndex::default();
        let builds = std::sync::atomic::AtomicUsize::new(0);
        for _ in 0..3 {
            let lookup = facade.matching_with(&["missing.dll".into()], || {
                builds.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                RegisteredRuntimeIndex::from_install_roots(Vec::<PathBuf>::new())
            });
            assert_eq!(
                lookup,
                RegisteredRuntimeLookup::Found(BTreeMap::from([(
                    "missing.dll".into(),
                    Vec::new()
                )]))
            );
        }
        assert_eq!(builds.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn directory_and_install_root_caps_also_fail_closed() {
        let parent = std::env::temp_dir().join(format!(
            "aexcompat-runtime-tree-caps-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = parent.join("first");
        let child = first.join("child");
        let second = parent.join("second");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(child.join("nested.dll"), b"nested").unwrap();
        std::fs::write(second.join("second.dll"), b"second").unwrap();

        let directory_capped = RegisteredRuntimeIndex::from_install_roots_with_limits(
            [first.clone()],
            MAX_INSTALL_ROOTS,
            1,
            MAX_INDEXED_FILES,
            MAX_INDEX_METADATA_BYTES,
        );
        assert_eq!(
            directory_capped.matching_roots_by_basename(&["nested.dll".into()], &[first.clone()]),
            RegisteredRuntimeLookup::IndexTruncated
        );
        let roots_capped = RegisteredRuntimeIndex::from_install_roots_with_limits(
            [first.clone(), second],
            1,
            MAX_VISITED_DIRS,
            MAX_INDEXED_FILES,
            MAX_INDEX_METADATA_BYTES,
        );
        assert_eq!(
            roots_capped.matching_roots_by_basename(&["nested.dll".into()], &[first]),
            RegisteredRuntimeLookup::IndexTruncated
        );
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn flattening_dependency_sets_deduplicates_before_the_root_limit() {
        let shared = PathBuf::from(r"C:\Runtime\Shared");
        let distinct = PathBuf::from(r"C:\Runtime\Zed");
        let found = BTreeMap::from([
            ("alpha.dll".into(), vec![shared.clone(); 15]),
            ("zed.dll".into(), vec![distinct.clone()]),
        ]);
        assert_eq!(unique_runtime_roots(found, 15), vec![shared, distinct]);
    }
}
