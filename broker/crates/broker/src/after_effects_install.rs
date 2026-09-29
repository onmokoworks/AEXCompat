//! Where the installed After Effects lives on this machine.
//!
//! After Effects puts its own executable directory (`Support Files`) on the DLL
//! search path of every plug-in it loads, so an AEX that imports an Adobe or
//! bundled runtime by name expects to find it there. Every host entry point
//! that loads an AEX outside AE needs the same folder, and all of them take it
//! from here so they agree on which install is "the" one.

use std::path::{Path, PathBuf};

/// `%ProgramFiles%\Adobe`, the root of Adobe app installs.
pub fn adobe_root() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles")?;
    Some(PathBuf::from(program_files).join("Adobe"))
}

/// The newest `Adobe After Effects <year>\Support Files\Plug-ins`, or `None`.
pub fn latest_after_effects_plugins() -> (Option<PathBuf>, bool) {
    let Some(adobe) = adobe_root() else {
        return (None, false);
    };
    newest_after_effects_plugins_under(&adobe)
}

fn newest_after_effects_plugins_under(adobe: &Path) -> (Option<PathBuf>, bool) {
    newest_versioned(
        adobe,
        "Adobe After Effects ",
        &["Support Files", "Plug-ins"],
    )
}

/// The newest `Adobe\Common\Plug-ins\<version>\MediaCore`, or `None`.
pub fn mediacore_dir() -> (Option<PathBuf>, bool) {
    let Some(adobe) = adobe_root() else {
        return (None, false);
    };
    let root = adobe.join("Common").join("Plug-ins");
    newest_versioned(&root, "", &["MediaCore"])
}

/// The `leaf` folder under the newest versioned subfolder of `root` whose name
/// starts with `prefix` (e.g. `Adobe After Effects 2025/Support Files/Plug-ins`).
///
/// The second value is false when the pick cannot be trusted to be the newest:
/// the folder could not be enumerated, an entry could not be read, or a version
/// *newer than the pick* was present without its `leaf`. That last case is what
/// an install being updated looks like, and silently falling back to an older
/// version while reporting a complete scan would make the newer version's
/// plug-ins look deleted — which prunes their cache entries and unregisters them
/// on the next launch, deleting objects from saved projects (issue #307). A
/// leafless *older* version is just an uninstall leftover and means nothing.
pub fn newest_versioned(root: &Path, prefix: &str, leaf: &[&str]) -> (Option<PathBuf>, bool) {
    let Ok(read) = std::fs::read_dir(root) else {
        return (None, false);
    };
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    let mut leafless: Vec<Vec<u64>> = Vec::new();
    let mut complete = true;
    for entry in read {
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(version) = name.strip_prefix(prefix) else {
            continue;
        };
        // A numbered name is what an install in progress looks like. Unnumbered
        // ones (`... (Beta)`) are still picked when nothing numbered exists, but a
        // missing leaf under them is not evidence of an incomplete install.
        let numbered = version
            .split(['.', ' '])
            .any(|part| part.parse::<u64>().is_ok());
        let key = version_key(version);
        let mut candidate = entry.path();
        // Tested through the path, not `DirEntry::file_type`, which reports a
        // directory junction as a symlink rather than a directory — Adobe installs
        // are routinely junctioned to another drive.
        if !candidate.is_dir() {
            // A plain file is clutter. A reparse point that will not resolve is an
            // install we simply could not see this launch, which must not read as
            // "its plug-ins are gone".
            let unresolved = candidate
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_symlink());
            if numbered && unresolved {
                leafless.push(key);
            }
            continue;
        }
        candidate.extend(leaf);
        if !candidate.is_dir() {
            if numbered {
                leafless.push(key);
            }
            continue;
        }
        if best.as_ref().is_none_or(|(best_key, _)| key > *best_key) {
            best = Some((key, candidate));
        }
    }
    // A leafless version above the pick means the newest install is not fully
    // visible this launch, so its absence is not evidence its plug-ins are gone.
    let best_key = best.as_ref().map(|(key, _)| key);
    complete &= !leafless
        .iter()
        .any(|key| best_key.is_none_or(|best_key| key > best_key));
    (best.map(|(_, path)| path), complete)
}

/// The newest installed `Adobe After Effects <year>\Support Files`, or `None`
/// when no After Effects install is visible.
pub fn latest_after_effects_support_files() -> Option<PathBuf> {
    newest_support_files_under(&adobe_root()?)
}

fn newest_support_files_under(adobe: &Path) -> Option<PathBuf> {
    newest_after_effects_plugins_under(adobe)
        .0
        .and_then(|plugins| plugins.parent().map(Path::to_path_buf))
        .filter(|support_files| support_files.is_dir())
}

/// The dependency search roots an in-place session gives a plug-in when the
/// caller has nothing more specific: the plug-in's own folder first, then the
/// newest installed AE `Support Files`, which is where After Effects itself
/// would have let the plug-in's by-name imports resolve. Without an AE install
/// only the plug-in's folder remains, as before.
pub fn in_place_dependency_search_dirs(plugin_directory: &Path) -> Vec<PathBuf> {
    search_dirs_with(plugin_directory, latest_after_effects_support_files())
}

fn search_dirs_with(plugin_directory: &Path, support_files: Option<PathBuf>) -> Vec<PathBuf> {
    std::iter::once(plugin_directory.to_path_buf())
        .chain(support_files)
        .collect()
}

/// A numerically-comparable key for a version token ("25.0" > "7.0", unlike a
/// lexical compare), falling back to 0 for non-numeric components.
fn version_key(version: &str) -> Vec<u64> {
    version
        .split(['.', ' '])
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_root(tag: &str) -> TempRoot {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-ae-install-{}-{tag}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        TempRoot(root)
    }

    #[test]
    fn support_files_come_from_the_newest_complete_after_effects_install() {
        let root = temp_root("newest");
        for year in ["2024", "2026"] {
            std::fs::create_dir_all(
                root.0
                    .join(format!("Adobe After Effects {year}"))
                    .join("Support Files")
                    .join("Plug-ins"),
            )
            .unwrap();
        }
        // Another Adobe product beside AE is not an AE install.
        std::fs::create_dir_all(root.0.join("Adobe Premiere Pro 2027").join("Plug-ins")).unwrap();
        assert_eq!(
            newest_support_files_under(&root.0),
            Some(
                root.0
                    .join("Adobe After Effects 2026")
                    .join("Support Files")
            )
        );
    }

    #[test]
    fn no_after_effects_install_means_no_support_files() {
        let root = temp_root("none");
        std::fs::create_dir_all(root.0.join("Adobe Photoshop 2025")).unwrap();
        assert_eq!(newest_support_files_under(&root.0), None);
        assert_eq!(newest_support_files_under(&root.0.join("missing")), None);
    }

    #[test]
    fn the_plugin_folder_is_searched_before_support_files() {
        let plugin = PathBuf::from(r"C:\Plugins\Vendor");
        let support = PathBuf::from(r"C:\Adobe\Adobe After Effects 2026\Support Files");
        assert_eq!(
            search_dirs_with(&plugin, Some(support.clone())),
            vec![plugin.clone(), support]
        );
        assert_eq!(search_dirs_with(&plugin, None), vec![plugin]);
    }
}
