use crate::secure_image_dispatch::ApprovedImageArtifact;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

pub const MAX_SESSION_DEPENDENCIES: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionDependencyManifestDto {
    pub schema_version: u32,
    pub dependencies: Vec<SessionDependencyDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionDependencyDto {
    pub path: PathBuf,
    pub basename: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedSessionDependencyManifest {
    dependencies: Vec<ApprovedImageArtifact>,
}

impl ValidatedSessionDependencyManifest {
    pub fn approved_image_artifacts(&self) -> &[ApprovedImageArtifact] {
        &self.dependencies
    }

    pub fn into_approved_image_artifacts(self) -> Vec<ApprovedImageArtifact> {
        self.dependencies
    }
}

/// Parses and validates dependencies approved for one dispatch session.
///
/// The main plugin remains outside this manifest. Its basename participates in
/// collision checks so a dependency cannot replace it in the sealed load tree.
pub fn parse_and_validate(
    json: &[u8],
    main_plugin: &ApprovedImageArtifact,
) -> io::Result<ValidatedSessionDependencyManifest> {
    let dto: SessionDependencyManifestDto =
        serde_json::from_slice(json).map_err(|error| invalid(error.to_string()))?;
    validate(dto, main_plugin)
}

pub fn validate(
    dto: SessionDependencyManifestDto,
    main_plugin: &ApprovedImageArtifact,
) -> io::Result<ValidatedSessionDependencyManifest> {
    validate_with_limit(dto, main_plugin, MAX_SESSION_DEPENDENCIES)
}

/// `validate` with a caller-chosen count limit.
///
/// `MAX_SESSION_DEPENDENCIES` bounds a manifest that arrives as an external JSON
/// document. A caller that assembled the list itself out of data it already
/// validated (the broker's import-closure resolver) owns that bound instead and
/// passes its own. Every per-dependency check below is unchanged either way.
pub fn validate_with_limit(
    dto: SessionDependencyManifestDto,
    main_plugin: &ApprovedImageArtifact,
    max_dependencies: usize,
) -> io::Result<ValidatedSessionDependencyManifest> {
    if dto.schema_version != 1 {
        return Err(invalid("unsupported session dependency manifest schema"));
    }
    if dto.dependencies.len() > max_dependencies {
        return Err(invalid("session dependency limit exceeded"));
    }

    validate_approved_artifact(main_plugin, "main plugin")?;

    let main_basename = basename_from_path(&main_plugin.path, "main plugin")?;
    validate_windows_basename("dependency", main_basename)?;
    let mut basenames = HashSet::with_capacity(dto.dependencies.len() + 1);
    basenames.insert(fold_windows(main_basename));
    let mut paths = HashSet::with_capacity(dto.dependencies.len());
    let mut approved = Vec::with_capacity(dto.dependencies.len());

    for dependency in dto.dependencies {
        if !dependency.path.is_absolute() {
            return Err(invalid("dependency path must be absolute"));
        }
        validate_windows_basename("dependency", &dependency.basename)?;
        if basename_from_path(&dependency.path, "dependency")? != dependency.basename {
            return Err(invalid("dependency path does not end with its basename"));
        }
        if !basenames.insert(fold_windows(&dependency.basename)) {
            return Err(invalid("duplicate or case-colliding dependency basename"));
        }
        if !paths.insert(fold_windows(&dependency.path.to_string_lossy())) {
            return Err(invalid("duplicate dependency path"));
        }
        if dependency.size == 0 {
            return Err(invalid("dependency size must be nonzero"));
        }
        let expected_sha256 = decode_sha256("dependency", &dependency.sha256)?;
        validate_source_file(&dependency.path, dependency.size, &expected_sha256)?;
        approved.push(ApprovedImageArtifact {
            path: dependency.path,
            expected_sha256,
            expected_size: dependency.size,
        });
    }

    Ok(ValidatedSessionDependencyManifest {
        dependencies: approved,
    })
}

fn validate_approved_artifact(artifact: &ApprovedImageArtifact, kind: &str) -> io::Result<()> {
    if !artifact.path.is_absolute() {
        return Err(invalid(format!("{kind} path must be absolute")));
    }
    if artifact.expected_size == 0 {
        return Err(invalid(format!("{kind} size must be nonzero")));
    }
    validate_source_file(
        &artifact.path,
        artifact.expected_size,
        &artifact.expected_sha256,
    )
}

fn basename_from_path<'a>(path: &'a Path, kind: &str) -> io::Result<&'a str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| invalid(format!("{kind} path must have a UTF-8 basename")))
}

/// Windows-safe basename rules shared with the cluster manifest (issue #405):
/// a single normal path component with no separators, drive letters, control
/// characters, trailing dots/spaces, or reserved device names. `kind` only
/// labels the error message ("dependency" keeps the historical wording).
pub(crate) fn validate_windows_basename(kind: &str, name: &str) -> io::Result<()> {
    let mut components = Path::new(name).components();
    if name.is_empty()
        || name == "."
        || name == ".."
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || name.ends_with(['.', ' '])
    {
        return Err(invalid(format!("{kind} basename is not Windows-safe")));
    }

    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', ' ']);
    let upper = stem.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || matches!(upper.as_bytes(), [b'C', b'O', b'M', b'1'..=b'9'])
        || matches!(upper.as_bytes(), [b'L', b'P', b'T', b'1'..=b'9']);
    if reserved {
        return Err(invalid(format!(
            "{kind} basename is a reserved Windows device name",
        )));
    }
    Ok(())
}

pub(crate) fn decode_sha256(kind: &str, value: &str) -> io::Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(format!(
            "{kind} SHA-256 must be 64 hexadecimal characters",
        )));
    }
    let mut digest = [0; 32];
    for (output, pair) in digest.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).map_err(|_| invalid("invalid SHA-256"))?;
        *output = u8::from_str_radix(pair, 16).map_err(|_| invalid("invalid SHA-256"))?;
    }
    Ok(digest)
}

fn fold_windows(value: &str) -> String {
    value.to_lowercase()
}

fn validate_source_file(
    path: &Path,
    expected_size: u64,
    expected_sha256: &[u8; 32],
) -> io::Result<()> {
    reject_reparse_components(path)?;
    let mut file = open_source(path)?;
    validate_regular_unique(&file)?;
    if file.metadata()?.len() != expected_size {
        return Err(invalid("dependency size does not match the source file"));
    }
    let mut bytes = Vec::with_capacity(expected_size as usize);
    file.read_to_end(&mut bytes)?;
    let actual = Sha256::digest(bytes);
    if actual.as_slice() != expected_sha256 {
        return Err(invalid(
            "approved artifact SHA-256 does not match the source file",
        ));
    }
    Ok(())
}

fn reject_reparse_components(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(invalid("dependency path contains a reparse point"));
        }
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
fn validate_regular_unique(file: &File) -> io::Result<()> {
    use std::mem::zeroed;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, GetFileInformationByHandle,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.nNumberOfLinks != 1
        || !file.metadata()?.is_file()
    {
        return Err(invalid(
            "dependency must be a regular, non-reparse, single-link file",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(invalid("dependency must be a regular, single-link file"));
    }
    Ok(())
}

#[cfg(not(any(windows, unix)))]
fn validate_regular_unique(file: &File) -> io::Result<()> {
    if !file.metadata()?.is_file() {
        return Err(invalid("dependency must be a regular file"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
