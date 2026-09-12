//! Local installation metadata for supported external render services.
//! Registration associates exact plugin paths, never an executable command
//! extracted from plugin bytes. It is not a plugin admission/hash policy.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

fn path_key(path: &str) -> String {
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!("//{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
    };
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

pub(crate) fn registration_path(directory: &Path, plugin: &Path) -> io::Result<PathBuf> {
    let canonical = plugin.canonicalize()?;
    let path = canonical.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "plugin path is not valid Unicode",
        )
    })?;
    Ok(directory.join(format!("{}.json", path_key(path))))
}

pub(crate) fn resolve_service_in_directory(
    directory: &Path,
    plugin: &Path,
) -> io::Result<Option<RenderServiceInstallation>> {
    resolve_registered_service(&registration_path(directory, plugin)?, plugin)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    schema_version: u32,
    backend: String,
    plugin_paths: Vec<PathBuf>,
    runtime_root: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RenderServiceInstallation {
    pub root: PathBuf,
    pub executable: PathBuf,
    pub entry: PathBuf,
}

/// Missing registration means no managed service; it must not prevent ordinary
/// AEX loading. A matching but broken registration is an actionable error.
pub(crate) fn resolve_registered_service(
    registration_path: &Path,
    plugin: &Path,
) -> io::Result<Option<RenderServiceInstallation>> {
    let file = match File::open(registration_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(65537).read_to_end(&mut bytes)?;
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid render-service registration",
        )
    };
    if bytes.len() > 65536 {
        return Err(invalid());
    }
    let registration: Registration = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if registration.schema_version != 1
        || registration.backend != "three-v1"
        || registration.plugin_paths.is_empty()
        || registration.plugin_paths.len() > 16
        || registration.plugin_paths.iter().any(|p| !p.is_absolute())
        || !registration.runtime_root.is_absolute()
    {
        return Err(invalid());
    }
    let plugin = plugin.canonicalize()?;
    if !registration
        .plugin_paths
        .iter()
        .any(|p| p.canonicalize().ok().as_ref() == Some(&plugin))
    {
        return Err(invalid());
    }
    let root = registration.runtime_root.canonicalize()?;
    // The Three v1 renderer uses nodeIntegration and has no preload behavior.
    // Some installed builds emit an empty index.mjs while main still references
    // index.js. That optional preload must not reject an otherwise usable host.
    let required = [
        "node_modules/electron/dist/electron.exe",
        "out/main/index.js",
        "out/renderer/index.html",
    ];
    let mut paths = Vec::new();
    for relative in required {
        let path = root.join(relative).canonicalize()?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err(invalid());
        }
        paths.push(path);
    }
    Ok(Some(RenderServiceInstallation {
        root,
        executable: paths.remove(0),
        entry: paths.remove(0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn damaged_registration_is_scoped_to_its_plugin_path() {
        let folder =
            std::env::temp_dir().join(format!("aex-reg-isolation-{}", rand::random::<u64>()));
        std::fs::create_dir(&folder).unwrap();
        let selected = folder.join("selected.aex");
        let ordinary = folder.join("ordinary.aex");
        std::fs::write(&selected, b"selected").unwrap();
        std::fs::write(&ordinary, b"ordinary").unwrap();
        let record = registration_path(&folder, &selected).unwrap();
        for bad in [
            b"{broken".to_vec(),
            vec![b' '; 65537],
            br#"{"schema_version":999}"#.to_vec(),
        ] {
            std::fs::write(&record, bad).unwrap();
            assert!(resolve_service_in_directory(&folder, &selected).is_err());
            assert!(
                resolve_service_in_directory(&folder, &ordinary)
                    .unwrap()
                    .is_none()
            );
        }
        let before = registration_path(&folder, &selected).unwrap();
        // Valid JSON at the selected key may not silently associate another AEX.
        std::fs::write(
            &record,
            serde_json::to_vec(&serde_json::json!({
                "schema_version":1,"backend":"three-v1",
                "plugin_paths":[ordinary],"runtime_root":folder
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(resolve_service_in_directory(&folder, &selected).is_err());
        assert!(
            resolve_service_in_directory(&folder, &ordinary)
                .unwrap()
                .is_none()
        );
        std::fs::write(&selected, b"rebuilt without approval").unwrap();
        assert_eq!(registration_path(&folder, &selected).unwrap(), before);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn path_key_normalizes_windows_namespace_and_ascii_case() {
        assert_eq!(
            path_key(r"\\?\C:\Plugins\Example.aex"),
            path_key("c:/plugins/example.aex")
        );
        assert_eq!(
            path_key(r"\\?\UNC\Server\Share\Example.aex"),
            path_key("//server/share/example.aex")
        );
    }
    #[test]
    fn registration_resolves_only_selected_plugin_and_complete_runtime() {
        let folder =
            std::env::temp_dir().join(format!("aex-service-reg-{}", rand::random::<u64>()));
        std::fs::create_dir(&folder).unwrap();
        let plugin = folder.join("selected.aex");
        let other = folder.join("other.aex");
        std::fs::write(&plugin, b"plugin").unwrap();
        std::fs::write(&other, b"other").unwrap();
        let registration = folder.join("registration.json");
        assert!(
            resolve_registered_service(&registration, &plugin)
                .unwrap()
                .is_none()
        );
        std::fs::write(&registration, serde_json::to_vec(&serde_json::json!({
            "schema_version":1,"backend":"three-v1","plugin_paths":[plugin],"runtime_root":folder
        })).unwrap()).unwrap();
        assert!(resolve_registered_service(&registration, &other).is_err());
        assert!(resolve_registered_service(&registration, &plugin).is_err());
        for relative in [
            "node_modules/electron/dist/electron.exe",
            "out/main/index.js",
            "out/renderer/index.html",
        ] {
            let path = folder.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fixture").unwrap();
        }
        let found = resolve_registered_service(&registration, &plugin)
            .unwrap()
            .unwrap();
        assert_eq!(found.root, folder.canonicalize().unwrap());
        assert_eq!(found.executable.file_name().unwrap(), "electron.exe");
        // Identity changes do not introduce a hash admission gate.
        std::fs::write(&plugin, b"rebuilt plugin").unwrap();
        assert!(
            resolve_registered_service(&registration, &plugin)
                .unwrap()
                .is_some()
        );
        std::fs::remove_dir_all(folder).unwrap();
    }
}
