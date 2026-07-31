use super::*;

#[test]
fn session_geometry_slot_layout_matches_the_protocol() {
    let geometry = SessionGeometry {
        width: 33,
        height: 17,
        output_capacity_width: 33,
        output_capacity_height: 17,
        pixel_format: RenderPixelFormat::Argb16,
        layer_slot_count: 0,
    };
    assert_eq!(geometry.input_slot_bytes(), 33 * 17 * 4); // 2244
    assert_eq!(geometry.output_slot_bytes(), 33 * 17 * 8); // 4488
    // Slots are 4096-aligned after the one-page header (protocol §6).
    assert_eq!(geometry.output_slot_offset() % SLOT_ALIGNMENT, 0);
    assert_eq!(geometry.output_slot_offset(), 4096 + 4096);
    // The section holds only header + input + output (#268); layers travel
    // as inherited file handles, so layer_slot_count no longer sizes it.
    assert_eq!(geometry.section_bytes(), 4096 + 4096 + 8192);
}

#[test]
fn section_bytes_ignore_layer_count() {
    // Layer pixels left the section (#268), so a section with many layers is
    // byte-for-byte the same size as one with none: header + input + output.
    let base = SessionGeometry {
        width: 8,
        height: 8,
        output_capacity_width: 8,
        output_capacity_height: 8,
        pixel_format: RenderPixelFormat::Argb8,
        layer_slot_count: 0,
    };
    let with_layers = SessionGeometry {
        layer_slot_count: 64,
        ..base
    };
    assert_eq!(base.section_bytes(), with_layers.section_bytes());
    // header(4096) + align(8*8*4=256 -> 4096) + align(8*8*4=256 -> 4096).
    assert_eq!(base.section_bytes(), 4096 + 4096 + 4096);
}

#[test]
fn worst_case_expanded_section_stays_under_the_cap() {
    // The largest representable section is a max-dimension 32-bit-float
    // render whose output expands to the resize bound: header + input
    // (4096*4096*4 = 64 MiB) + output (4096*4096*16 = 256 MiB) ~= 320 MiB,
    // far under the 1 GiB hard cap, for any layer count (#268).
    let geometry = SessionGeometry {
        width: MAX_DIMENSION,
        height: MAX_DIMENSION,
        output_capacity_width: MAX_RESIZE_DIMENSION,
        output_capacity_height: MAX_RESIZE_DIMENSION,
        pixel_format: RenderPixelFormat::Argb32f,
        layer_slot_count: 64,
    };
    assert!(geometry.section_bytes() as u64 <= SECTION_HARD_CAP_BYTES);
}

#[test]
fn frame_done_parsing_is_strict_about_unknown_fields() {
    let ok: Result<FrameDone, _> = serde_json::from_str(
        r#"{"v":1,"type":"frame_done","frame_index":0,"status":"ok",
                "output":{"width":1,"height":1,"rowbytes":4,"pixel_format":"argb8",
                          "checksum":"00","guards_intact":true},
                "render_error":0,"generation":1}"#,
    );
    assert!(ok.is_ok());
    let unknown: Result<FrameDone, _> = serde_json::from_str(
        r#"{"v":1,"type":"frame_done","frame_index":0,"status":"ok",
                "render_error":0,"generation":1,"surprise":true}"#,
    );
    assert!(unknown.is_err());
    let error_shape: FrameDone = serde_json::from_str(
        r#"{"v":1,"type":"frame_done","frame_index":3,"status":"error","render_error":-40}"#,
    )
    .unwrap();
    assert!(error_shape.output.is_none());
    assert!(error_shape.generation.is_none());
    assert_eq!(error_shape.render_error, -40);
    assert!(error_shape.missing_dependency.is_none());
    let dependency_error: FrameDone = serde_json::from_str(
        r#"{"v":1,"type":"frame_done","frame_index":4,"status":"error",
                "render_error":-47,"missing_dependency":"fixture_delay.dll"}"#,
    )
    .unwrap();
    assert!(valid_dependency_basename(
        dependency_error.missing_dependency.as_deref().unwrap()
    ));
    assert!(!valid_dependency_basename("..\\escape.dll"));
}

