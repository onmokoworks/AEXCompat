#[cfg(windows)]
mod windows {
    use aexcompat_broker::opencl_runtime_probe::{ProbeLaunchStatus, launch_system_opencl_probe};
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn restricted_worker_probe_is_bounded_and_never_claims_readiness() {
        let worker = Path::new(env!("CARGO_BIN_EXE_opencl-runtime-probe-worker"));
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("broker workspace repository");
        let report = launch_system_opencl_probe(worker, repository, Duration::from_secs(30))
            .expect("secure OpenCL worker launch");
        let serialized = serde_json::to_string(&report).expect("serialize report");
        assert!(!report.backend_ready);
        assert!(!serialized.contains(":\\"));
        assert!(!serialized.contains("candidate_path"));
        assert_eq!(
            report.candidate_evidence.individual_icd_binding,
            "not_attempted"
        );
        if report.launch.status == ProbeLaunchStatus::Observed {
            let aggregate = report
                .aggregate_loader_observation
                .as_ref()
                .expect("observed worker returns aggregate");
            assert_eq!(report.compute_ready, aggregate.compute_ready);
            assert_eq!(
                aggregate.compute_ready,
                aggregate.platforms.iter().all(|platform| {
                    platform.devices.iter().all(|device| {
                        device.compute.stage
                            == aexcompat_broker::opencl_runtime_probe::ComputeStage::Passed
                    })
                })
            );
        } else {
            assert!(report.aggregate_loader_observation.is_none());
            assert!(!report.compute_ready);
        }
    }
}
