use crate::runtime_module_policy::{MAX_RUNTIME_MODULES, RuntimeBackend, RuntimeModulePolicy};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io;
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
}
