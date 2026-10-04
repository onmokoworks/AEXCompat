//! Connect real report producers to their shareable contracts without hardware.
use aexcompat_broker::{
    cuda_compute_probe, opencl_icd_adapter_binding as binding, opencl_icd_collector as icd,
    opencl_runtime_probe, pnp_opencl_runtime_collector as pnp,
    runtime_module_identity::{
        AuthenticodeEvidence, FileIdentity, PeMachine, RuntimeModuleIdentityEvidence,
    },
    runtime_module_policy::{GpuPlatformIdentity, RuntimeBackend},
};
use serde_json::Value;
use std::{fs, path::Path};

fn validator(name: &str) -> jsonschema::Validator {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../schemas");
    let load = |name: &str| -> Value {
        serde_json::from_slice(&fs::read(directory.join(name)).expect("schema input"))
            .expect("valid schema JSON")
    };
    // The binding schema references the ICD schema by its published ID. Supply
    // that resource locally; enabling HTTP/file retrieval would hide missing
    // contract inputs behind the environment or a network lookup.
    let icd_schema = load("opencl-icd-collector.schema.json");
    let registry = jsonschema::Registry::new()
        .add(icd_schema["$id"].as_str().unwrap(), icd_schema.clone())
        .unwrap()
        .prepare()
        .unwrap();
    jsonschema::options()
        .with_registry(&registry)
        .build(&load(name))
        .expect("valid, locally resolvable schema")
}

fn validate_produced_report(name: &str, report: Value) {
    // Exercise the actual wire form, not only the in-memory representation.
    let bytes = serde_json::to_vec(&report).expect("serialize producer report");
    let wire: Value = serde_json::from_slice(&bytes).expect("parse wire report");
    let validator = validator(name);
    let errors: Vec<_> = validator
        .iter_errors(&wire)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "{name}: {errors:?}");

    let mut unexpected = wire.clone();
    unexpected["producer_drift"] = Value::Bool(true);
    assert!(
        !validator.is_valid(&unexpected),
        "{name}: unknown field accepted"
    );
    let mut missing = wire.clone();
    missing.as_object_mut().unwrap().remove("schema_version");
    assert!(
        !validator.is_valid(&missing),
        "{name}: missing version accepted"
    );
    let mut wrong_type = wire;
    wrong_type["schema_version"] = Value::String("1".into());
    assert!(
        !validator.is_valid(&wrong_type),
        "{name}: wrong version type accepted"
    );
}

