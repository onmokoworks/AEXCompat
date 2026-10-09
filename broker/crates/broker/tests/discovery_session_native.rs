//! Native discovery-session coverage that needs the real worker and a probe
//! AEX built from this checkout. It lives in its own target so the native
//! Rust partition (tools/run-broker-rust-tests.py) runs it after the worker
//! and probe artifacts exist, where a skip fails the run instead of passing.

#[cfg(test)]
#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::render_session::{
        DiscoverySession, InPlaceDiscoverySessionOpenRequest, InspectOutcome,
    };
    use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
    use aexcompat_broker::secure_launch::LaunchEnvironment;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    struct TempRepository(PathBuf);
    impl Drop for TempRepository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An old minihost source marker, so the copied worker is never judged
    /// older than its sources by the launch freshness check.
    fn write_freshness_source_marker(root: &Path) {
        let marker = root.join("minihost/src/session_fixture.cpp");
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"fixture source").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&marker)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
            )
            .unwrap();
    }

    fn approved_artifact(path: &Path) -> ApprovedImageArtifact {
        let bytes = std::fs::read(path).unwrap();
        ApprovedImageArtifact {
            path: path.to_path_buf(),
            expected_sha256: Sha256::digest(&bytes).into(),
            expected_size: bytes.len() as u64,
        }
    }

    /// Every column of one discovery session offers the layer-scoped suite
    /// catalog (issue #1764): the host catalog probe registers a BUTTON only
    /// when PF Path Data Suite v1 is offered at GLOBAL_SETUP, so a column that
    /// lost the catalog after a swap to another member, a swap back, or a
    /// re-inspect of the same member would report a CHECKBOX. The probe makes
    /// no masks, so this does not show that the per-column scene reset clears
    /// what an earlier member left behind.
    #[test]
    fn discovery_session_columns_share_the_layer_suite_catalog() {
        let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let worker = source_root.join("target/minihost-build/aex_worker.exe");
        let probe = source_root.join(
            "target/pf-host-catalog-param-probe-build/Release/pf_host_catalog_param_probe.aex",
        );
        if !worker.is_file() || !probe.is_file() {
            eprintln!(
                "skipping discovery catalog test: Release worker or host catalog probe missing"
            );
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "aexcompat-discovery-catalog-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        write_freshness_source_marker(&root);
        let worker_dir = root.join("target/minihost-build");
        std::fs::create_dir_all(&worker_dir).unwrap();
        std::fs::copy(&worker, worker_dir.join("aex_worker.exe")).unwrap();
        let mut plugins = Vec::new();
        for name in ["first", "second"] {
            let path = root.join(format!("{name}.aex"));
            std::fs::copy(&probe, &path).unwrap();
            plugins.push(approved_artifact(&path));
        }
        let repository = TempRepository(root);
        let mut session = DiscoverySession::open_in_place(InPlaceDiscoverySessionOpenRequest {
            repository: &repository.0,
            plugins,
            dependency_search_dirs: vec![repository.0.clone()],
            module_bound: 64,
            inspect_deadline: Some(Duration::from_secs(30)),
            launch_environment: LaunchEnvironment::default(),
        })
        .expect("open native discovery session");
        // first, second (swap), first again (swap back), first (re-inspect).
        for (request, plugin) in [0u32, 1, 0, 0].into_iter().enumerate() {
            let outcome = session.inspect_plugin(plugin, request as u32).unwrap();
            let InspectOutcome::Inspected { report } = outcome else {
                panic!("request {request} did not inspect: {outcome:?}");
            };
            let types: Vec<_> = report["parameters"]
                .as_array()
                .expect("inspect report lists parameters")
                .iter()
                .map(|parameter| parameter["type"].as_i64())
                .collect();
            // PF_Param_BUTTON
            assert_eq!(types, [Some(15)], "request {request}: {report}");
        }
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["inspects_ok"], 4, "close: {close}");
    }
}
