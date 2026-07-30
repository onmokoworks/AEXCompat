mod common;

#[cfg(windows)]
mod windows {
    use aexcompat_broker::cuda_compute_probe::{
        launch_system_cuda_compute_probe, CudaAggregateStatus, CudaStage, ProbeLaunchStatus,
    };
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn restricted_cuda_worker_is_bounded_fail_closed_and_cleans_up() {
        if crate::common::skip_without_restricted_token_launch(
            "restricted_cuda_worker_is_bounded_fail_closed_and_cleans_up",
        ) {
            return;
        }
        let worker = Path::new(env!("CARGO_BIN_EXE_cuda-compute-probe-worker"));
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("broker workspace repository");
        let report = launch_system_cuda_compute_probe(worker, repository, Duration::from_secs(30))
            .expect("secure CUDA worker launch");
        let serialized = serde_json::to_string(&report).expect("serialize report");
        assert!(!report.backend_ready);
        assert!(!serialized.contains(r":\"));
        assert!(!serialized.contains("raw_log"));
        if report.launch.status == ProbeLaunchStatus::Observed {
            let aggregate = report
                .aggregate_driver_observation
                .as_ref()
                .expect("observed worker returns aggregate");
            assert_eq!(report.cuda_compute_ready, aggregate.cuda_compute_ready);
            assert_eq!(
                aggregate.cuda_compute_ready,
                aggregate.status == CudaAggregateStatus::Observed
                    && aggregate
                        .devices
                        .iter()
                        .all(|device| device.stage == CudaStage::Passed)
            );
            assert!(aggregate.validate());
        } else {
            assert!(report.aggregate_driver_observation.is_none());
            assert!(!report.cuda_compute_ready);
        }
    }
}
