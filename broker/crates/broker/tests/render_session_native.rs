//! Real-worker session regressions run in the native CI partition, after the
//! Release worker and self-built sampling/detach fixtures are available.
//! Missing artifacts fail these tests instead of silently returning success.

#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat};
    use aexcompat_broker::render_session::{
        ClusterRenderPlugins, DiscoverySession, InPlaceDiscoverySessionOpenRequest, InspectOutcome,
        RenderSession, SessionOpenRequest, SwapOutcome,
    };
    use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
    use aexcompat_broker::secure_launch::LaunchEnvironment;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    const WIDTH: u32 = 8;
    const HEIGHT: u32 = 4;

    struct TempRepository(PathBuf);
    impl Drop for TempRepository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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

    fn input_pattern(seed: u8) -> Vec<u8> {
        (0..WIDTH * HEIGHT * 4)
            .map(|index| seed.wrapping_add(index as u8))
            .collect()
    }

    #[test]
    fn native_cluster_swap_records_each_members_depth_and_final_flags() {
        let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let worker = source_root.join("target/minihost-build/aex_worker.exe");
        let probe =
            source_root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        assert!(
            worker.is_file() && probe.is_file(),
            "native cluster depth test requires the Release worker and sampling probe"
        );
        let root = std::env::temp_dir().join(format!(
            "aexcompat-native-cluster-depth-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        write_freshness_source_marker(&root);
        let worker_dir = root.join("target/minihost-build");
        std::fs::create_dir_all(&worker_dir).unwrap();
        std::fs::copy(&worker, worker_dir.join("aex_worker.exe")).unwrap();
        let mut plugins = Vec::new();
        for marker in ["shallow", "floatonly"] {
            let path = root.join(format!("sampling-{marker}.aex"));
            std::fs::copy(&probe, &path).unwrap();
            plugins.push((
                path.clone(),
                format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            ));
        }
        let repository = TempRepository(root);
        let mut session = RenderSession::open_cluster(
            SessionOpenRequest {
                companions: Vec::new(),
                repository: &repository.0,
                plugin_path: &plugins[0].0,
                plugin_sha256: &plugins[0].1,
                parameters: None,
                payload_override: None,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                camera_trailer: None,
                render_environment_trailer: None,
                audio_trailer: None,
                alpha_as_coverage_params: &[],
                conformance_render_settings: None,
                layers: &[],
                dependencies: Vec::new(),
                dependency_search_dirs: vec![repository.0.clone()],
                width: WIDTH,
                height: HEIGHT,
                pixel_format: RenderPixelFormat::Argb16,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
                frame_deadline: Duration::from_secs(30),
                smart: false,
                gpu_backend: RenderGpuBackend::Cpu,
                gpu_runtime_policy: None,
                launch_environment: LaunchEnvironment::default(),
            },
            ClusterRenderPlugins {
                plugins: plugins
                    .iter()
                    .map(|(path, _)| approved_artifact(path))
                    .collect(),
                swap_payloads: vec![None, None],
                module_bound: 64,
            },
        )
        .expect("open native cluster session");
        let first = session.render_frame(0, 0, &input_pattern(3)).unwrap();
        let first_depth = first.depth_provenance.unwrap();
        assert_eq!(first_depth.planned_dispatch_pixel_bytes, Some(4));
        assert_eq!(first_depth.dispatch_pixel_bytes, Some(4));
        assert!(!first_depth.advertised_depth_supported);
        assert!(matches!(
            session.swap_plugin(1).unwrap(),
            SwapOutcome::Swapped
        ));
        let second = session.render_frame(1, 1, &input_pattern(9)).unwrap();
        let second_depth = second.depth_provenance.unwrap();
        assert_eq!(second_depth.planned_dispatch_pixel_bytes, Some(16));
        assert_eq!(second_depth.dispatch_pixel_bytes, Some(16));
        assert!(!second_depth.advertised_depth_supported);
        let close = session.close();
        assert_eq!(close["session_clean"], true, "{close}");
        assert_eq!(
            close["final_report"]["advertised_out_flags"],
            second_depth.advertised_out_flags
        );
        assert_eq!(
            close["final_report"]["advertised_out_flags2"],
            second_depth.advertised_out_flags2
        );
        assert_eq!(close["final_report"]["dispatch_pixel_bytes"], 16);
    }

    #[test]
    fn native_discovery_close_skips_post_report_dll_detach_delay() {
        let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let worker = source_root.join("target/minihost-build/aex_worker.exe");
        let fixture = source_root.join("target/minihost-build/worker_session_detach_fixture.dll");
        assert!(
            worker.is_file() && fixture.is_file(),
            "native discovery close test requires the Release worker and detach fixture"
        );
        let root = std::env::temp_dir().join(format!(
            "aexcompat-discovery-close-{}-{:032x}",
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
            std::fs::copy(&fixture, &path).unwrap();
            plugins.push(approved_artifact(&path));
        }
        let repository = TempRepository(root);
        let mut session = DiscoverySession::open_in_place(InPlaceDiscoverySessionOpenRequest {
            repository: &repository.0,
            plugins,
            dependency_search_dirs: vec![repository.0.clone()],
            module_bound: 64,
            inspect_deadline: Some(Duration::from_secs(30)),
            launch_environment: LaunchEnvironment::default()
                .with_child_var("AEXCOMPAT_DETACH_DELAY_MS", "5000"),
        })
        .expect("open native discovery session");
        for index in 0..2 {
            let outcome = session.inspect_plugin(index, index).unwrap();
            assert!(
                matches!(outcome, InspectOutcome::InspectError { ref error_kind, .. } if error_kind == "entrypoint_unresolved"),
                "fixture has no EffectMain: {outcome:?}"
            );
        }
        let started = Instant::now();
        let close = session.close();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "close stalled: {close}"
        );
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["inspects_errored"], 2, "close: {close}");
        assert_eq!(close["worker"]["classification"], "ok", "close: {close}");
        assert_eq!(
            close["final_report"]["status"], "discovery_session_completed",
            "close: {close}"
        );
    }
}
