#![cfg(windows)]

use aexcompat_broker::gpu_platform_collector::{
    collect_gpu_platform_identity, enumerate_gpu_adapters, privacy_bounded_identity_report,
};
use aexcompat_broker::runtime_module_policy::{
    RuntimeBackend, parse_and_validate_for_platform, validate_platform_binding,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    module: PathBuf,
    digest: String,
    size: u64,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "aex-gpu-platform-policy-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let module = root.join("runtime.dll");
        let bytes = b"issue 24 collector policy module";
        fs::write(&module, bytes).unwrap();
        Self {
            root,
            module,
            digest: hex(&Sha256::digest(bytes)),
            size: bytes.len() as u64,
        }
    }

    fn policy_json(&self, identity: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "expires": "2099-01-02T03:04:05Z",
            "platform": {
                "backend": identity["backend"],
                "adapter_luid": identity["adapter_luid"],
                "pci_vendor_id": identity["pci_vendor_id"],
                "pci_device_id": identity["pci_device_id"],
                "pci_subsystem_id": identity["pci_subsystem_id"],
                "pci_revision_id": identity["pci_revision_id"],
                "driver_inf": identity["driver_inf"],
                "driver_catalog_sha256": identity["driver_catalog_sha256"],
                "driver_version": identity["driver_version"],
                "os_build": identity["os_build"],
            },
            "modules": [{
                "path": self.module,
                "basename": "runtime.dll",
                "sha256": self.digest,
                "size": self.size,
                "backend": "directx",
            }],
        }))
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn collector_rejects_cpu_and_unknown_luid() {
    assert!(collect_gpu_platform_identity(1, RuntimeBackend::Cpu).is_err());
    assert!(collect_gpu_platform_identity(u64::MAX, RuntimeBackend::Directx).is_err());
}

#[test]
#[ignore = "requires a present physical PCI GPU and its installed driver catalog"]
fn windows_real_gpu_collector_binds_policy_and_rejects_driver_change() {
    let adapters = enumerate_gpu_adapters().unwrap();
    let mut collected = None;
    let mut errors = Vec::new();
    for adapter in adapters {
        match collect_gpu_platform_identity(adapter.adapter_luid, RuntimeBackend::Directx) {
            Ok(identity) => {
                collected = Some(identity);
                break;
            }
            Err(error) => errors.push(format!("{:016x}: {error}", adapter.adapter_luid)),
        }
    }
    let identity = collected.unwrap_or_else(|| panic!("no collectable PCI GPU: {errors:?}"));
    let report = privacy_bounded_identity_report(&identity);
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.to_ascii_lowercase().contains("driverstore"));
    assert!(!serialized.contains(":\\"));
    assert!(report["driver_inf"].as_str().unwrap().ends_with(".inf"));
    assert_eq!(report["driver_catalog_sha256"].as_str().unwrap().len(), 64);

    let fixture = Fixture::new();
    let policy = parse_and_validate_for_platform(&fixture.policy_json(&report), &identity).unwrap();
    validate_platform_binding(&policy, &identity).unwrap();

    let mut changed = identity.clone();
    changed.driver_version.push_str(".1");
    assert!(validate_platform_binding(&policy, &changed).is_err());

    let mut changed = identity.clone();
    changed.driver_catalog_sha256[0] ^= 0xff;
    assert!(validate_platform_binding(&policy, &changed).is_err());
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