#[test]
fn runtime_producer_observations_cover_nested_device_metadata() {
    use opencl_runtime_probe as cl;
    let opencl = cl::SystemOpenClProbeReport {
        schema_version: 2,
        contract: "system_opencl_loader_probe",
        candidate_evidence: cl::CandidateEvidenceBoundary::default(),
        aggregate_loader_observation: Some(cl::AggregateLoaderObservation {
            status: cl::AggregateStatus::Observed,
            platforms: vec![cl::PlatformObservation {
                name: "fixture platform".into(),
                vendor: "fixture vendor".into(),
                version: "OpenCL 3.0".into(),
                profile: "FULL_PROFILE".into(),
                extensions: vec!["cl_khr_icd".into()],
                icd_suffix: "FIXTURE".into(),
                devices: vec![cl::DeviceObservation {
                    device_type: 4,
                    vendor_id: 1,
                    name: "fixture device".into(),
                    vendor: "fixture vendor".into(),
                    driver_version: "1.0".into(),
                    version: "OpenCL 3.0".into(),
                    profile: "FULL_PROFILE".into(),
                    extensions: vec![],
                    available: true,
                    compiler_available: true,
                    max_compute_units: 1,
                    max_clock_frequency_mhz: 1,
                    max_work_group_size: 64,
                    max_work_item_sizes: vec![64, 64, 64],
                    max_mem_alloc_bytes: 1024,
                    global_mem_bytes: 4096,
                    local_mem_bytes: 128,
                    compute: cl::ComputeDeviceObservation::passed(cl::QueueApi::Legacy),
                }],
            }],
            diagnostics: vec![],
            compute_ready: true,
        }),
        launch: cl::ProbeLaunchObservation {
            status: cl::ProbeLaunchStatus::Observed,
            exit_code: Some(0),
            kill_reason: None,
            stdout_truncated: false,
            stderr_truncated: false,
            memory_limit_reached: false,
        },
        compute_ready: true,
        backend_ready: false,
    };
    let opencl_wire = serde_json::to_value(opencl).unwrap();
    validate_produced_report("opencl-runtime-probe.schema.json", opencl_wire.clone());
    let mut private_path = opencl_wire.clone();
    private_path["aggregate_loader_observation"]["platforms"][0]["devices"][0]["name"] =
        Value::String(r"C:\private-driver-package\device".into());
    assert!(!validator("opencl-runtime-probe.schema.json").is_valid(&private_path));
    let mut unknown_device_field = opencl_wire;
    unknown_device_field["aggregate_loader_observation"]["platforms"][0]["devices"][0]["drift"] =
        Value::Bool(true);
    assert!(!validator("opencl-runtime-probe.schema.json").is_valid(&unknown_device_field));

    use cuda_compute_probe as cu;
    let aggregate = cu::CudaAggregateObservation {
        status: cu::CudaAggregateStatus::Observed,
        driver_version: Some(12000),
        devices: vec![cu::CudaDeviceObservation::passed(
            0,
            "fixture device".into(),
            "ab".repeat(32),
            cu::PciLocation {
                domain: 0,
                bus: 1,
                device: 0,
            },
            8,
            0,
            4096,
        )],
        diagnostics: vec![],
        cuda_compute_ready: true,
    };
    let cuda = cu::report_from_worker_json(
        cu::ProbeLaunchObservation {
            status: cu::ProbeLaunchStatus::Observed,
            exit_code: Some(0),
            kill_reason: None,
            stdout_truncated: false,
            stderr_truncated: false,
            memory_limit_reached: false,
        },
        &serde_json::to_string(&aggregate).unwrap(),
    );
    assert_eq!(cuda.launch.status, cu::ProbeLaunchStatus::Observed);
    let cuda_wire = serde_json::to_value(cuda).unwrap();
    validate_produced_report("cuda-compute-probe.schema.json", cuda_wire.clone());
    let mut private_path = cuda_wire.clone();
    private_path["aggregate_driver_observation"]["devices"][0]["name"] =
        Value::String(r"C:\private-driver-package\device".into());
    assert!(!validator("cuda-compute-probe.schema.json").is_valid(&private_path));
    let mut unknown_device_field = cuda_wire;
    unknown_device_field["aggregate_driver_observation"]["devices"][0]["drift"] = Value::Bool(true);
    assert!(!validator("cuda-compute-probe.schema.json").is_valid(&unknown_device_field));
}

#[test]
fn runtime_producer_failures_match_schemas_without_gpu_workers() {
    validate_produced_report(
        "opencl-runtime-probe.schema.json",
        serde_json::to_value(opencl_runtime_probe::launch_error_report()).unwrap(),
    );
    validate_produced_report(
        "cuda-compute-probe.schema.json",
        serde_json::to_value(cuda_compute_probe::launch_error_report()).unwrap(),
    );
}

fn candidate() -> icd::OpenClIcdCandidate {
    icd::OpenClIcdCandidate {
        registry_view: icd::OpenClRegistryView::Registry64,
        path: Some(r"C:\private-driver-package\fixture.dll".into()),
        enabled: Some(false),
        classification: icd::OpenClIcdClassification::Disabled,
        identity: None,
    }
}

#[test]
fn collector_producers_match_schemas_with_nonempty_fail_closed_records() {
    let icd_report = icd::privacy_bounded_opencl_icd_report(&icd::OpenClIcdCollection {
        candidates: vec![candidate()],
        diagnostics: vec![icd::OpenClRegistryDiagnostic {
            registry_view: icd::OpenClRegistryView::Registry32,
            classification: icd::OpenClRegistryDiagnosticKind::MissingRegistryKey,
        }],
    });
    validate_produced_report("opencl-icd-collector.schema.json", icd_report);
    let binding_report = binding::privacy_bounded_opencl_icd_binding_report(
        &binding::OpenClIcdAdapterBindingCollection {
            bindings: vec![binding::OpenClIcdAdapterBinding {
                candidate: candidate(),
                status: binding::OpenClIcdBindingStatus::UnverifiedMissingCandidateIdentity,
                adapter: None,
            }],
            registry_diagnostics: vec![],
            platform_diagnostics: vec![binding::OpenClPlatformDiagnostic {
                adapter_luid: None,
                classification: binding::OpenClPlatformDiagnosticKind::AdapterEnumerationFailed,
            }],
        },
    );
    validate_produced_report("opencl-icd-adapter-binding.schema.json", binding_report);
    let pnp_report = pnp::privacy_bounded_pnp_opencl_report(&pnp::PnpOpenClCollection {
        candidates: vec![pnp::PnpOpenClCandidate {
            source_class: pnp::PnpOpenClSourceClass::DisplayAdapter,
            architecture: pnp::PnpOpenClArchitecture::Native,
            loader_selected: true,
            path: Some(r"C:\private-driver-package\fixture.dll".into()),
            classification: pnp::PnpOpenClClassification::MissingDll,
            identity: None,
            status: pnp::PnpOpenClBindingStatus::UnverifiedCandidate,
            registration_adapter_luid: None,
            adapter: None,
            legacy_merge: pnp::LegacyMergeStatus::NotEligible,
        }],
        diagnostics: vec![pnp::PnpOpenClDiagnostic {
            source_class: None,
            adapter_luid: None,
            classification: pnp::PnpOpenClDiagnosticKind::AdapterEnumerationFailed,
        }],
    });
    validate_produced_report("pnp-opencl-runtime-collector.schema.json", pnp_report);
}

