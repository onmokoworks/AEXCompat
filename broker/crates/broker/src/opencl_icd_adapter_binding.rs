use crate::gpu_platform_collector::{
    collect_gpu_platform_identity, enumerate_gpu_adapters, privacy_bounded_identity_report,
};
use crate::opencl_icd_collector::{
    OpenClIcdCandidate, OpenClIcdClassification, OpenClRegistryDiagnostic,
    collect_opencl_icd_candidates,
};
use crate::runtime_module_identity::AuthenticodeEvidence;
use crate::runtime_module_policy::{GpuPlatformIdentity, RuntimeBackend};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenClIcdBindingStatus {
    Verified,
    UnverifiedMissingCandidateIdentity,
    UnverifiedNoCatalogEvidence,
    UnverifiedNoMatchingDriverPackage,
    UnverifiedIncompletePlatformEvidence,
    AmbiguousMultipleDriverPackages,
    ConflictMissingCatalogDigest,
    ConflictDriverIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenClPlatformDiagnosticKind {
    AdapterEnumerationFailed,
    DriverIdentityCollectionFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OpenClPlatformDiagnostic {
    pub adapter_luid: Option<String>,
    pub classification: OpenClPlatformDiagnosticKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClIcdAdapterBinding {
    pub candidate: OpenClIcdCandidate,
    pub status: OpenClIcdBindingStatus,
    pub adapter: Option<GpuPlatformIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenClIcdAdapterBindingCollection {
    pub bindings: Vec<OpenClIcdAdapterBinding>,
    pub registry_diagnostics: Vec<OpenClRegistryDiagnostic>,
    pub platform_diagnostics: Vec<OpenClPlatformDiagnostic>,
}

/// Collects the legacy OpenCL ICD candidates and active Windows display-driver
/// packages, then binds them only when catalog membership proves one exact
/// adapter/package match. A shared package, incomplete evidence, or conflicting
/// observation remains fail-closed.
pub fn collect_opencl_icd_adapter_bindings() -> OpenClIcdAdapterBindingCollection {
    let collection = collect_opencl_icd_candidates();
    let mut platforms = Vec::new();
    let mut platform_diagnostics = Vec::new();
    match enumerate_gpu_adapters() {
        Ok(mut adapters) => {
            adapters.sort_by_key(|adapter| adapter.adapter_luid);
            for adapter in adapters {
                match collect_gpu_platform_identity(adapter.adapter_luid, RuntimeBackend::Opencl) {
                    Ok(identity) => platforms.push(identity),
                    Err(_) => platform_diagnostics.push(OpenClPlatformDiagnostic {
                        adapter_luid: Some(format!("{:016x}", adapter.adapter_luid)),
                        classification:
                            OpenClPlatformDiagnosticKind::DriverIdentityCollectionFailed,
                    }),
                }
            }
        }
        Err(_) => platform_diagnostics.push(OpenClPlatformDiagnostic {
            adapter_luid: None,
            classification: OpenClPlatformDiagnosticKind::AdapterEnumerationFailed,
        }),
    }
    bind_opencl_icd_candidates(
        collection.candidates,
        collection.diagnostics,
        platforms,
        platform_diagnostics,
    )
}

pub fn privacy_bounded_opencl_icd_binding_report(
    collection: &OpenClIcdAdapterBindingCollection,
) -> serde_json::Value {
    let bindings = collection
        .bindings
        .iter()
        .map(|binding| {
            let path_text = binding
                .candidate
                .path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned());
            let path_basename = path_text.as_deref().and_then(windows_basename);
            let path_fingerprint_sha256 = path_text.as_deref().map(path_fingerprint);
            let candidate_identity_sha256 = binding
                .candidate
                .identity
                .as_ref()
                .map(|identity| hex(&identity.sha256));
            let adapter = binding
                .adapter
                .as_ref()
                .map(privacy_bounded_identity_report);
            serde_json::json!({
                "registry_view": binding.candidate.registry_view,
                "candidate_classification": binding.candidate.classification,
                "candidate_path_basename": path_basename,
                "candidate_path_fingerprint_sha256": path_fingerprint_sha256,
                "candidate_identity_sha256": candidate_identity_sha256,
                "status": binding.status,
                "authoritative_evidence": if binding.status == OpenClIcdBindingStatus::Verified {
                    Some("catalog_sha256_exact_match")
                } else {
                    None
                },
                "adapter": adapter,
                "backend_ready": false,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": 1,
        "binding_kind": "opencl_icd_adapter_driver",
        "bindings": bindings,
        "registry_diagnostics": collection.registry_diagnostics,
        "platform_diagnostics": collection.platform_diagnostics,
    })
}

fn bind_opencl_icd_candidates(
    candidates: Vec<OpenClIcdCandidate>,
    registry_diagnostics: Vec<OpenClRegistryDiagnostic>,
    mut platforms: Vec<GpuPlatformIdentity>,
    platform_diagnostics: Vec<OpenClPlatformDiagnostic>,
) -> OpenClIcdAdapterBindingCollection {
    platforms.sort_by_key(|identity| identity.adapter_luid);
    let platform_evidence_complete = platform_diagnostics.is_empty();
    let bindings = candidates
        .into_iter()
        .map(|candidate| bind_candidate(candidate, &platforms, platform_evidence_complete))
        .collect();
    OpenClIcdAdapterBindingCollection {
        bindings,
        registry_diagnostics,
        platform_diagnostics,
    }
}

fn bind_candidate(
    candidate: OpenClIcdCandidate,
    platforms: &[GpuPlatformIdentity],
    platform_evidence_complete: bool,
) -> OpenClIcdAdapterBinding {
    let Some(identity) = candidate.identity.as_ref().filter(|_| {
        candidate.classification == OpenClIcdClassification::IdentityVerified
            && candidate.enabled == Some(true)
    }) else {
        return binding(
            candidate,
            OpenClIcdBindingStatus::UnverifiedMissingCandidateIdentity,
            None,
        );
    };
    if identity.authenticode == AuthenticodeEvidence::Embedded {
        return binding(
            candidate,
            OpenClIcdBindingStatus::UnverifiedNoCatalogEvidence,
            None,
        );
    }
    let Some(catalog_sha256) = identity.signing_catalog_sha256 else {
        return binding(
            candidate,
            OpenClIcdBindingStatus::ConflictMissingCatalogDigest,
            None,
        );
    };
    let matches = platforms
        .iter()
        .filter(|platform| {
            platform.backend == RuntimeBackend::Opencl
                && platform.driver_catalog_sha256 == catalog_sha256
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => binding(
            candidate,
            OpenClIcdBindingStatus::UnverifiedNoMatchingDriverPackage,
            None,
        ),
        [matched] if platform_evidence_complete => binding(
            candidate,
            OpenClIcdBindingStatus::Verified,
            Some((*matched).clone()),
        ),
        [_] => binding(
            candidate,
            OpenClIcdBindingStatus::UnverifiedIncompletePlatformEvidence,
            None,
        ),
        many if has_conflicting_driver_identity(many) => binding(
            candidate,
            OpenClIcdBindingStatus::ConflictDriverIdentity,
            None,
        ),
        _ => binding(
            candidate,
            OpenClIcdBindingStatus::AmbiguousMultipleDriverPackages,
            None,
        ),
    }
}

fn has_conflicting_driver_identity(matches: &[&GpuPlatformIdentity]) -> bool {
    matches.iter().enumerate().any(|(index, left)| {
        matches[index + 1..]
            .iter()
            .any(|right| left.adapter_luid == right.adapter_luid && *left != *right)
    })
}

fn binding(
    candidate: OpenClIcdCandidate,
    status: OpenClIcdBindingStatus,
    adapter: Option<GpuPlatformIdentity>,
) -> OpenClIcdAdapterBinding {
    OpenClIcdAdapterBinding {
        candidate,
        status,
        adapter,
    }
}

fn windows_basename(value: &str) -> Option<&str> {
    value
        .rsplit(['\\', '/'])
        .next()
        .filter(|basename| !basename.is_empty())
}

fn path_fingerprint(value: &str) -> String {
    hex(&Sha256::digest(value.to_ascii_lowercase().as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opencl_icd_collector::OpenClRegistryView;
    use crate::runtime_module_identity::{FileIdentity, PeMachine, RuntimeModuleIdentityEvidence};
    use std::path::PathBuf;

    fn candidate(
        authenticode: AuthenticodeEvidence,
        catalog: Option<[u8; 32]>,
    ) -> OpenClIcdCandidate {
        OpenClIcdCandidate {
            registry_view: OpenClRegistryView::Registry64,
            path: Some(PathBuf::from(r"C:\Vendor Private\OpenCL\vendor-opencl.dll")),
            enabled: Some(true),
            classification: OpenClIcdClassification::IdentityVerified,
            identity: Some(RuntimeModuleIdentityEvidence {
                canonical_path: PathBuf::from(r"C:\Vendor Private\OpenCL\vendor-opencl.dll"),
                size: 4096,
                sha256: [0x11; 32],
                pe_machine: PeMachine::Amd64,
                file_identity: FileIdentity {
                    volume_serial_number: 0x1234_5678,
                    file_index: 0x1234_5678_90ab_cdef,
                },
                authenticode,
                signing_catalog_sha256: catalog,
            }),
        }
    }

    fn platform(luid: u64, catalog: u8) -> GpuPlatformIdentity {
        GpuPlatformIdentity {
            backend: RuntimeBackend::Opencl,
            adapter_luid: luid,
            pci_vendor_id: 0x10de,
            pci_device_id: 0x2684,
            pci_subsystem_id: 0x1458_4100,
            pci_revision_id: 0xa1,
            driver_inf: "oem42.inf".into(),
            driver_catalog_sha256: [catalog; 32],
            driver_version: "32.0.15.9999".into(),
            os_build: 26100,
        }
    }

    fn bind(
        candidate: OpenClIcdCandidate,
        platforms: Vec<GpuPlatformIdentity>,
    ) -> OpenClIcdAdapterBinding {
        bind_opencl_icd_candidates(vec![candidate], vec![], platforms, vec![])
            .bindings
            .remove(0)
    }

    #[test]
    fn exact_catalog_match_is_the_only_verified_result() {
        let result = bind(
            candidate(AuthenticodeEvidence::Catalog, Some([0xaa; 32])),
            vec![platform(1, 0xbb), platform(2, 0xaa)],
        );
        assert_eq!(result.status, OpenClIcdBindingStatus::Verified);
        assert_eq!(result.adapter.unwrap().adapter_luid, 2);
    }

    #[test]
    fn zero_and_shared_package_matches_fail_closed() {
        let zero = bind(
            candidate(AuthenticodeEvidence::Catalog, Some([0xaa; 32])),
            vec![platform(1, 0xbb)],
        );
        assert_eq!(
            zero.status,
            OpenClIcdBindingStatus::UnverifiedNoMatchingDriverPackage
        );
        assert!(zero.adapter.is_none());

        let multiple = bind(
            candidate(AuthenticodeEvidence::Catalog, Some([0xaa; 32])),
            vec![platform(1, 0xaa), platform(2, 0xaa)],
        );
        assert_eq!(
            multiple.status,
            OpenClIcdBindingStatus::AmbiguousMultipleDriverPackages
        );
        assert!(multiple.adapter.is_none());
    }

    #[test]
    fn missing_incomplete_embedded_and_conflicting_evidence_fail_closed() {
        let missing = OpenClIcdCandidate {
            identity: None,
            classification: OpenClIcdClassification::MissingDll,
            ..candidate(AuthenticodeEvidence::Embedded, None)
        };
        assert_eq!(
            bind(missing, vec![]).status,
            OpenClIcdBindingStatus::UnverifiedMissingCandidateIdentity
        );
        assert_eq!(
            bind(
                candidate(AuthenticodeEvidence::Embedded, None),
                vec![platform(1, 0xaa)]
            )
            .status,
            OpenClIcdBindingStatus::UnverifiedNoCatalogEvidence
        );
        assert_eq!(
            bind(
                candidate(AuthenticodeEvidence::Catalog, None),
                vec![platform(1, 0xaa)]
            )
            .status,
            OpenClIcdBindingStatus::ConflictMissingCatalogDigest
        );

        let incomplete = bind_opencl_icd_candidates(
            vec![candidate(AuthenticodeEvidence::Catalog, Some([0xaa; 32]))],
            vec![],
            vec![platform(1, 0xaa)],
            vec![OpenClPlatformDiagnostic {
                adapter_luid: Some("0000000000000002".into()),
                classification: OpenClPlatformDiagnosticKind::DriverIdentityCollectionFailed,
            }],
        )
        .bindings
        .remove(0);
        assert_eq!(
            incomplete.status,
            OpenClIcdBindingStatus::UnverifiedIncompletePlatformEvidence
        );
        assert!(incomplete.adapter.is_none());

        let mut conflict = platform(1, 0xaa);
        conflict.driver_version = "32.0.15.0001".into();
        assert_eq!(
            bind(
                candidate(AuthenticodeEvidence::Catalog, Some([0xaa; 32])),
                vec![platform(1, 0xaa), conflict]
            )
            .status,
            OpenClIcdBindingStatus::ConflictDriverIdentity
        );
    }

    #[test]
    fn report_is_privacy_bounded_and_never_claims_backend_readiness() {
        let collection = bind_opencl_icd_candidates(
            vec![candidate(AuthenticodeEvidence::Catalog, Some([0xaa; 32]))],
            vec![],
            vec![platform(2, 0xaa)],
            vec![],
        );
        let report = privacy_bounded_opencl_icd_binding_report(&collection);
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains(r"C:\Vendor Private"));
        assert_eq!(report["bindings"][0]["status"], "verified");
        assert_eq!(
            report["bindings"][0]["authoritative_evidence"],
            "catalog_sha256_exact_match"
        );
        assert_eq!(report["bindings"][0]["backend_ready"], false);
        assert_eq!(
            report["bindings"][0]["candidate_path_basename"],
            "vendor-opencl.dll"
        );
    }
}
