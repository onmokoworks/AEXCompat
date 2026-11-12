#![cfg(windows)]
use aexcompat_broker::image_render::{InteractiveParameter, RenderGpuBackend, RenderPixelFormat};
use aexcompat_broker::render_session::{FrameStatus, RenderSession, SessionOpenRequest};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

fn open() -> RenderSession {
    open_deadline(Duration::from_secs(10))
}

fn open_deadline(deadline: Duration) -> RenderSession {
    open_route(deadline, false)
}

fn open_route(deadline: Duration, smart: bool) -> RenderSession {
    open_context(deadline, smart, None)
}

fn open_context(deadline: Duration, smart: bool, mask_trailer: Option<String>) -> RenderSession {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let plugin = repository.join(if smart {
        "target/minihost-build/worker_independent_smart_fixture.aex"
    } else {
        "target/minihost-build/worker_independent_case_fixture.aex"
    });
    let hash = format!(
        "{:x}",
        Sha256::digest(std::fs::read(&plugin).expect("build native case fixture"))
    );
    RenderSession::open(SessionOpenRequest {
        repository: &repository,
        plugin_path: &plugin,
        plugin_sha256: &hash,
        parameters: None,
        payload_override: None,
        parameter_animation: None,
        aux_manifest: None,
        world_dump_dir: None,
        output_checksum_detail: false,
        mask_trailer,
        spatial_trailer: None,
        camera_trailer: None,
        render_environment_trailer: None,
        audio_trailer: None,
        alpha_as_coverage_params: &[],
        conformance_render_settings: None,
        layers: &[],
        dependencies: vec![],
        companions: vec![],
        dependency_search_dirs: vec![plugin.parent().unwrap().into()],
        width: 8,
        height: 4,
        pixel_format: RenderPixelFormat::Argb8,
        time_step: 1,
        total_time: 30,
        time_scale: 30,
        frame_deadline: deadline,
        smart,
        gpu_backend: RenderGpuBackend::Cpu,
        gpu_runtime_policy: None,
        launch_environment: Default::default(),
    })
    .unwrap()
}

#[test]
fn independent_smart_error_is_not_retried() {
    let mut session = open_route(Duration::from_secs(10), true);
    session.require_independent_cases().unwrap();
    let input = vec![255; 8 * 4 * 4];
    for (index, (time, expected)) in [(0, 14), (22, 512), (23, 516)].into_iter().enumerate() {
        let status = session
            .render_case(index as u32, time, &input, &[])
            .unwrap()
            .status;
        assert!(
            matches!(status, FrameStatus::FrameError { render_error, .. } if render_error == expected),
            "{status:?}"
        );
        let report = session.take_case_report().unwrap();
        assert_eq!(report["smart_render_error"], expected, "{report}");
        assert_eq!(report["session_render_error"], expected, "{report}");
        assert_eq!(report["smart_render_attempts"], 1, "{report}");
        assert_eq!(report["gpu_fallback_used"], false, "{report}");
        assert_eq!(
            report["missing_suites"],
            serde_json::json!([{"name":"Independent smart selector 1","version":1}]),
            "selector count: {report}"
        );
    }
    let close = session.close();
    assert_eq!(close["frames_errored"], 3, "{close}");
}

