use crate::secure_image_dispatch::ApprovedImageArtifact;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_COMPANIONS: usize = 8;
pub const MAX_DECLARED_SUITES: usize = 32;
pub const MAX_SUITE_NAME_BYTES: usize = 255;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CompanionSuiteIdentity {
    pub name: String,
    pub api_version: u32,
    pub internal_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedCompanion {
    pub artifact: ApprovedImageArtifact,
    pub suites: Vec<CompanionSuiteIdentity>,
}

#[derive(Serialize)]
struct Manifest<'a> {
    schema: &'static str,
    companions: Vec<ManifestCompanion<'a>>,
}

#[derive(Serialize)]
struct ManifestCompanion<'a> {
    path: PathBuf,
    sha256: String,
    suites: &'a [CompanionSuiteIdentity],
}

pub struct CompanionManifestTransport(PathBuf);

impl CompanionManifestTransport {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for CompanionManifestTransport {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub fn write_transport(
    repository: &Path,
    companions: &[ApprovedCompanion],
) -> io::Result<Option<CompanionManifestTransport>> {
    if companions.is_empty() {
        return Ok(None);
    }
    if companions.len() > MAX_COMPANIONS {
        return Err(invalid("companion count exceeds 8"));
    }
    let mut paths = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut suite_count = 0usize;
    let mut records = Vec::with_capacity(companions.len());
    for companion in companions {
        if !companion.artifact.path.is_absolute() {
            return Err(invalid("companion path must be absolute"));
        }
        let canonical = companion
            .artifact
            .path
            .canonicalize()
            .map_err(|_| invalid("companion path is unavailable"))?;
        if !paths.insert(canonical.clone()) {
            return Err(invalid("companion path is duplicated"));
        }
        let metadata = fs::metadata(&canonical)?;
        if metadata.len() != companion.artifact.expected_size {
            tracing::warn!(
                "companion AEX size changed since discovery; recording current launch state"
            );
        }
        if companion.suites.is_empty() {
            return Err(invalid("companion declares no suites"));
        }
        suite_count = suite_count
            .checked_add(companion.suites.len())
            .ok_or_else(|| invalid("companion suite count overflows"))?;
        if suite_count > MAX_DECLARED_SUITES {
            return Err(invalid("companion suite count exceeds 32"));
        }
        for suite in &companion.suites {
            if suite.name.is_empty()
                || suite.name.as_bytes().len() > MAX_SUITE_NAME_BYTES
                || suite.name.as_bytes().contains(&0)
                || suite.api_version == 0
                || !identities.insert(suite.clone())
            {
                return Err(invalid("companion suite identity is invalid or duplicated"));
            }
        }
        records.push(ManifestCompanion {
            path: canonical,
            sha256: companion
                .artifact
                .expected_sha256
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            suites: &companion.suites,
        });
    }
    let root = repository.join("target").join("image-transport");
    fs::create_dir_all(&root)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = root.join(format!(
        "companion-manifest-{nonce}-{}.json",
        std::process::id()
    ));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let manifest = Manifest {
        schema: "companion-manifest-v1",
        companions: records,
    };
    if let Err(error) = serde_json::to_writer_pretty(&mut output, &manifest)
        .map_err(|error| invalid(error.to_string()))
        .and_then(|_| output.write_all(b"\n"))
        .and_then(|_| output.sync_all())
    {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(Some(CompanionManifestTransport(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(root: &Path, name: &str, byte: u8) -> ApprovedCompanion {
        let path = root.join(name);
        fs::write(&path, [byte]).unwrap();
        ApprovedCompanion {
            artifact: ApprovedImageArtifact {
                path: path.canonicalize().unwrap(),
                expected_sha256: [byte; 32],
                expected_size: 1,
            },
            suites: vec![CompanionSuiteIdentity {
                name: format!("Fixture Suite {byte}"),
                api_version: 1,
                internal_version: 0,
            }],
        }
    }

    #[test]
    fn transport_is_strict_bounded_and_owned_for_its_lifetime() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-companion-manifest-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let repository = root.canonicalize().unwrap();
        let companion = fixture(&repository, "provider.aex", 7);
        let transport = write_transport(&repository, std::slice::from_ref(&companion))
            .unwrap()
            .unwrap();
        let path = transport.path().to_path_buf();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(document["schema"], "companion-manifest-v1");
        assert_eq!(
            PathBuf::from(document["companions"][0]["path"].as_str().unwrap()),
            companion.artifact.path.canonicalize().unwrap()
        );
        assert_eq!(
            document["companions"][0]["suites"][0]["name"],
            "Fixture Suite 7"
        );
        drop(transport);
        assert!(!path.exists());

        let mut duplicate = companion.clone();
        duplicate.suites[0]
            .name
            .clone_from(&companion.suites[0].name);
        let error = write_transport(&repository, &[companion.clone(), duplicate])
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let mut too_many = Vec::new();
        for index in 0..=MAX_COMPANIONS {
            too_many.push(fixture(
                &repository,
                &format!("provider-{index}.aex"),
                index as u8,
            ));
        }
        assert!(write_transport(&repository, &too_many).is_err());

        let alias_directory = repository.join("alias");
        fs::create_dir_all(&alias_directory).unwrap();
        let mut noncanonical = fixture(&repository, "canonical-provider.aex", 99);
        noncanonical.artifact.path = alias_directory.join("..").join("canonical-provider.aex");
        let alias_transport = write_transport(&repository, &[noncanonical])
            .unwrap()
            .unwrap();
        let alias_document: serde_json::Value =
            serde_json::from_slice(&fs::read(alias_transport.path()).unwrap()).unwrap();
        assert_eq!(
            PathBuf::from(alias_document["companions"][0]["path"].as_str().unwrap()),
            repository
                .join("canonical-provider.aex")
                .canonicalize()
                .unwrap()
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