// Typed synthetic observations exercise serialization, not hardware identity
// collection. They are not recorded provenance from a real driver or module.
fn verified_candidate() -> icd::OpenClIcdCandidate {
    icd::OpenClIcdCandidate {
        enabled: Some(true),
        classification: icd::OpenClIcdClassification::IdentityVerified,
        identity: Some(RuntimeModuleIdentityEvidence {
            canonical_path: r"C:\private-driver-package\fixture.dll".into(),
            size: 4096,
            sha256: [0xab; 32],
            pe_machine: PeMachine::Amd64,
            file_identity: FileIdentity {
                volume_serial_number: 1,
                file_index: 2,
            },
            authenticode: AuthenticodeEvidence::Catalog,
            signing_catalog_sha256: Some([0xcd; 32]),
        }),
        ..candidate()
    }
}

fn adapter() -> GpuPlatformIdentity {
    GpuPlatformIdentity {
        backend: RuntimeBackend::Opencl,
        adapter_luid: 1,
        pci_vendor_id: 0x10de,
        pci_device_id: 1,
        pci_subsystem_id: 1,
        pci_revision_id: 1,
        driver_inf: "fixture.inf".into(),
        driver_catalog_sha256: [0xcd; 32],
        driver_version: "1.0.0.0".into(),
        os_build: 26100,
    }
}

#[test]
fn verified_collector_producers_cover_identity_adapter_and_catalog_branches() {
    let icd_report = icd::privacy_bounded_opencl_icd_report(&icd::OpenClIcdCollection {
        candidates: vec![verified_candidate()],
        diagnostics: vec![],
    });
    validate_produced_report("opencl-icd-collector.schema.json", icd_report.clone());
    let mut invalid_identity = icd_report;
    invalid_identity["candidates"][0]["identity"]["sha256"] = Value::String("not-a-digest".into());
    assert!(!validator("opencl-icd-collector.schema.json").is_valid(&invalid_identity));

    let binding_report = binding::privacy_bounded_opencl_icd_binding_report(
        &binding::OpenClIcdAdapterBindingCollection {
            bindings: vec![binding::OpenClIcdAdapterBinding {
                candidate: verified_candidate(),
                status: binding::OpenClIcdBindingStatus::Verified,
                adapter: Some(adapter()),
            }],
            registry_diagnostics: vec![],
            platform_diagnostics: vec![],
        },
    );
    validate_produced_report(
        "opencl-icd-adapter-binding.schema.json",
        binding_report.clone(),
    );
    let mut missing_adapter = binding_report;
    missing_adapter["bindings"][0]["adapter"] = Value::Null;
    assert!(!validator("opencl-icd-adapter-binding.schema.json").is_valid(&missing_adapter));

    let candidate = verified_candidate();
    let pnp_report = pnp::privacy_bounded_pnp_opencl_report(&pnp::PnpOpenClCollection {
        candidates: vec![pnp::PnpOpenClCandidate {
            source_class: pnp::PnpOpenClSourceClass::DisplayAdapter,
            architecture: pnp::PnpOpenClArchitecture::Native,
            loader_selected: true,
            path: candidate.path,
            classification: pnp::PnpOpenClClassification::IdentityVerified,
            identity: candidate.identity,
            status: pnp::PnpOpenClBindingStatus::Verified,
            registration_adapter_luid: Some(1),
            adapter: Some(adapter()),
            legacy_merge: pnp::LegacyMergeStatus::ExactMatch,
        }],
        diagnostics: vec![],
    });
    validate_produced_report(
        "pnp-opencl-runtime-collector.schema.json",
        pnp_report.clone(),
    );
    let mut missing_catalog = pnp_report;
    missing_catalog["candidates"][0]["identity"]["catalog_sha256"] = Value::Null;
    assert!(!validator("pnp-opencl-runtime-collector.schema.json").is_valid(&missing_catalog));
}
