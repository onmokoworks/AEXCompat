use crate::runtime_module_policy::{MAX_RUNTIME_MODULES, RuntimeBackend, RuntimeModulePolicy};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAGIC: &[u8; 8] = b"AEXRMA1\0";
const MAX_PATH_UTF16_UNITS: usize = 32_767;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RuntimeModulePurpose {
    PfParameterInspect = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeModuleAuthorizationManifest {
    pub bytes: Vec<u8>,
    pub sha256: [u8; 32],
    pub size: u64,
}

pub(crate) struct RuntimeAuthorizationTransport {
    path: PathBuf,
    hold: Option<File>,
    pub(crate) artifact: crate::secure_image_dispatch::ApprovedImageArtifact,
    pub(crate) basename: String,
    pub(crate) session_identity: [u8; 32],
}

struct PartialTransport(PathBuf);

impl Drop for PartialTransport {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl RuntimeAuthorizationTransport {
    pub(crate) fn append_launch(
        &self,
        in_place: bool,
        args_after_plugin: &mut Vec<String>,
        dependencies: &mut Vec<crate::secure_image_dispatch::ApprovedImageArtifact>,
    ) {
        args_after_plugin.push("--runtime-module-authorization-v1".to_owned());
        if in_place {
            args_after_plugin.push(self.path.to_string_lossy().into_owned());
        } else {
            args_after_plugin.push(self.basename.clone());
            dependencies.push(self.artifact.clone());
        }
    }
}

impl Drop for RuntimeAuthorizationTransport {
    fn drop(&mut self) {
        drop(self.hold.take());
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn prepare_runtime_authorization_transport(
    repository: &Path,
    policy: &RuntimeModulePolicy,
    backend: RuntimeBackend,
) -> io::Result<RuntimeAuthorizationTransport> {
    let mut session_identity = rand::random::<[u8; 32]>();
    if session_identity.iter().all(|byte| *byte == 0) {
        session_identity[0] = 1;
    }
    prepare_runtime_authorization_transport_with_identity(
        repository,
        policy,
        backend,
        session_identity,
    )
}

pub(crate) fn prepare_runtime_authorization_transport_with_identity(
    repository: &Path,
    policy: &RuntimeModulePolicy,
    backend: RuntimeBackend,
    session_identity: [u8; 32],
) -> io::Result<RuntimeAuthorizationTransport> {
    if session_identity.iter().all(|byte| *byte == 0) {
        return Err(invalid("runtime module session identity must be nonzero"));
    }
    let manifest = encode_runtime_module_authorization(
        policy,
        RuntimeModulePurpose::PfParameterInspect,
        backend,
        session_identity,
    )?;
    let root = repository.join("target/image-transport");
    fs::create_dir_all(&root)?;
    let root = fs::canonicalize(root)?;
    let basename = format!("runtime-authorization-{:032x}.bin", rand::random::<u128>());
    let path = root.join(&basename);
    let partial = PartialTransport(path.clone());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&manifest.bytes)?;
    file.sync_all()?;
    drop(file);
    let hold = open_transport_hold(&path)?;
    std::mem::forget(partial);
    Ok(RuntimeAuthorizationTransport {
        path: path.clone(),
        hold: Some(hold),
        artifact: crate::secure_image_dispatch::ApprovedImageArtifact {
            path,
            expected_sha256: manifest.sha256,
            expected_size: manifest.size,
        },
        basename,
        session_identity,
    })
}

#[cfg(windows)]
fn open_transport_hold(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
}

#[cfg(not(windows))]
fn open_transport_hold(path: &Path) -> io::Result<File> {
    File::open(path)
}

pub fn encode_runtime_module_authorization(
    policy: &RuntimeModulePolicy,
    purpose: RuntimeModulePurpose,
    backend: RuntimeBackend,
    session_identity: [u8; 32],
) -> io::Result<RuntimeModuleAuthorizationManifest> {
    encode_runtime_module_authorization_at(
        policy,
        purpose,
        backend,
        session_identity,
        SystemTime::now(),
    )
}

pub fn encode_runtime_module_authorization_at(
    policy: &RuntimeModulePolicy,
    purpose: RuntimeModulePurpose,
    backend: RuntimeBackend,
    session_identity: [u8; 32],
    now: SystemTime,
) -> io::Result<RuntimeModuleAuthorizationManifest> {
    let backend_id = match backend {
        RuntimeBackend::Cpu => return Err(invalid("CPU has no runtime module authorization")),
        RuntimeBackend::Cuda => 1u32,
        RuntimeBackend::Opencl => 2,
        RuntimeBackend::Directx => 3,
        RuntimeBackend::Opengl => 4,
    };
    if session_identity.iter().all(|byte| *byte == 0) {
        return Err(invalid(
            "runtime module authorization session must be nonzero",
        ));
    }
    if policy.expires() <= now {
        return Err(invalid("runtime module authorization policy is expired"));
    }
    let expiry_ms: u64 = policy
        .expires()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("runtime module authorization expiry predates Unix epoch"))?
        .as_millis()
        .try_into()
        .map_err(|_| invalid("runtime module authorization expiry is out of range"))?;
    let modules: Vec<_> = policy
        .modules()
        .iter()
        .filter(|module| module.backend == backend)
        .collect();
    if modules.is_empty() {
        return Err(invalid("runtime module authorization is empty"));
    }
    if modules.len() > MAX_RUNTIME_MODULES {
        return Err(invalid("runtime module authorization limit exceeded"));
    }

    let mut encoded_paths = Vec::with_capacity(modules.len());
    let mut paths = HashSet::with_capacity(modules.len());
    let mut basenames = HashSet::with_capacity(modules.len());
    for module in &modules {
        if module.size == 0 {
            return Err(invalid("runtime module authorization size must be nonzero"));
        }
        let path = module
            .path
            .to_str()
            .ok_or_else(|| invalid("runtime module authorization path must be valid Unicode"))?;
        let utf16: Vec<u16> = path.encode_utf16().collect();
        if utf16.is_empty() || utf16.len() > MAX_PATH_UTF16_UNITS {
            return Err(invalid(
                "runtime module authorization path length is invalid",
            ));
        }
        let folded_path = path.replace('/', "\\").to_lowercase();
        if !paths.insert(folded_path) {
            return Err(invalid("runtime module authorization path collision"));
        }
        if !basenames.insert(module.basename.to_lowercase()) {
            return Err(invalid("runtime module authorization basename collision"));
        }
        encoded_paths.push(utf16);
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(purpose as u32).to_le_bytes());
    bytes.extend_from_slice(&backend_id.to_le_bytes());
    bytes.extend_from_slice(&expiry_ms.to_le_bytes());
    bytes.extend_from_slice(&session_identity);
    bytes.extend_from_slice(&(modules.len() as u32).to_le_bytes());
    for (module, path) in modules.into_iter().zip(encoded_paths) {
        bytes.extend_from_slice(&(path.len() as u32).to_le_bytes());
        for unit in path {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&module.size.to_le_bytes());
        bytes.extend_from_slice(&module.sha256);
    }
    let size = bytes
        .len()
        .try_into()
        .map_err(|_| invalid("runtime module authorization manifest is too large"))?;
    let sha256 = Sha256::digest(&bytes).into();
    Ok(RuntimeModuleAuthorizationManifest {
        bytes,
        sha256,
        size,
    })
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_module_policy::parse_and_validate_at;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    fn policy() -> (RuntimeModulePolicy, Vec<PathBuf>) {
        let root = std::env::temp_dir().join(format!(
            "aex-rma-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir(&root).unwrap();
        let cuda = root.join("module-cuda.bin");
        let opencl = root.join("module-opencl.bin");
        fs::write(&cuda, b"cuda").unwrap();
        fs::write(&opencl, b"opencl").unwrap();
        let cuda = fs::canonicalize(cuda).unwrap();
        let opencl = fs::canonicalize(opencl).unwrap();
        let json = json!({
            "schema_version": 1,
            "expires": "2099-01-01T00:00:00Z",
            "modules": [
                {
                    "path": cuda.clone(),
                    "basename": "module-cuda.bin",
                    "sha256": format!("{:x}", Sha256::digest(b"cuda")),
                    "size": 4,
                    "backend": "cuda"
                },
                {
                    "path": opencl.clone(),
                    "basename": "module-opencl.bin",
                    "sha256": format!("{:x}", Sha256::digest(b"opencl")),
                    "size": 6,
                    "backend": "opencl"
                }
            ]
        });
        let policy = parse_and_validate_at(
            json.to_string().as_bytes(),
            UNIX_EPOCH + Duration::from_secs(1),
        )
        .unwrap();
        (policy, vec![cuda, opencl, root])
    }

    #[test]
    fn encodes_strict_little_endian_manifest_for_selected_backend() {
        let (policy, cleanup) = policy();
        let manifest = encode_runtime_module_authorization_at(
            &policy,
            RuntimeModulePurpose::PfParameterInspect,
            RuntimeBackend::Cuda,
            [7; 32],
            UNIX_EPOCH + Duration::from_secs(1),
        )
        .unwrap();

        assert_eq!(&manifest.bytes[..8], MAGIC);
        assert_eq!(&manifest.bytes[8..12], &1u32.to_le_bytes());
        assert_eq!(&manifest.bytes[12..16], &1u32.to_le_bytes());
        assert_eq!(&manifest.bytes[24..56], &[7; 32]);
        assert_eq!(&manifest.bytes[56..60], &1u32.to_le_bytes());
        let path_units = u32::from_le_bytes(manifest.bytes[60..64].try_into().unwrap()) as usize;
        let expected_path: Vec<u16> = cleanup[0].to_str().unwrap().encode_utf16().collect();
        assert_eq!(path_units, expected_path.len());
        let path_end = 64 + path_units * 2;
        let encoded_path: Vec<u8> = expected_path
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        assert_eq!(&manifest.bytes[64..path_end], encoded_path);
        assert_eq!(&manifest.bytes[path_end..path_end + 8], &4u64.to_le_bytes());
        assert_eq!(manifest.size, manifest.bytes.len() as u64);
        let expected_sha256: [u8; 32] = Sha256::digest(&manifest.bytes).into();
        assert_eq!(manifest.sha256, expected_sha256);

        drop(policy);
        fs::remove_dir_all(cleanup.last().unwrap()).unwrap();
    }

    #[test]
    fn rejects_cpu_zero_session_empty_backend_and_expired_policy() {
        let (policy, cleanup) = policy();
        let encode = |backend, session, now| {
            encode_runtime_module_authorization_at(
                &policy,
                RuntimeModulePurpose::PfParameterInspect,
                backend,
                session,
                now,
            )
        };
        let valid_now = UNIX_EPOCH + Duration::from_secs(1);
        assert!(encode(RuntimeBackend::Cpu, [1; 32], valid_now).is_err());
        assert!(encode(RuntimeBackend::Cuda, [0; 32], valid_now).is_err());
        assert!(encode(RuntimeBackend::Directx, [1; 32], valid_now).is_err());
        assert!(
            encode(
                RuntimeBackend::Cuda,
                [1; 32],
                UNIX_EPOCH + Duration::from_secs(4_102_444_800)
            )
            .is_err()
        );

        drop(policy);
        fs::remove_dir_all(cleanup.last().unwrap()).unwrap();
    }

    #[test]
    fn in_place_transport_uses_an_absolute_argument_without_a_staged_dependency() {
        let (policy, cleanup) = policy();
        let repository = cleanup.last().unwrap();
        let transport = prepare_runtime_authorization_transport_with_identity(
            repository,
            &policy,
            RuntimeBackend::Cuda,
            [9; 32],
        )
        .unwrap();
        let path = transport.path.clone();
        let mut args = Vec::new();
        let mut dependencies = Vec::new();
        transport.append_launch(true, &mut args, &mut dependencies);

        assert_eq!(args[0], "--runtime-module-authorization-v1");
        assert_eq!(PathBuf::from(&args[1]), path);
        assert!(path.is_absolute());
        let transport_root = fs::canonicalize(repository.join("target/image-transport")).unwrap();
        assert_eq!(path.parent(), Some(transport_root.as_path()));
        assert!(dependencies.is_empty());
        assert!(path.is_file());

        drop(transport);
        assert!(!path.exists());
        drop(policy);
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn staged_transport_keeps_the_basename_and_authenticated_dependency() {
        let (policy, cleanup) = policy();
        let repository = cleanup.last().unwrap();
        let transport = prepare_runtime_authorization_transport_with_identity(
            repository,
            &policy,
            RuntimeBackend::Opencl,
            [5; 32],
        )
        .unwrap();
        let mut args = Vec::new();
        let mut dependencies = Vec::new();
        transport.append_launch(false, &mut args, &mut dependencies);

        assert_eq!(
            args,
            ["--runtime-module-authorization-v1", &transport.basename]
        );
        assert_eq!(dependencies, [transport.artifact.clone()]);
        assert_eq!(dependencies[0].path, transport.path);

        drop(transport);
        drop(policy);
        fs::remove_dir_all(repository).unwrap();
    }
}
