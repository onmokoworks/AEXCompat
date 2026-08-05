//! Cluster session manifest (`cluster-manifest-v1`, issue #405,
//! docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md §2).
//!
//! One cluster session loads several plugins that share a single dependency
//! closure into one worker process. The manifest is the trust decision that
//! makes that safe: it names, in launch order, every plugin the worker may
//! ever load (by basename + SHA-256, never by a path the worker resolves)
//! plus the whole shared closure, and declares the module bound the module
//! audit is validated against (§5). The broker serializes it, stages the
//! document into the sealed load root next to the plugin images
//! (hash-verified like every other sealed entry), and hands the staged
//! absolute path over argv (`--cluster-manifest-v1 <path>`), so the worker's
//! "manifest lives inside the sealed root" pin holds (design §2.3). A swap
//! or inspect message then carries only an index into this
//! pre-authenticated list.
//!
//! The basename and SHA-256 rules are exactly those of
//! `session_dependency_manifest`; nothing here relaxes them.

use crate::secure_image_dispatch::ApprovedImageArtifact;
use crate::session_dependency_manifest::{decode_sha256, validate_windows_basename};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CLUSTER_MANIFEST_SCHEMA: &str = "cluster-manifest-v1";
/// Basename of the manifest document staged into the sealed load tree
/// (issue #405, design §2.3): the worker receives the staged absolute path
/// over argv and requires the manifest to sit directly inside the sealed
/// root, so this basename is reserved across the whole cluster — a plugin or
/// dependency carrying it collides and fails the launch closed.
pub const CLUSTER_MANIFEST_SEALED_BASENAME: &str = "cluster-manifest-v1.json";
/// Session-wide caps (design §2.1): exceeding any of them means the cluster
/// session does not come into existence and the caller falls back to the
/// per-plugin path (fail-closed).
pub const MAX_CLUSTER_PLUGINS: usize = 256;
pub const MAX_CLUSTER_MODULE_BOUND: u32 = 4096;
pub const MAX_CLUSTER_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
/// A plugin payload rides the same encoding and bound as the launch argv
/// payload (`encode_interactive_payload` / `SessionOpenRequest::payload_override`).
pub const MAX_CLUSTER_PAYLOAD_BYTES: usize = 16384;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterManifestDto {
    pub schema: String,
    pub plugins: Vec<ClusterPluginDto>,
    pub dependencies: Vec<ClusterDependencyDto>,
    pub module_bound: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterPluginDto {
    pub basename: String,
    pub sha256: String,
    /// Render sessions only; discovery manifests omit the key entirely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterDependencyDto {
    pub basename: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValidatedClusterPlugin {
    basename: String,
    sha256: [u8; 32],
    payload: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValidatedClusterDependency {
    basename: String,
    sha256: [u8; 32],
    size: u64,
}

/// A `cluster-manifest-v1` document that passed every structural check.
/// Produced either from broker-approved artifacts (`from_approved`, the
/// launch path) or by parsing a serialized document back (`parse`, the
/// round-trip path used by tests and diagnostics).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedClusterManifest {
    plugins: Vec<ValidatedClusterPlugin>,
    dependencies: Vec<ValidatedClusterDependency>,
    module_bound: u32,
}

impl ValidatedClusterManifest {
    /// Builds and validates a manifest from artifacts the broker already
    /// authenticated (the caller owns the closure resolution and the
    /// `session_dependency_manifest` re-validation; this manifest only
    /// carries the results). `plugins[0]` is the launch plugin of a render
    /// session. `swap_payloads`, when given, must parallel `plugins`; the
    /// entry for `plugins[0]` is ignored because the launch argv payload wins
    /// (design §2.2). Discovery sessions pass `None` so no `payload` key is
    /// emitted at all.
    pub fn from_approved(
        plugins: &[ApprovedImageArtifact],
        dependencies: &[ApprovedImageArtifact],
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
        let artifact_entry = |artifact: &ApprovedImageArtifact, kind: &str| {
            artifact
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("{kind} path must have a UTF-8 basename")))
        };
        let plugins = plugins
            .iter()
            .enumerate()
            .map(|(index, artifact)| {
                Ok(ClusterPluginDto {
                    basename: artifact_entry(artifact, "plugin")?,
                    sha256: hex_sha256(&artifact.expected_sha256),
                    payload: swap_payloads.and_then(|payloads| payloads[index].clone()),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        let dependencies = dependencies
            .iter()
            .map(|artifact| {
                Ok(ClusterDependencyDto {
                    basename: artifact_entry(artifact, "dependency")?,
                    sha256: hex_sha256(&artifact.expected_sha256),
                    size: artifact.expected_size,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Self::validate(ClusterManifestDto {
            schema: CLUSTER_MANIFEST_SCHEMA.to_owned(),
            plugins,
            dependencies,
            module_bound,
        })
    }

    /// Validates a manifest document structurally. The files it names are
    /// authenticated separately (SealedLoadTree staging re-hashes every
    /// entry); this is the fail-closed shape/bound/collision gate.
    pub fn validate(dto: ClusterManifestDto) -> io::Result<Self> {
        if dto.schema != CLUSTER_MANIFEST_SCHEMA {
            return Err(invalid("unsupported cluster manifest schema"));
        }
        if dto.plugins.is_empty() || dto.plugins.len() > MAX_CLUSTER_PLUGINS {
            return Err(invalid("cluster plugin count is outside 1..=256"));
        }
        if dto.module_bound == 0 || dto.module_bound > MAX_CLUSTER_MODULE_BOUND {
            return Err(invalid("cluster module bound is outside 1..=4096"));
        }
        // A snapshot always contains at least the declared modules, so a
        // declared set larger than the bound could never pass the audit:
        // reject the unsatisfiable manifest at build time.
        if dto.plugins.len() + dto.dependencies.len() > dto.module_bound as usize {
            return Err(invalid(
                "cluster declared module count exceeds the module bound",
            ));
        }

        let mut basenames = HashSet::with_capacity(dto.plugins.len() + dto.dependencies.len());
        let mut plugins = Vec::with_capacity(dto.plugins.len());
        for plugin in dto.plugins {
            validate_windows_basename("plugin", &plugin.basename)?;
            if !basenames.insert(plugin.basename.to_lowercase()) {
                return Err(invalid("duplicate or case-colliding cluster basename"));
            }
            let sha256 = decode_sha256("plugin", &plugin.sha256)?;
            if let Some(payload) = &plugin.payload {
                if payload.len() > MAX_CLUSTER_PAYLOAD_BYTES || !payload.is_ascii() {
                    return Err(invalid("cluster plugin payload is invalid"));
                }
            }
            plugins.push(ValidatedClusterPlugin {
                basename: plugin.basename,
                sha256,
                payload: plugin.payload,
            });
        }
        let mut dependencies = Vec::with_capacity(dto.dependencies.len());
        for dependency in dto.dependencies {
            validate_windows_basename("dependency", &dependency.basename)?;
            if !basenames.insert(dependency.basename.to_lowercase()) {
                return Err(invalid("duplicate or case-colliding cluster basename"));
            }
            if dependency.size == 0 {
                return Err(invalid("dependency size must be nonzero"));
            }
            dependencies.push(ValidatedClusterDependency {
                basename: dependency.basename,
                sha256: decode_sha256("dependency", &dependency.sha256)?,
                size: dependency.size,
            });
        }
        Ok(Self {
            plugins,
            dependencies,
            module_bound: dto.module_bound,
        })
    }

    /// Parses a serialized manifest (bounded to the documented 4 MiB body).
    pub fn parse(json: &[u8]) -> io::Result<Self> {
        if json.is_empty() || json.len() > MAX_CLUSTER_MANIFEST_BYTES {
            return Err(invalid("cluster manifest body is outside 1 byte..4 MiB"));
        }
        let dto: ClusterManifestDto =
            serde_json::from_slice(json).map_err(|error| invalid(error.to_string()))?;
        Self::validate(dto)
    }

    /// Serializes back to the wire document, enforcing the same body bound.
    pub fn to_json(&self) -> io::Result<String> {
        let dto = ClusterManifestDto {
            schema: CLUSTER_MANIFEST_SCHEMA.to_owned(),
            plugins: self
                .plugins
                .iter()
                .map(|plugin| ClusterPluginDto {
                    basename: plugin.basename.clone(),
                    sha256: hex_sha256(&plugin.sha256),
                    payload: plugin.payload.clone(),
                })
                .collect(),
            dependencies: self
                .dependencies
                .iter()
                .map(|dependency| ClusterDependencyDto {
                    basename: dependency.basename.clone(),
                    sha256: hex_sha256(&dependency.sha256),
                    size: dependency.size,
                })
                .collect(),
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

    pub fn plugin_basename(&self, index: usize) -> Option<&str> {
        self.plugins
            .get(index)
            .map(|plugin| plugin.basename.as_str())
    }

    pub fn plugin_sha256(&self, index: usize) -> Option<[u8; 32]> {
        self.plugins.get(index).map(|plugin| plugin.sha256)
    }

    pub fn module_bound(&self) -> u32 {
        self.module_bound
    }

    /// The declared module set (design §5): every basename a `plugin`-class
    /// audit entry may carry, plugins and pinned dependencies together.
    pub fn declared_basenames(&self) -> Vec<String> {
        self.plugins
            .iter()
            .map(|plugin| plugin.basename.clone())
            .chain(
                self.dependencies
                    .iter()
                    .map(|dependency| dependency.basename.clone()),
            )
            .collect()
    }
}

fn hex_sha256(digest: &[u8; 32]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Broker-owned temporary file holding the serialized manifest as the
/// *staging source* for the sealed load tree (issue #405, design §2.3). The
/// worker never receives this path: the dispatch stages the document into
/// the sealed root under `CLUSTER_MANIFEST_SEALED_BASENAME` (hash-verified
/// like every other entry) and hands the worker the staged path over argv
/// (`--cluster-manifest-v1 <path>`), so the manifest sits inside the sealed
/// root the worker pins against. This source file lives under
/// `<repository>/target/image-transport` only until staging completes and is
/// removed on drop.
#[derive(Debug)]
pub struct ClusterManifestTransport {
    path: PathBuf,
}

impl ClusterManifestTransport {
    pub fn write(repository: &Path, manifest: &ValidatedClusterManifest) -> io::Result<Self> {
        Self::write_named(
            repository,
            &manifest.to_json()?,
            CLUSTER_MANIFEST_SEALED_BASENAME,
        )
    }

    fn write_named(repository: &Path, json: &str, basename: &str) -> io::Result<Self> {
        let root = repository.join("target/image-transport");
        fs::create_dir_all(&root)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| invalid(error.to_string()))?
            .as_nanos();
        // The sealed tree binds every staged entry's basename to its source
        // file name, so the staging source must itself carry the reserved
        // basename; the nonce directory keeps concurrent cluster sessions
        // apart. The in-place transport (issue #751) reuses the same layout,
        // but the worker reads this document directly.
        let dir = root.join(format!("cluster-manifest-{nonce}"));
        fs::create_dir(&dir)?;
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
// The v2 manifest is the in-place counterpart of the sealed manifest above:
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
                if payload.len() > MAX_CLUSTER_PAYLOAD_BYTES || !payload.is_ascii() {
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
    use sha2::{Digest, Sha256};

    fn artifact(root: &Path, name: &str, bytes: &[u8]) -> ApprovedImageArtifact {
        let path = root.join(name);
        fs::write(&path, bytes).unwrap();
        ApprovedImageArtifact {
            path,
            expected_sha256: Sha256::digest(bytes).into(),
            expected_size: bytes.len() as u64,
        }
    }

    fn source_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-cluster-manifest-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn dto() -> ClusterManifestDto {
        ClusterManifestDto {
            schema: CLUSTER_MANIFEST_SCHEMA.to_owned(),
            plugins: vec![
                ClusterPluginDto {
                    basename: "alpha.aex".into(),
                    sha256: "a".repeat(64),
                    payload: None,
                },
                ClusterPluginDto {
                    basename: "beta.aex".into(),
                    sha256: "b".repeat(64),
                    payload: Some("v2|0=1.0".into()),
                },
            ],
            dependencies: vec![ClusterDependencyDto {
                basename: "helper.dll".into(),
                sha256: "c".repeat(64),
                size: 42,
            }],
            module_bound: 64,
        }
    }

    #[test]
    fn validates_and_round_trips_a_well_formed_manifest() {
        let manifest = ValidatedClusterManifest::validate(dto()).unwrap();
        assert_eq!(manifest.plugin_count(), 2);
        assert_eq!(manifest.plugin_basename(0), Some("alpha.aex"));
        assert_eq!(manifest.plugin_basename(1), Some("beta.aex"));
        assert_eq!(manifest.plugin_basename(2), None);
        assert_eq!(manifest.module_bound(), 64);
        assert_eq!(
            manifest.declared_basenames(),
            vec![
                "alpha.aex".to_owned(),
                "beta.aex".to_owned(),
                "helper.dll".to_owned()
            ]
        );
        let json = manifest.to_json().unwrap();
        let reparsed = ValidatedClusterManifest::parse(json.as_bytes()).unwrap();
        assert_eq!(manifest, reparsed);
        // A discovery manifest omits the payload key entirely.
        let without_payloads = ValidatedClusterManifest::validate(ClusterManifestDto {
            plugins: dto()
                .plugins
                .into_iter()
                .map(|mut plugin| {
                    plugin.payload = None;
                    plugin
                })
                .collect(),
            ..dto()
        })
        .unwrap();
        assert!(!without_payloads.to_json().unwrap().contains("payload"));
    }

    #[test]
    fn builds_from_approved_artifacts() {
        let source = source_dir();
        let plugins = vec![
            artifact(&source, "alpha.aex", b"alpha"),
            artifact(&source, "beta.aex", b"beta"),
        ];
        let dependencies = vec![artifact(&source, "helper.dll", b"helper")];
        let payloads = vec![None, Some("v2|0=2.0".to_owned())];
        let manifest =
            ValidatedClusterManifest::from_approved(&plugins, &dependencies, Some(&payloads), 64)
                .unwrap();
        assert_eq!(manifest.plugin_count(), 2);
        assert_eq!(
            manifest.plugin_sha256(0).unwrap(),
            Sha256::digest(b"alpha").as_ref()
        );
        let json = manifest.to_json().unwrap();
        assert!(json.contains("\"payload\": \"v2|0=2.0\""));
        assert!(
            ValidatedClusterManifest::from_approved(
                &plugins,
                &dependencies,
                Some(&payloads[..1]),
                64
            )
            .is_err()
        );
        fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn fails_closed_on_bound_excess() {
        let mut too_many_plugins = dto();
        too_many_plugins.plugins = (0..=MAX_CLUSTER_PLUGINS)
            .map(|index| ClusterPluginDto {
                basename: format!("plugin-{index}.aex"),
                sha256: "a".repeat(64),
                payload: None,
            })
            .collect();
        too_many_plugins.module_bound = MAX_CLUSTER_MODULE_BOUND;
        assert!(ValidatedClusterManifest::validate(too_many_plugins).is_err());

        let mut zero_bound = dto();
        zero_bound.module_bound = 0;
        assert!(ValidatedClusterManifest::validate(zero_bound).is_err());

        let mut over_bound = dto();
        over_bound.module_bound = MAX_CLUSTER_MODULE_BOUND + 1;
        assert!(ValidatedClusterManifest::validate(over_bound).is_err());

        // A declared set that cannot fit its own bound is unsatisfiable.
        let mut tautological = dto();
        tautological.module_bound = 2;
        assert!(ValidatedClusterManifest::validate(tautological).is_err());
    }

    #[test]
    fn fails_closed_on_bad_basenames_and_collisions() {
        for bad in ["..", "a/b.dll", "a\\b.dll", "CON.dll", "trail."] {
            let mut document = dto();
            document.dependencies[0].basename = bad.to_owned();
            assert!(
                ValidatedClusterManifest::validate(document).is_err(),
                "{bad} must be rejected"
            );
        }
        let mut colliding = dto();
        colliding.dependencies[0].basename = "ALPHA.AEX".into();
        assert!(ValidatedClusterManifest::validate(colliding).is_err());
        let mut duplicate = dto();
        duplicate.plugins.push(duplicate.plugins[0].clone());
        assert!(ValidatedClusterManifest::validate(duplicate).is_err());
    }

    #[test]
    fn fails_closed_on_bad_hashes_sizes_payloads_and_schema() {
        let mut bad_sha = dto();
        bad_sha.plugins[0].sha256 = "not-hex".into();
        assert!(ValidatedClusterManifest::validate(bad_sha).is_err());

        let mut zero_size = dto();
        zero_size.dependencies[0].size = 0;
        assert!(ValidatedClusterManifest::validate(zero_size).is_err());

        let mut non_ascii = dto();
        non_ascii.plugins[0].payload = Some("v2|0=ä".into());
        assert!(ValidatedClusterManifest::validate(non_ascii).is_err());

        let mut oversized = dto();
        oversized.plugins[0].payload = Some("x".repeat(MAX_CLUSTER_PAYLOAD_BYTES + 1));
        assert!(ValidatedClusterManifest::validate(oversized).is_err());

        let mut wrong_schema = dto();
        wrong_schema.schema = "cluster-manifest-v0".into();
        assert!(ValidatedClusterManifest::validate(wrong_schema).is_err());

        assert!(ValidatedClusterManifest::parse(b"").is_err());
        assert!(ValidatedClusterManifest::parse(&[0u8; MAX_CLUSTER_MANIFEST_BYTES + 1]).is_err());
    }

    fn in_place_dto() -> InPlaceClusterManifestDto {
        InPlaceClusterManifestDto {
            schema: IN_PLACE_CLUSTER_MANIFEST_SCHEMA.to_owned(),
            plugins: vec![
                InPlaceClusterPluginDto {
                    path: r"C:\effects\alpha.aex".into(),
                    sha256: "a".repeat(64),
                    payload: None,
                },
                InPlaceClusterPluginDto {
                    path: r"C:\effects\cyco\alpha.aex".into(),
                    sha256: "b".repeat(64),
                    payload: None,
                },
            ],
            search_dirs: vec![r"C:\ae\Support Files".into(), r"C:\effects".into()],
            module_bound: 4096,
        }
    }

    #[test]
    fn in_place_manifest_validates_and_round_trips() {
        let manifest = ValidatedInPlaceClusterManifest::validate(in_place_dto()).unwrap();
        assert_eq!(manifest.plugin_count(), 2);
        assert_eq!(
            manifest.plugin_path(0),
            Some(Path::new(r"C:\effects\alpha.aex"))
        );
        // Same basename in different directories is legal in place; identity
        // is the full path.
        assert_eq!(
            manifest.plugin_path(1),
            Some(Path::new(r"C:\effects\cyco\alpha.aex"))
        );
        let json = manifest.to_json().unwrap();
        let reparsed = ValidatedInPlaceClusterManifest::parse(json.as_bytes()).unwrap();
        assert_eq!(manifest, reparsed);
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
        colliding.plugins[1].path = r"C:\EFFECTS\ALPHA.AEX".into();
        assert!(ValidatedInPlaceClusterManifest::validate(colliding).is_err());

        let mut relative_dir = in_place_dto();
        relative_dir.search_dirs[0] = "relative".into();
        assert!(ValidatedInPlaceClusterManifest::validate(relative_dir).is_err());

        let mut too_many_dirs = in_place_dto();
        too_many_dirs.search_dirs = (0..17).map(|index| format!(r"C:\dir-{index}")).collect();
        assert!(ValidatedInPlaceClusterManifest::validate(too_many_dirs).is_err());

        let mut no_dirs = in_place_dto();
        no_dirs.search_dirs.clear();
        assert!(ValidatedInPlaceClusterManifest::validate(no_dirs).is_err());

        let mut wrong_schema = in_place_dto();
        wrong_schema.schema = CLUSTER_MANIFEST_SCHEMA.to_owned();
        assert!(ValidatedInPlaceClusterManifest::validate(wrong_schema).is_err());
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

    #[test]
    fn transport_writes_verifies_and_cleans_up() {
        let repository = source_dir();
        let manifest = ValidatedClusterManifest::validate(dto()).unwrap();
        let (path, dir) = {
            let transport = ClusterManifestTransport::write(&repository, &manifest).unwrap();
            let path = transport.path().to_owned();
            assert!(path.is_absolute());
            // The sealed tree binds the staged basename to the source file
            // name, so the staging source carries the sealed basename.
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(CLUSTER_MANIFEST_SEALED_BASENAME)
            );
            let on_disk = fs::read_to_string(&path).unwrap();
            assert_eq!(
                ValidatedClusterManifest::parse(on_disk.as_bytes()).unwrap(),
                manifest
            );
            let dir = path.parent().unwrap().to_owned();
            (path, dir)
        };
        assert!(!path.exists(), "drop removes the manifest file");
        assert!(!dir.exists(), "drop removes the staging directory");
        fs::remove_dir_all(repository).unwrap();
    }
}