#[test]
fn final_report_clean_fails_closed_on_missing_or_dirty_fields() {
    let clean = serde_json::json!({
        "status": "render_completed",
        "render_error": 0,
        "global_setdown_error": 0,
        "persistent_sequence_setup_error": 0,
        "persistent_sequence_setdown_error": 0,
        "guard_bytes_intact": true,
        "suite_leases_balanced": true,
        "suite_lease_warning": false,
        "suite_fault_observed": false,
        "suite_acquires": 2,
        "suite_releases": 2,
        "live_suite_lease_count": 0,
        "live_suite_leases": "",
        "handle_lifetimes_balanced": true,
        "world_lifetimes_balanced": true,
        "param_checkouts_balanced": true,
    });
    assert!(final_report_clean(&clean, false));
    for (key, dirty) in [
        // A protocol-violation or invariant-failure session loop ends
        // render_failed with render_error -1 even when the ledgers
        // balance; both fields must gate the clean verdict.
        ("status", serde_json::json!("render_failed")),
        ("render_error", serde_json::json!(-1)),
        // The worker's own exit gate does not include GLOBAL_SETDOWN, so
        // a teardown failure can hide behind exit 0; the broker gate must
        // catch it.
        ("global_setdown_error", serde_json::json!(25)),
        ("persistent_sequence_setup_error", serde_json::json!(25)),
        ("persistent_sequence_setdown_error", serde_json::json!(-1)),
        ("guard_bytes_intact", serde_json::json!(false)),
        ("suite_leases_balanced", serde_json::json!(false)),
        ("handle_lifetimes_balanced", serde_json::json!(false)),
        ("world_lifetimes_balanced", serde_json::json!(false)),
        ("param_checkouts_balanced", serde_json::json!(false)),
    ] {
        let mut report = clean.clone();
        report[key] = dirty;
        assert!(
            !final_report_clean(&report, false),
            "{key} must fail closed"
        );
        let mut missing = clean.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(
            !final_report_clean(&missing, false),
            "missing {key} must fail closed"
        );
    }
    // A parseable but unrelated report (an older worker) is not clean.
    assert!(!final_report_clean(
        &serde_json::json!({"status": "ok"}),
        false
    ));
}

#[test]
fn smart_final_report_clean_requires_the_session_fields() {
    let clean = serde_json::json!({
        "status": "render_completed",
        "global_setdown_error": 0,
        "guard_bytes_intact": true,
        "suite_leases_balanced": true,
        "suite_lease_warning": false,
        "suite_fault_observed": false,
        "suite_acquires": 2,
        "suite_releases": 2,
        "live_suite_lease_count": 0,
        "live_suite_leases": "",
        "handle_lifetimes_balanced": true,
        "world_lifetimes_balanced": true,
        "param_checkouts_balanced": true,
        "session_mode": true,
        "session_render_error": 0,
        "session_sequence_setup_error": 0,
        "session_sequence_setdown_error": 0,
    });
    assert!(final_report_clean(&clean, true));
    // The classic gate must not accept a smart report and vice versa:
    // each worker's session mechanics live under different keys, and a
    // missing key fails closed.
    assert!(!final_report_clean(&clean, false));
    for (key, dirty) in [
        ("session_mode", serde_json::json!(false)),
        // A clean close after frame-local errors keeps
        // session_render_error 0; -1 means the session mechanics broke.
        ("session_render_error", serde_json::json!(-1)),
        ("session_sequence_setup_error", serde_json::json!(25)),
        ("session_sequence_setdown_error", serde_json::json!(-1)),
        // The last rendered frame's selector errors do not gate a clean
        // close, but the shared host-state keys still do.
        ("guard_bytes_intact", serde_json::json!(false)),
    ] {
        let mut report = clean.clone();
        report[key] = dirty;
        assert!(!final_report_clean(&report, true), "{key} must fail closed");
        let mut missing = clean.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(
            !final_report_clean(&missing, true),
            "missing {key} must fail closed"
        );
    }
}

#[test]
fn final_report_clean_accepts_only_explicit_nonfaulting_suite_lease_warning() {
    let mut warned = serde_json::json!({
        "status": "render_completed",
        "global_setdown_error": 0,
        "guard_bytes_intact": true,
        "suite_leases_balanced": false,
        "suite_lease_warning": true,
        "suite_fault_observed": false,
        "suite_acquires": 123,
        "suite_releases": 24,
        "live_suite_lease_count": 1,
        "live_suite_leases": "PF World Suite@2=99",
        "handle_lifetimes_balanced": true,
        "world_lifetimes_balanced": true,
        "param_checkouts_balanced": true,
        "session_mode": true,
        "session_render_error": 0,
        "session_sequence_setup_error": 0,
        "session_sequence_setdown_error": 0,
    });
    assert_eq!(
        validate_final_report(&warned, true),
        Ok(FinalReportValidation::CleanWithSuiteLeaseWarning {
            suite_acquires: 123,
            suite_releases: 24,
            live_suite_lease_count: 1,
        })
    );

    for (key, value) in [
        ("suite_lease_warning", serde_json::json!(false)),
        ("suite_fault_observed", serde_json::json!(true)),
        ("suite_acquires", serde_json::json!(24)),
        ("live_suite_lease_count", serde_json::json!(0)),
        ("live_suite_leases", serde_json::json!("")),
    ] {
        warned[key] = value;
        assert!(!final_report_clean(&warned, true), "{key} must fail closed");
        warned[key] = match key {
            "suite_lease_warning" => serde_json::json!(true),
            "suite_fault_observed" => serde_json::json!(false),
            "suite_acquires" => serde_json::json!(123),
            "live_suite_lease_count" => serde_json::json!(1),
            "live_suite_leases" => serde_json::json!("PF World Suite@2=99"),
            _ => unreachable!(),
        };
    }
}

