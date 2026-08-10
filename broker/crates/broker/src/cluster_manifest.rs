//! In-place cluster session manifest (`cluster-manifest-v2`, issue #751/#816).
//! Plug-ins are named by absolute path and authenticated immediately before
//! load; dependency closure staging and the v1 manifest no longer exist.

use crate::secure_image_dispatch::ApprovedImageArtifact;
use crate::session_dependency_manifest::{decode_sha256, validate_windows_basename};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const TRANSPORT_DIRECTORY_ATTEMPTS: usize = 16;

/// Session-wide caps shared by the in-place cluster manifest and worker.
pub const MAX_CLUSTER_PLUGINS: usize = 256;
pub const MAX_CLUSTER_MODULE_BOUND: u32 = 4096;
pub const MAX_CLUSTER_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CLUSTER_PAYLOAD_BYTES: usize = 16384;
fn hex_sha256(digest: &[u8; 32]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Broker-owned temporary file holding the in-place v2 manifest.
#[derive(Debug)]
pub struct ClusterManifestTransport {
    path: PathBuf,
}

impl ClusterManifestTransport {
    fn create_directory(root: &Path, mut next_nonce: impl FnMut() -> u128) -> io::Result<PathBuf> {
        for _ in 0..TRANSPORT_DIRECTORY_ATTEMPTS {
            let dir = root.join(format!("cluster-manifest-{:032x}", next_nonce()));
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(dir),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cluster manifest transport namespace exhausted",
        ))
    }

    fn write_named(repository: &Path, json: &str, basename: &str) -> io::Result<Self> {
        let root = repository.join("target/image-transport");
        fs::create_dir_all(&root)?;
        // Random per-attempt identities avoid the coarse Windows clock
        // collision that a timestamp-only name permits between concurrent
        // discovery sessions. AlreadyExists is retried, while every other
        // filesystem error remains fail-closed.
        let dir = Self::create_directory(&root, rand::random::<u128>)?;
        let path = dir.join(basename);
        let mut output = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(output) => output,
            Err(error) => {
                let _ = fs::remove_dir(&dir);
                return Err(error);
            }
        };
        let written = output
            .write_all(json.as_bytes())
            .and_then(|()| output.sync_all());
        if let Err(error) = written {
            drop(output);
            let _ = fs::remove_file(&path);
            let _ = fs::remove_dir(&dir);
            return Err(error);
        }
        drop(output);
        // Verify the bytes on disk the way the aux sidecars are verified
        // after write; a torn transport must fail the launch, not the worker.
        if fs::read(&path)
            .map(|bytes| bytes != json.as_bytes())
            .unwrap_or(true)
        {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_dir(&dir);
            return Err(invalid("cluster manifest verification failed after write"));
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ClusterManifestTransport {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

// ---------------------------------------------------------------------------
// In-place cluster manifest (`cluster-manifest-v2`, issue #751).
//
// The v2 manifest is the sole cluster transport after #816:
// plug-ins are named by their real absolute path plus the SHA-256 the worker
// re-verifies right before each load, and instead of a pinned dependency
// closure it carries the broker-validated dependency search directories the
// worker admits into its DLL search set. Nothing is staged; the document is
// written under the broker-owned `target/image-transport` and read by the
// worker at launch, before any plug-in code runs (the aux-manifest transport
// argument). The module audit on this manifest is recorded, never enforced.
// ---------------------------------------------------------------------------

pub const IN_PLACE_CLUSTER_MANIFEST_SCHEMA: &str = "cluster-manifest-v2";
pub const IN_PLACE_CLUSTER_MANIFEST_BASENAME: &str = "cluster-manifest-v2.json";
pub const MAX_CLUSTER_SEARCH_DIRS: usize = 16;
/// The worker admits `search_dirs` plus every plug-in's parent directory
/// (deduplicated case-insensitively) into its DLL search set and rejects a
/// launch whose union exceeds this; validating the same bound here turns
/// that opaque worker exit into a build-time error.
pub const MAX_CLUSTER_ADMITTED_DIRS: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InPlaceClusterManifestDto {
    pub schema: String,
    pub plugins: Vec<InPlaceClusterPluginDto>,
    pub search_dirs: Vec<String>,
    pub module_bound: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InPlaceClusterPluginDto {
    pub path: String,
    pub sha256: String,
    /// Render sessions only; discovery manifests omit the key entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValidatedInPlacePlugin {
    path: PathBuf,
    sha256: [u8; 32],
    payload: Option<String>,
}

/// A `cluster-manifest-v2` document that passed every structural check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedInPlaceClusterManifest {
    plugins: Vec<ValidatedInPlacePlugin>,
    search_dirs: Vec<String>,
    module_bound: u32,
}

impl ValidatedInPlaceClusterManifest {
    /// Builds and validates a manifest from broker-approved artifacts and the
    /// already-validated (canonical, de-verbatim) search directory strings —
    /// the same values a `--dependency-dirs-v1` launch would carry.
    pub fn from_approved(
        plugins: &[ApprovedImageArtifact],
        search_dirs: &[String],
        swap_payloads: Option<&[Option<String>]>,
        module_bound: u32,
    ) -> io::Result<Self> {
        if let Some(payloads) = swap_payloads {
            if payloads.len() != plugins.len() {
                return Err(invalid(
                    "cluster swap payloads must parallel the plugin list",
                ));
            }
        }
        let plugins = plugins
            .iter()
            .enumerate()
            .map(|(index, artifact)| {
                Ok(InPlaceClusterPluginDto {
                    path: artifact
                        .path
                        .to_str()
                        .ok_or_else(|| invalid("in-place plugin path must be UTF-8"))?
                        .to_owned(),
                    sha256: hex_sha256(&artifact.expected_sha256),
                    payload: swap_payloads.and_then(|payloads| payloads[index].clone()),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Self::validate(InPlaceClusterManifestDto {
            schema: IN_PLACE_CLUSTER_MANIFEST_SCHEMA.to_owned(),
            plugins,
            search_dirs: search_dirs.to_vec(),
            module_bound,
        })
    }

    /// Validates a v2 document structurally: absolute unique plugin paths
    /// with Windows-safe basenames, bounded absolute unique search
    /// directories, and the shared plugin/bound caps.
    pub fn validate(dto: InPlaceClusterManifestDto) -> io::Result<Self> {
        if dto.schema != IN_PLACE_CLUSTER_MANIFEST_SCHEMA {
            return Err(invalid("unsupported cluster manifest schema"));
        }
        if dto.plugins.is_empty() || dto.plugins.len() > MAX_CLUSTER_PLUGINS {
            return Err(invalid("cluster plugin count is outside 1..=256"));
        }
        if dto.module_bound == 0 || dto.module_bound > MAX_CLUSTER_MODULE_BOUND {
            return Err(invalid("cluster module bound is outside 1..=4096"));
        }
        if dto.search_dirs.is_empty() || dto.search_dirs.len() > MAX_CLUSTER_SEARCH_DIRS {
            return Err(invalid("cluster search directory count is outside 1..=16"));
        }
        let mut seen_paths = HashSet::with_capacity(dto.plugins.len());
        let mut plugins = Vec::with_capacity(dto.plugins.len());
        for plugin in dto.plugins {
            let path = PathBuf::from(&plugin.path);
            if !path.is_absolute() || plugin.path.starts_with(r"\\?\") {
                return Err(invalid(
                    "in-place plugin path must be absolute and de-verbatim",
                ));
            }
            let basename = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid("in-place plugin path must have a UTF-8 basename"))?;
            validate_windows_basename("plugin", basename)?;
            if !seen_paths.insert(plugin.path.to_lowercase()) {
                return Err(invalid("duplicate or case-colliding cluster plugin path"));
            }
            let sha256 = decode_sha256("plugin", &plugin.sha256)?;
            if let Some(payload) = &plugin.payload {
                // Printable ASCII only, matching the worker's byte gate
                // exactly (the sealed manifest's `is_ascii()` admits control
                // bytes the worker then rejects as an opaque exit).
                if payload.len() > MAX_CLUSTER_PAYLOAD_BYTES
                    || payload.bytes().any(|byte| !(0x20..=0x7e).contains(&byte))
                {
                    return Err(invalid("cluster plugin payload is invalid"));
                }
            }
            plugins.push(ValidatedInPlacePlugin {
                path,
                sha256,
                payload: plugin.payload,
            });
        }
        let mut seen_dirs = HashSet::with_capacity(dto.search_dirs.len());
        for dir in &dto.search_dirs {
            let path = Path::new(dir);
            if dir.is_empty() || !path.is_absolute() || dir.starts_with(r"\\?\") {
                return Err(invalid(
                    "cluster search directory must be absolute and de-verbatim",
                ));
            }
            if !seen_dirs.insert(dir.to_lowercase()) {
                return Err(invalid("duplicate cluster search directory"));
            }
        }
        // Mirror the worker's admitted-directory bound (search dirs plus
        // every plug-in's parent, deduplicated case-insensitively) so an
        // over-scattered cluster fails here instead of as a worker exit.
        let mut admitted = seen_dirs;
        for plugin in &plugins {
            if let Some(parent) = plugin.path.parent().and_then(|parent| parent.to_str()) {
                admitted.insert(parent.to_lowercase());
            }
        }
        if admitted.len() > MAX_CLUSTER_ADMITTED_DIRS {
            return Err(invalid(
                "cluster search and plugin directories exceed the admitted bound",
            ));
        }
        Ok(Self {
            plugins,
            search_dirs: dto.search_dirs,
            module_bound: dto.module_bound,
        })
    }

    /// Parses a serialized manifest (bounded to the documented 4 MiB body).
    pub fn parse(json: &[u8]) -> io::Result<Self> {
        if json.is_empty() || json.len() > MAX_CLUSTER_MANIFEST_BYTES {
            return Err(invalid("cluster manifest body is outside 1 byte..4 MiB"));
        }
        let dto: InPlaceClusterManifestDto =
            serde_json::from_slice(json).map_err(|error| invalid(error.to_string()))?;
        Self::validate(dto)
    }

    /// Serializes back to the wire document, enforcing the same body bound.
    pub fn to_json(&self) -> io::Result<String> {
        let dto = InPlaceClusterManifestDto {
            schema: IN_PLACE_CLUSTER_MANIFEST_SCHEMA.to_owned(),
            plugins: self
                .plugins
                .iter()
                .map(|plugin| {
                    Ok(InPlaceClusterPluginDto {
                        path: plugin
                            .path
                            .to_str()
                            .ok_or_else(|| invalid("in-place plugin path must be UTF-8"))?
                            .to_owned(),
                        sha256: hex_sha256(&plugin.sha256),
                        payload: plugin.payload.clone(),
                    })
                })
                .collect::<io::Result<Vec<_>>>()?,
            search_dirs: self.search_dirs.clone(),
            module_bound: self.module_bound,
        };
        let json =
            serde_json::to_string_pretty(&dto).map_err(|error| invalid(error.to_string()))?;
        if json.len() > MAX_CLUSTER_MANIFEST_BYTES {
            return Err(invalid("cluster manifest body exceeds 4 MiB"));
        }
        Ok(json)
    }

    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    pub fn plugin_path(&self, index: usize) -> Option<&Path> {
        self.plugins.get(index).map(|plugin| plugin.path.as_path())
    }

    pub fn plugin_sha256(&self, index: usize) -> Option<[u8; 32]> {
        self.plugins.get(index).map(|plugin| plugin.sha256)
    }

    pub fn module_bound(&self) -> u32 {
        self.module_bound
    }

    /// Writes the manifest to the broker-owned transport directory under the
    /// v2 reserved basename. Unlike the sealed transport this is the document
    /// the worker actually reads (at launch, before any plug-in code runs),
    /// so the caller keeps the transport alive for the session lifetime.
    pub fn write_transport(&self, repository: &Path) -> io::Result<ClusterManifestTransport> {
        ClusterManifestTransport::write_named(
            repository,
            &self.to_json()?,
            IN_PLACE_CLUSTER_MANIFEST_BASENAME,
        )
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-cluster-manifest-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn in_place_dto() -> InPlaceClusterManifestDto {
        #[cfg(windows)]
        let (first, second, support, effects) = (
            r"C:\effects\alpha.aex",
            r"C:\effects\cyco\alpha.aex",
            r"C:\ae\Support Files",
            r"C:\effects",
        );
        #[cfg(not(windows))]
        let (first, second, support, effects) = (
            "/effects/alpha.aex",
            "/effects/cyco/alpha.aex",
            "/ae/Support Files",
            "/effects",
        );
        InPlaceClusterManifestDto {
            schema: IN_PLACE_CLUSTER_MANIFEST_SCHEMA.to_owned(),
            plugins: vec![
                InPlaceClusterPluginDto {
                    path: first.into(),
                    sha256: "a".repeat(64),
                    payload: None,
                },
                InPlaceClusterPluginDto {
                    path: second.into(),
                    sha256: "b".repeat(64),
                    payload: None,
                },
            ],
            search_dirs: vec![support.into(), effects.into()],
            module_bound: 4096,
        }
    }

    #[test]
    fn in_place_manifest_validates_and_round_trips() {
        let manifest = ValidatedInPlaceClusterManifest::validate(in_place_dto()).unwrap();
        assert_eq!(manifest.plugin_count(), 2);
        let expected = in_place_dto();
        assert_eq!(
            manifest.plugin_path(0),
            Some(Path::new(&expected.plugins[0].path))
        );
        // Same basename in different directories is legal in place; identity
        // is the full path.
        assert_eq!(
            manifest.plugin_path(1),
            Some(Path::new(&expected.plugins[1].path))
        );
        let json = manifest.to_json().unwrap();
        let reparsed = ValidatedInPlaceClusterManifest::parse(json.as_bytes()).unwrap();
        assert_eq!(manifest, reparsed);
    }

    #[test]
    fn transport_directory_retries_an_existing_nonce() {
        let repository = source_dir();
        let root = repository.join("target/image-transport");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir(root.join(format!("cluster-manifest-{:032x}", 7u128))).unwrap();
        let mut nonces = [7u128, 8u128].into_iter();
        let created = ClusterManifestTransport::create_directory(&root, || {
            nonces
                .next()
                .expect("bounded retry consumed only supplied nonces")
        })
        .unwrap();
        assert_eq!(
            created.file_name().and_then(|name| name.to_str()),
            Some("cluster-manifest-00000000000000000000000000000008")
        );
        fs::remove_dir(&created).unwrap();
        fs::remove_dir_all(&repository).unwrap();
    }

    #[test]
    fn in_place_manifest_fails_closed_on_bad_paths_dirs_and_bounds() {
        let mut relative = in_place_dto();
        relative.plugins[0].path = r"relative\alpha.aex".into();
        assert!(ValidatedInPlaceClusterManifest::validate(relative).is_err());

        let mut verbatim = in_place_dto();
        verbatim.plugins[0].path = r"\\?\C:\effects\alpha.aex".into();
        assert!(ValidatedInPlaceClusterManifest::validate(verbatim).is_err());

        let mut colliding = in_place_dto();
        colliding.plugins[1].path = if cfg!(windows) {
            r"C:\EFFECTS\ALPHA.AEX".into()
        } else {
            "/EFFECTS/ALPHA.AEX".into()
        };
        assert!(ValidatedInPlaceClusterManifest::validate(colliding).is_err());

        let mut relative_dir = in_place_dto();
        relative_dir.search_dirs[0] = "relative".into();
        assert!(ValidatedInPlaceClusterManifest::validate(relative_dir).is_err());

        let mut too_many_dirs = in_place_dto();
        too_many_dirs.search_dirs = (0..17)
            .map(|index| {
                if cfg!(windows) {
                    format!(r"C:\dir-{index}")
                } else {
                    format!("/dir-{index}")
                }
            })
            .collect();
        assert!(ValidatedInPlaceClusterManifest::validate(too_many_dirs).is_err());

        let mut no_dirs = in_place_dto();
        no_dirs.search_dirs.clear();
        assert!(ValidatedInPlaceClusterManifest::validate(no_dirs).is_err());

        let mut wrong_schema = in_place_dto();
        wrong_schema.schema = "cluster-manifest-v1".to_owned();
        assert!(ValidatedInPlaceClusterManifest::validate(wrong_schema).is_err());

        // The union of search dirs and plugin parents mirrors the worker's
        // admitted-directory bound; scattering plugins across 65 directories
        // fails at build time instead of as an opaque worker exit.
        let mut scattered = in_place_dto();
        scattered.plugins = (0..65)
            .map(|index| InPlaceClusterPluginDto {
                path: if cfg!(windows) {
                    format!(r"C:\scattered\dir-{index}\plugin.aex")
                } else {
                    format!("/scattered/dir-{index}/plugin.aex")
                },
                sha256: "a".repeat(64),
                payload: None,
            })
            .collect();
        assert!(ValidatedInPlaceClusterManifest::validate(scattered).is_err());
    }

    #[test]
    fn in_place_transport_carries_the_v2_reserved_basename() {
        let repository = source_dir();
        let manifest = ValidatedInPlaceClusterManifest::validate(in_place_dto()).unwrap();
        let transport = manifest.write_transport(&repository).unwrap();
        assert_eq!(
            transport.path().file_name().and_then(|name| name.to_str()),
            Some(IN_PLACE_CLUSTER_MANIFEST_BASENAME)
        );
        let on_disk = fs::read(transport.path()).unwrap();
        assert_eq!(
            ValidatedInPlaceClusterManifest::parse(&on_disk).unwrap(),
            manifest
        );
        drop(transport);
        fs::remove_dir_all(repository).unwrap();
    }
}
