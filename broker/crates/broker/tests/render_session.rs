//! Broker `RenderSession` integration tests (issue #98 stage 1 PR-C).
//!
//! Every test drives a real isolated process (restricted token, sealed tree,
//! kill-on-close Job Object) through the session transport. The worker side is
//! the `session_protocol_worker` fixture, which speaks
//! docs/RENDER_SESSION_PROTOCOL_2026-07-19.md faithfully and can misbehave on
//! demand, so per-frame validation, the frame-deadline watchdog, and crash
//! invalidation are exercised without a native minihost build.

#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{
        InteractiveParameter, ParameterAnimation, RenderGpuBackend, RenderPixelFormat,
    };
    use aexcompat_broker::render_session::{
        run_video_batch, AudioRenderSession, AudioSessionOpenRequest, AudioSpanStatus, FrameStatus,
        RenderSession, SessionLayer, SessionOpenRequest,
    };
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
            let guard = BEHAVIOR_LOCK.lock().unwrap_or_else(|error| error.into_inner());
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
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &[],
            dependencies: Vec::new(),
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

    fn build_audio_fixture() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(manifest)
            .args(["-p", "dummy-workers", "--bin", "audio_session_protocol_worker"])
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
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_audio_repository();
        let mut session = AudioRenderSession::open(AudioSessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            dependencies: Vec::new(),
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

    /// The broker independently bounds the reported output window against the
    /// submitted input span (Codex #252): a worker that reports
    /// start_sample + sample_count past input.len() is rejected as a
    /// host-protection invariant breach, not published as a valid span.
    #[test]
    fn audio_session_rejects_out_of_range_output_start() {
        let _behavior = BehaviorGuard::set(Some("audio_out_of_range_start"));
        let (repository, plugin, sha) = temp_audio_repository();
        let mut session = AudioRenderSession::open(AudioSessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            dependencies: Vec::new(),
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
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            layers: &[],
            alpha_as_coverage_params: &[],
            dependencies: Vec::new(),
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

    #[test]
    fn smart_session_with_an_explicit_gpu_backend_requires_a_policy() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            layers: &[],
            alpha_as_coverage_params: &[],
            dependencies: Vec::new(),
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
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            layers: &[],
            alpha_as_coverage_params: &[],
            dependencies: Vec::new(),
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
            },
            SessionLayer {
                slot: 7,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![7u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
            },
        ];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &layers,
            dependencies: Vec::new(),
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

    #[test]
    fn timed_layers_travel_the_session_trailer_into_their_slots() {
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
            },
            SessionLayer {
                slot: 5,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![5u8; (WIDTH * HEIGHT * 4) as usize],
                timed: Some((7, 30)),
            },
            SessionLayer {
                slot: 9,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![9u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
            },
        ];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &layers,
            dependencies: Vec::new(),
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
            },
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: Some((2, 60)),
            },
        ];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &layers,
            dependencies: Vec::new(),
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
            },
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: Some((7, 30)),
            },
        ];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &layers,
            dependencies: Vec::new(),
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
            },
            SessionLayer {
                slot: 4,
                width: WIDTH,
                height: HEIGHT,
                rgba: vec![4u8; (WIDTH * HEIGHT * 4) as usize],
                timed: None,
            },
        ];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &layers,
            dependencies: Vec::new(),
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
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[0, 3],
            layers: &[],
            dependencies: Vec::new(),
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
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[1025],
            layers: &[],
            dependencies: Vec::new(),
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
    fn open_rejects_layer_pixels_that_do_not_fit_the_slot() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let layers = vec![SessionLayer {
            slot: 3,
            width: WIDTH,
            height: HEIGHT,
            // One byte short of the declared geometry.
            rgba: vec![3u8; (WIDTH * HEIGHT * 4 - 1) as usize],
            timed: None,
        }];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &layers,
            dependencies: Vec::new(),
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
        assert!(error.to_string().contains("do not fit"), "{error}");
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
        }];
        let error = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: None,
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &layers,
            dependencies: Vec::new(),
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

    #[test]
    fn animation_sidecar_rides_the_session_and_is_cleaned_up() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let parameters = [float_parameter(1)];
        let animations = [scalar_animation(1)];
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &repository.0,
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: Some(&parameters),
            parameter_animation: Some(&animations),
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &[],
            dependencies: Vec::new(),
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
            parameter_animation: Some(&animations),
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &[],
            dependencies: Vec::new(),
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
            parameter_animation: None,
            aux_manifest: Some(&manifest),
            world_dump_dir: Some(&dump_dir),
            output_checksum_detail: true,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &[],
            dependencies: Vec::new(),
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
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: Some(&reused),
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &[],
            dependencies: Vec::new(),
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
            parameter_animation: None,
            aux_manifest: None,
            world_dump_dir: Some(&missing),
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &[],
            dependencies: Vec::new(),
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
            parameter_animation: Some(&animations),
            aux_manifest: None,
            world_dump_dir: None,
            output_checksum_detail: false,
            mask_trailer: None,
            spatial_trailer: None,
            render_environment_trailer: None,
            alpha_as_coverage_params: &[],
            layers: &[],
            dependencies: Vec::new(),
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
                FrameStatus::Rendered { pixels, checksum, .. } => {
                    let expected: Vec<u8> = input.iter().map(|byte| 255 - byte).collect();
                    assert_eq!(pixels, expected, "slot transfer round-trips the render");
                    checksums.push(checksum);
                }
                FrameStatus::FrameError { render_error } => {
                    panic!("frame {frame_index} unexpectedly errored: {render_error}")
                }
                FrameStatus::ResizeNeeded { width, height } => {
                    panic!("frame {frame_index} unexpectedly resized to {width}x{height}")
                }
            }
        }
        assert_ne!(checksums[0], checksums[1], "distinct inputs produce distinct outputs");
        let close = session.close();
        assert_eq!(close["frames_ok"], 2);
        assert_eq!(close["invalidated"], false);
        assert_eq!(close["session_clean"], true, "close: {close}");
        assert_eq!(close["worker"]["classification"], "ok");
        assert_eq!(close["final_report"]["session_frames"], 2);
    }

    #[test]
    fn per_frame_parameters_ride_the_v2_message_and_reach_the_worker() {
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let input = input_pattern(31);
        let inverted: Vec<u8> = input.iter().map(|byte| 255 - byte).collect();

        // Frame 0 stays a v:1 message: plain inverted transfer.
        let outcome = session.render_frame(0, 0, &input).expect("v1 frame renders");
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
        let outcome = session.render_frame(2, 2, &input).expect("v1 frame renders again");
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
        use aexcompat_broker::image_render::RenderUiAction;
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let input = input_pattern(29);
        let inverted: Vec<u8> = input.iter().map(|byte| 255 - byte).collect();

        // Frame 0 stays a v:1 message: plain inverted transfer, no stamp.
        let outcome = session.render_frame(0, 0, &input).expect("v1 frame renders");
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
        assert_eq!(&pixels[32..64], Sha256::digest(encoded.as_bytes()).as_slice());
        // The parameters region stays untouched, and everything past the stamp
        // is the plain inverted transfer.
        assert_eq!(&pixels[..32], &inverted[..32]);
        assert_eq!(&pixels[64..], &inverted[64..]);

        // Frame 2 carries both parameters and ui_action in one v:2 message,
        // proving the attributes co-exist (§4.2.1). Both digests are stamped.
        let mut updated = float_parameter(1);
        updated.value = 42.5;
        let outcome = session
            .render_frame_with_attributes(2, 2, &input, Some(std::slice::from_ref(&updated)), Some(&action))
            .expect("v2 combined frame renders");
        let FrameStatus::Rendered { pixels, .. } = outcome.status else {
            panic!("v2 combined frame errored");
        };
        let payload = aexcompat_broker::image_render::encode_interactive_payload(
            std::slice::from_ref(&updated),
        )
        .expect("payload encodes");
        assert_eq!(&pixels[..32], Sha256::digest(payload.as_bytes()).as_slice());
        assert_eq!(&pixels[32..64], Sha256::digest(encoded.as_bytes()).as_slice());

        // Frame 3 reverts to v:1: both stamps disappear (attributes were
        // frame-scoped, not sticky).
        let outcome = session.render_frame(3, 3, &input).expect("v1 frame renders again");
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
        use aexcompat_broker::image_render::{InteractiveRenderSession, InteractiveSessionOpen};
        let _behavior = BehaviorGuard::set(None);
        let (repository, plugin, sha) = temp_repository();
        let parameters = [float_parameter(1)];
        let mut session = InteractiveRenderSession::open(InteractiveSessionOpen {
            repository: &repository.0,
            plugin_id: "experimental",
            plugin_path: &plugin,
            plugin_sha256: &sha,
            parameters: Some(&parameters),
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

        let input = input_pattern(53);
        let mut updated = float_parameter(1);
        updated.value = 7.5;
        for (frame, time) in [(0u32, 0i32), (1, 1)] {
            let output = repository.0.join(format!("live-{frame}.png"));
            let report = session
                .render(&input, time, Some(std::slice::from_ref(&updated)), &output)
                .expect("interactive frame renders");
            assert_eq!(report["stage"], "interactive_image_render");
            assert_eq!(report["render_path"], "classic");
            assert_eq!(report["worker_classification"], "resident_session");
            assert_eq!(report["passed"], true);
            assert_eq!(report["resident_session"]["frame_index"], frame);
            assert_eq!(report["resident_session"]["parameter_update"], true);
            assert!(output.is_file(), "preview PNG is written per frame");
        }
        let close = session.close();
        assert_eq!(close["frames_ok"], 2);
        assert_eq!(close["parameter_update_frames"], 2);
        assert_eq!(close["session_clean"], true, "close: {close}");
    }

    #[test]
    fn rejected_per_frame_parameters_leave_the_session_usable() {
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
        let _behavior = BehaviorGuard::set(Some("error_frame_0"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let outcome = session
            .render_frame(0, 0, &input_pattern(1))
            .expect("frame-local errors do not invalidate the session");
        assert!(matches!(
            outcome.status,
            FrameStatus::FrameError { render_error: -40 }
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
    fn frame_deadline_watchdog_terminates_the_job() {
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
    fn worker_crash_invalidates_the_session_with_diagnostics() {
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
        assert_eq!(close["worker"]["diagnostics"]["minidump"], "written bytes=4096");

        // Exactly one dump is finalized at close; the broker withholds the
        // completion marker from the published file.
        let dumps: Vec<PathBuf> = std::fs::read_dir(&dump_dir)
            .expect("managed dump directory exists")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "dmp"))
            .collect();
        assert_eq!(dumps.len(), 1, "exactly one finalized dump: {dumps:?}");
        let bytes = std::fs::read(&dumps[0]).unwrap();
        assert!(bytes.starts_with(b"MDMP"), "published dump keeps the streamed body");
        assert!(!bytes.ends_with(b"AEXDUMP-COMPLETE"), "marker withheld from the file");
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
        let _behavior = BehaviorGuard::set(Some("fatal_error_frame_0"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(12))
            .expect_err("a reserved fatal error code must not read as frame-local");
        assert!(error.to_string().contains("worker_invariant_failure"), "{error}");
        let close = session.close();
        assert_eq!(close["invalidated"], true);
        assert_eq!(close["invalidated_reason"]["reason"], "worker_invariant_failure");
        assert_eq!(close["session_clean"], false);
    }

    #[test]
    fn a_framing_violation_from_a_live_worker_invalidates_promptly() {
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
        assert!(
            error.to_string().contains("does not advance"),
            "{error}"
        );
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
        let _behavior = BehaviorGuard::set(Some("error_mutates_header"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(10))
            .expect_err("a header mutation must not hide behind an error response");
        assert!(error.to_string().contains("frame_invariant_failure"), "{error}");
        assert_eq!(session.close()["invalidated"], true);
    }

    #[test]
    fn a_unilateral_worker_exit_breaks_the_close_handshake_contract() {
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
        let _behavior = BehaviorGuard::set(Some("stale_generation"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(6))
            .expect_err("a stale generation must invalidate the session");
        assert!(error.to_string().contains("frame_invariant_failure"), "{error}");
        assert_eq!(session.close()["invalidated"], true);
    }

    #[test]
    fn mutated_static_header_is_fail_closed() {
        let _behavior = BehaviorGuard::set(Some("mutate_header"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(7))
            .expect_err("a mutated broker-owned header must invalidate the session");
        assert!(error.to_string().contains("frame_invariant_failure"), "{error}");
        assert_eq!(session.close()["invalidated"], true);
    }

    #[test]
    fn output_checksum_mismatch_is_fail_closed() {
        let _behavior = BehaviorGuard::set(Some("bad_checksum"));
        let (repository, plugin, sha) = temp_repository();
        let mut session = open_session(&repository.0, &plugin, &sha, Duration::from_secs(30));
        let error = session
            .render_frame(0, 0, &input_pattern(8))
            .expect_err("a checksum mismatch must invalidate the session");
        assert!(error.to_string().contains("output_checksum_mismatch"), "{error}");
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
        let passed = run_video_batch(&repository.0, &request_path, &report_path)
            .expect("batch render runs");
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
        assert!(passed, "report: {report}");
        assert_eq!(report["frame_count"], 3);
        assert_eq!(report["frames_ok"], 3);
        assert_eq!(report["aborted"], false);
        assert_eq!(report["session"]["session_clean"], true);
        for index in 0..3 {
            assert!(
                output_directory.join(format!("frame-{index:06}.png")).is_file(),
                "output frame {index} exists"
            );
        }
    }

    #[test]
    fn video_batch_aborts_on_a_frame_error_by_default() {
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
        let passed = run_video_batch(&repository.0, &request_path, &report_path)
            .expect("batch render runs");
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
}
