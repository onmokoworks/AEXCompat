//! Behavioural verification of the length-1 session wrapper (issue #98 stage
//! W2, converted from A/B in #361).
//!
//! These were A/B tests against the one-shot argv transport. That comparison
//! could only ever check the transport and the lifecycle: both routes converge
//! on the same `smart_render_once` and the same dispatch, so a regression in
//! the render core moved both sides together and compared equal. Each test now
//! verifies the session directly -- health conditions, determinism, and the
//! effect the feature under test is supposed to have -- without freezing any
//! value into the test, so the assertions hold on any machine
//! (docs/EVIDENCE_POLICY_2026-07-18.md).
//!
//! Requires the real workers and the probe fixtures from this checkout; each
//! test skips with a message when its fixture is not built.

#[cfg(test)]
#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{
        AnimationInterpolation, AnimationTime, AnimationValue, InteractiveParameter,
        ParameterAnimation, ParameterAnimationKey, RENDER_SESSION_WRAPPER_RENDERS,
        RenderArtifactKind, RenderGpuBackend, RenderPixelFormat, RenderTiming, RenderUiAction,
        TimedLayerImage, render_declarative_fixture, render_experimental_artifact_at_time,
        render_experimental_audio, render_experimental_image, render_experimental_image_at_time,
        render_experimental_image_at_time_with_format_and_context,
        render_experimental_image_at_time_with_format_context_and_ui_action,
        render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend,
        render_experimental_image_with_audio_sidecar,
        render_experimental_image_with_parameter_animation,
        render_experimental_image_with_timed_layers, render_experimental_smart_image,
        render_experimental_smart_image_at_time,
    };
    // The fault-injection knob exists only in debug builds (image_render.rs), so
    // the test that uses it is gated to debug too.
    #[cfg(debug_assertions)]
    use aexcompat_broker::image_render::FORCE_SESSION_FALLBACK_ENV;
    use aexcompat_broker::render_request::HostContext;
    use aexcompat_broker::render_session::{FrameStatus, RenderSession, SessionOpenRequest};
    use aexcompat_broker::secure_launch::LaunchEnvironment;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root")
    }

    // Every test here asserts on deltas of RENDER_SESSION_WRAPPER_RENDERS, and
    // the two fail-closed diagnostics toggle the process-global
    // FORCE_SESSION_FALLBACK_ENV. cargo runs a binary's tests concurrently, so
    // without this lock a concurrent session render perturbs another test's
    // counter assertion, and a forced fallback bleeds into another test's
    // render.
    static SESSION_ROUTE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Absolute health conditions every session render must satisfy, whatever
    /// the plug-in or the depth. These are the assertions the A/B against the
    /// one-shot could never make: both routes converge on the same
    /// `smart_render_once` and the same dispatch, so a regression in the render
    /// core moved both sides together and compared equal. Nothing here is a
    /// frozen value -- each is a condition the report must meet on any machine
    /// (docs/EVIDENCE_POLICY_2026-07-18.md rejects frozen-identity re-assertion
    /// in favour of machine-portable behavioural checks).
    fn assert_session_render_is_healthy(report: &serde_json::Value, label: &str) {
        let object = report
            .as_object()
            .unwrap_or_else(|| panic!("{label}: report is not an object: {report}"));
        let require = |key: &str, want: serde_json::Value| {
            assert_eq!(
                object.get(key),
                Some(&want),
                "{label}: {key} must be {want}: {report}"
            );
        };
        // The broker's own verdict and the worker's exit.
        require("passed", serde_json::json!(true));
        require("worker_classification", serde_json::json!("ok"));
        // The geometry contracts are host-side invariants, so they are required
        // here. parameter_count_contract_ok is not: it is an observation about
        // the plug-in's declared parameter count, and a fixture may legitimately
        // fail it (pf_layer_param_probe declares num_params=1 while the render
        // supplies a layer, so it reports false and host_contract_warning goes
        // true). Measured -- requiring host_contract_warning == false for every
        // render fails a correct oversized-layer render. Each test pins that
        // field itself where it is meaningful.
        require("output_origin_contract_ok", serde_json::json!(true));
        require("spatial_contract_ok", serde_json::json!(true));
        // Ownership ledgers and guard pages: a malformed plug-in must produce a
        // diagnostic, never a corrupted host (CLAUDE.md host-protection
        // invariants). Required outright -- an absent key means the report shape
        // changed, which is itself a regression.
        for key in [
            "guard_bytes_intact",
            "handle_lifetimes_balanced",
            "suite_leases_balanced",
            "world_lifetimes_balanced",
            "param_checkouts_balanced",
        ] {
            require(key, serde_json::json!(true));
        }
        let gpu_memory = object
            .get("gpu_memory")
            .unwrap_or_else(|| panic!("{label}: gpu_memory missing: {report}"));
        assert_eq!(
            gpu_memory.get("lifetimes_balanced"),
            Some(&serde_json::json!(true)),
            "{label}: GPU allocations did not balance: {report}"
        );
        assert_eq!(
            gpu_memory.get("invalid_operations"),
            Some(&serde_json::json!(0)),
            "{label}: invalid GPU memory operations: {report}"
        );
        // Route-specific fields. The public report carries both routes' keys and
        // nulls the ones that do not apply, so requiring a value from the wrong
        // route asserts against a sentinel. Measured: a classic render reports
        // no render_error at all (it lives in the stage events) and nulls
        // smart_render_error; a smart render has no pre_render_error key.
        if object.get("render_path").and_then(|v| v.as_str()) == Some("smartfx") {
            require("smart_render_error", serde_json::json!(0));
            require("output_pixels_valid", serde_json::json!(true));
            require("extra_pixels_contract_violation", serde_json::json!(false));
            require("result_within_request", serde_json::json!(true));
            // A legally empty result rect means the render selector is never
            // dispatched, so its error stays at the not-dispatched sentinel.
            // Measured -- requiring 0 unconditionally fails the empty frame,
            // which is a correct render.
            if object.get("empty_result_rect") == Some(&serde_json::json!(true)) {
                require("smart_render_selector_dispatched", serde_json::json!(false));
            } else {
                require("smart_render_selector_dispatched", serde_json::json!(true));
                require("smart_render_selector_error", serde_json::json!(0));
            }
        }
        // Present on whichever route tracks it; a null means not applicable.
        for key in ["pf_path_lifetimes_balanced"] {
            match object.get(key) {
                None | Some(serde_json::Value::Null) => {}
                Some(value) => assert_eq!(
                    value,
                    &serde_json::json!(true),
                    "{label}: {key} must be true: {report}"
                ),
            }
        }
        for key in ["invalid_pf_path_operations"] {
            match object.get(key) {
                None | Some(serde_json::Value::Null) => {}
                Some(value) => assert_eq!(
                    value,
                    &serde_json::json!(0),
                    "{label}: {key} must be zero: {report}"
                ),
            }
        }
    }

    /// Report fields that legitimately differ between two runs of the same
    /// render: output paths carry a per-run nonce, and the worker diagnostics
    /// carry timings and memory peaks.
    const VOLATILE_REPORT_KEYS: [&str; 3] = ["output_png", "output_raw", "worker_diagnostics"];

    fn stable_report(report: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        let mut object = report.as_object().expect("report object").clone();
        for key in VOLATILE_REPORT_KEYS {
            object.remove(key);
        }
        object
    }

    /// Two renders of the same input must agree field for field. Catches
    /// nondeterminism without freezing any byte string into the test.
    fn assert_reports_agree(first: &serde_json::Value, second: &serde_json::Value, label: &str) {
        let a = stable_report(first);
        let b = stable_report(second);
        assert_eq!(
            a.keys().collect::<Vec<_>>(),
            b.keys().collect::<Vec<_>>(),
            "{label}: report key sets diverge"
        );
        for (key, value) in &a {
            assert_eq!(
                Some(value),
                b.get(key),
                "{label}: report field {key} differs"
            );
        }
    }

    /// The CPU SmartFX single-image render, verified without the one-shot.
    ///
    /// This used to be an A/B against `--smart-image`. That comparison could
    /// only ever check the transport and the lifecycle: both routes converge on
    /// the same `smart_render_once` and the same dispatch, so a regression in
    /// the render core moved both sides together and compared equal. The
    /// assertions below are the ones the A/B could not make -- health
    /// conditions, determinism, and sensitivity to the input -- and none of
    /// them freezes a byte string, so they hold on any machine
    /// (docs/EVIDENCE_POLICY_2026-07-18.md, issue #361).
    #[test]
    fn smart_single_image_renders_deterministically_and_follows_its_input() {
        // Still takes the route lock: the tests that assert on exact deltas of
        // RENDER_SESSION_WRAPPER_RENDERS are perturbed by a concurrent session
        // render, and the two fail-closed diagnostics set a process-global
        // fault-injection env that would otherwise leak into this render.
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex =
            root.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping smart session render: build aex_smart_worker.exe and                  pf_smart_geometry_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-smart-session-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let make_input = |name: &str, seed: u32| {
            let path = scratch.join(name);
            image::RgbaImage::from_fn(64, 48, |x, y| {
                image::Rgba([
                    ((x * 3) as u32 + seed) as u8,
                    ((y * 5) as u32 + seed) as u8,
                    (x + y) as u8,
                    255,
                ])
            })
            .save(&path)
            .unwrap();
            path
        };
        let input = make_input("input.png", 0);

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_first = scratch.join("first.png");
        let first = render_experimental_smart_image(&root, &aex, &sha, &input, &out_first, &[])
            .expect("session-route smart render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the smart render must be carried by the session"
        );
        assert_session_render_is_healthy(&first, "smart single image");
        // The health helper keys its smart assertions off render_path, so pin the
        // route here: a regression flipping it to classic would otherwise skip
        // those assertions silently rather than fail.
        assert_eq!(
            first.get("render_path"),
            Some(&serde_json::json!("smartfx"))
        );

        // Determinism: the same input twice must agree field for field and byte
        // for byte, which is the property the frozen A/B output was standing in
        // for -- without pinning the bytes to this machine's toolchain.
        let out_repeat = scratch.join("repeat.png");
        let repeat = render_experimental_smart_image(&root, &aex, &sha, &input, &out_repeat, &[])
            .expect("second session-route smart render");
        assert_reports_agree(&first, &repeat, "smart single image repeat");
        assert_eq!(
            std::fs::read(&out_first).unwrap(),
            std::fs::read(&out_repeat).unwrap(),
            "the same input produced different pixels across two session renders"
        );

        // Sensitivity: the render must actually depend on its input. Which
        // property to assert is a per-fixture question -- this probe fills the
        // output with a constant and only checks the input layer out
        // (instruments/pf-smart-geometry-probe), so its pixels are
        // input-independent by design and the meaningful dependence is on the
        // input *geometry*. Measured, not assumed: asserting pixel dependence
        // here fails against this probe.
        // The pixel *count* has to change, not just the shape: this probe fills
        // a constant, so 96x32 hashes identically to 64x48 (both 3072 pixels).
        // Measured -- that pair silently passed the geometry assertion and then
        // failed the checksum one.
        let wide = scratch.join("wide.png");
        image::RgbaImage::from_fn(96, 48, |x, y| {
            image::Rgba([(x * 2) as u8, (y * 7) as u8, 0, 255])
        })
        .save(&wide)
        .unwrap();
        let out_wide = scratch.join("wide-out.png");
        let wide_report = render_experimental_smart_image(&root, &aex, &sha, &wide, &out_wide, &[])
            .expect("session-route smart render at another size");
        assert_session_render_is_healthy(&wide_report, "smart single image (other geometry)");
        assert_eq!(
            (wide_report.get("width"), wide_report.get("height")),
            (Some(&serde_json::json!(96)), Some(&serde_json::json!(48))),
            "the render did not follow the input geometry: {wide_report}"
        );
        assert_ne!(
            first.get("output_sha256"),
            wide_report.get("output_sha256"),
            "a differently sized input produced the same output"
        );

        // A legally empty result still has to be reported as one: the probe
        // answers an empty result_rect at current_time % 4 == 3, and neither the
        // PNG nor a checksum may be invented for it.
        let out_empty = scratch.join("empty.png");
        let empty = render_experimental_smart_image_at_time(
            &root,
            &aex,
            &sha,
            &input,
            &out_empty,
            &[],
            RenderTiming {
                current_time: 3,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
            },
        )
        .expect("session-route empty smart frame");
        assert_session_render_is_healthy(&empty, "smart empty result");
        assert_eq!(
            empty.get("empty_result_rect"),
            Some(&serde_json::json!(true)),
            "the session must report the empty result: {empty}"
        );
        assert!(
            !out_empty.exists(),
            "an empty result must not write a PNG: {empty}"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Argb32f smart Auto without a policy folds to the CPU session
    /// (--smart-session32-cpu-v1). #292 settled that the session is the anchor
    /// here and that its no-GPU-attempt report is canonical, so the conversion
    /// pins the fold itself rather than comparing against the one-shot's futile
    /// preflight record (#361).
    #[test]
    fn smart_argb32f_auto_without_a_policy_folds_to_the_cpu_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex =
            root.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping Argb32f smart Auto: build the smart worker and the geometry probe");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-smart32-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        let timing = RenderTiming {
            current_time: 0,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
        };
        let render = |output: &Path, backend: RenderGpuBackend| {
            render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
                &root,
                &aex,
                &sha,
                &input,
                output,
                &[],
                timing,
                true,
                RenderPixelFormat::Argb32f,
                None,
                None,
                backend,
            )
        };

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_auto = scratch.join("auto.png");
        let auto = render(&out_auto, RenderGpuBackend::Auto)
            .expect("session-route Argb32f Auto smart render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the Argb32f Auto smart render must be carried by the session"
        );
        assert_session_render_is_healthy(&auto, "Argb32f Auto");
        assert_eq!(auto.get("render_path"), Some(&serde_json::json!("smartfx")));
        assert_eq!(
            auto.get("pixel_format"),
            Some(&serde_json::json!("argb32f")),
            "the render did not stay at float32: {auto}"
        );

        // The fold is the point: with no authenticated policy the session must
        // not attempt a device, and must say so rather than leaving it unstated.
        assert_eq!(
            auto.get("gpu_attempt"),
            Some(&serde_json::Value::Null),
            "a policy-less Auto session must record no GPU attempt: {auto}"
        );
        assert_eq!(
            auto.get("gpu_fallback_used"),
            Some(&serde_json::json!(false)),
            "the session folds at open; it does not fall back mid-flight: {auto}"
        );
        assert_eq!(
            auto.get("gpu_render_dispatched"),
            Some(&serde_json::json!(false)),
            "no GPU render may be dispatched without a policy: {auto}"
        );

        // Folding to CPU must be exactly that: the same render the explicit CPU
        // backend produces. That is the equivalence the A/B against the one-shot
        // was standing in for, and it holds between two session renders.
        let out_cpu = scratch.join("cpu.png");
        let cpu = render(&out_cpu, RenderGpuBackend::Cpu)
            .expect("session-route Argb32f Cpu smart render");
        assert_session_render_is_healthy(&cpu, "Argb32f Cpu");
        assert_eq!(
            auto.get("output_sha256"),
            cpu.get("output_sha256"),
            "the Auto fold did not produce the explicit-CPU render"
        );
        assert_eq!(
            std::fs::read(&out_auto).unwrap(),
            std::fs::read(&out_cpu).unwrap(),
            "the Auto fold PNG differs from the explicit-CPU PNG"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn render_artifact_pipeline_commits_raw_and_exr_sets_after_a_clean_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex =
            root.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping render artifact pipeline: build smart worker and geometry probe");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-render-artifact-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 31) as u8, (y * 47) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        let timing = RenderTiming {
            current_time: 0,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
        };
        let mut pf32_identity = None;
        for format in [
            RenderPixelFormat::Argb8,
            RenderPixelFormat::Argb16,
            RenderPixelFormat::Argb32f,
        ] {
            let output = scratch.join(format!("raw-{}", format.report_name()));
            let report = render_experimental_artifact_at_time(
                &root,
                &aex,
                &sha,
                &input,
                &output,
                &[],
                timing,
                true,
                format,
                RenderArtifactKind::Raw,
            )
            .unwrap();
            assert_eq!(report["passed"], true);
            assert!(report["output_png"].is_null());
            let metadata: serde_json::Value =
                serde_json::from_slice(&std::fs::read(output.join("output.json")).unwrap())
                    .unwrap();
            let raw = std::fs::read(output.join("output.bin")).unwrap();
            assert_eq!(metadata, report["render_artifact"]);
            assert_eq!(metadata["pixel_format"], format.report_name());
            assert_eq!(metadata["data_size_bytes"], raw.len());
            assert_eq!(
                metadata["data_sha256"],
                format!("{:x}", Sha256::digest(&raw))
            );
            assert_eq!(metadata["premultiplication"], report["premultiplication"]);
            assert_eq!(metadata["working_space"], "None");
            assert_eq!(metadata["render_mode"], "software");
            assert_eq!(metadata["row_padding"], "excluded");
            if format == RenderPixelFormat::Argb32f {
                pf32_identity = Some(metadata["comparison_identity"].clone());
            }
        }
        let exr_output = scratch.join("exr");
        let exr_report = render_experimental_artifact_at_time(
            &root,
            &aex,
            &sha,
            &input,
            &exr_output,
            &[],
            timing,
            true,
            RenderPixelFormat::Argb32f,
            RenderArtifactKind::Float32Exr,
        )
        .unwrap();
        assert_eq!(exr_report["passed"], true);
        let exr_metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(exr_output.join("output.json")).unwrap())
                .unwrap();
        assert_eq!(exr_metadata["compression"], "none");
        assert_eq!(exr_metadata["storage"], "scanline");
        assert_eq!(
            exr_metadata["comparison_identity"],
            pf32_identity.expect("PF32 raw comparison identity")
        );
        assert_eq!(
            exr_metadata["premultiplication"],
            exr_report["premultiplication"]
        );
        let exr = exr_output.join("output.exr");
        let script = "import OpenEXR,sys; f=OpenEXR.File(sys.argv[1]); h=f.header(); assert str(h['compression'])=='Compression.NO_COMPRESSION'; assert str(h['type'])=='Storage.scanlineimage'; assert 'RGBA' in f.channels()";
        let decoded = std::process::Command::new("uv")
            .args(["run", "--project"])
            .arg(&root)
            .args(["python", "-c", script])
            .arg(&exr)
            .status()
            .expect("launch independent OpenEXR decoder");
        assert!(decoded.success(), "OpenEXR rejected {}", exr.display());
        let raw32 = scratch.join("raw-argb32f");
        let compared = std::process::Command::new("uv")
            .args(["run", "--project"])
            .arg(&root)
            .arg("python")
            .arg(root.join("tools/compare-pixel-oracles.py"))
            .args(["--raw-u32", "--raw"])
            .arg(raw32.join("output.bin"))
            .arg("--render")
            .arg(&exr)
            .arg("--raw-metadata")
            .arg(raw32.join("output.json"))
            .arg("--render-metadata")
            .arg(exr_output.join("output.json"))
            .output()
            .unwrap();
        assert!(
            compared.status.success(),
            "artifact-bound raw-u32 comparison failed: {}",
            String::from_utf8_lossy(&compared.stderr)
        );
        let comparison: serde_json::Value = serde_json::from_slice(&compared.stdout).unwrap();
        assert_eq!(comparison["match"], true);
        assert_eq!(
            comparison["comparison_boundary"]["claim_level"],
            "raw_u32_exact"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn declarative_fixture_commits_exr_and_selected_native_checkpoints_together() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex =
            root.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping declarative fixture: build smart worker and geometry probe");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-declarative-fixture-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        image::RgbaImage::from_fn(8, 6, |x, y| {
            image::Rgba([(x * 17) as u8, (y * 29) as u8, (x + y) as u8, 255])
        })
        .save(scratch.join("primary.png"))
        .unwrap();
        let fixture = serde_json::json!({
            "schema":"aexcompat.render_fixture", "schema_version":1,
            "primary_layer":"primary.png", "parameters":[], "pixel_format":"argb32f",
            "render_path":"smart",
            "premultiplication":"straight",
            "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
            "final_artifact":"exr",
            "checkpoints":[
                {"id":"input_world","stage":"smart-input"},
                {"id":"output_world","stage":"smart-output"}
            ]
        });
        let fixture_path = scratch.join("fixture.json");
        std::fs::write(&fixture_path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
        let output = scratch.join("result");
        let report = render_declarative_fixture(&root, &aex, &sha, &fixture_path, &output)
            .expect("declarative fixture render");
        assert_eq!(report["pixel_format"], "argb32f");
        assert_eq!(report["final_artifact"]["compression"], "none");
        assert!(output.join("final/output.exr").is_file());
        for (id, stage) in [
            ("input_world", "smart-input"),
            ("output_world", "smart-output"),
        ] {
            let metadata: serde_json::Value = serde_json::from_slice(
                &std::fs::read(output.join("checkpoints").join(id).join("output.json")).unwrap(),
            )
            .unwrap();
            let raw =
                std::fs::read(output.join("checkpoints").join(id).join("output.bin")).unwrap();
            assert_eq!(metadata["schema_version"], 2);
            assert_eq!(metadata["checkpoint_identity"]["id"], id);
            assert_eq!(metadata["checkpoint_identity"]["stage"], stage);
            assert_eq!(
                metadata["comparison_identity"]["checkpoint"],
                metadata["checkpoint_identity"]
            );
            assert_eq!(metadata["pixel_format"], "argb32f");
            assert_eq!(metadata["channel_order"], "ARGB");
            assert_eq!(
                metadata["component_representation"],
                "ieee754_binary32_raw_words"
            );
            assert_eq!(metadata["data_size_bytes"], raw.len());
            assert_eq!(
                metadata["rowbytes"].as_u64().unwrap() * metadata["height"].as_u64().unwrap(),
                raw.len() as u64
            );
        }
        for (format, representation, component_bytes) in [
            ("argb8", "unsigned_integer_0_255", 1u64),
            ("argb16", "unsigned_integer_0_32768_ae_internal", 2u64),
        ] {
            let mut depth_fixture = fixture.clone();
            depth_fixture["pixel_format"] = serde_json::json!(format);
            depth_fixture["final_artifact"] = serde_json::json!("raw");
            depth_fixture["checkpoints"] =
                serde_json::json!([{"id":"input_world","stage":"smart-input"}]);
            let path = scratch.join(format!("fixture-{format}.json"));
            std::fs::write(&path, serde_json::to_vec_pretty(&depth_fixture).unwrap()).unwrap();
            let depth_output = scratch.join(format!("result-{format}"));
            render_declarative_fixture(&root, &aex, &sha, &path, &depth_output)
                .unwrap_or_else(|error| panic!("{format} fixture render failed: {error}"));
            let metadata: serde_json::Value = serde_json::from_slice(
                &std::fs::read(depth_output.join("checkpoints/input_world/output.json")).unwrap(),
            )
            .unwrap();
            let raw =
                std::fs::read(depth_output.join("checkpoints/input_world/output.bin")).unwrap();
            assert_eq!(metadata["pixel_format"], format);
            assert_eq!(metadata["component_bytes"], component_bytes);
            assert_eq!(metadata["component_representation"], representation);
            if format == "argb16" {
                assert!(
                    raw.chunks_exact(2)
                        .all(|word| { u16::from_le_bytes([word[0], word[1]]) <= 32768 })
                );
            }
        }
        let mut missing_fixture = fixture.clone();
        missing_fixture["pixel_format"] = serde_json::json!("argb8");
        missing_fixture["final_artifact"] = serde_json::json!("raw");
        missing_fixture["checkpoints"] =
            serde_json::json!([{"id":"missing_layer","stage":"smart-layer-slot99"}]);
        let missing_path = scratch.join("fixture-missing.json");
        std::fs::write(
            &missing_path,
            serde_json::to_vec_pretty(&missing_fixture).unwrap(),
        )
        .unwrap();
        let missing_output = scratch.join("missing-result");
        assert!(
            render_declarative_fixture(&root, &aex, &sha, &missing_path, &missing_output).is_err()
        );
        assert!(
            !missing_output.exists(),
            "a missing checkpoint published a partial fixture set"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn declarative_classic_fixture_carries_secondary_layer_and_scalar_parameter() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-layer-param-probe-build/Release/pf_layer_param_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping declarative layer fixture: build classic worker and layer probe");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-declarative-layer-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 255]))
            .save(scratch.join("primary.png"))
            .unwrap();
        let secondary_pixels = image::RgbaImage::from_fn(4, 3, |x, y| {
            image::Rgba([(x * 31) as u8, (y * 47) as u8, 93, 255])
        });
        secondary_pixels
            .save(scratch.join("secondary.png"))
            .unwrap();
        let render = |value: f64, name: &str| {
            let parameters = vec![
                layer_parameter(1, Path::new("secondary.png")),
                float_parameter(2, value),
            ];
            let fixture = serde_json::json!({
                "schema":"aexcompat.render_fixture", "schema_version":1,
                "primary_layer":"primary.png", "parameters":parameters,
                "pixel_format":"argb8", "render_path":"classic",
                "premultiplication":"straight",
                "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
                "final_artifact":"raw",
                "checkpoints":[{"id":"secondary_world","stage":"classic-layer-slot1"}]
            });
            let fixture_path = scratch.join(format!("{name}.json"));
            std::fs::write(&fixture_path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
            let output = scratch.join(name);
            render_declarative_fixture(&root, &aex, &sha, &fixture_path, &output)
                .unwrap_or_else(|error| panic!("classic layer fixture failed: {error}"));
            output
        };
        let low = render(20.0, "low");
        let high = render(200.0, "high");
        let checkpoint = std::fs::read(low.join("checkpoints/secondary_world/output.bin")).unwrap();
        let expected = secondary_pixels
            .into_raw()
            .chunks_exact(4)
            .flat_map(|rgba| [rgba[3], rgba[0], rgba[1], rgba[2]])
            .collect::<Vec<_>>();
        assert_eq!(checkpoint, expected, "secondary checkpoint words changed");
        assert_ne!(
            std::fs::read(low.join("final/output.bin")).unwrap(),
            std::fs::read(high.join("final/output.bin")).unwrap(),
            "changing the fixture scalar parameter did not change the render"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Three timed secondary layers at one slot, carried by the smart session's
    /// layer trailer (#294) at both Argb16 and Argb32f (#353). Verified by the
    /// layers reaching the plug-in rather than by agreeing with the one-shot
    /// (#361).
    #[test]
    fn smart_timed_multilayer_reaches_the_plug_in_at_every_depth() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex = root
            .join("target/pf-smart-timed-multilayer-probe-build/Release")
            .join("pf_smart_timed_multilayer_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping smart timed-multilayer: build the smart worker and the probe");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-timed-multilayer-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 6) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        let make_layer = |name: &str, seed: u8| {
            let path = scratch.join(name);
            image::RgbaImage::from_fn(48, 32, |x, y| {
                image::Rgba([
                    (x as u8).wrapping_add(seed),
                    (y as u8).wrapping_mul(2).wrapping_add(seed),
                    seed,
                    255,
                ])
            })
            .save(&path)
            .unwrap();
            path
        };
        let timed_layers = |seed_shift: u8| {
            vec![
                TimedLayerImage {
                    slot: 1,
                    time: AnimationTime { value: 6, scale: 8 },
                    image_path: make_layer(&format!("layer0-{seed_shift}.png"), 10 + seed_shift),
                },
                TimedLayerImage {
                    slot: 1,
                    time: AnimationTime { value: 1, scale: 3 },
                    image_path: make_layer(&format!("layer1-{seed_shift}.png"), 40 + seed_shift),
                },
                TimedLayerImage {
                    slot: 1,
                    time: AnimationTime { value: 5, scale: 4 },
                    image_path: make_layer(&format!("layer2-{seed_shift}.png"), 70 + seed_shift),
                },
            ]
        };
        // A null-path layer parameter names slot 1 without contributing a static
        // secondary, so the timed layers are the only ones the probe checks out.
        let layer_decl: InteractiveParameter = serde_json::from_value(serde_json::json!({
            "slot": 1, "name": "layer", "kind": "layer",
            "minimum": 0.0, "maximum": 0.0, "value": 0.0,
            "choices": [], "color": [0, 0, 0, 0], "components": [0.0, 0.0, 0.0],
            "component_count": 0, "layer_path": null,
            "enabled": true, "visible": true, "supervised": false,
        }))
        .expect("layer declaration");
        let params = vec![layer_decl];
        let timing = RenderTiming {
            current_time: 0,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
        };

        for pixel_format in [RenderPixelFormat::Argb16, RenderPixelFormat::Argb32f] {
            let label = match pixel_format {
                RenderPixelFormat::Argb8 => "argb8",
                RenderPixelFormat::Argb16 => "argb16",
                RenderPixelFormat::Argb32f => "argb32f",
            };
            let render = |output: &Path, layers: &[TimedLayerImage]| {
                render_experimental_image_with_timed_layers(
                    &root,
                    &aex,
                    &sha,
                    &input,
                    output,
                    &params,
                    layers,
                    timing,
                    true,
                    pixel_format,
                )
                .unwrap_or_else(|error| panic!("{label} timed-multilayer render: {error}"))
            };

            let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
            let out_base = scratch.join(format!("{label}-base.png"));
            let base = render(&out_base, &timed_layers(0));
            assert!(
                RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
                "the {label} timed-multilayer render must be carried by the session"
            );
            assert_session_render_is_healthy(&base, label);
            assert_eq!(base.get("render_path"), Some(&serde_json::json!("smartfx")));
            assert_eq!(
                base.get("pixel_format"),
                Some(&serde_json::json!(label)),
                "the render did not stay at {label}: {base}"
            );
            // #353 admitted layered Argb32f on the strength of both routes
            // staying on the CPU. render_experimental_image_with_timed_layers
            // hardcodes Auto, and with no policy the session must fold; if it
            // ever took a device, health and determinism would both still pass.
            assert_eq!(
                base.get("gpu_render_dispatched"),
                Some(&serde_json::json!(false)),
                "{label}: a layered session dispatched a GPU render: {base}"
            );
            assert_eq!(
                base.get("gpu_attempt"),
                Some(&serde_json::Value::Null),
                "{label}: a layered session attempted a device: {base}"
            );

            let out_repeat = scratch.join(format!("{label}-repeat.png"));
            let repeat = render(&out_repeat, &timed_layers(0));
            assert_reports_agree(&base, &repeat, label);

            // The timed layers reach the plug-in: shifting their pixels must move
            // the output. Without this, a render that dropped the trailer would
            // still have matched the one-shot, which dropped it the same way.
            let out_shift = scratch.join(format!("{label}-shift.png"));
            let shifted = render(&out_shift, &timed_layers(17));
            assert_session_render_is_healthy(&shifted, label);
            assert_ne!(
                base.get("output_sha256"),
                shifted.get("output_sha256"),
                "{label}: changing the timed layers did not change the output"
            );
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }

    fn layer_parameter(slot: u32, path: &Path) -> InteractiveParameter {
        serde_json::from_value(serde_json::json!({
            "slot": slot, "name": "layer", "kind": "layer",
            "minimum": 0.0, "maximum": 0.0, "value": 0.0,
            "choices": [], "color": [0, 0, 0, 0], "components": [0.0, 0.0, 0.0],
            "component_count": 0, "layer_path": path,
            "enabled": true, "visible": true, "supervised": false,
        }))
        .expect("layer parameter fixture")
    }

    fn float_parameter(slot: u32, value: f64) -> InteractiveParameter {
        serde_json::from_value(serde_json::json!({
            "slot": slot, "name": "amount", "kind": "float",
            "minimum": 0.0, "maximum": 255.0, "value": value,
            "choices": [], "color": [0, 0, 0, 0], "components": [0.0, 0.0, 0.0],
            "component_count": 0, "layer_path": null,
            "enabled": true, "visible": true, "supervised": false,
        }))
        .expect("float parameter fixture")
    }

    /// Real-AEX coverage of the session wrapper's secondary-layer transport
    /// (issue #98 W1-4) for the case pf_sampling_probe cannot cover (issue
    /// #195): an AEX that declares a `PF_Param_LAYER` secondary layer (slot 1)
    /// plus a float slider (slot 2) and composites both on the classic render
    /// path. Both inputs must actually reach the plug-in -- changing either one
    /// changes the output -- so this cannot pass vacuously by the probe ignoring
    /// them, and the render must be deterministic at time 0 and at a nonzero
    /// time. This was an A/B against the one-shot argv transport until #361
    /// converted it and #365 deleted the comparison target. Gated on the
    /// locally built worker and the pf-layer-param-probe fixture.

    /// The host-context shapes the session must carry, verified per shape.
    ///
    /// The A/B this replaces covered six sub-cases in one test; the first
    /// conversion kept only the plain render and silently dropped the other
    /// five, which the dead `HostContext` import gave away. Each is restored
    /// here. The counter assertion remains the point of the test (#361): before
    /// #365 a gate regression could push a shape onto the one-shot and still
    /// return Ok, and now that there is no second transport the counter is what
    /// proves the render was carried by the session at all, rather than
    /// short-circuiting somewhere that also returns Ok.
    #[test]
    fn host_context_shapes_stay_on_the_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping host-context shapes: build the worker and pf_sampling_probe.aex");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-host-context-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        let carried = |output: &Path, timing: RenderTiming, context: Option<&HostContext>| {
            let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
            let report = render_experimental_image_at_time_with_format_and_context(
                &root,
                &aex,
                &sha,
                &input,
                output,
                &[],
                timing,
                false,
                RenderPixelFormat::Argb8,
                context,
            )
            .expect("session-route context render");
            assert!(
                RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
                "this shape was not carried by the session: {report}"
            );
            report
        };

        // A non-default time: the session hoists SEQUENCE_SETUP, so a frame at
        // t != 0 exercises the seeding the launch does once.
        let timed = carried(
            &scratch.join("timed.png"),
            RenderTiming {
                current_time: 7,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
            },
            None,
        );
        assert_session_render_is_healthy(&timed, "timed");
        assert_eq!(timed.get("current_time"), Some(&serde_json::json!(7)));

        // Spatial + render environment. The non-default downsample and
        // full-resolution values are what make spatial_contract_ok meaningful --
        // under the default context it is nearly free.
        let context: HostContext = serde_json::from_value(serde_json::json!({
            "mask_scene": {"masks": []},
            "spatial": {
                "downsample_x": {"numerator": 1, "denominator": 2},
                "downsample_y": {"numerator": 1, "denominator": 2},
                "pixel_aspect_ratio": {"numerator": 1, "denominator": 1},
                "full_resolution_width": 128,
                "full_resolution_height": 64,
            },
            "render_environment": {
                "quality": "low",
                "field": "upper",
                "shutter_angle": 0.5,
                "shutter_phase": -0.25,
            },
        }))
        .expect("host context fixture");
        let context_report = carried(
            &scratch.join("context.png"),
            RenderTiming::default(),
            Some(&context),
        );
        assert_session_render_is_healthy(&context_report, "spatial context");
        assert_eq!(
            context_report.get("downsample_x"),
            Some(&serde_json::json!([1, 2])),
            "the spatial trailer did not reach the worker: {context_report}"
        );
        assert_eq!(
            context_report.get("full_resolution_dimensions"),
            Some(&serde_json::json!([128, 64])),
            "the full-resolution hint did not reach the worker: {context_report}"
        );

        // A non-empty mask scene travels as the `v2|` trailer.
        let mask_context: HostContext = serde_json::from_value(serde_json::json!({
            "mask_scene": {
                "masks": [{
                    "open": false,
                    "vertices": [
                        {"x": 4.0, "y": 4.0},
                        {"x": 40.0, "y": 8.0},
                        {"x": 20.0, "y": 28.0},
                    ],
                }],
            },
        }))
        .expect("mask host context fixture");
        let mask_report = carried(
            &scratch.join("mask.png"),
            RenderTiming::default(),
            Some(&mask_context),
        );
        assert_session_render_is_healthy(&mask_report, "mask scene");

        // Alpha-as-coverage rides the `--alpha-as-coverage-v1` auxiliary option.
        let coverage_context: HostContext = serde_json::from_value(serde_json::json!({
            "mask_scene": {"masks": []},
            "alpha_as_coverage_params": [0],
        }))
        .expect("alpha-as-coverage host context fixture");
        let coverage_report = carried(
            &scratch.join("coverage.png"),
            RenderTiming::default(),
            Some(&coverage_context),
        );
        assert_session_render_is_healthy(&coverage_report, "alpha as coverage");

        // Aux channels ride `--aux-manifest-v1`. This end-to-end path is what
        // surfaced the verbatim-path defect in #231, so it is worth keeping as a
        // live render rather than a source-string check.
        // The aux sidecar must live under <repository>/target, which is the only
        // tree the broker will read a manifest-referenced path from.
        let aux_source_dir = root.join(format!(
            "target/aux-source-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&aux_source_dir).unwrap();
        let aux_source = aux_source_dir.join("depth.f32le");
        let depth_type = i32::from_be_bytes(*b"DPTH");
        // 64 * 32 * 1 component, packed little-endian f32; a bounded ramp keeps
        // every sample finite (prepare_aux_transport rejects NaN/inf).
        let depth_bytes: Vec<u8> = (0..64u32 * 32)
            .flat_map(|index| ((index % 251) as f32 / 251.0).to_le_bytes())
            .collect();
        std::fs::write(&aux_source, &depth_bytes).unwrap();
        let aux_context: HostContext = serde_json::from_value(serde_json::json!({
            "mask_scene": {"masks": []},
            "aux_channels": [{
                "param_index": 0,
                "channel": {
                    "type": depth_type,
                    "name": "Depth",
                    "data_type": "f32le",
                    "dimension": 1,
                    "width": 64,
                    "height": 32,
                    "downsample_x": {"numerator": 1, "denominator": 1},
                    "downsample_y": {"numerator": 1, "denominator": 1},
                    "samples": [{
                        "time": 0,
                        "time_scale": 30,
                        "path": aux_source.to_string_lossy(),
                        "sampling": "hold",
                        "interpretation": "depth",
                    }],
                },
            }],
        }))
        .expect("aux host context fixture");
        let aux_report = carried(
            &scratch.join("aux.png"),
            RenderTiming::default(),
            Some(&aux_context),
        );
        assert_session_render_is_healthy(&aux_report, "aux channels");
        let _ = std::fs::remove_dir_all(&aux_source_dir);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The plain classic render, verified without the one-shot.
    ///
    /// pf_sampling_probe samples its input, so this fixture supports the
    /// sensitivity assertion the geometry probe could not (#361).
    #[test]
    fn classic_render_is_healthy_deterministic_and_input_dependent() {
        // Still takes the route lock: the tests that assert on exact deltas of
        // RENDER_SESSION_WRAPPER_RENDERS are perturbed by a concurrent session
        // render, and the two fail-closed diagnostics set a process-global
        // fault-injection env that would otherwise leak into this render.
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping classic session render: build aex_render_worker.exe and                  pf_sampling_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-classic-session-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let write_input = |name: &str, seed: u32| {
            let path = scratch.join(name);
            image::RgbaImage::from_fn(64, 32, |x, y| {
                image::Rgba([
                    ((x * 3) as u32 + seed) as u8,
                    ((y * 5) as u32 + seed * 2) as u8,
                    (x + y) as u8,
                    255,
                ])
            })
            .save(&path)
            .unwrap();
            path
        };
        let input = write_input("input.png", 0);

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_first = scratch.join("first.png");
        let first = render_experimental_image(&root, &aex, &sha, &input, &out_first, &[])
            .expect("session-route classic render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the session wrapper did not carry the render"
        );
        assert_session_render_is_healthy(&first, "classic render");
        assert_eq!(
            first.get("render_path"),
            Some(&serde_json::json!("classic"))
        );

        let out_repeat = scratch.join("repeat.png");
        let repeat = render_experimental_image(&root, &aex, &sha, &input, &out_repeat, &[])
            .expect("second session-route classic render");
        assert_reports_agree(&first, &repeat, "classic render repeat");
        assert_eq!(
            std::fs::read(&out_first).unwrap(),
            std::fs::read(&out_repeat).unwrap(),
            "the same input produced different pixels across two session renders"
        );

        let other = write_input("other.png", 61);
        let out_other = scratch.join("other-out.png");
        let other_report = render_experimental_image(&root, &aex, &sha, &other, &out_other, &[])
            .expect("session-route classic render of a different input");
        assert_session_render_is_healthy(&other_report, "classic render (other input)");
        assert_ne!(
            first.get("output_sha256"),
            other_report.get("output_sha256"),
            "a different input produced the same output; the render ignores its input"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A secondary layer plus a slider, verified without the one-shot.
    ///
    /// pf_layer_param_probe documents its output as a pure integer function of
    /// (input, layer, slider), so each of those three can be varied and the
    /// output must follow. That is a stronger statement than "the two routes
    fn scalar_key(
        time: (i32, u32),
        interpolation: AnimationInterpolation,
        value: f64,
    ) -> ParameterAnimationKey {
        ParameterAnimationKey {
            time: AnimationTime {
                value: time.0,
                scale: time.1,
            },
            interpolation,
            value: AnimationValue::Scalar { value },
        }
    }

    /// Session/one-shot A/B for parameter animation (issue #227). Until #227 the
    /// interactive dispatch handed `render_with_artifact` a fixed `"v5|"` base
    /// payload, which never matched the session wrapper's
    /// `encode_interactive_payload(parameters)` reconstruction, so the length-1
    /// session gate refused every animated render and parameter animation only
    /// ever ran one-shot. With the base payload now derived from the parameters,
    /// the gate holds and the session wrapper carries the animation sidecar. This
    /// proves the two routes are equivalent with animation present: the same
    /// public report (field-for-field, minus the volatile process/output keys)
    /// and the same PNG bytes at a keyframe time and at an interpolated
    /// mid-timeline time, and that the animated value actually moves the output
    /// over time (so a cross-route match is not a vacuous "animation ignored"
    /// pass). Gated on the locally built worker and the pf-layer-param-probe
    /// fixture, like the sibling A/B tests.

    /// agree", which held even when a parameter was ignored on both (#361).
    #[test]
    fn layer_and_slider_reach_the_plug_in_through_the_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-layer-param-probe-build/Release/pf_layer_param_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping layer+slider render: build aex_render_worker.exe and                  pf_layer_param_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-layer-slider-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        let write_layer = |name: &str, seed: u32| {
            let path = scratch.join(name);
            image::RgbaImage::from_fn(64, 32, |x, y| {
                image::Rgba([
                    ((x + y) as u32 + seed) as u8,
                    ((x * 7) as u32 + seed) as u8,
                    (y * 9) as u8,
                    255,
                ])
            })
            .save(&path)
            .unwrap();
            path
        };
        let secondary = write_layer("layer.png", 0);

        let render = |output: &Path, params: &[InteractiveParameter]| {
            let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
            let report = render_experimental_image_at_time(
                &root,
                &aex,
                &sha,
                &input,
                output,
                params,
                RenderTiming::default(),
            )
            .expect("layer+slider session render");
            assert!(
                RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
                "the layer+slider render must be carried by the session"
            );
            report
        };

        let base_params = vec![layer_parameter(1, &secondary), float_parameter(2, 200.0)];
        let out_base = scratch.join("base.png");
        let base = render(&out_base, &base_params);
        assert_session_render_is_healthy(&base, "layer+slider");
        assert_eq!(
            base.get("secondary_layers"),
            Some(&serde_json::json!([{"slot": 1, "width": 64, "height": 32}])),
            "the secondary layer did not reach the report at its declared slot              and size: {base}"
        );
        // The probe declares input + layer + slider; a count that drifts means
        // the parameter table the worker saw is not the one that was sent.
        assert_eq!(
            base.get("in_data_num_params"),
            Some(&serde_json::json!(3)),
            "the plug-in did not see all three parameters: {base}"
        );

        let out_repeat = scratch.join("repeat.png");
        let repeat = render(&out_repeat, &base_params);
        assert_reports_agree(&base, &repeat, "layer+slider repeat");

        // The slider must move the output: a render that dropped the parameter
        // would still have matched the one-shot, because the one-shot dropped it
        // the same way.
        let out_slider = scratch.join("slider.png");
        let slider = render(
            &out_slider,
            &[layer_parameter(1, &secondary), float_parameter(2, 40.0)],
        );
        assert_session_render_is_healthy(&slider, "layer+slider (other slider)");
        assert_ne!(
            base.get("output_sha256"),
            slider.get("output_sha256"),
            "changing the slider did not change the output"
        );

        // Same for the secondary layer's pixels.
        let other_layer = write_layer("layer2.png", 83);
        let out_layer = scratch.join("layer-out.png");
        let other = render(
            &out_layer,
            &[layer_parameter(1, &other_layer), float_parameter(2, 200.0)],
        );
        assert_session_render_is_healthy(&other, "layer+slider (other layer)");
        assert_ne!(
            base.get("output_sha256"),
            other.get("output_sha256"),
            "changing the secondary layer did not change the output"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn parameter_animation_drives_the_render_through_the_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // Deliberately a plain (non-canonicalized) repository root, unlike the
        // sibling tests' `repository_root()`. On Windows `canonicalize()` yields
        // a `\\?\` verbatim path, and the worker's parameter-animation sidecar
        // loader pins the sidecar's parent to `current_path()/image-transport`;
        // a verbatim sidecar path fails that string compare
        // (its `\\?\` prefix survives `canonical()` while the cwd-derived owned
        // path has none), rejecting the launch with exit 3. Production derives
        // the repository from `current_exe()` (a plain path), so this mirrors
        // production and keeps the A/B focused on transport routing rather than
        // that separate verbatim-path fragility (tracked apart from #227).
        let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_path_buf();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-layer-param-probe-build/Release/pf_layer_param_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping animation A/B: build aex_render_worker.exe and \
                 pf_layer_param_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-wrapper-anim-ab-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        let secondary = scratch.join("layer.png");
        image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x + y) as u8, (x * 7) as u8, (y * 9) as u8, 255])
        })
        .save(&secondary)
        .unwrap();

        // Slot 2 animates 20 -> 220 linearly over times 0..60 @ scale 30. The
        // render timing shares that scale, so current_time 0 evaluates to the
        // first keyframe value (20) and current_time 30 to the exact midpoint
        // (120) between the two keys.
        let params = vec![layer_parameter(1, &secondary), float_parameter(2, 20.0)];
        let animations = [ParameterAnimation {
            slot: 2,
            keys: vec![
                scalar_key((0, 30), AnimationInterpolation::Linear, 20.0),
                scalar_key((60, 30), AnimationInterpolation::Linear, 220.0),
            ],
        }];
        let timing = |current_time: i32| RenderTiming {
            current_time,
            time_step: 1,
            total_time: 60,
            time_scale: 30,
        };

        // Same preamble as the A/B this replaces; only the reference route is
        // gone. Rewriting the setup from scratch made the session worker exit
        // before frame 0, so the working setup is kept verbatim and the
        // assertions are added on top (#361).
        let render = |output: &Path, current_time: i32| {
            let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
            let report = render_experimental_image_with_parameter_animation(
                &root,
                &aex,
                &sha,
                &input,
                output,
                &params,
                &animations,
                timing(current_time),
            )
            .expect("parameter animation render");
            assert!(
                RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
                "the session wrapper did not carry the animation render (t={current_time})"
            );
            report
        };

        let out_keyframe = scratch.join("keyframe.png");
        let keyframe = render(&out_keyframe, 0);
        assert_session_render_is_healthy(&keyframe, "animation keyframe");
        let out_interpolated = scratch.join("interpolated.png");
        let interpolated = render(&out_interpolated, 30);
        assert_session_render_is_healthy(&interpolated, "animation interpolated");

        // The animated value has to reach the plug-in. A sidecar that never
        // arrived still matched the one-shot, which never received it either --
        // so this is the assertion the A/B could not make.
        assert_ne!(
            keyframe.get("output_sha256"),
            interpolated.get("output_sha256"),
            "the animated slider did not move the render between t=0 and t=30"
        );

        // And it has to evaluate to the right value, not merely to some value:
        // the keyframe time must render exactly what the static parameter does.
        let out_pinned = scratch.join("pinned.png");
        let pinned = render_experimental_image_at_time(
            &root,
            &aex,
            &sha,
            &input,
            &out_pinned,
            &[layer_parameter(1, &secondary), float_parameter(2, 20.0)],
            timing(0),
        )
        .expect("static render at the first keyframe value");
        assert_session_render_is_healthy(&pinned, "animation pinned");
        assert_eq!(
            keyframe.get("output_sha256"),
            pinned.get("output_sha256"),
            "the first keyframe did not evaluate to its own value"
        );

        // Determinism, so a difference above cannot be run-to-run noise.
        let out_again = scratch.join("again.png");
        let again = render(&out_again, 0);
        assert_reports_agree(&keyframe, &again, "animation repeat");
        let _ = std::fs::remove_dir_all(&scratch);
    }
    #[test]
    fn audio_render_goes_through_the_session_and_transforms_its_input() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/sdk-fixtures/sdk-backwards/SDK_Backwards.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping audio render: build the worker and SDK_Backwards.aex");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-audio-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let write_input = |name: &str, phase: f32| {
            let path = scratch.join(name);
            let mut bytes = Vec::with_capacity(512 * 4);
            for index in 0..512 {
                let sample = (((index as f32) * 0.05) + phase).sin() * 0.5;
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            std::fs::write(&path, &bytes).unwrap();
            (path, bytes)
        };
        let (input_path, input_bytes) = write_input("input.f32", 0.0);

        let out_first = scratch.join("first.f32");
        let first = render_experimental_audio(&root, &aex, &sha, &input_path, &out_first, &[])
            .expect("session-route audio render");
        // The audio session names itself in the report; the one-shot did not.
        // That is what proves the session carried this, now that there is no
        // second route to compare against.
        assert_eq!(
            first.get("render_path"),
            Some(&serde_json::json!("audio_session")),
            "the audio render did not go through the session: {first}"
        );
        // The audio report has its own shape: no top-level worker_classification
        // (it is nested under session_close.worker), and its own selector errors.
        // Measured against the real report rather than reused from the image
        // contract.
        let audio_health = |report: &serde_json::Value, label: &str| {
            for (key, want) in [
                ("status", serde_json::json!("render_completed")),
                ("session_clean", serde_json::json!(true)),
                ("session_invariant_failure", serde_json::json!(false)),
                ("session_protocol_violation", serde_json::json!(false)),
                ("output_created", serde_json::json!(true)),
                ("samples_finite", serde_json::json!(true)),
                ("guard_bytes_intact", serde_json::json!(true)),
                ("audio_lifetimes_balanced", serde_json::json!(true)),
                ("invalid_audio_operations", serde_json::json!(0)),
                ("audio_setup_error", serde_json::json!(0)),
                ("audio_render_error", serde_json::json!(0)),
                ("audio_setdown_error", serde_json::json!(0)),
                ("global_setup_error", serde_json::json!(0)),
                ("global_setdown_error", serde_json::json!(0)),
            ] {
                assert_eq!(
                    report.get(key),
                    Some(&want),
                    "{label}: {key} must be {want}: {report}"
                );
            }
            assert_eq!(
                report.pointer("/session_close/worker/classification"),
                Some(&serde_json::json!("ok")),
                "{label}: the audio worker did not exit cleanly: {report}"
            );
            // Absolute key presence, which two agreeing runs cannot supply: they
            // agree just as well on a key that vanished from both. A session that
            // produced correct samples but mislabelled the rate, channel count or
            // format would otherwise pass (#361 acceptance criterion 2.3).
            for key in [
                "sample_rate",
                "channels",
                "sample_format",
                "output_transport",
                "setup_range_valid",
                "output_samples",
                "input_samples",
                "input_sha256",
                "output_sha256",
                "output_start_sample",
            ] {
                assert!(
                    report.get(key).is_some_and(|value| !value.is_null()),
                    "{label}: the audio report lost {key}: {report}"
                );
            }
            for (key, want) in [
                ("sample_rate", serde_json::json!(44100)),
                ("channels", serde_json::json!(1)),
                ("sample_format", serde_json::json!("float32")),
                ("output_transport", serde_json::json!("mono_f32le_44100")),
                ("setup_range_valid", serde_json::json!(true)),
            ] {
                assert_eq!(
                    report.get(key),
                    Some(&want),
                    "{label}: {key} must be {want}: {report}"
                );
            }
        };
        audio_health(&first, "audio render");

        // Determinism, then the transform itself: SDK_Backwards reverses the
        // samples, so the output must differ from the input and must move when
        // the input moves.
        let out_repeat = scratch.join("repeat.f32");
        let repeat = render_experimental_audio(&root, &aex, &sha, &input_path, &out_repeat, &[])
            .expect("second session-route audio render");
        audio_health(&repeat, "audio repeat");
        assert_eq!(
            first.get("output_sha256"),
            repeat.get("output_sha256"),
            "the same audio input hashed differently across two renders"
        );
        assert_eq!(
            std::fs::read(&out_first).unwrap(),
            std::fs::read(&out_repeat).unwrap(),
            "the same audio input produced different samples across two renders"
        );
        let rendered = std::fs::read(&out_first).unwrap();
        assert!(!rendered.is_empty(), "the audio render wrote nothing");
        assert_ne!(
            rendered, input_bytes,
            "the effect returned its input unchanged"
        );

        let (other_path, _) = write_input("other.f32", 1.7);
        let out_other = scratch.join("other-out.f32");
        let other = render_experimental_audio(&root, &aex, &sha, &other_path, &out_other, &[])
            .expect("session-route audio render of a different input");
        assert_eq!(
            other.get("render_path"),
            Some(&serde_json::json!("audio_session")),
            "the second audio render did not go through the session: {other}"
        );
        audio_health(&other, "audio render (other input)");
        assert_ne!(
            std::fs::read(&out_first).unwrap(),
            std::fs::read(&out_other).unwrap(),
            "a different audio input produced the same output"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A custom-UI click rides the session's per-frame message and reaches the
    /// plug-in. Verified by what the click is supposed to do -- change the
    /// colour param, which RENDER then fills the frame from -- rather than by
    /// agreeing with the one-shot (#361).
    #[test]
    fn custom_ui_click_reaches_the_plug_in_and_changes_the_render() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-custom-ui-probe-build/Release/pf_custom_ui_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping custom UI click: build the worker and pf_custom_ui_probe.aex");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-customui-click-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        let render = |output: &Path, action: Option<RenderUiAction>| {
            render_experimental_image_at_time_with_format_context_and_ui_action(
                &root,
                &aex,
                &sha,
                &input,
                output,
                &[],
                RenderTiming::default(),
                false,
                RenderPixelFormat::Argb8,
                None,
                action,
            )
            .expect("custom UI session render")
        };

        // Baseline: no click, so the probe renders from its default colour.
        let out_plain = scratch.join("plain.png");
        let plain = render(&out_plain, None);
        assert_session_render_is_healthy(&plain, "custom UI baseline");
        assert_eq!(
            plain.get("custom_ui_click_dispatched"),
            Some(&serde_json::json!(false)),
            "no click was requested: {plain}"
        );

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_click = scratch.join("click.png");
        let clicked = render(
            &out_click,
            Some(RenderUiAction::Click {
                point: [20, 16],
                color: [0.85, 0.2, 0.6, 1.0],
            }),
        );
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the custom UI click render must be carried by the session"
        );
        assert_session_render_is_healthy(&clicked, "custom UI click");

        // The click was dispatched, the host picker ran, and the param moved.
        assert_eq!(
            clicked.get("custom_ui_click_dispatched"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            clicked.get("custom_ui_click_changed_value"),
            Some(&serde_json::json!(true))
        );
        // app_color_picker_calls is a worker-report field and is not flattened
        // into the public report (measured: it comes back null). What the public
        // report does expose is the suite timeline, which records the App Suite
        // being taken and returned inside the EVENT selector -- that is the
        // picker call, observed through the lease ledger.
        let timeline = clicked
            .get("suite_timeline")
            .and_then(|value| value.as_array())
            .unwrap_or_else(|| panic!("no suite_timeline in the report: {clicked}"));
        let picker_leases: Vec<_> = timeline
            .iter()
            .filter(|entry| {
                entry.get("name").and_then(|v| v.as_str()) == Some("PF AE App Suite")
                    && entry.get("selector").and_then(|v| v.as_str()) == Some("EVENT")
            })
            .collect();
        assert_eq!(
            picker_leases.len(),
            2,
            "expected one acquire and one release of the App Suite during EVENT: {clicked}"
        );
        assert!(
            picker_leases
                .iter()
                .all(|entry| entry.get("result") == Some(&serde_json::json!(0))),
            "an App Suite lease failed during the click: {clicked}"
        );
        assert_eq!(
            clicked.get("custom_ui_lifecycle_errors"),
            Some(&serde_json::json!([0, 0, 0, 0])),
            "the custom UI lifecycle reported an error: {clicked}"
        );
        assert_eq!(
            clicked.get("custom_ui_context_closed"),
            Some(&serde_json::json!(true)),
            "the custom UI context was not closed: {clicked}"
        );

        // And it reached the pixels: the probe fills the frame from the colour
        // the click stored, so the clicked render cannot equal the baseline.
        assert_ne!(
            plain.get("output_sha256"),
            clicked.get("output_sha256"),
            "the click changed the param but not the render"
        );

        // A second click with a different colour must move the output again;
        // otherwise the first difference could have been the click's mere
        // presence rather than its value.
        let out_second = scratch.join("second.png");
        let second = render(
            &out_second,
            Some(RenderUiAction::Click {
                point: [20, 16],
                color: [0.1, 0.9, 0.35, 1.0],
            }),
        );
        assert_session_render_is_healthy(&second, "custom UI click (other colour)");
        assert_ne!(
            clicked.get("output_sha256"),
            second.get("output_sha256"),
            "a different picked colour produced the same render"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A custom-UI draw rides the session's per-frame message. Verified by the
    /// draw's own lifecycle and out-flags rather than by agreeing with the
    /// one-shot (#361).
    #[test]
    fn custom_ui_draw_reaches_the_plug_in_and_completes_its_lifecycle() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-custom-ui-probe-build/Release/pf_custom_ui_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping custom UI draw: build the worker and pf_custom_ui_probe.aex");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-customui-draw-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        let render = |output: &Path, action: Option<RenderUiAction>| {
            render_experimental_image_at_time_with_format_context_and_ui_action(
                &root,
                &aex,
                &sha,
                &input,
                output,
                &[],
                RenderTiming::default(),
                false,
                RenderPixelFormat::Argb8,
                None,
                action,
            )
            .expect("custom UI draw session render")
        };

        let out_plain = scratch.join("plain.png");
        let plain = render(&out_plain, None);
        assert_session_render_is_healthy(&plain, "custom UI draw baseline");
        assert_eq!(
            plain.get("custom_ui_draw_dispatched"),
            Some(&serde_json::json!(false)),
            "no draw was requested: {plain}"
        );

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_draw = scratch.join("draw.png");
        let drawn = render(&out_draw, Some(RenderUiAction::Draw));
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the custom UI draw render must be carried by the session"
        );
        assert_session_render_is_healthy(&drawn, "custom UI draw");

        assert_eq!(
            drawn.get("custom_ui_draw_dispatched"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            drawn.get("custom_ui_draw_error"),
            Some(&serde_json::json!(0)),
            "the draw selector reported an error: {drawn}"
        );
        // PF_EO_HANDLED_EVENT: the plug-in must claim the event it drew for.
        let out_flags = drawn
            .get("custom_ui_draw_out_flags")
            .and_then(|value| value.as_u64())
            .unwrap_or_else(|| panic!("no draw out-flags: {drawn}"));
        assert_eq!(
            out_flags & 1,
            1,
            "the draw did not report the handled-event flag: {drawn}"
        );
        assert_eq!(
            drawn.get("custom_ui_lifecycle_errors"),
            Some(&serde_json::json!([0, 0, 0, 0])),
            "the custom UI lifecycle reported an error: {drawn}"
        );
        assert_eq!(
            drawn.get("custom_ui_context_closed"),
            Some(&serde_json::json!(true)),
            "the custom UI context was not closed: {drawn}"
        );

        // A draw paints the UI, not the frame: the rendered pixels must be the
        // same as without it. That direction is as important as the click's --
        // a draw that leaked into the render would be a defect.
        assert_eq!(
            plain.get("output_sha256"),
            drawn.get("output_sha256"),
            "the draw changed the rendered frame: {drawn}"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The offer itself, through a real AEX rather than a recording fake.
    ///
    /// `pf_frame_origin_offered_probe` derives its FRAME_SETUP answer from what
    /// it finds in `out_data->width/height` on entry, which is the shape issue
    /// #984 is about (AE's Basic_3D does the same and answered 1x1 from zero),
    /// and returns PF_Err_INTERNAL_STRUCT_DAMAGED if it finds zero. Every other
    /// fixture derives its answer from the layer parameter, so without this a
    /// regression that stopped the offer reaching a plug-in's out_data - the
    /// classic layout losing `world_width`, say - would leave every AEX-backed
    /// test green and only the native self-test, which drives `begin_frame`
    /// directly, would notice.
    #[test]
    fn frame_setup_is_offered_the_output_extent_through_a_real_plugin() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root
            .join("target/pf-frame-origin-probe-build/Release/pf_frame_origin_offered_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping offered-extent render: build the worker and pf_frame_origin_offered_probe.aex"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-offered-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        let output = scratch.join("out.png");
        let report = render_experimental_image(&root, &aex, &sha, &input, &output, &[])
            .expect("session-route offered-extent render");
        assert_session_render_is_healthy(&report, "offered-extent render");
        // 64x48 was offered and the probe added its delta to it. Had the offer
        // not happened the probe would have refused the frame outright.
        assert_eq!(report.get("width"), Some(&serde_json::json!(68)));
        assert_eq!(report.get("height"), Some(&serde_json::json!(52)));
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A classic expand that also states PF_OutData::origin has to come back
    /// with the frame report's own convention, which is the negation of it.
    ///
    /// The resize probes never state an origin, so before this the classic
    /// negation at `frame.origin_x = -classic_output.input_origin_x` had no
    /// end-to-end coverage at all: a sign error would have placed every
    /// expanded classic frame on the opposite side of the layer origin with the
    /// whole suite green. The probe also fails its own RENDER with
    /// PF_Err_INTERNAL_STRUCT_DAMAGED unless the host relayed the origin back
    /// through `in_data->output_origin_x/y`, so a healthy render is itself the
    /// evidence that the relay happened.
    #[test]
    fn a_classic_expand_reports_its_origin_in_layer_coordinates() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-frame-origin-probe-build/Release/pf_frame_origin_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping origin render: build the worker and pf_frame_origin_probe.aex");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-origin-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        let output = scratch.join("out.png");
        let report = render_experimental_image(&root, &aex, &sha, &input, &output, &[])
            .expect("session-route origin render");
        assert_session_render_is_healthy(&report, "origin render");

        // The probe grows by 4 on each axis and states origin (3,3).
        assert_eq!(report.get("width"), Some(&serde_json::json!(68)));
        assert_eq!(report.get("height"), Some(&serde_json::json!(52)));
        // What the host put into in_data for the plug-in, in PF_OutData::origin's
        // own convention. The probe already refuses to render unless it reads
        // back what it stated; this pins it in the report too, so a relay that
        // silently stopped happening is visible here and not only as a changed
        // error code.
        assert_eq!(
            report.get("output_origin"),
            Some(&serde_json::json!([3, 3])),
            "the stated origin did not reach in_data: {report}"
        );

        // The negated, layer-relative origin never reaches the image report -
        // `image_render::session` destructures FrameStatus::Rendered and drops it
        // - so the conversion has to be read off the session frame directly.
        // Without this the sign could be flipped in worker_render_session.cpp and
        // nothing in the suite would notice: in_data holds 3 either way.
        let mut session = RenderSession::open(SessionOpenRequest {
            repository: &root,
            plugin_path: &aex,
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
            companions: Vec::new(),
            dependency_search_dirs: vec![aex.parent().unwrap().to_path_buf()],
            width: 64,
            height: 48,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 1,
            time_scale: 1,
            frame_deadline: Duration::from_secs(30),
            smart: false,
            gpu_backend: RenderGpuBackend::Cpu,
            gpu_runtime_policy: None,
            launch_environment: LaunchEnvironment::default(),
        })
        .expect("open a classic session on the origin probe");
        let frame = session
            .render_frame(0, 0, &vec![0x7fu8; 64 * 48 * 4])
            .expect("the origin probe renders one session frame");
        match frame.status {
            FrameStatus::Rendered {
                width,
                height,
                origin_x,
                origin_y,
                ..
            } => {
                assert_eq!(
                    (width, height),
                    (68, 52),
                    "the session frame did not expand"
                );
                // The input's top-left sits 3px inside the grown buffer, so the
                // buffer's own top-left sits 3px outside the layer.
                assert_eq!(
                    (origin_x, origin_y),
                    (-3, -3),
                    "PF_OutData::origin was not negated into layer coordinates"
                );
            }
            FrameStatus::FrameError { render_error, .. } => {
                panic!("the origin probe frame failed with {render_error}")
            }
            FrameStatus::SmartOutputUntouched => {
                panic!("the classic origin probe reported a Smart-only untouched output")
            }
        }
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The sibling of the expand test below, for the other direction. An effect
    /// that declared PF_OutFlag_I_SHRINK_BUFFER and answers a smaller extent at
    /// FRAME_SETUP must render at that extent rather than be refused by the
    /// host's output validation.
    ///
    /// This exists because it was missing. The shrink fixture has been built
    /// alongside the expand one since #262 with nothing consuming it, so when
    /// issue #984's origin bound was first written as "the whole source has to
    /// fit inside the output" - unsatisfiable for any shrink - every declared
    /// shrink began failing output validation and tearing down the session, and
    /// the whole suite stayed green.
    #[test]
    fn shrink_output_renders_at_the_smaller_extent() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex =
            root.join("target/pf-frame-resize-probe-build/Release/pf_shrink_allowed_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping shrink render: build the worker and pf_shrink_allowed_probe.aex");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-shrink-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        let output = scratch.join("out.png");
        let report = render_experimental_image(&root, &aex, &sha, &input, &output, &[])
            .expect("session-route shrink render");
        assert_session_render_is_healthy(&report, "shrink render");

        // The probe answers `params[0]->u.ld.width + AEXCOMPAT_RESIZE_DELTA`
        // with a negative delta of 4, so 64x48 becomes 60x44. The report has to
        // describe the shrunk frame while still naming the full-size input.
        assert_eq!(report.get("width"), Some(&serde_json::json!(60)));
        assert_eq!(report.get("height"), Some(&serde_json::json!(44)));
        assert_eq!(report.get("input_width"), Some(&serde_json::json!(64)));
        assert_eq!(report.get("input_height"), Some(&serde_json::json!(48)));

        let decoded = image::open(&output).expect("decode the shrunk PNG");
        assert_eq!(
            (decoded.width(), decoded.height()),
            (60, 44),
            "the written PNG is not the shrunk frame"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// An effect that expands its output past the launch slot grows the shared
    /// section in place (#262). Verified by the lifecycle the grow must not
    /// replay, not by agreeing with the one-shot (#361).
    #[test]
    fn expand_output_grows_in_place_without_replaying_the_lifecycle() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex =
            root.join("target/pf-frame-resize-probe-build/Release/pf_expand_allowed_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping expand render: build the worker and pf_expand_allowed_probe.aex");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-expand-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        // The probe appends one byte per lifecycle selector in every worker it is
        // spawned into: 'S' for FRAME_SETUP, 'R' for RENDER. That is the only
        // cross-process observable of a re-opened worker. An expand that overran
        // the launch slot and re-opened the session (the pre-#262 behaviour) runs
        // each selector once in the discarded worker and again in the
        // replacement; the in-session grow keeps the same worker, so each runs
        // exactly once. Self-computed, so it needs no reference route.
        let render_log = scratch.join("selector-dispatches.bin");
        std::fs::remove_file(&render_log).ok();
        unsafe { std::env::set_var("AEXCOMPAT_RESIZE_RENDER_LOG", &render_log) };
        let count_marker = |marker: u8| -> usize {
            std::fs::read(&render_log)
                .map(|bytes| bytes.iter().filter(|byte| **byte == marker).count())
                .unwrap_or(0)
        };

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output = scratch.join("out.png");
        let report = render_experimental_image(&root, &aex, &sha, &input, &output, &[])
            .expect("session-route expand render");
        unsafe { std::env::remove_var("AEXCOMPAT_RESIZE_RENDER_LOG") };
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the session wrapper did not carry the expand render"
        );
        assert_session_render_is_healthy(&report, "expand render");

        // The output really expanded past the input, and the report describes the
        // expanded frame rather than the launch slot.
        assert_eq!(report.get("width"), Some(&serde_json::json!(68)));
        assert_eq!(report.get("height"), Some(&serde_json::json!(52)));
        assert_eq!(report.get("input_width"), Some(&serde_json::json!(64)));
        assert_eq!(report.get("input_height"), Some(&serde_json::json!(48)));

        // Exactly-once for the whole lifecycle: the grow must not replay
        // FRAME_SETUP or RENDER, which a re-open into a second worker would.
        let (setups, renders) = (count_marker(b'S'), count_marker(b'R'));
        assert_eq!(
            (setups, renders),
            (1, 1),
            "the expand ran FRAME_SETUP {setups}x and RENDER {renders}x (expected 1/1)"
        );

        // The PNG must carry the expanded frame, not a slot-sized crop.
        let decoded = image::open(&output).expect("decode the expanded PNG");
        assert_eq!(
            (decoded.width(), decoded.height()),
            (68, 52),
            "the written PNG is not the expanded frame"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Fail-closed fallback removal (#264, #98 W4): when the classic session is
    /// attempted (eligible render, escape hatch unset) but cannot carry the
    /// render, the render must surface an explicit error instead of silently
    /// rerunning the one-shot transport (which would let the session path rot
    /// undetected). The fault-injection env forces the session wrapper to report
    /// a Fallback without a real failure; the caller must then error (naming the
    /// override) and leave the session-carried counter unchanged — neither a
    /// session success nor a silent one-shot. On the pre-#264 code this test
    /// fails: the forced Fallback fell through to a successful one-shot render.
    ///
    /// Debug-only: the fault-injection knob it drives is compiled out of release
    /// builds (image_render.rs), so this test is gated to debug too.
    #[cfg(debug_assertions)]
    #[test]
    fn session_infra_failure_fails_closed_without_silent_one_shot() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping fail-closed test: build aex_render_worker.exe and pf_sampling_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-failclosed-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        // Eligible render; the fault injection forces the session to report a
        // Fallback.
        unsafe { std::env::set_var(FORCE_SESSION_FALLBACK_ENV, "1") };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output = scratch.join("out.png");
        let result = render_experimental_image(&root, &aex, &sha, &input, &output, &[]);
        unsafe { std::env::remove_var(FORCE_SESSION_FALLBACK_ENV) };

        let error = result.expect_err("a forced session fallback must fail closed, not fall back");
        let message = error.to_string();
        assert!(
            message.contains("no alternate transport"),
            "the fail-closed error must state that no other transport exists; got: {message}"
        );
        // Secondary sanity check: the session wrapper did not carry a render.
        // (This counter only tracks the session path, so it alone does not
        // distinguish fail-closed from a silent rerun; the discriminators are
        // the Err above and the absent output PNG below, both of which a silent
        // one-shot success would violate.)
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            before,
            "no session render should have been carried"
        );
        assert!(
            !output.exists(),
            "a fail-closed render must not write an output PNG (a silent one-shot would)"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A secondary layer larger than the primary input: the session sizes the
    /// layer slot to the layer's own dimensions rather than the primary's.
    /// Verified against that property, not against the one-shot (#361).
    #[test]
    fn oversized_layer_is_carried_at_its_own_dimensions() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-layer-param-probe-build/Release/pf_layer_param_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!("skipping oversized-layer render: build the worker and the layer probe");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-oversized-layer-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        // The layer's pixel count (64*48) exceeds the primary's (40*30), which is
        // what used to push this shape off the session.
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(40, 30, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        // The seed has to move a channel the plug-in actually reads. This probe
        // takes green from the layer and blue from the mean of input and layer,
        // and never reads the layer's red -- measured: seeding red alone leaves
        // the output byte-identical, which reads like a transport failure and is
        // not one.
        let write_layer = |name: &str, seed: u32| {
            let path = scratch.join(name);
            image::RgbaImage::from_fn(64, 48, |x, y| {
                image::Rgba([
                    (x + y) as u8,
                    ((x * 7) as u32 + seed) as u8,
                    ((y * 9) as u32 + seed) as u8,
                    255,
                ])
            })
            .save(&path)
            .unwrap();
            path
        };
        let secondary = write_layer("layer.png", 0);

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output = scratch.join("out.png");
        let report = render_experimental_image(
            &root,
            &aex,
            &sha,
            &input,
            &output,
            &[layer_parameter(1, &secondary)],
        )
        .expect("session-route oversized-layer render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the oversized-layer render must be carried by the session"
        );
        assert_session_render_is_healthy(&report, "oversized layer");

        // The layer reached the worker at its own size, not cropped to the
        // primary. That is the capability this shape exercises.
        let layers = report
            .get("secondary_layers")
            .and_then(|value| value.as_array())
            .unwrap_or_else(|| panic!("no secondary_layers in the report: {report}"));
        assert_eq!(layers.len(), 1, "expected one secondary layer: {report}");
        assert_eq!(
            (layers[0].get("width"), layers[0].get("height")),
            (Some(&serde_json::json!(64)), Some(&serde_json::json!(48))),
            "the secondary layer was not carried at its own dimensions: {report}"
        );
        // The output stays the primary's size; an oversized layer must not resize
        // the frame.
        assert_eq!(report.get("width"), Some(&serde_json::json!(40)));
        assert_eq!(report.get("height"), Some(&serde_json::json!(30)));
        // This fixture declares num_params=1 while the render supplies a layer,
        // so the parameter-count contract legitimately reports false and raises
        // the warning. Pinned here rather than hidden: a change to true would
        // mean the probe or the counting rule moved.
        assert_eq!(
            report.get("parameter_count_contract_ok"),
            Some(&serde_json::json!(false)),
            "pf_layer_param_probe under-declares its parameters; expected the              contract observation to say so: {report}"
        );
        assert_eq!(
            report.get("host_contract_warning"),
            Some(&serde_json::json!(true)),
            "the parameter-count observation must raise the warning: {report}"
        );

        // The oversized layer's pixels reach the plug-in: changing them changes
        // the output.
        let other_layer = write_layer("layer2.png", 91);
        let out_other = scratch.join("other.png");
        let other = render_experimental_image(
            &root,
            &aex,
            &sha,
            &input,
            &out_other,
            &[layer_parameter(1, &other_layer)],
        )
        .expect("session-route oversized-layer render with different layer pixels");
        assert_session_render_is_healthy(&other, "oversized layer (other pixels)");
        assert_ne!(
            report.get("output_sha256"),
            other.get("output_sha256"),
            "changing the oversized layer did not change the output"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A zero-duration render (total_time == 0) is the single t=0 frame; the
    /// session carries it since #272. Verified against the timing contract
    /// rather than against the one-shot (#361).
    #[test]
    fn zero_duration_render_produces_the_single_frame_on_the_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping zero-duration render: build aex_render_worker.exe and                  pf_sampling_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-zero-duration-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([(x * 5) as u8, (y * 3) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        let zero = RenderTiming {
            current_time: 0,
            time_step: 1,
            total_time: 0,
            time_scale: 30,
        };

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_zero = scratch.join("zero.png");
        let report =
            render_experimental_image_at_time(&root, &aex, &sha, &input, &out_zero, &[], zero)
                .expect("session-route zero-duration render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the zero-duration render must be carried by the session"
        );
        assert_session_render_is_healthy(&report, "zero-duration render");
        // The timing the caller asked for has to be the timing the report
        // describes; a zero duration must not be silently widened.
        assert_eq!(
            report.get("total_time"),
            Some(&serde_json::json!(0)),
            "the report did not preserve the zero duration: {report}"
        );
        assert_eq!(
            report.get("current_time"),
            Some(&serde_json::json!(0)),
            "the single frame must be t=0: {report}"
        );

        // The frame a zero duration produces is the same one a non-zero duration
        // produces at t=0. That is the property the A/B against the one-shot was
        // standing in for, and it is checkable without the one-shot.
        let out_span = scratch.join("span.png");
        let span_report = render_experimental_image_at_time(
            &root,
            &aex,
            &sha,
            &input,
            &out_span,
            &[],
            RenderTiming {
                current_time: 0,
                time_step: 1,
                total_time: 300,
                time_scale: 30,
            },
        )
        .expect("session-route t=0 render over a non-zero duration");
        assert_session_render_is_healthy(&span_report, "t=0 over a span");
        assert_eq!(
            report.get("output_sha256"),
            span_report.get("output_sha256"),
            "the zero-duration frame differs from t=0 of a non-zero duration"
        );
        assert_eq!(
            std::fs::read(&out_zero).unwrap(),
            std::fs::read(&out_span).unwrap(),
            "the zero-duration PNG differs from the t=0 PNG"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A conformance render carries the render-settings trailer into the
    /// session (#275). Verified by the effect it is asked for -- the alpha
    /// pre-transform -- and by the setting not surviving its own scope, rather
    /// than by agreeing with the one-shot (#361).
    #[test]
    fn conformance_render_settings_change_the_render_and_do_not_leak() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping conformance render: build aex_render_worker.exe and \
                 pf_sampling_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-conformance-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        // Alpha below 255 so the premultiply pre-transform actually changes the
        // colour channels; an opaque input would make the comparison vacuous.
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([(x * 5) as u8, (y * 3) as u8, (x + y) as u8, 200])
        })
        .save(&input)
        .unwrap();

        struct EnvVarGuard {
            name: &'static str,
            previous: Option<std::ffi::OsString>,
        }
        impl EnvVarGuard {
            fn set(name: &'static str, value: &str) -> Self {
                let previous = std::env::var_os(name);
                unsafe { std::env::set_var(name, value) };
                Self { name, previous }
            }
        }
        impl Drop for EnvVarGuard {
            fn drop(&mut self) {
                match &self.previous {
                    Some(value) => unsafe { std::env::set_var(self.name, value) },
                    None => unsafe { std::env::remove_var(self.name) },
                }
            }
        }

        // Baseline first, with no conformance settings at all.
        let out_plain = scratch.join("plain.png");
        let plain = render_experimental_image(&root, &aex, &sha, &input, &out_plain, &[])
            .expect("session-route plain render");
        assert_session_render_is_healthy(&plain, "conformance baseline");

        let guard = EnvVarGuard::set(
            "AEXCOMPAT_CONFORMANCE_RENDER_SETTINGS",
            "v1|premultiplied|0|-|0|software",
        );
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_conf = scratch.join("conformance.png");
        let conformance = render_experimental_image(&root, &aex, &sha, &input, &out_conf, &[])
            .expect("session-route conformance render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the conformance render must be carried by the session"
        );
        assert_session_render_is_healthy(&conformance, "conformance render");

        // What this test can and cannot see. The flattened public report carries
        // no render_settings block -- that field lives in the worker report the
        // session close returns. `premultiplication` looks like a way in, since
        // the worker sources it from the trailer, but it is projected only on the
        // smart path; a classic render reports null for it whatever the trailer
        // says. And picking a non-default alpha mode to make it observable does
        // not work either: "straight" is the identity for the broker's
        // pre-transform, so it would erase the input difference asserted below.
        // Both measured. The worker-side arrival of this trailer is therefore not
        // observable from a classic public report at all, and the session
        // protocol's own tests are what cover it.
        //
        // What is observable is the effect the setting is asked for: the broker's
        // alpha pre-transform rewrites the input the plug-in sees.
        assert_ne!(
            plain.get("input_sha256"),
            conformance.get("input_sha256"),
            "the premultiply pre-transform did not change the input"
        );
        assert_ne!(
            plain.get("output_sha256"),
            conformance.get("output_sha256"),
            "a premultiplied translucent input rendered identically to the plain one"
        );

        drop(guard);
        // Without the env the render must go back to the baseline, so the
        // setting is not leaking into later renders through the session.
        let out_after = scratch.join("after.png");
        let after = render_experimental_image(&root, &aex, &sha, &input, &out_after, &[])
            .expect("session-route render after the conformance env cleared");
        assert_eq!(
            plain.get("output_sha256"),
            after.get("output_sha256"),
            "the conformance setting leaked past its guard"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Audio fail-closed fallback removal (#264, #98 W4): when the audio session
    /// cannot carry the render, the render must surface an explicit error
    /// instead of silently rerunning the one-shot `--render-audio` transport.
    /// The fault-injection env forces the audio session wrapper to report a
    /// Fallback without a real failure; the caller must then error and write no
    /// output. On the pre-#264 code the forced Fallback fell through to a
    /// successful one-shot render; since #365 that transport no longer exists,
    /// so the error states there is no alternate transport at all.
    ///
    /// Debug-only: the fault-injection knob it drives is compiled out of release
    /// builds (image_render.rs), so this test is gated to debug too.
    #[cfg(debug_assertions)]
    #[test]
    fn audio_session_failure_fails_closed_without_silent_one_shot() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/sdk-fixtures/sdk-backwards/SDK_Backwards.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping audio fail-closed test: build aex_render_worker.exe and \
                 SDK_Backwards.aex (tools/build-sdk-backwards.ps1) first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-audio-failclosed-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input_path = scratch.join("input.f32");
        let samples: Vec<f32> = (0..256).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
        let mut input_bytes = Vec::with_capacity(samples.len() * 4);
        for sample in &samples {
            input_bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&input_path, &input_bytes).unwrap();

        // Eligible audio render; the fault injection forces the session to
        // report a Fallback.
        unsafe { std::env::set_var(FORCE_SESSION_FALLBACK_ENV, "1") };
        let output = scratch.join("out.f32");
        let result = render_experimental_audio(&root, &aex, &sha, &input_path, &output, &[]);
        unsafe { std::env::remove_var(FORCE_SESSION_FALLBACK_ENV) };

        let error =
            result.expect_err("a forced audio session fallback must fail closed, not fall back");
        let message = error.to_string();
        assert!(
            message.contains("no alternate transport"),
            "the fail-closed error must state that no other transport exists; got: {message}"
        );
        assert!(
            !output.exists(),
            "a fail-closed audio render must not write an output file (a silent one-shot would)"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Resolve a pf-visual-audio-probe artifact across the layouts its build can
    /// produce. `tools/build-pf-visual-audio-probe.ps1` uses a private
    /// multi-config tree and takes `-Configuration Debug|Release`, so both
    /// config subdirectories are searched. The `instruments-build` candidates
    /// cover a hand-run configure of `instruments` (single-config Ninja, or
    /// multi-config); CI does not build this probe at all, since its Ninja
    /// configure of `instruments` deliberately runs without AE_SDK_ROOT and the
    /// probe lives inside that guard. Returns `None` when the probe is unbuilt.
    fn visual_audio_probe(root: &Path, target: &str) -> Option<PathBuf> {
        let private = root.join("target/pf-visual-audio-probe-build/pf-visual-audio-probe");
        ["Release", "Debug"]
            .into_iter()
            .map(|configuration| private.join(configuration).join(format!("{target}.aex")))
            .chain([
                root.join(format!(
                    "target/instruments-build/pf-visual-audio-probe/{target}.aex"
                )),
                root.join(format!(
                    "target/instruments-build/pf-visual-audio-probe/Release/{target}.aex"
                )),
            ])
            .find(|path| path.is_file())
    }

    /// Image render + audio sidecar on the classic session (#339). Verified by
    /// the plug-in reading back exactly the window it asked for, rather than by
    /// agreeing with the one-shot (#361).
    #[test]
    fn image_audio_sidecar_reaches_the_plug_in_through_the_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let Some(aex) = visual_audio_probe(&root, "pf_visual_audio_sidecar_probe") else {
            eprintln!("skipping image+audio: run tools/build-pf-visual-audio-probe.ps1 first");
            return;
        };
        if !worker.is_file() {
            eprintln!("skipping image+audio: build aex_render_worker.exe first");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-image-audio-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([(x * 5) as u8, (y * 3) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        // The probe checks out samples 4..9 at 44100 and returns PF_Err_NONE only
        // when it reads back 0.25 at the window start and 0.5625 at its last
        // in-range sample. A render that completes is therefore itself the
        // assertion that the sidecar arrived intact -- no reference route needed.
        let write_sidecar = |name: &str, correct: bool| {
            let path = scratch.join(name);
            let mut samples = [0.0f32; 10];
            samples[4] = if correct { 0.25 } else { 0.75 };
            samples[9] = 0.5625;
            std::fs::write(
                &path,
                samples
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect::<Vec<u8>>(),
            )
            .unwrap();
            path
        };
        let sidecar = write_sidecar("audio.f32", true);

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output = scratch.join("out.png");
        let report = render_experimental_image_with_audio_sidecar(
            &root,
            &aex,
            &sha,
            &input,
            &sidecar,
            &output,
            &[],
            RenderTiming::default(),
        )
        .expect("session-route audio sidecar render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the audio sidecar render must be carried by the session"
        );
        assert_session_render_is_healthy(&report, "image+audio");

        // The audio telemetry has to be in the public report, and describe the
        // window the probe asked for.
        assert_eq!(
            report.get("audio_sidecar_transport"),
            Some(&serde_json::json!("mono_f32le_44100"))
        );
        for (key, want) in [
            ("audio_usage_advertised", serde_json::json!(true)),
            ("audio_checkout_allowed", serde_json::json!(true)),
            // audio_source_available is a worker-report field and is not among
            // the keys the public report projects (measured); the gate checks it
            // on the broker side before this report is built.
            ("audio_lifetimes_balanced", serde_json::json!(true)),
            ("invalid_audio_operations", serde_json::json!(0)),
            ("audio_checkout_calls", serde_json::json!(1)),
            ("audio_checkin_calls", serde_json::json!(1)),
            ("audio_get_data_calls", serde_json::json!(1)),
            ("last_audio_checkout_start_time", serde_json::json!(4)),
            ("last_audio_checkout_duration", serde_json::json!(6)),
            ("last_audio_checkout_time_scale", serde_json::json!(44100)),
        ] {
            assert_eq!(
                report.get(key),
                Some(&want),
                "{key} must be {want}: {report}"
            );
        }
        // The digest is of the bytes that were handed over, so it must follow the
        // sidecar rather than being a constant.
        let digest = report
            .get("audio_sidecar_input_sha256")
            .and_then(|value| value.as_str())
            .unwrap_or_else(|| panic!("no sidecar digest: {report}"))
            .to_owned();
        assert_eq!(
            digest,
            format!("{:x}", Sha256::digest(std::fs::read(&sidecar).unwrap())),
            "the reported digest is not the sidecar that was passed"
        );

        // A sidecar whose window does not match is refused, which is what proves
        // the samples reach the plug-in rather than merely being transported.
        let wrong = write_sidecar("wrong.f32", false);
        let out_wrong = scratch.join("wrong.png");
        let refused = render_experimental_image_with_audio_sidecar(
            &root,
            &aex,
            &sha,
            &input,
            &wrong,
            &out_wrong,
            &[],
            RenderTiming::default(),
        );
        assert!(
            refused.is_err(),
            "a sidecar with the wrong samples rendered anyway: {refused:?}"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Combined classic audio + secondary layer (#341): the session has
    /// independent transports for its audio trailer and its inherited layer
    /// handles, so it carries both in one render. The fixture folds the
    /// checked-out audio window and the layer pixels into the PNG, so changing
    /// either input independently must change the output. The legacy
    /// `--render-image-audio` command was pinned to 16 argv slots and could
    /// never express this shape; #365 deleted it rather than extending it, so
    /// the former forced-one-shot assertion here has no transport left to name.
    #[test]
    fn image_audio_and_secondary_layer_are_jointly_consumed_by_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let Some(aex) = visual_audio_probe(&root, "pf_visual_audio_layer_sidecar_probe") else {
            eprintln!(
                "skipping image+audio+layer session test: run \
                 tools/build-pf-visual-audio-probe.ps1 \
                 -Target pf_visual_audio_layer_sidecar_probe first"
            );
            return;
        };
        if !worker.is_file() {
            eprintln!("skipping image+audio+layer session test: build aex_render_worker.exe first");
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-image-audio-layer-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([(x * 5) as u8, (y * 3) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();

        let layer_a = scratch.join("layer-a.png");
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([(x + y) as u8, (x * 3) as u8, (y * 7) as u8, 255])
        })
        .save(&layer_a)
        .unwrap();
        let layer_b = scratch.join("layer-b.png");
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([(x * 2) as u8, (y * 5) as u8, (x * y) as u8, 255])
        })
        .save(&layer_b)
        .unwrap();

        let write_audio = |name: &str, start: f32, end: f32| {
            let mut samples = [0.0f32; 10];
            samples[4] = start;
            samples[9] = end;
            let path = scratch.join(name);
            std::fs::write(
                &path,
                samples
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect::<Vec<u8>>(),
            )
            .unwrap();
            path
        };
        let audio_a = write_audio("audio-a.f32", 0.25, 0.5625);
        let audio_b = write_audio("audio-b.f32", 0.75, 0.125);

        let render = |output: &Path, audio: &Path, layer: &Path| {
            let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
            let parameters = vec![layer_parameter(1, layer)];
            let report = render_experimental_image_with_audio_sidecar(
                &root,
                &aex,
                &sha,
                &input,
                audio,
                output,
                &parameters,
                RenderTiming::default(),
            )
            .expect("combined audio+layer session render");
            assert!(
                RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
                "combined audio+layer render did not use the session"
            );
            report
        };

        let baseline = scratch.join("baseline.png");
        let baseline_report = render(&baseline, &audio_a, &layer_a);
        assert_eq!(
            baseline_report.get("audio_checkout_calls"),
            Some(&serde_json::json!(1)),
            "the combined fixture did not consume the audio window"
        );
        assert_eq!(
            baseline_report.get("last_audio_checkout_start_time"),
            Some(&serde_json::json!(4))
        );
        assert_eq!(
            baseline_report.get("last_audio_checkout_duration"),
            Some(&serde_json::json!(6))
        );
        assert_eq!(
            baseline_report.get("secondary_layers"),
            Some(&serde_json::json!([{"slot": 1, "width": 48, "height": 32}])),
            "the combined fixture did not receive secondary-layer slot 1"
        );
        let baseline_bytes = std::fs::read(&baseline).unwrap();

        let changed_audio = scratch.join("changed-audio.png");
        render(&changed_audio, &audio_b, &layer_a);
        assert_ne!(
            baseline_bytes,
            std::fs::read(&changed_audio).unwrap(),
            "changing only the audio window did not change the output"
        );

        let changed_layer = scratch.join("changed-layer.png");
        render(&changed_layer, &audio_a, &layer_b);
        assert_ne!(
            baseline_bytes,
            std::fs::read(&changed_layer).unwrap(),
            "changing only the secondary-layer pixels did not change the output"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// The audio gate must apply on the session route too (issue #339): a
    /// plug-in that never advertised `PF_OutFlag_I_USE_AUDIO` cannot be handed
    /// an audio source, and both routes must refuse it.
    ///
    /// The session wrapper hardcoded `audio_present: false` before this issue,
    /// so it skipped the gate entirely and rendered a plug-in the one-shot
    /// rejected. The byte-equivalence A/B above cannot catch that, because its
    /// fixture does advertise audio and passes the gate either way.
    #[test]
    fn an_unadvertised_plugin_with_a_sidecar_is_refused_by_the_session() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        // pf_sampling_probe never advertises audio and never touches the audio
        // suite, so the rest of its report is clean and the only thing that can
        // refuse it is the audio gate itself. pf_visual_audio_unadvertised_probe
        // does not work here: it deliberately *attempts* an unadvertised
        // checkout, so the worker already marks the render failed and the
        // session refuses at close, before the broker gate is consulted.
        let aex = root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping unadvertised-audio gate check: build aex_render_worker.exe and \
                 pf_sampling_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-unadvertised-audio-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 40, 60, 255]))
            .save(&input)
            .unwrap();
        let sidecar = scratch.join("audio.f32");
        std::fs::write(&sidecar, [0u8; 40]).unwrap();

        // Only the session is exercised now: the one-shot half of this check went
        // away with the rest of the A/Bs (#361). The gate itself is route-shared,
        // and the session is the route that has to refuse.
        {
            let label = "session";
            let output = scratch.join(format!("out-{label}.png"));
            let result = render_experimental_image_with_audio_sidecar(
                &root,
                &aex,
                &sha,
                &input,
                &sidecar,
                &output,
                &[],
                RenderTiming::default(),
            );
            let error = result.err().unwrap_or_else(|| {
                panic!("the {label} route accepted a sidecar for an unadvertised plug-in")
            });
            assert!(
                error.to_string().contains("failed validation"),
                "the {label} route refused for the wrong reason: {error}"
            );
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