#[test]
fn ordinary_smart_session_keeps_premiere_retry() {
    let mut session = open_route(Duration::from_secs(10), true);
    let input = vec![255; 8 * 4 * 4];
    let status = session.render_frame(0, 22, &input).unwrap().status;
    assert!(
        matches!(
            status,
            FrameStatus::FrameError {
                render_error: 7,
                ..
            }
        ),
        "{status:?}"
    );
    let close = session.close();
    assert_eq!(
        close["final_report"]["missing_suites"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "{close}"
    );
}

fn pixel(status: FrameStatus) -> Vec<u8> {
    match status {
        FrameStatus::Rendered { pixels, .. } => pixels[..4].to_vec(),
        other => panic!("expected pixels: {other:?}"),
    }
}

#[test]
fn independent_instances_reset_sequence_and_parameters_and_continue_after_error() {
    let mut session = open();
    let input = vec![255; 8 * 4 * 4];
    assert!(session.render_case(0, 0, &input, &[]).is_err());
    session.require_independent_cases().unwrap();
    let mut edit: InteractiveParameter = serde_json::from_value(serde_json::json!({
        "slot":1,"name":"Value","kind":"integer","minimum":0,"maximum":100,"value":51,
        "choices":[],"color":[255,0,0,0],"components":[0,0,0],"component_count":0,
        "layer_path":null,"enabled":true,"visible":true,"supervised":false,
        "debug_summary":null,"custom_ui_events":0,"control_size":[0,0]
    }))
    .unwrap();
    assert_eq!(
        pixel(
            session
                .render_case(0, 0, &input, &[edit.clone()])
                .unwrap()
                .status
        ),
        [1, 51, 1, 255]
    );
    edit.value = 99.0;
    assert_eq!(
        pixel(session.render_case(1, 0, &input, &[]).unwrap().status),
        [1, 17, 1, 255]
    );
    assert!(matches!(
        session.render_case(2, 13, &input, &[edit]).unwrap().status,
        FrameStatus::FrameError {
            render_error: 7,
            ..
        }
    ));
    assert_eq!(
        pixel(session.render_case(3, 0, &input, &[]).unwrap().status),
        [1, 17, 1, 255]
    );
    let close = session.close();
    assert_eq!(close["session_clean"], true, "{close}");
    assert_eq!(close["frames_ok"], 3);
    assert_eq!(close["frames_errored"], 1);
    let mut session = open();
    session.require_independent_cases().unwrap();
    assert!(matches!(
        session.render_case(0, 13, &input, &[]).unwrap().status,
        FrameStatus::FrameError {
            render_error: 7,
            ..
        }
    ));
    assert_eq!(session.close()["session_clean"], true);
}

#[test]
fn selector_fault_and_deadline_invalidate_without_reopening() {
    for (time, deadline) in [
        (15, Duration::from_secs(10)),
        (16, Duration::from_millis(150)),
        (18, Duration::from_secs(10)),
        (19, Duration::from_secs(10)),
    ] {
        let mut session = open_deadline(deadline);
        session.require_independent_cases().unwrap();
        let input = vec![255; 8 * 4 * 4];
        assert!(session.render_case(0, time, &input, &[]).is_err());
        assert!(session.invalidation().is_some());
        assert!(session.render_case(1, 0, &input, &[]).is_err());
        assert_ne!(session.close()["session_clean"], true);
    }
}

#[test]
fn persistent_frames_keep_sequence_and_failed_setdown_poison_cases() {
    let input = vec![255; 8 * 4 * 4];
    let mut session = open();
    assert_eq!(
        pixel(session.render_frame(0, 0, &input).unwrap().status),
        [1, 17, 1, 255]
    );
    assert_eq!(
        pixel(session.render_frame(1, 0, &input).unwrap().status),
        [2, 17, 1, 255]
    );
    assert_eq!(session.close()["session_clean"], true);
    let mut session = open();
    session.require_independent_cases().unwrap();
    assert!(session.render_case(0, 14, &input, &[]).is_err());
    assert!(session.invalidation().is_some());
    assert!(session.render_case(1, 0, &input, &[]).is_err());
    assert_ne!(session.close()["session_clean"], true);
}

#[test]
fn aegp_memory_leak_invalidates_before_next_case() {
    let mut session = open();
    session.require_independent_cases().unwrap();
    let input = vec![255; 8 * 4 * 4];
    assert!(session.render_case(0, 20, &input, &[]).is_err());
    assert!(session.invalidation().is_some());
    assert!(session.render_case(1, 0, &input, &[]).is_err());
    assert_ne!(session.close()["session_clean"], true);
}

#[test]
fn compatibility_diagnostics_are_local_to_each_case() {
    let mut session = open();
    session.require_independent_cases().unwrap();
    let input = vec![255; 8 * 4 * 4];
    for index in 0..2 {
        session.render_case(index * 2, 21, &input, &[]).unwrap();
        let failed = session.take_case_report().unwrap();
        assert!(
            !failed["missing_suites"].as_array().unwrap().is_empty(),
            "{failed}"
        );
        assert!(
            !failed["unsupported_suite_calls"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{failed}"
        );
        assert_eq!(
            pixel(
                session
                    .render_case(index * 2 + 1, 0, &input, &[])
                    .unwrap()
                    .status
            ),
            [1, 17, 1, 255]
        );
        let good = session.take_case_report().unwrap();
        assert_eq!(good["missing_suites"], serde_json::json!([]), "{good}");
        assert_eq!(
            good["unsupported_suite_calls"],
            serde_json::json!([]),
            "{good}"
        );
    }
    let close = session.close();
    assert!(
        !close["final_report"]["missing_suites"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{close}"
    );
    assert_eq!(
        close["final_report"]["unsupported_suite_calls"][0]["call_count"], 2,
        "{close}"
    );
}

#[test]
fn pf_path_checkout_and_preparation_leaks_invalidate_next_case() {
    let input = vec![255; 8 * 4 * 4];
    for time in [24, 25, 26] {
        let mut session = open_context(
            Duration::from_secs(10),
            false,
            Some("v2|0:0,0,0,0,0,0/7,0,0,0,0,0/7,3,0,0,0,0/0,3,0,0,0,0".into()),
        );
        session.require_independent_cases().unwrap();
        let result = session.render_case(0, time, &input, &[]);
        if time == 26 {
            assert!(matches!(
                result.unwrap().status,
                FrameStatus::FrameError {
                    render_error: 7,
                    ..
                }
            ));
            assert_eq!(
                pixel(session.render_case(1, 0, &input, &[]).unwrap().status),
                [1, 17, 1, 255]
            );
            assert_eq!(session.close()["session_clean"], true);
        } else {
            assert!(result.is_err(), "leaked path operation at {time}");
            assert!(session.invalidation().is_some());
            assert!(session.render_case(1, 0, &input, &[]).is_err());
            assert_ne!(session.close()["session_clean"], true);
        }
    }
}

#[test]
fn scene_effect_leak_rejects_next_case() {
    let input = vec![255; 8 * 4 * 4];
    let mut session = open();
    session.require_independent_cases().unwrap();
    let result = session.render_case(0, 27, &input, &[]);
    assert!(result.is_err(), "undetected effect lease: {result:?}");
    assert!(session.invalidation().is_some());
    assert!(session.render_case(1, 28, &input, &[]).is_err());
    assert_ne!(session.close()["session_clean"], true);
}

#[test]
fn balanced_scene_effect_cases_can_reacquire() {
    let input = vec![255; 8 * 4 * 4];
    let mut session = open();
    session.require_independent_cases().unwrap();
    for index in 0..3 {
        assert_eq!(
            pixel(session.render_case(index, 28, &input, &[]).unwrap().status),
            [1, 17, 1, 255]
        );
    }
    assert_eq!(session.close()["session_clean"], true);
}

#[test]
fn automatic_audio_cleanup_allows_successful_next_case() {
    let input = vec![255; 8 * 4 * 4];
    let mut session = open();
    session.require_independent_cases().unwrap();
    for index in 0..2 {
        assert_eq!(
            pixel(session.render_case(index, 29, &input, &[]).unwrap().status),
            [1, 17, 1, 255]
        );
        let report = session.take_case_report().unwrap();
        assert_eq!(report["automatic_audio_checkins"], index + 1, "{report}");
        assert_eq!(report["audio_lifetimes_balanced"], true, "{report}");
    }
    assert_eq!(
        pixel(session.render_case(2, 0, &input, &[]).unwrap().status),
        [1, 17, 1, 255]
    );
    assert_eq!(session.close()["session_clean"], true);
}
