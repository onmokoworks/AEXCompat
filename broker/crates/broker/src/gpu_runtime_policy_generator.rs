use crate::gpu_platform_collector::{
    collect_active_driver_module_identity, privacy_bounded_identity_report,
};
use crate::pnp_opencl_runtime_collector::{
    PnpOpenClArchitecture, PnpOpenClBindingStatus, PnpOpenClClassification,
    collect_pnp_opencl_runtime_candidates,
};
use crate::runtime_module_identity::RuntimeModuleIdentityEvidence;
use crate::runtime_module_policy::{
    GpuPlatformIdentity, RuntimeBackend, RuntimeModule, RuntimeModulePolicy,
};
use std::io;
use std::time::{Duration, SystemTime};

/// Generates a short-lived OpenCL policy only when exactly one loader-selected
/// native ICD belongs to exactly one active hardware adapter's signed catalog.
/// Registry names, embedded signatures, and driver basenames are candidates,
/// never authorization evidence by themselves.
pub fn generate_opencl_runtime_policy() -> io::Result<RuntimeModulePolicy> {
    let collection = collect_pnp_opencl_runtime_candidates();
    let mut verified = Vec::new();
    for candidate in collection.candidates.iter().filter(|candidate| {
        candidate.architecture == PnpOpenClArchitecture::Native
            && candidate.classification == PnpOpenClClassification::IdentityVerified
            && candidate.loader_selected
            && matches!(
                candidate.status,
                PnpOpenClBindingStatus::Verified
                    | PnpOpenClBindingStatus::UnverifiedNoCatalogEvidence
            )
    }) {
        let (Some(path), Some(registration_luid), Some(original)) = (
            candidate.path.as_deref(),
            candidate.registration_adapter_luid,
            candidate.identity.as_ref(),
        ) else {
            continue;
        };
        if let Ok((platform, module)) =
            collect_active_driver_module_identity(registration_luid, RuntimeBackend::Opencl, path)
        {
            if same_registration_identity(original, &module)
                && candidate
                    .adapter
                    .as_ref()
                    .is_none_or(|recorded| recorded == &platform)
            {
                verified.push((platform, module));
            }
        }
    }
    let [(platform, module)] = verified.as_slice() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "OpenCL runtime requires exactly one active catalog-bound native ICD; found {}",
                verified.len()
            ),
        ));
    };
    RuntimeModulePolicy::from_authenticated_modules_at(
        platform.clone(),
        std::slice::from_ref(module),
        Duration::from_secs(120),
        SystemTime::now(),
    )
}

fn same_registration_identity(
    original: &RuntimeModuleIdentityEvidence,
    current: &RuntimeModuleIdentityEvidence,
) -> bool {
    original.canonical_path == current.canonical_path
        && original.file_identity == current.file_identity
        && original.size == current.size
        && original.sha256 == current.sha256
        && original.pe_machine == current.pe_machine
}

/// A shareable receipt without private module or DriverStore paths. The
/// authoritative policy stays in memory and is never reconstructed from this
/// diagnostic projection.
pub fn privacy_bounded_gpu_policy_report(policy: &RuntimeModulePolicy) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "platform": policy.platform().map(privacy_bounded_identity_report),
        "modules": policy.modules().iter().map(|module| serde_json::json!({
            "basename": module.basename,
            "sha256": module.sha256.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "size": module.size,
        })).collect::<Vec<_>>(),
    })
}

/// Re-observes the active adapter package and every authorized module before
/// launching a GPU worker. A driver update, package swap, or module replacement
/// invalidates a generated policy instead of silently inheriting its authority.
pub(crate) fn validate_active_gpu_policy(policy: &RuntimeModulePolicy) -> io::Result<()> {
    let Some(expected) = policy.platform() else {
        // Recorded schema-v1 policies do not contain a platform identity.
        return Ok(());
    };
    if expected.backend != RuntimeBackend::Opencl || policy.modules().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "active platform revalidation does not support this GPU policy",
        ));
    }
    for module in policy.modules() {
        if module.backend != expected.backend {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "GPU policy module backend changed",
            ));
        }
        let (observed, evidence) = collect_active_driver_module_identity(
            expected.adapter_luid,
            expected.backend,
            &module.path,
        )?;
        verify_active_binding(expected, module, &observed, &evidence)?;
    }
    Ok(())
}