#[test]
fn close_report_validation_is_typed_and_rejects_unexpected_or_incomplete_leases() {
    let clean = serde_json::json!({
        "status": "render_completed",
        "render_error": 0,
        "global_setdown_error": 0,
        "persistent_sequence_setup_error": 0,
        "persistent_sequence_setdown_error": 0,
        "guard_bytes_intact": true,
        "suite_leases_balanced": false,
        "suite_lease_warning": true,
        "suite_fault_observed": false,
        "suite_acquires": 8,
        "suite_releases": 7,
        "live_suite_lease_count": 1,
        "live_suite_leases": "PF Handle Suite@2=1",
        "handle_lifetimes_balanced": true,
        "world_lifetimes_balanced": true,
        "param_checkouts_balanced": true,
    });
    let close = serde_json::json!({
        "invalidated": false,
        "worker": {"classification": "ok"},
        "final_report": clean,
        // Deliberately stale: consumers must use the shared final-report
        // verdict rather than this convenience field.
        "session_clean": false,
    });
    assert!(matches!(
        validate_close_report(&close, false),
        Ok(FinalReportValidation::CleanWithSuiteLeaseWarning { .. })
    ));
    // Classic workers published no `suite_fault_observed: false` field before
    // the warning exception existed.  Its absence is not a claim of safety;
    // a positive observation still rejects the close, while the independently
    // required warning/count/list evidence prevents a generic fail-open.
    let mut classic_legacy = close.clone();
    classic_legacy["final_report"]
        .as_object_mut()
        .unwrap()
        .remove("suite_fault_observed");
    assert!(matches!(
        validate_close_report(&classic_legacy, false),
        Ok(FinalReportValidation::CleanWithSuiteLeaseWarning { .. })
    ));

    let cases = [
        (
            "suite_fault_observed",
            serde_json::json!(true),
            CloseReportInvariant::SuiteFaultObserved,
        ),
        (
            "live_suite_lease_count",
            serde_json::json!(0),
            CloseReportInvariant::SuiteLeaseCounts,
        ),
        (
            "live_suite_leases",
            serde_json::json!(""),
            CloseReportInvariant::SuiteLeaseList,
        ),
        (
            "handle_lifetimes_balanced",
            serde_json::json!(false),
            CloseReportInvariant::HandleLifetimes,
        ),
        (
            "world_lifetimes_balanced",
            serde_json::json!(false),
            CloseReportInvariant::WorldLifetimes,
        ),
        (
            "param_checkouts_balanced",
            serde_json::json!(false),
            CloseReportInvariant::ParameterCheckouts,
        ),
    ];
    for (key, value, expected) in cases {
        let mut rejected = close.clone();
        rejected["final_report"][key] = value;
        assert_eq!(validate_close_report(&rejected, false), Err(expected));
    }

    let mut missing_warning_metadata = close.clone();
    missing_warning_metadata["final_report"]
        .as_object_mut()
        .unwrap()
        .remove("suite_lease_warning");
    assert_eq!(
        validate_close_report(&missing_warning_metadata, false),
        Err(CloseReportInvariant::SuiteLeaseWarningMetadata)
    );
    let mut missing_count_metadata = close.clone();
    missing_count_metadata["final_report"]
        .as_object_mut()
        .unwrap()
        .remove("suite_acquires");
    assert_eq!(
        validate_close_report(&missing_count_metadata, false),
        Err(CloseReportInvariant::SuiteLeaseWarningMetadata)
    );

    let mut unexpected_live_lease = close.clone();
    unexpected_live_lease["final_report"]["suite_leases_balanced"] = serde_json::json!(true);
    unexpected_live_lease["final_report"]["suite_lease_warning"] = serde_json::json!(false);
    unexpected_live_lease["final_report"]["suite_acquires"] = serde_json::json!(8);
    unexpected_live_lease["final_report"]["suite_releases"] = serde_json::json!(8);
    assert_eq!(
        validate_close_report(&unexpected_live_lease, false),
        Err(CloseReportInvariant::UnexpectedLiveSuiteLease)
    );
}

