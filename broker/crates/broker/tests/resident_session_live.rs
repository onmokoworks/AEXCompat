//! End-to-end coverage for the resident interactive render session
//! (issue #107): the harness's live-render adapter drives the REAL render
//! worker with per-frame parameter updates (protocol v:2) and every frame's
//! output must reflect that frame's values. Requires the real worker and the
//! pf_parameter_echo_probe fixture from this checkout; skips (with a message)
//! when either is not built.

#[cfg(test)]
#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{
        InteractiveParameter, InteractiveRenderSession, InteractiveSessionOpen, RenderGpuBackend,
        RenderPixelFormat,
    };
    use aexcompat_broker::render_session::{
        ClusterRenderPlugins, FrameStatus, RenderSession, SessionLayer, SessionOpenRequest,
        SwapOutcome,
    };
    use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root")
    }

    fn echo_parameter(value: f64) -> InteractiveParameter {
        serde_json::from_value(serde_json::json!({
            "slot": 1, "name": "Echo", "kind": "float",
            "minimum": 0.0, "maximum": 255.0, "value": value,
            "choices": [], "color": [0, 0, 0, 0], "components": [0.0, 0.0, 0.0],
            "component_count": 0, "layer_path": null,
            "enabled": true, "visible": true, "supervised": false,
        }))
        .expect("echo parameter fixture")
    }

    fn built_artifacts() -> Option<(PathBuf, PathBuf, String)> {
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_worker.exe");
        let aex =
            root.join("target/pf-parameter-echo-probe-build/Release/pf_parameter_echo_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping resident-session live test: build aex_worker.exe and \
                 pf_parameter_echo_probe.aex first"
            );
            return None;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        Some((root, aex, sha))
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-resident-live-{tag}-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        scratch
    }

    fn approved_artifact(path: &Path) -> ApprovedImageArtifact {
        let bytes = std::fs::read(path).unwrap();
        ApprovedImageArtifact {
            path: path.to_path_buf(),
            expected_sha256: Sha256::digest(&bytes).into(),
            expected_size: bytes.len() as u64,
        }
    }

    fn first_built_artifact(root: &Path, candidates: &[&str]) -> PathBuf {
        candidates
            .iter()
            .map(|candidate| root.join(candidate))
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| root.join(candidates[0]))
    }

    fn de_verbatim(path: &Path) -> PathBuf {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    }

    #[test]
    fn smart_cluster_swaps_real_effects_and_restores_the_first_effect() {
        let root = de_verbatim(&repository_root());
        let worker = root.join("target/minihost-build/aex_worker.exe");
        let geometry = first_built_artifact(
            &root,
            &[
                "target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex",
                "target/pf-smart-geometry-probe-build/pf_smart_geometry_probe.aex",
            ],
        );
        let passthrough = first_built_artifact(
            &root,
            &[
                "target/pf-smart-param-time-probe-build/Release/pf_smart_param_time_probe.aex",
                "target/pf-smart-param-time-probe-build/pf_smart_param_time_probe.aex",
            ],
        );
        let setup_error = first_built_artifact(
            &root,
            &[
                "target/pf-smart-param-time-probe-build/Release/pf_smart_global_setup_error_probe.aex",
                "target/pf-smart-param-time-probe-build/pf_smart_global_setup_error_probe.aex",
            ],
        );
        if !worker.is_file()
            || !geometry.is_file()
            || !passthrough.is_file()
            || !setup_error.is_file()
        {
            eprintln!(
                "skipping real SmartFX cluster test: build aex_worker.exe, the geometry probe, and the smart-param-time probe first"
            );
            return;
        }

        let plugins = vec![
            approved_artifact(&geometry),
            approved_artifact(&passthrough),
            approved_artifact(&setup_error),
        ];
        let geometry_sha = format!("{:x}", Sha256::digest(std::fs::read(&geometry).unwrap()));
        let mut session = RenderSession::open_cluster(
            SessionOpenRequest {
                repository: &root,
                plugin_path: &geometry,
                plugin_sha256: &geometry_sha,
                parameters: None,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                render_environment_trailer: None,
                audio_trailer: None,
                alpha_as_coverage_params: &[],
                conformance_render_settings: None,
                layers: &[],
                dependencies: Vec::new(),
                companions: Vec::new(),
                dependency_search_dirs: vec![
                    geometry.parent().unwrap().to_path_buf(),
                    passthrough.parent().unwrap().to_path_buf(),
                    setup_error.parent().unwrap().to_path_buf(),
                ],
                width: 64,
                height: 48,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
                frame_deadline: std::time::Duration::from_secs(30),
                smart: true,
                gpu_backend: RenderGpuBackend::Cpu,
                gpu_runtime_policy: None,
                payload_override: None,
                launch_environment: Default::default(),
            },
            ClusterRenderPlugins {
                swap_payloads: vec![None; plugins.len()],
                plugins,
                module_bound: 64,
            },
        )
        .expect("open real SmartFX cluster");

        let input: Vec<u8> = (0..64 * 48 * 4).map(|index| (index % 251) as u8).collect();
        let render = |session: &mut RenderSession, frame_index| {
            let frame = session
                .render_frame(frame_index, 0, &input)
                .expect("cluster frame transport");
            match frame.status {
                FrameStatus::Rendered { pixels, .. } => pixels,
                status => panic!("cluster frame did not render: {status:?}"),
            }
        };

        let first = render(&mut session, 0);
        let to_second = session.swap_plugin(1);
        assert!(
            matches!(to_second, Ok(SwapOutcome::Swapped)),
            "swap to effect B failed: {to_second:?}"
        );
        let second = render(&mut session, 1);
        assert_ne!(
            first, second,
            "two different effects produced the same frame"
        );
        let to_first = session.swap_plugin(0);
        assert!(
            matches!(to_first, Ok(SwapOutcome::Swapped)),
            "swap back to effect A failed: {to_first:?}"
        );
        let restored = render(&mut session, 2);
        assert_eq!(first, restored, "A -> B -> A did not restore effect A");

        let failed_setup = session
            .swap_plugin(2)
            .expect("a plug-in-local setup failure still completes the exchange");
        let SwapOutcome::PluginError { global_setup_error } = failed_setup else {
            panic!("GLOBAL_SETUP failure was not propagated: {failed_setup:?}");
        };
        assert_eq!(global_setup_error, 512);

        let close = session.close();
        assert_eq!(close["invalidated"], false, "close: {close}");
        assert_eq!(close["frames_ok"], 3, "close: {close}");
        assert_eq!(close["session_clean"], false, "close: {close}");
        assert_eq!(
            close["worker"]["diagnostics"]["first_failure_stage"], "global_setup",
            "the plug-in-local swap failure must remain visible at close: {close}"
        );
    }

    #[test]
    fn smart_cluster_swap_preserves_dynamic_secondary_layer_pixels() {
        let root = de_verbatim(&repository_root());
        let worker = root.join("target/minihost-build/aex_worker.exe");
        let fixture = root.join(
            "target/pf-smart-timed-multilayer-probe-build/Release/pf_smart_timed_multilayer_probe.aex",
        );
        if !worker.is_file() || !fixture.is_file() {
            eprintln!(
                "skipping clustered layer pixels: build aex_worker.exe and pf_smart_timed_multilayer_probe.aex first"
            );
            return;
        }

        let scratch = scratch_dir("cluster-layers");
        let first = scratch.join("first.aex");
        let second = scratch.join("second.aex");
        std::fs::copy(&fixture, &first).unwrap();
        std::fs::copy(&fixture, &second).unwrap();
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&first).unwrap()));
        let (width, height) = (16u32, 8u32);
        let solid = |rgba: [u8; 4]| {
            std::iter::repeat_n(rgba, (width * height) as usize)
                .flatten()
                .collect::<Vec<_>>()
        };
        let initial_layer = solid([10, 20, 30, 255]);
        let updated_layer = solid([50, 60, 70, 255]);
        let layers = vec![SessionLayer {
            slot: 1,
            width,
            height,
            rgba: initial_layer.clone(),
            timed: None,
            dynamic: true,
        }];
        let input = solid([1, 2, 3, 255]);
        fn request<'a>(
            root: &'a Path,
            plugin_path: &'a Path,
            sha: &'a str,
            layers: &'a [SessionLayer],
            scratch: &Path,
            width: u32,
            height: u32,
        ) -> SessionOpenRequest<'a> {
            SessionOpenRequest {
                repository: root,
                plugin_path,
                plugin_sha256: sha,
                parameters: None,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                render_environment_trailer: None,
                audio_trailer: None,
                alpha_as_coverage_params: &[],
                conformance_render_settings: None,
                layers,
                dependencies: Vec::new(),
                companions: Vec::new(),
                dependency_search_dirs: vec![scratch.to_path_buf()],
                width,
                height,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
                frame_deadline: std::time::Duration::from_secs(30),
                smart: true,
                gpu_backend: RenderGpuBackend::Cpu,
                gpu_runtime_policy: None,
                payload_override: None,
                launch_environment: Default::default(),
            }
        }
        let pixels = |session: &mut RenderSession, frame_index| match session
            .render_frame(frame_index, 0, &input)
            .unwrap()
            .status
        {
            FrameStatus::Rendered { pixels, .. } => pixels,
            status => panic!("layer-dependent frame did not render: {status:?}"),
        };

        let mut cluster = RenderSession::open_cluster(
            request(&root, &first, &sha, &layers, &scratch, width, height),
            ClusterRenderPlugins {
                plugins: vec![approved_artifact(&first), approved_artifact(&second)],
                swap_payloads: vec![None, None],
                module_bound: 64,
            },
        )
        .expect("open layer-dependent cluster");
        let first_pixels = pixels(&mut cluster, 0);
        assert_eq!(
            first_pixels, initial_layer,
            "effect A did not read its dynamic layer"
        );
        cluster
            .update_dynamic_layer(1, &updated_layer)
            .expect("update the shipping dynamic layer before swap");
        assert!(matches!(cluster.swap_plugin(1), Ok(SwapOutcome::Swapped)));
        let clustered = pixels(&mut cluster, 1);
        let cluster_close = cluster.close();
        assert_eq!(cluster_close["session_clean"], true, "{cluster_close}");

        let fresh_layers = vec![SessionLayer {
            slot: 1,
            width,
            height,
            rgba: updated_layer.clone(),
            timed: None,
            dynamic: true,
        }];
        let mut fresh = RenderSession::open(request(
            &root,
            &second,
            &sha,
            &fresh_layers,
            &scratch,
            width,
            height,
        ))
        .expect("open fresh layer session");
        let individual = pixels(&mut fresh, 0);
        let fresh_close = fresh.close();
        assert_eq!(fresh_close["session_clean"], true, "{fresh_close}");
        assert_eq!(clustered, individual, "swap changed secondary-layer pixels");
        assert_eq!(
            clustered, updated_layer,
            "effect B did not read the updated dynamic layer"
        );

        std::fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn per_frame_parameter_updates_change_the_real_render() {
        let Some((root, aex, sha)) = built_artifacts() else {
            return;
        };
        let scratch = scratch_dir("echo");
        let (width, height) = (64u32, 32u32);
        let rgba = vec![0u8; (width * height * 4) as usize];
        let declared = [echo_parameter(0.0)];
        let mut session = InteractiveRenderSession::open(InteractiveSessionOpen {
            repository: &root,
            plugin_id: "experimental",
            plugin_path: &aex,
            plugin_sha256: &sha,
            parameters: Some(&declared),
            selection: aexcompat_broker::image_render::InteractiveSessionSelection::new(
                aexcompat_broker::image_render::InteractiveRenderPath::Classic,
                aexcompat_broker::image_render::InteractiveCapabilitySource::AdvertisedClassic,
                1,
                0,
            )
            .expect("valid classic selection"),
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width,
            height,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            timeout_ms: 30_000,
        })
        .expect("open resident session against the real worker");

        // The echo probe fills every pixel with (value, 255-value, 128, 255):
        // each live update must land in exactly that frame's output.
        for (index, value) in [(0i32, 32u8), (1, 200), (2, 91)] {
            let update = [echo_parameter(f64::from(value))];
            let output = scratch.join(format!("frame-{index}.png"));
            let report = session
                .render(&rgba, index, Some(&update), &output)
                .expect("live frame renders");
            assert_eq!(report["passed"], true, "report: {report}");
            let decoded = image::open(&output).expect("output PNG decodes").to_rgba8();
            assert_eq!(
                decoded.get_pixel(0, 0).0,
                [value, 255 - value, 128, 255],
                "frame {index} must carry the value {value}"
            );
        }
        let close = session.close();
        assert_eq!(close["frames_ok"], 3);
        assert_eq!(close["parameter_update_frames"], 3);
        assert_eq!(close["session_clean"], true, "close: {close}");
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Manual latency comparison for docs/RENDER_SESSION_INVESTIGATION notes
    /// (`cargo test ... resident_session_latency -- --ignored --nocapture`).
    /// Reference values only, machine dependent, never frozen evidence.
    #[test]
    #[ignore = "manual latency measurement; prints medians"]
    fn resident_session_latency_versus_one_shot() {
        let Some((root, aex, sha)) = built_artifacts() else {
            return;
        };
        let scratch = scratch_dir("latency");
        let (width, height) = (1920u32, 1080u32);
        let input = scratch.join("input.png");
        image::RgbaImage::from_pixel(width, height, image::Rgba([8, 16, 32, 255]))
            .save(&input)
            .unwrap();
        let rgba = vec![0u8; (width * height * 4) as usize];
        let runs = 12usize;

        let median = |mut samples: Vec<f64>| {
            samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
            samples[samples.len() / 2]
        };

        // One-shot per parameter change: the pre-#107 GUI behavior (each
        // render pays worker start + AEX load + lifecycle + staging).
        let mut one_shot = Vec::new();
        for index in 0..runs {
            let parameters = [echo_parameter((index % 200) as f64)];
            let output = scratch.join(format!("oneshot-{index}.png"));
            let started = std::time::Instant::now();
            aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                &root,
                &aex,
                &sha,
                &input,
                &output,
                &parameters,
                aexcompat_broker::image_render::RenderTiming {
                    current_time: 0,
                    time_step: 1,
                    total_time: 300,
                    time_scale: 30,
                },
                false,
                RenderPixelFormat::Argb8,
                None,
                None,
                aexcompat_broker::image_render::RenderGpuBackend::Auto,
                Vec::new(),
            )
            .expect("one-shot render");
            one_shot.push(started.elapsed().as_secs_f64() * 1000.0);
        }

        // Resident session: open once, per-frame parameter updates.
        let declared = [echo_parameter(0.0)];
        let open_started = std::time::Instant::now();
        let mut session = InteractiveRenderSession::open(InteractiveSessionOpen {
            repository: &root,
            plugin_id: "experimental",
            plugin_path: &aex,
            plugin_sha256: &sha,
            parameters: Some(&declared),
            selection: aexcompat_broker::image_render::InteractiveSessionSelection::new(
                aexcompat_broker::image_render::InteractiveRenderPath::Classic,
                aexcompat_broker::image_render::InteractiveCapabilitySource::AdvertisedClassic,
                1,
                0,
            )
            .expect("valid classic selection"),
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width,
            height,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            timeout_ms: 30_000,
        })
        .expect("open resident session");
        let open_ms = open_started.elapsed().as_secs_f64() * 1000.0;
        let mut resident = Vec::new();
        let mut worker = Vec::new();
        for index in 0..runs {
            let update = [echo_parameter((index % 200) as f64)];
            let output = scratch.join(format!("resident-{index}.png"));
            let started = std::time::Instant::now();
            let report = session
                .render(&rgba, index as i32, Some(&update), &output)
                .expect("resident render");
            resident.push(started.elapsed().as_secs_f64() * 1000.0);
            worker.push(report["resident_session"]["render_ms"].as_u64().unwrap() as f64);
        }
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");

        println!(
            "one_shot_ms median={:.1} min={:.1} max={:.1}",
            median(one_shot.clone()),
            one_shot.iter().cloned().fold(f64::INFINITY, f64::min),
            one_shot.iter().cloned().fold(0.0, f64::max),
        );
        println!("resident_open_ms={open_ms:.1}");
        println!(
            "resident_frame_ms median={:.1} min={:.1} max={:.1}",
            median(resident.clone()),
            resident.iter().cloned().fold(f64::INFINITY, f64::min),
            resident.iter().cloned().fold(0.0, f64::max),
        );
        println!(
            "resident_worker_ms median={:.1} min={:.1} max={:.1}",
            median(worker.clone()),
            worker.iter().cloned().fold(f64::INFINITY, f64::min),
            worker.iter().cloned().fold(0.0, f64::max),
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