fn verify_active_binding(
    expected: &GpuPlatformIdentity,
    module: &RuntimeModule,
    observed: &GpuPlatformIdentity,
    evidence: &RuntimeModuleIdentityEvidence,
) -> io::Result<()> {
    if observed != expected
        || evidence.canonical_path != module.path
        || evidence.sha256 != module.sha256
        || evidence.size != module.size
        // A locally generated policy retains the opened file identity. A
        // parsed v2 policy has no serialized file identity; its current bytes
        // and active catalog membership are still re-authenticated here.
        || module
            .file_identity
            .is_some_and(|identity| identity != evidence.file_identity)
        || evidence.signing_catalog_sha256 != Some(expected.driver_catalog_sha256)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GPU adapter, driver package, or module identity changed",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_module_identity::{AuthenticodeEvidence, FileIdentity, PeMachine};
    use std::path::PathBuf;

    #[test]
    fn active_binding_rejects_driver_adapter_os_and_module_changes() {
        let platform = GpuPlatformIdentity {
            backend: RuntimeBackend::Opencl,
            adapter_luid: 1,
            pci_vendor_id: 0x10de,
            pci_device_id: 0x2782,
            pci_subsystem_id: 1,
            pci_revision_id: 1,
            driver_inf: "oem1.inf".into(),
            driver_catalog_sha256: [7; 32],
            driver_version: "1.2.3.4".into(),
            os_build: 26200,
        };
        let module = RuntimeModule {
            path: PathBuf::from(r"C:\Windows\System32\driver.dll"),
            basename: "driver.dll".into(),
            sha256: [8; 32],
            size: 512,
            file_identity: Some(FileIdentity {
                volume_serial_number: 1,
                file_index: 2,
            }),
            backend: RuntimeBackend::Opencl,
            signer_thumbprint: None,
            version: None,
        };
        let evidence = RuntimeModuleIdentityEvidence {
            canonical_path: module.path.clone(),
            size: module.size,
            sha256: module.sha256,
            pe_machine: PeMachine::Amd64,
            file_identity: FileIdentity {
                volume_serial_number: 1,
                file_index: 2,
            },
            authenticode: AuthenticodeEvidence::Catalog,
            signing_catalog_sha256: Some(platform.driver_catalog_sha256),
        };
        assert!(verify_active_binding(&platform, &module, &platform, &evidence).is_ok());
        let mut changed = platform.clone();
        changed.adapter_luid += 1;
        assert!(verify_active_binding(&platform, &module, &changed, &evidence).is_err());
        changed = platform.clone();
        changed.driver_version = "1.2.3.5".into();
        assert!(verify_active_binding(&platform, &module, &changed, &evidence).is_err());
        changed = platform.clone();
        changed.os_build += 1;
        assert!(verify_active_binding(&platform, &module, &changed, &evidence).is_err());
        let mut changed_evidence = evidence.clone();
        changed_evidence.sha256 = [9; 32];
        assert!(verify_active_binding(&platform, &module, &platform, &changed_evidence).is_err());
        changed_evidence = evidence.clone();
        changed_evidence.file_identity.file_index += 1;
        assert!(verify_active_binding(&platform, &module, &platform, &changed_evidence).is_err());
        changed_evidence = evidence.clone();
        changed_evidence.signing_catalog_sha256 = None;
        assert!(verify_active_binding(&platform, &module, &platform, &changed_evidence).is_err());
        let parsed_module = RuntimeModule {
            file_identity: None,
            ..module
        };
        assert!(verify_active_binding(&platform, &parsed_module, &platform, &evidence).is_ok());
        assert!(same_registration_identity(&evidence, &evidence));
        let mut replaced_registration = evidence.clone();
        replaced_registration.file_identity.file_index += 1;
        assert!(!same_registration_identity(
            &evidence,
            &replaced_registration
        ));
    }
}