#[test]
fn close_report_validation_rejects_crash_and_missing_report() {
    let crashed = serde_json::json!({
        "invalidated": true,
        "worker": {"classification": "crashed"},
        "final_report": null,
    });
    assert_eq!(
        validate_close_report(&crashed, false),
        Err(CloseReportInvariant::CloseInvalidated)
    );
    let no_report = serde_json::json!({
        "invalidated": false,
        "worker": {"classification": "ok"},
        "final_report": null,
    });
    assert_eq!(
        validate_close_report(&no_report, false),
        Err(CloseReportInvariant::FinalReportMissing)
    );
}

#[test]
fn open_rejects_timing_the_worker_could_never_render() {
    // Timing is validated before any file or transport work, so fake
    // paths never get touched when the timing is invalid.
    for (time_step, total_time, time_scale) in [
        (0, 300, 30),
        // total_time == 0 is now valid (the zero-duration t=0 render, #272);
        // only a negative total_time is rejected.
        (1, -1, 30),
        (1, 300, 0),
        // The worker parses per-frame scales as signed 32-bit.
        (1, 300, i32::MAX as u32 + 1),
    ] {
        let result = RenderSession::open(SessionOpenRequest {
            repository: Path::new("missing-repository"),
            plugin_path: Path::new("missing-plugin.aex"),
            plugin_sha256: &"0".repeat(64),
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
            width: 8,
            height: 4,
            pixel_format: RenderPixelFormat::Argb8,
            time_step,
            total_time,
            time_scale,
            frame_deadline: Duration::from_secs(1),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
        });
        let Err(error) = result else {
            panic!("invalid timing must be rejected before launch");
        };
        assert_eq!(error.to_string(), "render session timing is invalid");
    }
}

#[test]
fn fatal_session_error_codes_match_the_worker_contract() {
    // kSessionGenerationMismatch .. kSessionOutputValidationError, plus
    // the deferred-setup failure (-47).
    for code in [-41, -42, -43, -44, -45, -47] {
        assert!(is_fatal_session_error(code), "{code} is session-fatal");
    }
    // Time-scale (-40) and time-range (-46) rejections are frame-local,
    // as are ordinary positive selector errors.
    for code in [-40, -46, 516, 25, -1] {
        assert!(!is_fatal_session_error(code), "{code} stays frame-local");
    }
}

#[test]
fn depth_codes_and_commands_are_depth_explicit() {
    assert_eq!(depth_code(RenderPixelFormat::Argb8), 8);
    assert_eq!(depth_code(RenderPixelFormat::Argb16), 16);
    assert_eq!(depth_code(RenderPixelFormat::Argb32f), 32);
    for (pixel_format, expected) in [
        (RenderPixelFormat::Argb8, "--render-session-v1"),
        (RenderPixelFormat::Argb16, "--render-session16-v1"),
        (RenderPixelFormat::Argb32f, "--render-session32-v1"),
    ] {
        assert_eq!(
            session_command(pixel_format, false, RenderGpuBackend::Cpu).unwrap(),
            expected
        );
        assert_eq!(
            session_command(pixel_format, false, RenderGpuBackend::Auto).unwrap(),
            expected
        );
    }
}

#[test]
fn smart_session_commands_carry_the_gpu_backend() {
    for (backend, expected) in [
        (RenderGpuBackend::Auto, "--smart-session32-v1"),
        (RenderGpuBackend::Cuda, "--smart-session32-v1"),
        (RenderGpuBackend::OpenCl, "--smart-session32-opencl-v1"),
        (RenderGpuBackend::DirectX, "--smart-session32-directx-v1"),
        (RenderGpuBackend::Cpu, "--smart-session32-cpu-v1"),
    ] {
        assert_eq!(
            session_command(RenderPixelFormat::Argb32f, true, backend).unwrap(),
            expected
        );
    }
    assert_eq!(
        session_command(RenderPixelFormat::Argb8, true, RenderGpuBackend::Auto).unwrap(),
        "--smart-session-v1"
    );
    assert_eq!(
        session_command(RenderPixelFormat::Argb16, true, RenderGpuBackend::Cpu).unwrap(),
        "--smart-session16-v1"
    );
    // Explicit GPU backends exist only for SmartFX ARGB32f; every other
    // combination fails closed instead of silently degrading.
    for (pixel_format, smart) in [
        (RenderPixelFormat::Argb8, true),
        (RenderPixelFormat::Argb16, true),
        (RenderPixelFormat::Argb32f, false),
    ] {
        for backend in [
            RenderGpuBackend::Cuda,
            RenderGpuBackend::OpenCl,
            RenderGpuBackend::DirectX,
        ] {
            assert!(session_command(pixel_format, smart, backend).is_err());
        }
    }
}
