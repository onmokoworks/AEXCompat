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
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let plugin = repository.join("target/minihost-build/worker_independent_case_fixture.aex");
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
        mask_trailer: None,
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
        smart: false,
        gpu_backend: RenderGpuBackend::Cpu,
        gpu_runtime_policy: None,
        launch_environment: Default::default(),
    })
    .unwrap()
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
