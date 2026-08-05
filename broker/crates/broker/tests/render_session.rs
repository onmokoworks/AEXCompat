//! Broker `RenderSession` integration tests (issue #98 stage 1 PR-C).
//!
//! Every test drives a real isolated process (restricted token, sealed tree,
//! kill-on-close Job Object) through the session transport. The worker side is
//! the `session_protocol_worker` fixture, which speaks
//! docs/RENDER_SESSION_PROTOCOL_2026-07-19.md faithfully and can misbehave on
//! demand, so per-frame validation, the frame-deadline watchdog, and crash
//! invalidation are exercised without a native minihost build.

mod common;

#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{
        InteractiveParameter, ParameterAnimation, RenderGpuBackend, RenderPixelFormat,
    };
    use aexcompat_broker::render_session::{
        AudioRenderSession, AudioSessionOpenRequest, AudioSpanStatus, ClusterRenderPlugins,
        DiscoverySession, FrameStatus, InPlaceDiscoverySessionOpenRequest, InspectOutcome,
        RenderSession, SessionLayer, SessionOpenRequest, SwapOutcome, run_video_batch,
    };
    use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    const WIDTH: u32 = 8;
    const HEIGHT: u32 = 4;

    // The fixture behavior is selected through the inherited environment, so
    // tests that configure it must not interleave.
    static BEHAVIOR_LOCK: Mutex<()> = Mutex::new(());

    struct BehaviorGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);
    impl BehaviorGuard {
        fn set(behavior: Option<&str>) -> Self {
            let guard = BEHAVIOR_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            unsafe {
                match behavior {
                    Some(value) => std::env::set_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR", value),
                    None => std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR"),
                }
            }
            Self(guard)
        }
    }
    impl Drop for BehaviorGuard {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("AEXCOMPAT_TEST_SESSION_BEHAVIOR");
            }
        }
    }

    struct TempRepository(PathBuf);
    impl Drop for TempRepository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Sets the process-global opt-in minidump directory env for the duration
    /// of a test and removes it on drop (panic-safe). Callers hold the behavior
    /// lock, which serializes every session test that touches process-global
    /// env, so the window cannot interleave with another behavior test.
    struct MinidumpDirGuard;
    impl MinidumpDirGuard {
        fn set(value: &str) -> Self {
            unsafe { std::env::set_var("AEXCOMPAT_MINIDUMP_DIR", value) };
            Self
        }
    }
    impl Drop for MinidumpDirGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var("AEXCOMPAT_MINIDUMP_DIR") };
        }
    }

    fn build_fixture() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(manifest)
            .args(["-p", "dummy-workers", "--bin", "session_protocol_worker"])
            .status()
            .expect("run cargo build for the session protocol fixture");
        assert!(status.success(), "session protocol fixture build failed");
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("session_protocol_worker.exe")
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

    /// A temp repository whose `target/minihost-build/aex_render_worker.exe`
    /// is the protocol fixture; the "plugin" is inert bytes sealed and staged
    /// like a real AEX.
    fn temp_repository() -> (TempRepository, PathBuf, String) {
        let fixture = build_fixture();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-render-session-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        write_freshness_source_marker(&root);
        let worker_dir = root.join("target/minihost-build");
        std::fs::create_dir_all(&worker_dir).unwrap();
        std::fs::copy(&fixture, worker_dir.join("aex_render_worker.exe")).unwrap();
        // The smart session dispatches the smart worker binary; the fixture
        // serves both roles and keys its final-report contract off the
        // session command word.
        std::fs::copy(&fixture, worker_dir.join("aex_smart_worker.exe")).unwrap();
        let plugin = root.join("plugin.plugin");
        let plugin_bytes = b"render session dummy plugin";
        std::fs::write(&plugin, plugin_bytes).unwrap();
        let sha = format!("{:x}", Sha256::digest(plugin_bytes));
        (TempRepository(root), plugin, sha)
    }

    fn open_session(
        repository: &Path,
        plugin: &Path,
        sha: &str,
        frame_deadline: Duration,
    ) -> RenderSession {
        RenderSession::open(SessionOpenRequest {
            repository,
            plugin_path: plugin,
            plugin_sha256: sha,
            parameters: None,
            payload_override: None,
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
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline,
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open render session")
    }

    fn input_pattern(seed: u8) -> Vec<u8> {
        (0..WIDTH * HEIGHT * 4)
            .map(|index| seed.wrapping_add(index as u8))
            .collect()
    }

    fn current_desktop_name() -> String {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentThreadId() -> u32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetThreadDesktop(thread_id: u32) -> *mut std::ffi::c_void;
            fn GetUserObjectInformationW(
                object: *mut std::ffi::c_void,
                index: i32,
                buffer: *mut std::ffi::c_void,
                length: u32,
                required: *mut u32,
            ) -> i32;
        }
        unsafe {
            let desktop = GetThreadDesktop(GetCurrentThreadId());
            let mut required = 0u32;
            GetUserObjectInformationW(desktop, 2, std::ptr::null_mut(), 0, &mut required);
            let mut buffer = vec![0u16; (required as usize / 2).saturating_add(1)];
            assert_ne!(
                GetUserObjectInformationW(
                    desktop,
                    2,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 2) as u32,
                    &mut required,
                ),
                0
            );
            let end = buffer.iter().position(|value| *value == 0).unwrap_or(0);
            String::from_utf16(&buffer[..end]).unwrap()
        }
    }

    fn build_audio_fixture() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(manifest)
            .args([
                "-p",
                "dummy-workers",
                "--bin",
                "audio_session_protocol_worker",
            ])
            .status()
            .expect("run cargo build for the audio session fixture");
        assert!(status.success(), "audio session fixture build failed");
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("audio_session_protocol_worker.exe")
    }

    fn temp_audio_repository() -> (TempRepository, PathBuf, String) {
        let fixture = build_audio_fixture();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-audio-session-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        write_freshness_source_marker(&root);
        let worker_dir = root.join("target/minihost-build");
        std::fs::create_dir_all(&worker_dir).unwrap();
        std::fs::copy(&fixture, worker_dir.join("aex_render_worker.exe")).unwrap();
        let plugin = root.join("plugin.plugin");
        let plugin_bytes = b"audio session dummy plugin";
        std::fs::write(&plugin, plugin_bytes).unwrap();
        let sha = format!("{:x}", Sha256::digest(plugin_bytes));
        (TempRepository(root), plugin, sha)
    }

    #[test]
    fn audio_session_renders_spans_and_closes_clean() {
        if crate::common::skip_without_sealed_worker_launch(
            "audio_session_renders_spans_and_closes_clean",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_audio_repository();
        let mut session = AudioRenderSession::open(AudioSessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            max_samples: 1024,
            channels: 1,
            time_scale: 44100,
            frame_deadline: Duration::from_secs(30),
        })
        .expect("open audio session");

        // The fixture negates each f32 input sample into the output slot.
        let input: Vec<f32> = (0..8).map(|i| i as f32 * 0.25 - 1.0).collect();
        let outcome = session.render_span(0, &input).expect("span 0 renders");
        let AudioSpanStatus::Rendered {
            samples,
            checksum,
            output_start,
        } = outcome.status
        else {
            panic!("span 0 errored");
        };
        let expected: Vec<u8> = input.iter().flat_map(|s| (-s).to_le_bytes()).collect();
        assert_eq!(samples, expected);
        assert_eq!(checksum, format!("{:x}", Sha256::digest(&expected)));
        // The fixture reports the input start it received back through
        // start_sample (Codex #252); the wrapper carries it to the report.
        assert_eq!(output_start, 0);

        // A second span advances the generation and renders independently.
        let input2: Vec<f32> = vec![0.5, -0.5, 1.0];
        let outcome2 = session.render_span(1, &input2).expect("span 1 renders");
        let AudioSpanStatus::Rendered { samples: s2, .. } = outcome2.status else {
            panic!("span 1 errored");
        };
        let expected2: Vec<u8> = input2.iter().flat_map(|s| (-s).to_le_bytes()).collect();
        assert_eq!(s2, expected2);

        // Reusing a request index does not advance the generation: a caller
        // error that leaves the session usable.
        assert!(session.render_span(1, &input2).is_err());

        let close = session.close();
        assert_eq!(close["requests_ok"], 2);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    /// In-place parity (issue #751): an audio session opened with
    /// `dependency_search_dirs` launches the worker on the plug-in's real path
    /// (no staging) and still renders and closes clean. This exercises the
    /// broker-side in-place branch (search-dir validation + in-place launch)
    /// end to end; the fixture worker shape-gates the appended
    /// `--dependency-dirs-v1` pair the way the real worker's
    /// `apply_dependency_search_dirs` does, so a malformed broker join fails
    /// here instead of passing silently.
    #[test]
    fn audio_session_renders_in_place() {
        if crate::common::skip_without_sealed_worker_launch("audio_session_renders_in_place") {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_audio_repository();
        let search_dir = repository.0.join("deps");
        std::fs::create_dir_all(&search_dir).unwrap();
        let mut session = AudioRenderSession::open(AudioSessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            dependencies: Vec::new(),
            dependency_search_dirs: vec![search_dir],
            max_samples: 1024,
            channels: 1,
            time_scale: 44100,
            frame_deadline: Duration::from_secs(30),
        })
        .expect("open in-place audio session");

        let input: Vec<f32> = vec![0.5, -0.25, 1.0, -1.0];
        let outcome = session.render_span(0, &input).expect("span renders");
        let AudioSpanStatus::Rendered { samples, .. } = outcome.status else {
            panic!("span errored");
        };
        let expected: Vec<u8> = input.iter().flat_map(|s| (-s).to_le_bytes()).collect();
        assert_eq!(samples, expected);

        let close = session.close();
        assert_eq!(close["requests_ok"], 1);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    /// The broker independently bounds the reported output window against the
    /// submitted input span (Codex #252): a worker that reports
    /// start_sample + sample_count past input.len() is rejected as a
    /// host-protection invariant breach, not published as a valid span.
    #[test]
    fn audio_session_rejects_out_of_range_output_start() {
        if crate::common::skip_without_sealed_worker_launch(
            "audio_session_rejects_out_of_range_output_start",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("audio_out_of_range_start"));
        let (repository, plugin, sha) = temp_audio_repository();
        let mut session = AudioRenderSession::open(AudioSessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            max_samples: 1024,
            channels: 1,
            time_scale: 44100,
            frame_deadline: Duration::from_secs(30),
        })
        .expect("open audio session");

        let input: Vec<f32> = vec![0.25, -0.5, 0.75, -1.0];
        // The fixture reports start_sample = input_samples + 1, so the output
        // window runs past the input span and the broker must reject it.
        assert!(
            session.render_span(0, &input).is_err(),
            "an out-of-range output start must invalidate the span"
        );
        let close = session.close();
        assert_eq!(close["invalidated"], true, "close: {close}");
    }

    #[test]
    fn smart_session_dispatches_the_smart_worker_and_closes_clean() {
        if crate::common::skip_without_sealed_worker_launch(
            "smart_session_dispatches_the_smart_worker_and_closes_clean",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            layers: &[],
            alpha_as_coverage_params: &[],
            conformance_render_settings: None,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: true,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open smart render session");
        let outcome = session
            .render_frame(0, 0, &input_pattern(17))
            .expect("smart session frame renders");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["render_path"], "smart", "close: {close}");
        assert_eq!(close["session_clean"], true, "close: {close}");
        // The smart clean verdict comes from the smart report's dedicated
        // session_* fields, not the classic persistent-sequence keys.
        assert_eq!(close["final_report"]["session_mode"], true);
        assert_eq!(close["final_report"]["session_render_error"], 0);
    }

    /// In-place session launch (issue #751): the positional argv slot carries
    /// the plug-in's real path (no sealed staging exists) and the
    /// `--dependency-dirs-v1` auxiliary pair carries the admitted search
    /// directories, which the fixture shape-checks like the real worker's
    /// apply_dependency_search_dirs. The session then renders and closes
    /// clean over the same transport as a staged launch.
    #[test]
    fn in_place_session_renders_with_dependency_search_dirs() {
        if crate::common::skip_without_sealed_worker_launch(
            "in_place_session_renders_with_dependency_search_dirs",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let search_dir = plugin.parent().expect("plugin parent").to_path_buf();
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            layers: &[],
            alpha_as_coverage_params: &[],
            conformance_render_settings: None,
            dependencies: Vec::new(),
            dependency_search_dirs: vec![search_dir],
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open in-place render session");
        let outcome = session
            .render_frame(0, 0, &input_pattern(29))
            .expect("in-place session frame renders");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn smart_session_with_an_explicit_gpu_backend_requires_a_policy() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            layers: &[],
            alpha_as_coverage_params: &[],
            conformance_render_settings: None,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb32f,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: true,
            gpu_backend: RenderGpuBackend::DirectX,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("an explicit GPU backend without a policy must fail closed");
        assert!(
            error.to_string().contains("runtime module policy"),
            "{error}"
        );
    }

    #[test]
    fn smart_auto_backend_without_a_policy_degrades_to_the_cpu_session() {
        if crate::common::skip_without_sealed_worker_launch(
            "smart_auto_backend_without_a_policy_degrades_to_the_cpu_session",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // Auto + no policy opens the CPU smart session command; the fixture
        // rejects every command word except the two CPU session commands, so
        // reaching a rendered frame proves no GPU command was attempted.
        // (Argb8 keeps the fixture's depth-8 transport contract.)
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            layers: &[],
            alpha_as_coverage_params: &[],
            conformance_render_settings: None,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: true,
            gpu_backend: RenderGpuBackend::Auto,
            gpu_runtime_policy: None,
        })
        .expect("open smart render session with the auto backend");
        let outcome = session
            .render_frame(0, 0, &input_pattern(23))
            .expect("smart auto session frame renders");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    fn float_parameter(slot: u32) -> InteractiveParameter {
        serde_json::from_value(serde_json::json!({
            "slot": slot, "name": "amount", "kind": "float",
            "minimum": 0.0, "maximum": 100.0, "value": 1.0,
            "choices": [], "color": [0, 0, 0, 0], "components": [0.0, 0.0, 0.0],
            "component_count": 0, "layer_path": null,
            "enabled": true, "visible": true, "supervised": false,
        }))
        .expect("interactive parameter fixture")
    }

    fn scalar_animation(slot: u32) -> ParameterAnimation {
        serde_json::from_value(serde_json::json!({
            "slot": slot,
            "keys": [
                {"time": {"value": 0, "scale": 30}, "interpolation": "linear",
                 "value": {"type": "scalar", "value": 1.0}},
                {"time": {"value": 60, "scale": 30}, "interpolation": "linear",
                 "value": {"type": "scalar", "value": 50.0}},
            ],
        }))
        .expect("parameter animation fixture")
    }

    fn session_sidecars(repository: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(repository.join("target/image-transport"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.file_name()
                            .map(|name| {
                                name.to_string_lossy()
                                    .starts_with("parameter-animation-session-")
                            })
                            .unwrap_or(false)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn secondary_layers_reach_their_shared_slots() {
        if crate::common::skip_without_sealed_worker_launch(
            "secondary_layers_reach_their_shared_slots",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // Each layer's slot is filled with its slot number as a byte; the
        // fixture reads the first byte of each layer slot and rejects the
        // session unless the metadata and pixels landed in the right slot.
        let layers = vec![
            SessionLayer {
                slot: 3,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![3u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
                dynamic: false,
            },
            SessionLayer {
                slot: 7,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![7u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
                dynamic: false,
            },
        ];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open render session with secondary layers");
        let outcome = session
            .render_frame(0, 0, &input_pattern(5))
            .expect("frame renders with layers in their slots");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    /// A layer opened `dynamic` must show each frame the bytes written for that
    /// frame, not the ones the session opened with (issue #674): AviUtl2's
    /// virtual buffer drives a displacement map that has to follow a moving
    /// scene. The fixture worker re-reads the layer per frame and requires its
    /// first byte to be `slot + frame_index`, so a worker that consumed the
    /// layer at open, or an update that never reached the file, fails the frame.
    #[test]
    fn a_dynamic_layer_shows_each_frame_its_own_pixels() {
        if crate::common::skip_without_sealed_worker_launch(
            "a_dynamic_layer_shows_each_frame_its_own_pixels",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        const SLOT: u32 = 3;
        let bytes = (WIDTH * HEIGHT * 4) as usize;
        let layers = vec![SessionLayer {
            slot: SLOT,
            width: WIDTH,
            height: HEIGHT,
            rgba: vec![SLOT as u8; bytes],
            timed: None,
            dynamic: true,
        }];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open render session with a dynamic layer");
        // Frame 0 renders on the bytes the session opened with.
        let outcome = session
            .render_frame(0, 0, &input_pattern(5))
            .expect("frame 0 renders on the opening layer");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        // Every later frame gets its own map.
        for frame in 1u32..4 {
            session
                .update_dynamic_layer(SLOT, &vec![(SLOT as u8).wrapping_add(frame as u8); bytes])
                .expect("the layer accepts this frame's pixels");
            let outcome = session
                .render_frame(frame, 0, &input_pattern(5))
                .unwrap_or_else(|error| panic!("frame {frame} renders on its own map: {error}"));
            assert!(
                matches!(outcome.status, FrameStatus::Rendered { .. }),
                "frame {frame} status"
            );
        }
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    /// Geometry is fixed at open, and a slot that was not opened dynamic has no
    /// file to rewrite; both are caller errors rather than something to resize
    /// or invent, and neither may take the session down with it.
    #[test]
    fn a_dynamic_layer_update_is_bounded_by_what_it_opened_with() {
        if crate::common::skip_without_sealed_worker_launch(
            "a_dynamic_layer_update_is_bounded_by_what_it_opened_with",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        const SLOT: u32 = 3;
        let bytes = (WIDTH * HEIGHT * 4) as usize;
        let layers = vec![SessionLayer {
            slot: SLOT,
            width: WIDTH,
            height: HEIGHT,
            rgba: vec![SLOT as u8; bytes],
            timed: None,
            dynamic: true,
        }];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open render session with a dynamic layer");
        // Frame 0 first: the session numbers frames from zero, so starting at
        // one would fail for a reason that has nothing to do with layers.
        let outcome = session
            .render_frame(0, 0, &input_pattern(5))
            .expect("frame 0 renders on the opening layer");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let short = session.update_dynamic_layer(SLOT, &vec![0u8; bytes - 4]);
        assert!(short.is_err(), "a differently sized update is refused");
        let unknown = session.update_dynamic_layer(SLOT + 1, &vec![0u8; bytes]);
        assert!(
            unknown.is_err(),
            "a slot opened static has nothing to write"
        );
        // Refusing an update leaves the session usable: frame 1 still renders,
        // on the map this frame actually wrote.
        session
            .update_dynamic_layer(SLOT, &vec![(SLOT as u8).wrapping_add(1); bytes])
            .expect("a correctly sized update still lands");
        let outcome = session
            .render_frame(1, 0, &input_pattern(5))
            .expect("the session survives refused updates");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn timed_layers_travel_the_session_trailer_into_their_slots() {
        if crate::common::skip_without_sealed_worker_launch(
            "timed_layers_travel_the_session_trailer_into_their_slots",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // Two timed entries share slot 5 at different rational times, plus a
        // static secondary in slot 9. Each physical slot is filled with its
        // slot number as a byte; the fixture validates the header's
        // layer_slot_count and every slot's first byte, so the 5-field timed
        // trailer form must reach the same slots the static form does.
        let layers = vec![
            SessionLayer {
                slot: 5,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![5u8; (WIDTH * HEIGHT * 4) as usize],
                timed: Some((0, 30)),
                dynamic: false,
            },
            SessionLayer {
                slot: 5,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![5u8; (WIDTH * HEIGHT * 4) as usize],
                timed: Some((7, 30)),
                dynamic: false,
            },
            SessionLayer {
                slot: 9,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![9u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
                dynamic: false,
            },
        ];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open render session with timed layers");
        let outcome = session
            .render_frame(0, 0, &input_pattern(5))
            .expect("frame renders with timed layers in their slots");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn open_rejects_two_timed_layers_at_the_same_slot_and_time() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // Same slot, equal rational time (2/60 == 1/30): a per-frame collision
        // the worker parser would reject, so open must fail fast the same way.
        let layers = vec![
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: Some((1, 30)),
                dynamic: false,
            },
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: Some((2, 60)),
                dynamic: false,
            },
        ];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("open must reject a same-slot same-time timed collision");
        assert!(
            error.to_string().contains("unique"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn open_admits_a_static_and_timed_layer_at_the_same_slot() {
        if crate::common::skip_without_sealed_worker_launch(
            "open_admits_a_static_and_timed_layer_at_the_same_slot",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // A static entry and a timed entry share slot 4: the valid one-shot
        // representation of a layer parameter sampled at current_time (static)
        // and at another time (timed). The session must admit exactly what the
        // one-shot layered path admits, so open succeeds and both physical
        // slots (indexed by position) carry the slot-4 byte the fixture checks.
        let layers = vec![
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
                dynamic: false,
            },
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: Some((7, 30)),
                dynamic: false,
            },
        ];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open must admit a same-slot static and timed mix");
        let outcome = session
            .render_frame(0, 0, &input_pattern(4))
            .expect("frame renders with the static+timed slot mix");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn open_rejects_two_static_layers_at_the_same_slot() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // Two static entries at one slot are ambiguous per frame; open must
        // fail closed, the same rule the one-shot parser applies.
        let layers = vec![
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
                dynamic: false,
            },
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
                dynamic: false,
            },
        ];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("open must reject two static layers at one slot");
        assert!(
            error.to_string().contains("unique"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn alpha_as_coverage_params_travel_the_session_launch() {
        if crate::common::skip_without_sealed_worker_launch(
            "alpha_as_coverage_params_travel_the_session_launch",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // The slots ride the `--alpha-as-coverage-v1` auxiliary option, which
        // the worker peels from argv's tail before the session contract; the
        // fixture strips the pair and still resolves the 10-slot contract, so
        // open succeeds and frames render (issue #98 W1-4c).
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            alpha_as_coverage_params: &[0, 3],
            conformance_render_settings: None,
            layers: &[],
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open with alpha-as-coverage slots");
        let outcome = session
            .render_frame(0, 0, &input_pattern(6))
            .expect("frame renders with alpha-as-coverage slots");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn open_rejects_an_out_of_range_alpha_as_coverage_slot() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // Same bound the one-shot path enforces (slot <= 1024); open must fail
        // fast rather than launch a worker that rejects the option.
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            alpha_as_coverage_params: &[1025],
            conformance_render_settings: None,
            layers: &[],
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("open must reject an out-of-range alpha-as-coverage slot");
        assert!(
            error.to_string().contains("alpha-as-coverage"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn open_rejects_layer_pixels_that_do_not_match_dimensions() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let layers = vec![SessionLayer {
            slot: 3,
            width: WIDTH,
            height: HEIGHT,
            // One byte short of the declared geometry.
            rgba: vec![3u8; (WIDTH * HEIGHT * 4 - 1) as usize],
            timed: None,
            dynamic: false,
        }];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("mismatched layer pixels fail fast at open");
        assert!(
            error.to_string().contains("do not match dimensions"),
            "{error}"
        );
    }

    #[test]
    fn open_rejects_a_zero_layer_slot() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let layers = vec![SessionLayer {
            slot: 0,
            width: WIDTH,
            height: HEIGHT,
            rgba: vec![0u8; (WIDTH * HEIGHT * 4) as usize],
            timed: None,
            dynamic: false,
        }];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            layers: &layers,
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("a zero layer slot fails fast at open");
        assert!(error.to_string().contains("slot or dimensions"), "{error}");
    }

    /// `payload_override` must reach the worker's payload argv slot byte for
    /// byte (#365).
    ///
    /// The fixture-manifest route (`render_image`, scattermap) builds its
    /// payload from a descriptor profile via `encode_worker_payload`, which the
    /// `InteractiveParameter` list cannot express: ids are the profile's, not
    /// `param_<slot>`. That was the last reason a second (one-shot) transport
    /// had to exist, so the session grew this passthrough instead. The fixture
    /// echoes the slot it received as `launch_payload`, which is the only way to
    /// see what actually arrived.
    ///
    /// Two claims, and the first is what makes the second non-vacuous:
    ///   1. with no override, the slot carries exactly what
    ///      `encode_interactive_payload` produces for `parameters`;
    ///   2. with an override, the slot carries the override verbatim -- an id
    ///      shape the encoder in (1) can never emit, so a passthrough that
    ///      silently re-encoded `parameters` would fail here.
    #[test]
    fn a_payload_override_reaches_the_worker_verbatim() {
        if crate::common::skip_without_sealed_worker_launch(
            "a_payload_override_reaches_the_worker_verbatim",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let parameters = [float_parameter(1)];

        let launch_payload = |override_payload: Option<&str>| -> String {
            let mut session = RenderSession::open(SessionOpenRequest {
                repository: &repository.0,
                plugin_path: &plugin,
                plugin_sha256: &sha,
                parameters: Some(&parameters),
                payload_override: override_payload,
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
                dependency_search_dirs: Vec::new(),
                width: WIDTH,
                height: HEIGHT,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
                frame_deadline: Duration::from_secs(30),
                smart: false,
                gpu_backend: RenderGpuBackend::Cpu,
                gpu_runtime_policy: None,
            })
            .expect("open render session");
            let outcome = session
                .render_frame(0, 0, &input_pattern(3))
                .expect("frame");
            assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
            let close = session.close();
            assert_eq!(close["session_clean"], true, "close: {close}");
            close["final_report"]["launch_payload"]
                .as_str()
                .unwrap_or_else(|| panic!("no launch_payload in the report: {close}"))
                .to_owned()
        };

        // (1) No override: the encoder's own output reaches the worker. This
        // also pins the encoding the fixture route has to be distinguishable
        // from.
        let encoded = aexcompat_broker::image_render::encode_interactive_payload(&parameters)
            .expect("encode the parameter payload");
        assert_eq!(encoded, "v2|param_1@1:f64=1");
        assert_eq!(launch_payload(None), encoded);

        // (2) An override in the fixture route's shape: a descriptor id that is
        // not `param_<slot>`, so re-encoding `parameters` could not produce it.
        let fixture_payload = "v3|density@1:i32=7;tint@2:argb8=255,10,20,30";
        assert_ne!(fixture_payload, encoded);
        assert_eq!(launch_payload(Some(fixture_payload)), fixture_payload);
    }

    #[test]
    fn animation_sidecar_rides_the_session_and_is_cleaned_up() {
        if crate::common::skip_without_sealed_worker_launch(
            "animation_sidecar_rides_the_session_and_is_cleaned_up",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let parameters = [float_parameter(1)];
        let animations = [scalar_animation(1)];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: Some(&parameters),
            payload_override: None,
            parameter_animation: Some(&animations),
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
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open render session with parameter animation");
        assert_eq!(
            session_sidecars(&repository.0).len(),
            1,
            "the sidecar exists for the session lifetime"
        );
        let outcome = session
            .render_frame(0, 0, &input_pattern(9))
            .expect("frame renders with an animation sidecar attached");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(
            session_sidecars(&repository.0).len(),
            0,
            "the sidecar is removed when the session ends"
        );
    }

    #[test]
    fn arbitrary_data_parameters_accept_arbitrary_animation() {
        if crate::common::skip_without_sealed_worker_launch(
            "arbitrary_data_parameters_accept_arbitrary_animation",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let parameters: [InteractiveParameter; 1] = [serde_json::from_value(serde_json::json!({
            "slot": 1, "name": "state", "kind": "arbitrary_data",
            "minimum": 0.0, "maximum": 0.0, "value": 0.0,
            "choices": [], "color": [0, 0, 0, 0], "components": [0.0, 0.0, 0.0],
            "component_count": 0, "layer_path": null,
            "enabled": true, "visible": true, "supervised": false,
            "debug_summary": "state",
        }))
        .expect("arbitrary parameter fixture")];
        let animations: [ParameterAnimation; 1] = [serde_json::from_value(serde_json::json!({
            "slot": 1,
            "keys": [
                {"time": {"value": 0, "scale": 30}, "interpolation": "hold",
                 "value": {"type": "arbitrary", "value": [1, 2, 3]}},
            ],
        }))
        .expect("arbitrary animation fixture")];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: Some(&parameters),
            payload_override: None,
            parameter_animation: Some(&animations),
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
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("arbitrary_data parameters bind arbitrary animation timelines");
        let outcome = session
            .render_frame(0, 0, &input_pattern(3))
            .expect("frame renders");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        assert_eq!(session.close()["session_clean"], true);
    }

    #[test]
    fn auxiliary_options_ride_the_session_argv_tail() {
        if crate::common::skip_without_sealed_worker_launch(
            "auxiliary_options_ride_the_session_argv_tail",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        // A manifest the real worker's loader would accept: one depth channel
        // whose f32le sidecar exists next to it with a matching hash. The
        // fixture enforces the top-level gate (schema/nonce/non-empty
        // channels); the deep per-channel validation stays with the real
        // worker's own transport tests.
        let sidecar = repository.0.join("aux-1-0-0.f32le");
        let payload: Vec<u8> = [0.0f32, 0.25, 1.0, 2.0]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        std::fs::write(&sidecar, &payload).unwrap();
        let manifest = repository.0.join("aux-manifest.json");
        std::fs::write(
            &manifest,
            serde_json::json!({
                "schema": "aux-manifest-v1",
                "nonce": "1",
                "channels": [{
                    "param_index": 0,
                    "type": 0x4450_5448,
                    "name": "Depth",
                    "data_type": "f32le",
                    "dimension": 1,
                    "width": 2,
                    "height": 2,
                    "samples": [{
                        "time": 0,
                        "time_scale": 30,
                        "path": sidecar.to_string_lossy(),
                        "sampling": "hold",
                        "interpretation": "depth",
                        "expected_byte_length": payload.len(),
                        "sha256": format!("{:x}", Sha256::digest(&payload)),
                    }],
                }],
            })
            .to_string(),
        )
        .unwrap();
        // Under the broker-managed dump boundary; the resolver creates it.
        let dump_dir = repository.0.join("target/world-dumps");
        // The fixture worker validates each pair like the real worker's
        // auxiliary gates (existing file / existing directory / literal "1"),
        // so a mangled pair would kill the session before the first frame.
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: Some(&manifest),
            world_dump_dir: Some(&dump_dir),
            output_checksum_detail: true,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            alpha_as_coverage_params: &[],
            conformance_render_settings: None,
            layers: &[],
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open render session with auxiliary options");
        let outcome = session
            .render_frame(0, 0, &input_pattern(7))
            .expect("frame renders with auxiliary options attached");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        assert_eq!(session.close()["session_clean"], true);
    }

    #[test]
    fn open_rejects_a_non_empty_world_dump_directory() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let reused = repository.0.join("target/reused-dumps");
        std::fs::create_dir_all(&reused).unwrap();
        std::fs::write(reused.join("000-stale.bin"), b"stale").unwrap();
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: Some(&reused),
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            alpha_as_coverage_params: &[],
            conformance_render_settings: None,
            layers: &[],
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("a reused dump directory fails fast at open");
        assert!(error.to_string().contains("empty"), "{error}");
    }

    #[test]
    fn open_rejects_a_world_dump_directory_outside_the_target_tree() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let missing = repository.0.join("outside-dumps");
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: Some(&missing),
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            audio_trailer: None,
            alpha_as_coverage_params: &[],
            conformance_render_settings: None,
            layers: &[],
            dependencies: Vec::new(),
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("a dump directory outside the managed tree fails fast at open");
        assert!(error.to_string().contains("target tree"), "{error}");
    }

    #[test]
    fn open_rejects_animation_bound_to_an_unknown_slot() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let animations = [scalar_animation(2)];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
            parameter_animation: Some(&animations),
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
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .map(|_| ())
        .expect_err("an animation without a matching parameter fails before launch");
        assert!(error.to_string().contains("unknown slot"), "{error}");
        assert_eq!(
            session_sidecars(&repository.0).len(),
            0,
            "a rejected open writes no sidecar"
        );
    }

    #[test]
    fn session_renders_frames_and_validates_slot_transfers() {
        if crate::common::skip_without_sealed_worker_launch(
            "session_renders_frames_and_validates_slot_transfers",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let mut checksums = Vec::new();
        for (frame_index, seed) in [(0u32, 11u8), (1, 173)] {
            let input = input_pattern(seed);
            let outcome = session
                .render_frame(frame_index, frame_index as i32, &input)
                .expect("frame renders");
            match outcome.status {
                FrameStatus::Rendered { pixels, .. } => {
                    let expected: Vec<u8> = input.iter().map(|byte| 255 - byte).collect();
                    assert_eq!(pixels, expected, "slot transfer round-trips the render");
                    // Hashed here rather than taken from the frame: the
                    // transport stopped carrying a per-frame content hash in
                    // issue #690, and what this asserts is that two inputs
                    // produce two outputs.
                    checksums.push(format!("{:x}", Sha256::digest(&pixels)));
                }
                FrameStatus::FrameError { render_error, .. } => {
                    panic!("frame {frame_index} unexpectedly errored: {render_error}")
                }
            }
        }
        assert_ne!(
            checksums[0], checksums[1],
            "distinct inputs produce distinct outputs"
        );
        let close = session.close();
        assert_eq!(close["frames_ok"], 2);
        assert_eq!(close["invalidated"], false);
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["worker"]["classification"], "ok");
        assert_eq!(close["final_report"]["session_frames"], 2);
    }

    #[test]
    fn zero_duration_session_renders_the_single_frame() {
        if crate::common::skip_without_sealed_worker_launch(
            "zero_duration_session_renders_the_single_frame",
        ) {
            return;
        }
        // A zero-duration render (total_time == 0) is valid and renders the
        // single current_time == 0 frame, matching the one-shot worker (#272).
        // It is no longer routed to the one-shot path.
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 0,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("a zero-duration session opens");
        let input = input_pattern(19);
        let outcome = session
            .render_frame(0, 0, &input)
            .expect("the t=0 frame renders");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("the t=0 frame errored");
        };
        let expected: Vec<u8> = input.iter().map(|byte| 255 - byte).collect();
        assert_eq!(pixels, expected);
        // total_time == 0 admits exactly the t=0 frame: a later current_time is
        // outside the session's total time and is rejected per-frame without
        // tearing the session down (a plain validation error, not an
        // invalidation), so the session still closes clean afterwards.
        assert!(
            session.render_frame(1, 1, &input).is_err(),
            "a frame past total_time must be rejected"
        );
        let close = session.close();
        assert_eq!(close["frames_ok"], 1);
        assert_eq!(close["invalidated"], false);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn per_frame_parameters_ride_the_v2_message_and_reach_the_worker() {
        if crate::common::skip_without_sealed_worker_launch(
            "per_frame_parameters_ride_the_v2_message_and_reach_the_worker",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let input = input_pattern(31);
        let inverted: Vec<u8> = input.iter().map(|byte| 255 - byte).collect();

        // Frame 0 stays a v:1 message: plain inverted transfer.
        let outcome = session
            .render_frame(0, 0, &input)
            .expect("v1 frame renders");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("v1 frame errored");
        };
        assert_eq!(pixels, inverted);

        // Frame 1 carries per-frame parameters; the fixture stamps the digest
        // of the received payload into the frame, proving delivery.
        let mut updated = float_parameter(1);
        updated.value = 42.5;
        let outcome = session
            .render_frame_with_parameters(1, 1, &input, Some(std::slice::from_ref(&updated)))
            .expect("v2 frame renders");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("v2 frame errored");
        };
        let payload = aexcompat_broker::image_render::encode_interactive_payload(
            std::slice::from_ref(&updated),
        )
        .expect("payload encodes");
        assert_eq!(&pixels[..32], Sha256::digest(payload.as_bytes()).as_slice());
        assert_eq!(&pixels[32..], &inverted[32..]);

        // Frame 2 reverts to v:1 and the stamp disappears: the update was
        // frame-scoped, not sticky.
        let outcome = session
            .render_frame(2, 2, &input)
            .expect("v1 frame renders again");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("post-update v1 frame errored");
        };
        assert_eq!(pixels, inverted);

        let close = session.close();
        assert_eq!(close["frames_ok"], 3);
        assert_eq!(close["parameter_update_frames"], 1);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn per_frame_ui_action_rides_the_v2_message_and_reaches_the_worker() {
        if crate::common::skip_without_sealed_worker_launch(
            "per_frame_ui_action_rides_the_v2_message_and_reaches_the_worker",
        ) {
            return;
        }
        use aexcompat_broker::image_render::RenderUiAction;
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let input = input_pattern(29);
        let inverted: Vec<u8> = input.iter().map(|byte| 255 - byte).collect();

        // Frame 0 stays a v:1 message: plain inverted transfer, no stamp.
        let outcome = session
            .render_frame(0, 0, &input)
            .expect("v1 frame renders");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("v1 frame errored");
        };
        assert_eq!(pixels, inverted);

        // Frame 1 carries a ui_action only; the fixture stamps the digest of the
        // received ui_action string into bytes [32,64), proving delivery.
        let action = RenderUiAction::Click {
            point: [64, 48],
            color: [1.0, 0.0, 0.0, 1.0],
        };
        let encoded = action.encode_ui_field().expect("ui_action encodes");
        let outcome = session
            .render_frame_with_attributes(1, 1, &input, None, Some(&action))
            .expect("v2 ui_action frame renders");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("v2 ui_action frame errored");
        };
        assert_eq!(
            &pixels[32..64],
            Sha256::digest(encoded.as_bytes()).as_slice()
        );
        // The parameters region stays untouched, and everything past the stamp
        // is the plain inverted transfer.
        assert_eq!(&pixels[..32], &inverted[..32]);
        assert_eq!(&pixels[64..], &inverted[64..]);

        // Frame 2 carries both parameters and ui_action in one v:2 message,
        // proving the attributes co-exist (§4.2.1). Both digests are stamped.
        let mut updated = float_parameter(1);
        updated.value = 42.5;
        let outcome = session
            .render_frame_with_attributes(
                2,
                2,
                &input,
                Some(std::slice::from_ref(&updated)),
                Some(&action),
            )
            .expect("v2 combined frame renders");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("v2 combined frame errored");
        };
        let payload = aexcompat_broker::image_render::encode_interactive_payload(
            std::slice::from_ref(&updated),
        )
        .expect("payload encodes");
        assert_eq!(&pixels[..32], Sha256::digest(payload.as_bytes()).as_slice());
        assert_eq!(
            &pixels[32..64],
            Sha256::digest(encoded.as_bytes()).as_slice()
        );

        // Frame 3 reverts to v:1: both stamps disappear (attributes were
        // frame-scoped, not sticky).
        let outcome = session
            .render_frame(3, 3, &input)
            .expect("v1 frame renders again");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("post-attribute v1 frame errored");
        };
        assert_eq!(pixels, inverted);

        let close = session.close();
        assert_eq!(close["frames_ok"], 4);
        // Only frame 2 carried `parameters`; a ui_action-only frame is not a
        // parameter update.
        assert_eq!(close["parameter_update_frames"], 1);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn interactive_session_renders_reports_and_previews_across_frames() {
        if crate::common::skip_without_sealed_worker_launch(
            "interactive_session_renders_reports_and_previews_across_frames",
        ) {
            return;
        }
        use aexcompat_broker::image_render::{InteractiveRenderSession, InteractiveSessionOpen};
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let parameters = [float_parameter(1)];
        let input = input_pattern(53);
        let mut updated = float_parameter(1);
        updated.value = 7.5;
        for (smart, capability_source, render_path, capability_identity) in [
            (false, "advertised_classic", "classic", 0),
            // This fixture supports the SmartFX session protocol.  The
            // assertion proves InteractiveRenderSession does not silently
            // send an advertised SmartFX selection to Classic RENDER (#606).
            (true, "advertised_smart", "smartfx", 1 << 10),
        ] {
            let mut session = InteractiveRenderSession::open(InteractiveSessionOpen {
                repository: &repository.0,
                plugin_id: "experimental",
                plugin_path: &plugin,
                plugin_sha256: &sha,
                parameters: Some(&parameters),
                selection: aexcompat_broker::image_render::InteractiveSessionSelection::new(
                    if smart {
                        aexcompat_broker::image_render::InteractiveRenderPath::SmartFx
                    } else {
                        aexcompat_broker::image_render::InteractiveRenderPath::Classic
                    },
                    if smart {
                        aexcompat_broker::image_render::InteractiveCapabilitySource::AdvertisedSmart
                    } else {
                        aexcompat_broker::image_render::InteractiveCapabilitySource::AdvertisedClassic
                    },
                    1,
                    capability_identity,
                )
                .expect("valid inspection-bound selection"),
                dependencies: Vec::new(),
                width: WIDTH,
                height: HEIGHT,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
                timeout_ms: 30_000,
            })
            .expect("open interactive session");
            assert!(!session.invalidated());

            for (frame, time) in [(0u32, 0i32), (1, 1)] {
                let output = repository.0.join(format!("live-{render_path}-{frame}.png"));
                let report = session
                    .render(&input, time, Some(std::slice::from_ref(&updated)), &output)
                    .expect("interactive frame renders");
                assert_eq!(report["stage"], "interactive_image_render");
                assert_eq!(report["render_path"], render_path);
                assert_eq!(report["smart_capability_source"], capability_source);
                assert_eq!(report["smart_capability_identity"], capability_identity);
                assert_eq!(report["smart_capability_version"], 1);
                assert_eq!(report["worker_classification"], "resident_session");
                assert_eq!(report["passed"], true);
                assert_eq!(report["resident_session"]["frame_index"], frame);
                assert_eq!(report["resident_session"]["parameter_update"], true);
                assert!(output.is_file(), "preview PNG is written per frame");
            }
            let close = session.close();
            assert_eq!(close["frames_ok"], 2);
            assert_eq!(close["parameter_update_frames"], 2);
            assert_eq!(close["render_path"], render_path);
            assert_eq!(close["smart_capability_source"], capability_source);
            assert_eq!(close["smart_capability_identity"], capability_identity);
            assert_eq!(close["smart_capability_version"], 1);
            let selector_count = |selector: &str| {
                close["final_report"]["selector_counters"][selector]
                    .as_u64()
                    .expect("worker must return bounded selector counter evidence")
            };
            if smart {
                assert_eq!(close["final_report"]["session_mode"], true);
                assert_eq!(selector_count("classic_render"), 0);
                assert!(selector_count("smart_pre_render") >= 2);
                assert!(selector_count("smart_render") >= 2);
            } else {
                assert!(close["final_report"]["session_mode"].is_null());
                assert_eq!(selector_count("smart_pre_render"), 0);
                assert_eq!(selector_count("smart_render"), 0);
                assert!(selector_count("classic_render") >= 2);
            }
            assert_eq!(close["session_clean"], true, "close: {close}");
        }
    }

    #[test]
    fn rejected_per_frame_parameters_leave_the_session_usable() {
        if crate::common::skip_without_sealed_worker_launch(
            "rejected_per_frame_parameters_leave_the_session_usable",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        // Out-of-range value: the broker-side validation rejects the set
        // before anything reaches the transport, exactly like the launch
        // payload validation would.
        let mut out_of_range = float_parameter(1);
        out_of_range.value = 1000.0;
        let error = session
            .render_frame_with_parameters(0, 0, &input_pattern(7), Some(&[out_of_range]))
            .expect_err("out-of-range parameters are rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(session.invalidation().is_none(), "session stays usable");
        let outcome = session
            .render_frame(0, 0, &input_pattern(7))
            .expect("the session still renders after the rejection");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["frames_ok"], 1);
        assert_eq!(close["parameter_update_frames"], 0);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn frame_local_error_keeps_the_session_usable() {
        if crate::common::skip_without_sealed_worker_launch(
            "frame_local_error_keeps_the_session_usable",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("error_frame_0"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let outcome = session
            .render_frame(0, 0, &input_pattern(1))
            .expect("frame-local errors do not invalidate the session");
        assert!(matches!(
            outcome.status,
            FrameStatus::FrameError {
                render_error: -40,
                missing_dependency: None,
                return_message: None,
            }
        ));
        let outcome = session
            .render_frame(1, 1, &input_pattern(2))
            .expect("the session continues after a frame-local error");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["frames_ok"], 1);
        assert_eq!(close["frames_errored"], 1);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn empty_smart_result_frame_is_accepted_as_a_valid_empty_render() {
        if crate::common::skip_without_sealed_worker_launch(
            "empty_smart_result_frame_is_accepted_as_a_valid_empty_render",
        ) {
            return;
        }
        // A SmartFX frame whose PreRender returned a legally empty result_rect
        // (#278) reports a 0x0 ok frame with the explicit empty_result flag. The
        // session accepts it as a valid empty render (not a dimension invariant
        // failure) with no output pixels, and stays usable for later frames.
        let _behavior = BehaviorGuard::set(Some("empty_result_frame_0"));
        let (repository, plugin, sha) = temp_repository();
        // Only a SmartFX session may report an empty result, so open a smart
        // session (the broker rejects an empty result on a classic session).
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            payload_override: None,
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
            dependency_search_dirs: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline: Duration::from_secs(30),
            smart: true,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        })
        .expect("open a smart render session");
        let outcome = session
            .render_frame(0, 0, &input_pattern(1))
            .expect("an empty-result frame is a valid render, not an invalidation");
        let FrameStatus::Rendered {
            pixels,
            width,
            height,
            ..
        } = outcome.status
        else {
            panic!("the empty-result frame did not render");
        };
        assert_eq!((width, height), (0, 0), "an empty result has zero geometry");
        assert!(pixels.is_empty(), "an empty result carries no pixels");
        // The session stays usable: a normal frame renders afterwards.
        let outcome = session
            .render_frame(1, 1, &input_pattern(2))
            .expect("the session continues after an empty-result frame");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["frames_ok"], 2);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn frame_deadline_watchdog_terminates_the_job() {
        if crate::common::skip_without_sealed_worker_launch(
            "frame_deadline_watchdog_terminates_the_job",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("hang_frame"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(2));
        let error = session
            .render_frame(0, 0, &input_pattern(3))
            .expect_err("a hung frame must trip the watchdog");
        assert!(error.to_string().contains("frame_deadline"), "{error}");
        let follow_up = session
            .render_frame(1, 1, &input_pattern(4))
            .expect_err("an invalidated session refuses further frames");
        assert!(follow_up.to_string().contains("invalidated"), "{follow_up}");
        let close = session.close();
        assert_eq!(close["invalidated"], true);
        assert_eq!(close["invalidated_reason"]["reason"], "frame_deadline");
        assert_eq!(close["session_clean"], false);
    }

    #[test]
    fn modal_ui_worker_uses_a_private_desktop_before_session_timeout() {
        if crate::common::skip_without_sealed_worker_launch(
            "modal_ui_worker_uses_a_private_desktop_before_session_timeout",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("modal_frame"));
        let report_path = std::env::temp_dir().join(format!(
            "aexcompat-session-desktop-{:032x}.txt",
            rand::random::<u128>()
        ));
        unsafe {
            std::env::set_var("AEXCOMPAT_TEST_SESSION_DESKTOP_REPORT", &report_path);
        }
        let parent_desktop = current_desktop_name();
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(2));
        let error = session
            .render_frame(0, 0, &input_pattern(17))
            .expect_err("a modal worker must trip the session watchdog");
        assert!(error.to_string().contains("frame_deadline"), "{error}");
        let close = session.close();
        unsafe {
            std::env::remove_var("AEXCOMPAT_TEST_SESSION_DESKTOP_REPORT");
        }
        let worker_desktop = std::fs::read_to_string(&report_path).unwrap();
        let _ = std::fs::remove_file(report_path);
        assert!(
            worker_desktop.starts_with("AEXCompatWorkerDesktop-"),
            "worker desktop was not private: {worker_desktop:?}"
        );
        assert_ne!(worker_desktop, parent_desktop);
        assert_eq!(close["invalidated"], true);
        assert_eq!(close["invalidated_reason"]["reason"], "frame_deadline");
        assert_eq!(close["session_clean"], false);
    }

    #[test]
    fn worker_crash_invalidates_the_session_with_diagnostics() {
        if crate::common::skip_without_sealed_worker_launch(
            "worker_crash_invalidates_the_session_with_diagnostics",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("crash_frame"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(5))
            .expect_err("a crashed worker must invalidate the session");
        assert!(error.to_string().contains("worker_exited"), "{error}");
        let close = session.close();
        assert_eq!(close["invalidated"], true);
        assert_eq!(close["worker"]["classification"], "crashed");
        assert_eq!(close["session_clean"], false);
    }

    #[test]
    fn a_crashing_resident_session_captures_an_opt_in_minidump() {
        if crate::common::skip_without_sealed_worker_launch(
            "a_crashing_resident_session_captures_an_opt_in_minidump",
        ) {
            return;
        }
        // Opt-in on: the broker creates one inherited dump pipe for the session
        // launch (the same launch-boundary plumbing the one-shot path uses,
        // issue #18/#224) because AEXCOMPAT_MINIDUMP_DIR resolves under the
        // repository target tree. The fixture streams a marker-terminated image
        // through that pipe from its crash frame, and the broker finalizes it
        // into a .dmp when the session collects the exit at close.
        let _behavior = BehaviorGuard::set(Some("crash_frame_minidump"));
        let (repository, plugin, sha) = temp_repository();
        let _minidump_dir = MinidumpDirGuard::set("target/crash-dumps");
        let dump_dir = repository.0.join("target").join("crash-dumps");

        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(5))
            .expect_err("a crashed worker must invalidate the session");
        assert!(error.to_string().contains("worker_exited"), "{error}");
        let close = session.close();
        assert_eq!(close["invalidated"], true);
        assert_eq!(close["worker"]["classification"], "crashed");
        // The session report surfaces the capture through the same path-free
        // `minidump` marker the one-shot path uses (issue #224 item 2).
        assert_eq!(
            close["worker"]["diagnostics"]["minidump"],
            "written bytes=4096"
        );

        // Exactly one dump is finalized at close; the broker withholds the
        // completion marker from the published file.
        let dumps: Vec<PathBuf> = std::fs::read_dir(&dump_dir)
            .expect("managed dump directory exists")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "dmp"))
            .collect();
        assert_eq!(dumps.len(), 1, "exactly one finalized dump: {dumps:?}");
        let bytes = std::fs::read(&dumps[0]).unwrap();
        assert!(
            bytes.starts_with(b"MDMP"),
            "published dump keeps the streamed body"
        );
        assert!(
            !bytes.ends_with(b"AEXDUMP-COMPLETE"),
            "marker withheld from the file"
        );
        assert_eq!(bytes.len(), 4096);
        // No orphan reservation is left behind.
        let parts = std::fs::read_dir(&dump_dir)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "part"))
            .count();
        assert_eq!(parts, 0, "no unfinalized .dmp.part remains");
    }

    #[test]
    fn a_reserved_fatal_session_error_invalidates_instead_of_continuing() {
        if crate::common::skip_without_sealed_worker_launch(
            "a_reserved_fatal_session_error_invalidates_instead_of_continuing",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("fatal_error_frame_0"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(12))
            .expect_err("a reserved fatal error code must not read as frame-local");
        assert!(
            error.to_string().contains("worker_invariant_failure"),
            "{error}"
        );
        let close = session.close();
        assert_eq!(close["invalidated"], true);
        assert_eq!(
            close["invalidated_reason"]["reason"],
            "worker_invariant_failure"
        );
        assert_eq!(close["session_clean"], false);
    }

    #[test]
    fn a_framing_violation_from_a_live_worker_invalidates_promptly() {
        if crate::common::skip_without_sealed_worker_launch(
            "a_framing_violation_from_a_live_worker_invalidates_promptly",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("bad_framing_frame_0"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let started = std::time::Instant::now();
        let error = session
            .render_frame(0, 0, &input_pattern(13))
            .expect_err("broken framing must invalidate the session");
        assert!(
            error.to_string().contains("response_framing_violation"),
            "{error}"
        );
        // The worker stays alive after the bad prefix, so only the explicit
        // reader event (not the deadline, not process death) can be this fast.
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "invalidation took {:?}",
            started.elapsed()
        );
        assert_eq!(session.close()["invalidated"], true);
    }

    #[test]
    fn reused_frame_indices_are_rejected_without_killing_the_session() {
        if crate::common::skip_without_sealed_worker_launch(
            "reused_frame_indices_are_rejected_without_killing_the_session",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let outcome = session
            .render_frame(0, 0, &input_pattern(14))
            .expect("frame 0 renders");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        // Replaying frame 0 would recompute generation 1, which the output
        // slot already holds: a stale frame_done could then pass as fresh.
        let error = session
            .render_frame(0, 0, &input_pattern(15))
            .expect_err("a reused frame index must be rejected");
        assert!(error.to_string().contains("does not advance"), "{error}");
        // The rejection is caller-local: the session keeps rendering.
        let outcome = session
            .render_frame(1, 1, &input_pattern(16))
            .expect("frame 1 renders after the rejected dispatch");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["invalidated"], false);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn error_response_with_a_mutated_header_is_fail_closed() {
        if crate::common::skip_without_sealed_worker_launch(
            "error_response_with_a_mutated_header_is_fail_closed",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("error_mutates_header"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(10))
            .expect_err("a header mutation must not hide behind an error response");
        assert!(
            error.to_string().contains("frame_invariant_failure"),
            "{error}"
        );
        assert_eq!(session.close()["invalidated"], true);
    }

    #[test]
    fn a_unilateral_worker_exit_breaks_the_close_handshake_contract() {
        if crate::common::skip_without_sealed_worker_launch(
            "a_unilateral_worker_exit_breaks_the_close_handshake_contract",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("exit_after_frame_0"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let outcome = session
            .render_frame(0, 0, &input_pattern(11))
            .expect("the frame itself completes");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        // Let the worker's exit complete; close() then detects it through the
        // synchronous process-handle check even if the async watcher event
        // has not been delivered yet.
        std::thread::sleep(Duration::from_millis(500));
        // The worker exited 0 with a clean-looking final report, but it never
        // received close: the session must not read as clean.
        let close = session.close();
        assert_eq!(close["invalidated"], true, "close: {close}");
        assert_eq!(close["invalidated_reason"]["reason"], "premature_exit");
        assert_eq!(close["session_clean"], false);
        assert_eq!(close["worker"]["classification"], "ok");
    }

    #[test]
    fn process_death_is_seen_even_when_a_descendant_holds_the_pipe() {
        if crate::common::skip_without_sealed_worker_launch(
            "process_death_is_seen_even_when_a_descendant_holds_the_pipe",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("exit_leaving_descendant"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let started = std::time::Instant::now();
        let error = session
            .render_frame(0, 0, &input_pattern(9))
            .expect_err("a dead worker must invalidate the session");
        // The sleeping descendant keeps the response pipe open, so only the
        // process watcher can observe the death; it must beat the 30s frame
        // deadline by a wide margin.
        assert!(error.to_string().contains("worker_exited"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "invalidation took {:?}, the frame deadline masked the process death",
            started.elapsed()
        );
        let close = session.close();
        assert_eq!(close["invalidated"], true);
        assert_eq!(close["invalidated_reason"]["reason"], "worker_exited");
        assert_eq!(close["worker"]["classification"], "nonzero_exit");
    }

    #[test]
    fn stale_generation_and_missing_header_update_are_fail_closed() {
        if crate::common::skip_without_sealed_worker_launch(
            "stale_generation_and_missing_header_update_are_fail_closed",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("stale_generation"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(6))
            .expect_err("a stale generation must invalidate the session");
        assert!(
            error.to_string().contains("frame_invariant_failure"),
            "{error}"
        );
        assert_eq!(session.close()["invalidated"], true);
    }

    #[test]
    fn mutated_static_header_is_fail_closed() {
        if crate::common::skip_without_sealed_worker_launch("mutated_static_header_is_fail_closed")
        {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("mutate_header"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(7))
            .expect_err("a mutated broker-owned header must invalidate the session");
        assert!(
            error.to_string().contains("frame_invariant_failure"),
            "{error}"
        );
        assert_eq!(session.close()["invalidated"], true);
    }

    #[test]
    fn output_extent_mismatch_is_fail_closed() {
        if crate::common::skip_without_sealed_worker_launch("output_extent_mismatch_is_fail_closed")
        {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("bad_extent"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(8))
            .expect_err("an extent disagreement must invalidate the session");
        assert!(
            error.to_string().contains("output_extent_mismatch"),
            "{error}"
        );
        assert_eq!(session.close()["invalidated"], true);
    }

    fn write_input_frames(root: &Path, count: u32) -> Vec<String> {
        (0..count)
            .map(|index| {
                let path = root.join(format!("input-{index}.png"));
                let image = image::RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
                    image::Rgba([
                        (x * 20 + index) as u8,
                        (y * 40 + index) as u8,
                        index as u8,
                        255,
                    ])
                });
                image.save(&path).unwrap();
                path.to_string_lossy().into_owned()
            })
            .collect()
    }

    #[test]
    fn video_batch_cli_renders_a_png_sequence_through_one_session() {
        if crate::common::skip_without_sealed_worker_launch(
            "video_batch_cli_renders_a_png_sequence_through_one_session",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, _sha) = temp_repository();
        let inputs = write_input_frames(&repository.0, 3);
        let output_directory = repository.0.join("batch-out");
        // Auxiliary observation options flow from the batch request into the
        // session argv tail; the fixture worker validates each pair, so a
        // dropped or mangled option would kill the batch.
        let dump_dir = repository.0.join("target/batch-dumps");
        let request_path = repository.0.join("request.json");
        std::fs::write(
            &request_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "plugin": plugin.to_string_lossy(),
                "input_frames": inputs,
                "output_directory": output_directory.to_string_lossy(),
                "world_dump_dir": dump_dir.to_string_lossy(),
                "output_checksum_detail": true,
            }))
            .unwrap(),
        )
        .unwrap();
        let report_path = repository.0.join("report.json");
        let passed =
            run_video_batch(&repository.0, &request_path, &report_path).expect("batch render runs");
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert!(passed, "report: {report}");
        assert_eq!(report["frame_count"], 3);
        assert_eq!(report["frames_ok"], 3);
        assert_eq!(report["aborted"], false);
        assert_eq!(report["session"]["session_clean"], true);
        for index in 0..3 {
            assert!(
                output_directory
                    .join(format!("frame-{index:06}.png"))
                    .is_file(),
                "output frame {index} exists"
            );
        }
    }

    #[test]
    fn video_batch_reports_an_empty_smart_frame_without_a_png() {
        if crate::common::skip_without_sealed_worker_launch(
            "video_batch_reports_an_empty_smart_frame_without_a_png",
        ) {
            return;
        }
        // A SmartFX batch frame that legally renders an empty result (#278) has
        // no pixels, so there is no PNG or raw to write. The batch must report it
        // as a legal empty frame and keep going, not abort on a 0x0 image. The
        // fixture answers frame 0 empty; frames 1-2 render normally.
        let _behavior = BehaviorGuard::set(Some("empty_result_frame_0"));
        let (repository, plugin, _sha) = temp_repository();
        let inputs = write_input_frames(&repository.0, 3);
        let output_directory = repository.0.join("empty-batch-out");
        let request_path = repository.0.join("empty-request.json");
        std::fs::write(
            &request_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "plugin": plugin.to_string_lossy(),
                "input_frames": inputs,
                "output_directory": output_directory.to_string_lossy(),
                // Only a SmartFX session may report an empty result.
                "smart": true,
            }))
            .unwrap(),
        )
        .unwrap();
        let report_path = repository.0.join("empty-report.json");
        let passed =
            run_video_batch(&repository.0, &request_path, &report_path).expect("batch render runs");
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert!(passed, "the empty frame must not abort the batch: {report}");
        assert_eq!(report["frame_count"], 3);
        assert_eq!(report["frames_ok"], 3);
        assert_eq!(report["aborted"], false);
        // Frame 0 rendered empty: legal, no PNG.
        let frame0 = &report["frames"][0];
        assert_eq!(frame0["status"], "ok");
        assert_eq!(frame0["empty_result"], true);
        assert_eq!(frame0["width"], 0);
        assert_eq!(frame0["output_png"], serde_json::Value::Null);
        assert!(
            !output_directory.join("frame-000000.png").exists(),
            "an empty frame writes no PNG"
        );
        // The remaining frames rendered normally with PNGs.
        for index in 1..3 {
            assert!(
                output_directory
                    .join(format!("frame-{index:06}.png"))
                    .is_file(),
                "non-empty frame {index} writes a PNG"
            );
        }
    }

    #[test]
    fn video_batch_empty_frame_rejects_a_stale_output_png() {
        if crate::common::skip_without_sealed_worker_launch(
            "video_batch_empty_frame_rejects_a_stale_output_png",
        ) {
            return;
        }
        // The empty-frame arm writes no PNG, but it must still honor the
        // fresh-output contract the non-empty arm enforces (#278): a stale
        // frame-*.png left in the output directory from a previous run would
        // otherwise keep old pixels on disk while the report claims the frame is
        // empty, so a directory glob would ingest the wrong frame. Reject it.
        let _behavior = BehaviorGuard::set(Some("empty_result_frame_0"));
        let (repository, plugin, _sha) = temp_repository();
        let inputs = write_input_frames(&repository.0, 3);
        let output_directory = repository.0.join("stale-batch-out");
        std::fs::create_dir_all(&output_directory).unwrap();
        // A leftover frame 0 from an earlier run, which frame 0 now renders empty.
        std::fs::write(output_directory.join("frame-000000.png"), b"stale").unwrap();
        let request_path = repository.0.join("stale-request.json");
        std::fs::write(
            &request_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "plugin": plugin.to_string_lossy(),
                "input_frames": inputs,
                "output_directory": output_directory.to_string_lossy(),
                "smart": true,
            }))
            .unwrap(),
        )
        .unwrap();
        let report_path = repository.0.join("stale-report.json");
        let passed = run_video_batch(&repository.0, &request_path, &report_path)
            .expect("the batch runs and writes a report");
        // The stale PNG aborts the batch (a failed frame), so it does not pass
        // and the empty frame is recorded as failed rather than silently ok.
        assert!(
            !passed,
            "an empty frame must not silently leave a stale PNG on disk"
        );
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(report["aborted"], true, "report: {report}");
        assert_eq!(report["frames"][0]["status"], "failed", "report: {report}");
    }

    #[test]
    fn video_batch_aborts_on_a_frame_error_by_default() {
        if crate::common::skip_without_sealed_worker_launch(
            "video_batch_aborts_on_a_frame_error_by_default",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("error_frame_0"));
        let (repository, plugin, _sha) = temp_repository();
        let inputs = write_input_frames(&repository.0, 2);
        let request_path = repository.0.join("request.json");
        std::fs::write(
            &request_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "plugin": plugin.to_string_lossy(),
                "input_frames": inputs,
                "output_directory": repository.0.join("batch-out").to_string_lossy(),
            }))
            .unwrap(),
        )
        .unwrap();
        let report_path = repository.0.join("report.json");
        let passed =
            run_video_batch(&repository.0, &request_path, &report_path).expect("batch render runs");
        assert!(!passed);
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert_eq!(report["aborted"], true);
        assert_eq!(report["frames_ok"], 0);
        assert_eq!(report["frames"][0]["status"], "error");
        assert_eq!(report["frames"][0]["render_error"], -40);
        // The frame-local error was recorded, the session itself closed clean.
        assert_eq!(report["session"]["invalidated"], false);
    }

    // -----------------------------------------------------------------------
    // Cluster session integration tests (issue #405,
    // docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md). The fixture worker speaks
    // swap_plugin/swap_done and the discovery session against the
    // cluster-manifest-v1 transport, so the whole protocol round trip is
    // exercised without a native minihost build.
    // -----------------------------------------------------------------------

    /// A temp repository with a two-plugin cluster (alpha/beta) sharing one
    /// closure dependency (helper.dll); each member's (path, sha256) pair.
    struct TempCluster {
        repository: TempRepository,
        plugins: Vec<(PathBuf, String)>,
        dependency: PathBuf,
    }

    fn approved_artifact(path: &Path) -> ApprovedImageArtifact {
        let bytes = std::fs::read(path).unwrap();
        ApprovedImageArtifact {
            path: path.to_path_buf(),
            expected_sha256: Sha256::digest(&bytes).into(),
            expected_size: bytes.len() as u64,
        }
    }

    fn temp_cluster_repository() -> TempCluster {
        let fixture = build_fixture();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-cluster-session-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        write_freshness_source_marker(&root);
        let worker_dir = root.join("target/minihost-build");
        std::fs::create_dir_all(&worker_dir).unwrap();
        std::fs::copy(&fixture, worker_dir.join("aex_render_worker.exe")).unwrap();
        let mut plugins = Vec::new();
        for (name, bytes) in [
            ("alpha.plugin", b"cluster plugin alpha" as &[u8]),
            ("beta.plugin", b"cluster plugin beta"),
        ] {
            let path = root.join(name);
            std::fs::write(&path, bytes).unwrap();
            plugins.push((path, format!("{:x}", Sha256::digest(bytes))));
        }
        let dependency = root.join("helper.dll");
        std::fs::write(&dependency, b"cluster shared dependency").unwrap();
        TempCluster {
            repository: TempRepository(root),
            plugins,
            dependency,
        }
    }

    fn open_cluster_render_session(cluster: &TempCluster) -> RenderSession {
        RenderSession::open_cluster(
            SessionOpenRequest {
                repository: &cluster.repository.0,
                plugin_path: &cluster.plugins[0].0,
                plugin_sha256: &cluster.plugins[0].1,
                parameters: None,
                payload_override: None,
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
                // The shared closure rides the base request's dependencies.
                dependencies: vec![approved_artifact(&cluster.dependency)],
                dependency_search_dirs: Vec::new(),
                width: WIDTH,
                height: HEIGHT,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
                frame_deadline: Duration::from_secs(30),
                smart: false,
                gpu_backend: RenderGpuBackend::Cpu,
                gpu_runtime_policy: None,
            },
            ClusterRenderPlugins {
                plugins: cluster
                    .plugins
                    .iter()
                    .map(|(path, _)| approved_artifact(path))
                    .collect(),
                swap_payloads: vec![None, Some("v2|0=2.0".to_owned())],
                module_bound: 64,
            },
        )
        .expect("open cluster render session")
    }

    fn open_discovery_session(cluster: &TempCluster) -> DiscoverySession {
        DiscoverySession::open_in_place(InPlaceDiscoverySessionOpenRequest {
            repository: &cluster.repository.0,
            plugins: cluster
                .plugins
                .iter()
                .map(|(path, _)| approved_artifact(path))
                .collect(),
            dependency_search_dirs: vec![cluster.dependency.parent().unwrap().to_path_buf()],
            module_bound: 64,
            inspect_deadline: Duration::from_secs(30),
        })
        .expect("open discovery session")
    }

    #[test]
    fn cluster_render_session_swaps_plugins_and_closes_clean() {
        if crate::common::skip_without_sealed_worker_launch(
            "cluster_render_session_swaps_plugins_and_closes_clean",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let cluster = temp_cluster_repository();
        let mut session = open_cluster_render_session(&cluster);

        let outcome = session
            .render_frame(0, 0, &input_pattern(3))
            .expect("frame 0 renders on plugins[0]");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));

        // The swap selects a manifest member by index only.
        let swap = session.swap_plugin(1).expect("swap to plugins[1]");
        assert!(matches!(swap, SwapOutcome::Swapped));

        let outcome = session
            .render_frame(1, 1, &input_pattern(9))
            .expect("frame 1 renders on plugins[1]");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));

        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["invalidated"], false, "close: {close}");
        assert_eq!(close["frames_ok"], 2, "close: {close}");
        // The swap rode the manifest: the final report's cluster module
        // audit records the epoch and stays inside the declared set.
        let epochs = close["final_report"]["module_audit"]["epochs"]
            .as_array()
            .expect("cluster audit carries epochs");
        assert_eq!(epochs.len(), 1, "close: {close}");
        assert_eq!(epochs[0]["plugin_index"], 0);
    }

    /// In-place cluster render session (issue #751): the v2 manifest names
    /// the cluster by real paths, the launch admits the search directories,
    /// and the swap selects members by index over the same transport as the
    /// sealed session.
    #[test]
    fn in_place_cluster_render_session_swaps_plugins_and_closes_clean() {
        if crate::common::skip_without_sealed_worker_launch(
            "in_place_cluster_render_session_swaps_plugins_and_closes_clean",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let cluster = temp_cluster_repository();
        let mut session = RenderSession::open_cluster(
            SessionOpenRequest {
                repository: &cluster.repository.0,
                plugin_path: &cluster.plugins[0].0,
                plugin_sha256: &cluster.plugins[0].1,
                parameters: None,
                payload_override: None,
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
                dependency_search_dirs: vec![cluster.repository.0.clone()],
                width: WIDTH,
                height: HEIGHT,
                pixel_format: RenderPixelFormat::Argb8,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
                frame_deadline: Duration::from_secs(30),
                smart: false,
                gpu_backend: RenderGpuBackend::Cpu,
                gpu_runtime_policy: None,
            },
            ClusterRenderPlugins {
                plugins: cluster
                    .plugins
                    .iter()
                    .map(|(path, _)| approved_artifact(path))
                    .collect(),
                swap_payloads: vec![None, Some("v2|0=2.0".to_owned())],
                module_bound: 4096,
            },
        )
        .expect("open in-place cluster render session");

        let outcome = session
            .render_frame(0, 0, &input_pattern(3))
            .expect("frame 0 renders on plugins[0]");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let swap = session.swap_plugin(1).expect("swap to plugins[1]");
        assert!(matches!(swap, SwapOutcome::Swapped));
        let outcome = session
            .render_frame(1, 1, &input_pattern(9))
            .expect("frame 1 renders on plugins[1]");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["invalidated"], false, "close: {close}");
        assert_eq!(close["frames_ok"], 2, "close: {close}");
    }

    #[test]
    fn cluster_swap_rejects_out_of_manifest_and_current_index_as_caller_errors() {
        if crate::common::skip_without_sealed_worker_launch(
            "cluster_swap_rejects_out_of_manifest_and_current_index_as_caller_errors",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let cluster = temp_cluster_repository();
        let mut session = open_cluster_render_session(&cluster);

        // An index outside the manifest, and the current index, are caller
        // errors rejected before anything is sent; the session stays usable.
        assert!(session.swap_plugin(2).is_err());
        assert!(session.swap_plugin(0).is_err());
        let outcome = session
            .render_frame(0, 0, &input_pattern(5))
            .expect("the session still renders after rejected swaps");
        assert!(matches!(outcome.status, FrameStatus::Rendered { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn cluster_swap_done_mismatch_invalidates_the_session() {
        if crate::common::skip_without_sealed_worker_launch(
            "cluster_swap_done_mismatch_invalidates_the_session",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("swap_done_wrong_index"));
        let cluster = temp_cluster_repository();
        let mut session = open_cluster_render_session(&cluster);
        session
            .render_frame(0, 0, &input_pattern(3))
            .expect("frame 0 renders");
        let error = session
            .swap_plugin(1)
            .expect_err("a mismatched swap_done is a protocol violation");
        assert!(error.to_string().contains("swap_done_mismatch"), "{error}");
        assert_eq!(
            session
                .invalidation()
                .map(|invalidation| invalidation.reason),
            Some("swap_done_mismatch")
        );
        let close = session.close();
        assert_eq!(close["invalidated"], true, "close: {close}");
    }

    #[test]
    fn cluster_swap_worker_death_is_detected_by_the_three_way_wait() {
        if crate::common::skip_without_sealed_worker_launch(
            "cluster_swap_worker_death_is_detected_by_the_three_way_wait",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("crash_on_swap"));
        let cluster = temp_cluster_repository();
        let mut session = open_cluster_render_session(&cluster);
        session
            .render_frame(0, 0, &input_pattern(3))
            .expect("frame 0 renders");
        let error = session
            .swap_plugin(1)
            .expect_err("a worker dying mid-swap must fail the swap");
        assert!(error.to_string().contains("worker_exited"), "{error}");
        let close = session.close();
        assert_eq!(close["invalidated"], true, "close: {close}");
    }

    #[test]
    fn cluster_swap_global_setup_error_is_plugin_local() {
        if crate::common::skip_without_sealed_worker_launch(
            "cluster_swap_global_setup_error_is_plugin_local",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("swap_global_setup_error"));
        let cluster = temp_cluster_repository();
        let mut session = open_cluster_render_session(&cluster);
        let swap = session.swap_plugin(1).expect("the swap exchange completes");
        let SwapOutcome::PluginError { global_setup_error } = swap else {
            panic!("expected a plugin-local GLOBAL_SETUP error, got {swap:?}");
        };
        assert_eq!(global_setup_error, 25);
        // The session continues; the caller decided to keep using it.
        let close = session.close();
        assert_eq!(close["invalidated"], false, "close: {close}");
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn cluster_close_records_an_audit_module_outside_the_declared_set() {
        if crate::common::skip_without_sealed_worker_launch(
            "cluster_close_records_an_audit_module_outside_the_declared_set",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("audit_undeclared_module"));
        let cluster = temp_cluster_repository();
        let mut session = open_cluster_render_session(&cluster);
        session
            .render_frame(0, 0, &input_pattern(3))
            .expect("frame 0 renders");
        session.swap_plugin(1).expect("swap to plugins[1]");
        let close = session.close();
        // The observed union carried a module the manifest never declared.
        // Since issue #730 that is a recorded observation on the close report,
        // not an invalidation: the module list explains the frames, it does
        // not decide whether they were valid.
        assert_eq!(close["invalidated"], false, "close: {close}");
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert!(
            close["module_audit_warning"]
                .as_str()
                .is_some_and(|warning| !warning.is_empty()),
            "close: {close}"
        );
    }

    #[test]
    fn discovery_session_inspects_every_cluster_plugin() {
        if crate::common::skip_without_sealed_worker_launch(
            "discovery_session_inspects_every_cluster_plugin",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let cluster = temp_cluster_repository();
        let mut session = open_discovery_session(&cluster);

        let first = session.inspect_plugin(0, 0).expect("inspect plugins[0]");
        let InspectOutcome::Inspected { report } = first else {
            panic!("inspect plugins[0] errored: {first:?}");
        };
        assert_eq!(report["plugin"]["basename"], "alpha.plugin");
        assert_eq!(report["plugin"]["sha256"], cluster.plugins[0].1);

        let second = session.inspect_plugin(1, 1).expect("inspect plugins[1]");
        let InspectOutcome::Inspected { report } = second else {
            panic!("inspect plugins[1] errored: {second:?}");
        };
        assert_eq!(report["plugin"]["basename"], "beta.plugin");

        // Re-inspecting the current plugin is legal (design §4.2).
        let again = session.inspect_plugin(1, 2).expect("re-inspect plugins[1]");
        assert!(matches!(again, InspectOutcome::Inspected { .. }));

        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["inspects_ok"], 3, "close: {close}");
        // The inspect swap rode the manifest: one epoch, declared-set audit.
        let epochs = close["final_report"]["module_audit"]["epochs"]
            .as_array()
            .expect("cluster audit carries epochs");
        assert_eq!(epochs.len(), 1, "close: {close}");
        assert_eq!(epochs[0]["plugin_index"], 0);
    }

    /// In-place discovery session (issue #751, cluster-manifest-v2): the
    /// manifest names each plug-in by its real path and the search
    /// directories ride the manifest instead of a pinned closure; the
    /// exchanges and the close contract match the sealed session.
    #[test]
    fn in_place_discovery_session_inspects_by_real_path() {
        if crate::common::skip_without_sealed_worker_launch(
            "in_place_discovery_session_inspects_by_real_path",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let cluster = temp_cluster_repository();
        let mut session = DiscoverySession::open_in_place(InPlaceDiscoverySessionOpenRequest {
            repository: &cluster.repository.0,
            plugins: cluster
                .plugins
                .iter()
                .map(|(path, _)| approved_artifact(path))
                .collect(),
            dependency_search_dirs: vec![cluster.repository.0.clone()],
            module_bound: 64,
            inspect_deadline: Duration::from_secs(30),
        })
        .expect("open in-place discovery session");

        let first = session.inspect_plugin(0, 0).expect("inspect plugins[0]");
        let InspectOutcome::Inspected { report } = first else {
            panic!("inspect plugins[0] errored: {first:?}");
        };
        assert_eq!(report["plugin"]["basename"], "alpha.plugin");
        assert_eq!(report["plugin"]["sha256"], cluster.plugins[0].1);
        let second = session.inspect_plugin(1, 1).expect("inspect plugins[1]");
        let InspectOutcome::Inspected { report } = second else {
            panic!("inspect plugins[1] errored: {second:?}");
        };
        assert_eq!(report["plugin"]["basename"], "beta.plugin");

        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["inspects_ok"], 2, "close: {close}");
        assert_eq!(close["invalidated"], false, "close: {close}");
    }

    /// An in-place identity mismatch (issue #751, the #309 state transition)
    /// is plug-in-local: the structured `identity_changed` reaches the caller
    /// and the session keeps serving other members.
    #[test]
    fn in_place_discovery_identity_change_is_plugin_local() {
        if crate::common::skip_without_sealed_worker_launch(
            "in_place_discovery_identity_change_is_plugin_local",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("inspect_identity_changed_plugin_1"));
        let cluster = temp_cluster_repository();
        let mut session = DiscoverySession::open_in_place(InPlaceDiscoverySessionOpenRequest {
            repository: &cluster.repository.0,
            plugins: cluster
                .plugins
                .iter()
                .map(|(path, _)| approved_artifact(path))
                .collect(),
            dependency_search_dirs: vec![cluster.repository.0.clone()],
            module_bound: 64,
            inspect_deadline: Duration::from_secs(30),
        })
        .expect("open in-place discovery session");
        let first = session.inspect_plugin(0, 0).expect("inspect plugins[0]");
        assert!(matches!(first, InspectOutcome::Inspected { .. }));
        let second = session.inspect_plugin(1, 1).expect("inspect plugins[1]");
        let InspectOutcome::InspectError { error_kind, .. } = second else {
            panic!("plugins[1] must report the identity change: {second:?}");
        };
        assert_eq!(error_kind, "identity_changed");
        // The session survives the mismatch and keeps serving.
        let again = session.inspect_plugin(0, 2).expect("re-inspect plugins[0]");
        assert!(matches!(again, InspectOutcome::Inspected { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["inspects_errored"], 1, "close: {close}");
    }

    #[test]
    fn discovery_session_rejects_out_of_manifest_index_and_off_serial_requests() {
        if crate::common::skip_without_sealed_worker_launch(
            "discovery_session_rejects_out_of_manifest_index_and_off_serial_requests",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(None);
        let cluster = temp_cluster_repository();
        let mut session = open_discovery_session(&cluster);

        // Both are caller errors rejected before anything is sent; the
        // session stays usable.
        assert!(session.inspect_plugin(2, 0).is_err());
        assert!(session.inspect_plugin(0, 5).is_err());
        let outcome = session.inspect_plugin(0, 0).expect("inspect plugins[0]");
        assert!(matches!(outcome, InspectOutcome::Inspected { .. }));
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn discovery_session_reports_parameter_local_error_and_continues() {
        if crate::common::skip_without_sealed_worker_launch(
            "discovery_session_reports_parameter_local_error_and_continues",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("inspect_error_plugin_1"));
        let cluster = temp_cluster_repository();
        let mut session = open_discovery_session(&cluster);
        let first = session.inspect_plugin(0, 0).expect("inspect plugins[0]");
        assert!(matches!(first, InspectOutcome::Inspected { .. }));
        let second = session
            .inspect_plugin(1, 1)
            .expect("the exchange completes");
        let InspectOutcome::InspectError { error_kind, report } = second else {
            panic!("expected a parameter-local inspect error, got {second:?}");
        };
        assert_eq!(error_kind, "selector_error");
        assert!(report.is_none());
        let close = session.close();
        assert_eq!(close["inspects_ok"], 1, "close: {close}");
        assert_eq!(close["inspects_errored"], 1, "close: {close}");
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn discovery_session_worker_death_is_detected_by_the_three_way_wait() {
        if crate::common::skip_without_sealed_worker_launch(
            "discovery_session_worker_death_is_detected_by_the_three_way_wait",
        ) {
            return;
        }
        let _behavior = BehaviorGuard::set(Some("crash_on_inspect"));
        let cluster = temp_cluster_repository();
        let mut session = open_discovery_session(&cluster);
        let error = session
            .inspect_plugin(0, 0)
            .expect_err("a worker dying mid-inspect must fail the request");
        assert!(error.to_string().contains("worker_exited"), "{error}");
        let close = session.close();
        assert_eq!(close["invalidated"], true, "close: {close}");
    }
}
