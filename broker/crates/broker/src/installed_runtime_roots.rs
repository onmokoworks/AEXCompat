//! Bounded discovery of runtime DLL directories below Windows-registered installs.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

const MAX_INSTALL_ROOTS: usize = 256;
const MAX_VISITED_DIRS: usize = 4096;
const MAX_DEPTH: usize = 3;
const MAX_MATCHES: usize = 15;

pub fn matching_registered_runtime_roots(unresolved_basenames: &[String]) -> Vec<PathBuf> {
    matching_runtime_roots(unresolved_basenames, registered_install_locations())
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
}
