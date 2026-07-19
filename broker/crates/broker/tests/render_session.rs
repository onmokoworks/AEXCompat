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
    use aexcompat_broker::image_render::RenderPixelFormat;
    use aexcompat_broker::render_session::{
        run_video_batch, FrameStatus, RenderSession, SessionOpenRequest,
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
        std::fs::copy(fixture, worker_dir.join("aex_render_worker.exe")).unwrap();
        let plugin = root.join("plugin.aex");
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
            dependencies: Vec::new(),
            width: WIDTH,
            height: HEIGHT,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            frame_deadline,
        })
        .expect("open render session")
    }

    fn input_pattern(seed: u8) -> Vec<u8> {
        (0..WIDTH * HEIGHT * 4)
            .map(|index| seed.wrapping_add(index as u8))
            .collect()
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
                FrameStatus::Rendered { pixels, checksum } => {
                    let expected: Vec<u8> = input.iter().map(|byte| 255 - byte).collect();
                    assert_eq!(pixels, expected, "slot transfer round-trips the render");
                    checksums.push(checksum);
                }
                FrameStatus::FrameError { render_error } => {
                    panic!("frame {frame_index} unexpectedly errored: {render_error}")
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
        let request_path = repository.0.join("request.json");
        std::fs::write(
            &request_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "plugin": plugin.to_string_lossy(),
                "input_frames": inputs,
                "output_directory": output_directory.to_string_lossy(),
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
