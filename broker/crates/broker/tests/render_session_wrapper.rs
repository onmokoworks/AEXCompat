//! A/B equivalence for the length-1 session wrapper (issue #98 stage W2):
//! the resident-session route must produce the same public
//! `interactive_image_render` report and PNG bytes as the one-shot argv
//! transport for a plain classic render. Requires the real render worker
//! executable and the pf_sampling_probe fixture from this checkout; skips
//! (with a message) when either is not built.

mod common;

#[cfg(test)]
#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{
        AnimationInterpolation, AnimationTime, AnimationValue, DISABLE_SESSION_WRAPPER_ENV,
        InteractiveParameter, ParameterAnimation, ParameterAnimationKey,
        RENDER_SESSION_WRAPPER_RENDERS, RenderGpuBackend, RenderPixelFormat, RenderTiming,
        RenderUiAction, TimedLayerImage, render_experimental_audio, render_experimental_image,
        render_experimental_image_at_time,
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
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root")
    }

    // Both tests toggle the process-global DISABLE_SESSION_WRAPPER_ENV to force
    // the one-shot route, and both assert on RENDER_SESSION_WRAPPER_RENDERS
    // deltas. cargo runs a binary's tests concurrently, so without this lock one
    // test's forced one-shot could bleed into the other's session-routing
    // assertion. Serialize the env-sensitive tests.
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
        if crate::common::skip_without_restricted_token_launch(
            "smart_single_image_renders_deterministically_and_follows_its_input",
        ) {
            return;
        }
        // Still takes the route lock: the A/Bs that remain assert on exact
        // deltas of RENDER_SESSION_WRAPPER_RENDERS, and a concurrent session
        // render perturbs them. Measured -- without this, conformance_render_settings
        // fails with "the escape hatch did not force the one-shot transport".
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

    #[test]
    fn smart_argb32f_auto_is_session_canonical_and_pixel_matches_one_shot() {
        if crate::common::skip_without_restricted_token_launch(
            "smart_argb32f_auto_is_session_canonical_and_pixel_matches_one_shot",
        ) {
            return;
        }
        // Argb32f smart Auto (policy-none) is now carried by the length-1 session
        // (#292): the session folds Auto to CPU and renders on
        // --smart-session32-cpu-v1. One-shot Argb32f Auto fails its policy-less
        // GPU preflight *before touching any device* and falls back to the same
        // CPU render, so the pixels match, but one-shot records
        // gpu_attempt/gpu_fallback_used while the session does not. Per W4 (#264)
        // the session is the anchor, so its no-GPU-attempt report is canonical;
        // assert the pixel equivalence and pin the intended report divergence.
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex =
            root.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping Argb32f smart Auto A/B: build aex_smart_worker.exe and \
                 pf_smart_geometry_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-smart32-ab-{}-{:032x}",
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
        let render = |output: &Path| {
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
                RenderGpuBackend::Auto,
            )
        };

        // Run A: default routing carries Argb32f Auto on the session.
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_a = scratch.join("a.png");
        let report_a = render(&out_a).expect("session-route Argb32f Auto smart render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the Argb32f Auto smart render must now be carried by the session"
        );

        // Run B: escape hatch forces the one-shot transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let out_b = scratch.join("b.png");
        let report_b = render(&out_b);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot Argb32f Auto smart render");

        // Pixel equivalence: both render on the smart worker's CPU path.
        assert_eq!(
            report_a.get("output_sha256"),
            report_b.get("output_sha256"),
            "the Argb32f Auto render output must match between the routes"
        );
        assert!(
            report_a
                .get("output_sha256")
                .is_some_and(|value| !value.is_null()),
            "the session render must report an output_sha256: {report_a}"
        );

        // Session-canonical report: the session takes no GPU attempt.
        assert_eq!(
            report_a.get("gpu_fallback_used"),
            Some(&serde_json::Value::Bool(false)),
            "the session must not report a GPU fallback: {report_a}"
        );
        assert_eq!(
            report_a.get("gpu_attempt"),
            Some(&serde_json::Value::Null),
            "the session must record no GPU attempt: {report_a}"
        );

        // One-shot records the futile policy-less preflight and CPU fallback:
        // exactly the artifact the session drops. Pinning it fixes the intended
        // divergence so a later change cannot silently converge or widen it.
        assert_eq!(
            report_b.get("gpu_fallback_used"),
            Some(&serde_json::Value::Bool(true)),
            "one-shot Argb32f Auto must record the CPU fallback: {report_b}"
        );
        assert!(
            report_b
                .get("gpu_attempt")
                .is_some_and(|value| !value.is_null()),
            "one-shot Argb32f Auto must record the GPU preflight attempt: {report_b}"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn smart_timed_multilayer_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "smart_timed_multilayer_matches_the_one_shot_transport",
        ) {
            return;
        }
        // Smart sessions now carry the secondary-layer trailer (#294): a SmartFX
        // effect that checks out three timed layers renders byte-identically on
        // the length-1 session and the one-shot --smart-image16-layer route.
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex = root.join(
            "target/pf-smart-timed-multilayer-probe-build/Release/pf_smart_timed_multilayer_probe.aex",
        );
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping smart timed-multilayer A/B: build aex_smart_worker.exe and \
                 pf_smart_timed_multilayer_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-smart-timedlayer-{}-{:032x}",
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
        // Three secondary layers at slot 1, one per time the probe checks out
        // (comp times 6/8, 1/3, 5/4). Distinct pixels so the composite is not a
        // vacuous match.
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
        let timed_layers = vec![
            TimedLayerImage {
                slot: 1,
                time: AnimationTime { value: 6, scale: 8 },
                image_path: make_layer("layer0.png", 10),
            },
            TimedLayerImage {
                slot: 1,
                time: AnimationTime { value: 1, scale: 3 },
                image_path: make_layer("layer1.png", 40),
            },
            TimedLayerImage {
                slot: 1,
                time: AnimationTime { value: 5, scale: 4 },
                image_path: make_layer("layer2.png", 70),
            },
        ];
        // Declare slot 1 as a layer input (a null-path layer parameter names
        // the slot without contributing a static secondary), so the three timed
        // layers are the only secondaries the probe checks out.
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
        // Argb32f joins Argb16 here (issue #353). It was the last depth the
        // layered gate excluded: the worker's gpu_negotiation ignores layers and
        // turns on for float32 whenever the plug-in advertises GPU support, so a
        // GPU-declaring plug-in would take the device on the one-shot layered
        // command while a policy-less Auto session folds to CPU. This probe
        // advertises no GPU support, so both routes render on CPU and the
        // equivalence below is what the gate change relies on.
        for pixel_format in [RenderPixelFormat::Argb16, RenderPixelFormat::Argb32f] {
            let render = |output: &Path| {
                render_experimental_image_with_timed_layers(
                    &root,
                    &aex,
                    &sha,
                    &input,
                    output,
                    &params,
                    &timed_layers,
                    timing,
                    true,
                    pixel_format,
                )
            };
            let label = match pixel_format {
                RenderPixelFormat::Argb8 => "argb8",
                RenderPixelFormat::Argb16 => "argb16",
                RenderPixelFormat::Argb32f => "argb32f",
            };

            // Run A: default routing carries the smart layered render on the session.
            unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
            let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
            let out_a = scratch.join(format!("a-{label}.png"));
            let report_a = render(&out_a)
                .unwrap_or_else(|error| panic!("session-route {label} layered render: {error}"));
            assert!(
                RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
                "the {label} smart timed-multilayer render must be carried by the session"
            );

            // Run B: escape hatch forces the one-shot --smart-image*-layer transport.
            unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
            let out_b = scratch.join(format!("b-{label}.png"));
            let report_b = render(&out_b);
            unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
            let report_b =
                report_b.unwrap_or_else(|error| panic!("one-shot {label} layered render: {error}"));

            assert_eq!(
                report_a.get("output_sha256"),
                report_b.get("output_sha256"),
                "the {label} smart timed-multilayer output must match between the routes"
            );
            assert!(
                report_a
                    .get("output_sha256")
                    .is_some_and(|value| !value.is_null()),
                "the session render must report an output_sha256: {report_a}"
            );
            assert_eq!(
                std::fs::read(&out_a).unwrap(),
                std::fs::read(&out_b).unwrap(),
                "the {label} smart timed-multilayer PNG differs between the routes"
            );

            // Compare the whole report, not just the pixels. Argb32f is the depth
            // where gpu_attempt / gpu_fallback_used can appear, and those are
            // exactly the fields that would show one route reaching a device and
            // the other not -- an output_sha256 match alone would not.
            let volatile = ["output_png", "output_raw", "worker_diagnostics"];
            let mut flat_a = report_a.as_object().expect("report A object").clone();
            let mut flat_b = report_b.as_object().expect("report B object").clone();
            for key in volatile {
                flat_a.remove(key);
                flat_b.remove(key);
            }
            assert_eq!(
                flat_a.keys().collect::<Vec<_>>(),
                flat_b.keys().collect::<Vec<_>>(),
                "the {label} report key sets diverge between the routes"
            );
            for (key, value_a) in &flat_a {
                assert_eq!(
                    Some(value_a),
                    flat_b.get(key),
                    "the {label} report field {key} differs between the routes"
                );
            }
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

    /// Companion to `wrapper_report_matches_the_one_shot_transport` for the case
    /// pf_sampling_probe cannot cover (issue #195): an AEX that declares a
    /// `PF_Param_LAYER` secondary layer (slot 1) plus a float slider (slot 2) and
    /// composites both on the classic render path. This is the first real-AEX A/B
    /// for the session wrapper's secondary-layer transport (issue #98 W1-4): the
    /// session route must deliver the same secondary-layer pixels and the same
    /// user-parameter value as the one-shot argv transport, field-for-field and
    /// byte-for-byte, at time 0 and at a nonzero time. It also proves both inputs
    /// are actually consumed (changing either changes the output), so a match is
    /// not a vacuous "the probe ignored them" pass. Gated on the locally built
    /// worker and the pf-layer-param-probe fixture, like the sibling test.

    /// The plain classic render, verified without the one-shot.
    ///
    /// pf_sampling_probe samples its input, so this fixture supports the
    /// sensitivity assertion the geometry probe could not (#361).
    #[test]
    fn classic_render_is_healthy_deterministic_and_input_dependent() {
        if crate::common::skip_without_restricted_token_launch(
            "classic_render_is_healthy_deterministic_and_input_dependent",
        ) {
            return;
        }
        // Still takes the route lock: the A/Bs that remain assert on exact
        // deltas of RENDER_SESSION_WRAPPER_RENDERS, and a concurrent session
        // render perturbs them. Measured -- without this, conformance_render_settings
        // fails with "the escape hatch did not force the one-shot transport".
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
        if crate::common::skip_without_restricted_token_launch(
            "layer_and_slider_reach_the_plug_in_through_the_session",
        ) {
            return;
        }
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
            base.get("secondary_layers")
                .and_then(|layers| layers.as_array())
                .map(|layers| layers.len()),
            Some(1),
            "the secondary layer did not reach the report: {base}"
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
    fn wrapper_parameter_animation_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "wrapper_parameter_animation_matches_the_one_shot_transport",
        ) {
            return;
        }
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // Deliberately a plain (non-canonicalized) repository root, unlike the
        // sibling tests' `repository_root()`. On Windows `canonicalize()` yields
        // a `\\?\` verbatim path, and the worker's parameter-animation sidecar
        // loader pins the sidecar's parent to `current_path()/target/
        // image-transport`; a verbatim sidecar path fails that string compare
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

        let volatile = ["output_png", "output_raw", "worker_diagnostics"];
        let render = |output: &Path, current_time: i32, disable_session: bool| {
            if disable_session {
                unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
            }
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
            let carried = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before;
            if disable_session {
                unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
            }
            (report, carried)
        };

        for (label, current_time) in [("keyframe", 0), ("interpolated", 30)] {
            let out_a = scratch.join(format!("{label}-a.png"));
            let (report_a, carried_a) = render(&out_a, current_time, false);
            assert!(
                carried_a,
                "the session wrapper did not carry the animation render ({label})"
            );
            let out_b = scratch.join(format!("{label}-b.png"));
            let (report_b, carried_b) = render(&out_b, current_time, true);
            assert!(
                !carried_b,
                "the escape hatch did not force the one-shot transport ({label})"
            );

            let mut flat_a = report_a.as_object().expect("report A object").clone();
            let mut flat_b = report_b.as_object().expect("report B object").clone();
            for key in volatile {
                flat_a.remove(key);
                flat_b.remove(key);
            }
            assert_eq!(
                flat_a.keys().collect::<Vec<_>>(),
                flat_b.keys().collect::<Vec<_>>(),
                "report key sets diverge ({label})"
            );
            for (key, value_a) in &flat_a {
                assert_eq!(
                    Some(value_a),
                    flat_b.get(key),
                    "report field {key} differs between the routes ({label})"
                );
            }
            assert_eq!(
                flat_a.get("passed"),
                Some(&serde_json::json!(true)),
                "animation render did not pass ({label}): {report_a}"
            );
            assert_eq!(
                std::fs::read(&out_a).unwrap(),
                std::fs::read(&out_b).unwrap(),
                "PNG bytes differ between the session and one-shot routes ({label})"
            );
        }

        // The animated slider must actually move the output over time, otherwise
        // the cross-route match above is a vacuous "the probe ignored animation"
        // pass. Both frames render on the session route.
        let keyframe = scratch.join("keyframe-a.png");
        let interpolated = scratch.join("interpolated-a.png");
        assert_ne!(
            std::fs::read(&keyframe).unwrap(),
            std::fs::read(&interpolated).unwrap(),
            "the animated slider did not change the output between timeline positions"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A/B equivalence for the length-1 *audio* session wrapper (issue #251):
    /// the resident audio-session route must produce the same public
    /// `render_experimental_audio` report contract and the same f32 output
    /// bytes as the one-shot `--render-audio` transport, for a real audio AEX
    /// (SDK_Backwards). Requires the render worker and the SDK_Backwards
    /// fixture from this checkout; skips (with a message) when either is
    /// missing.
    #[test]
    fn audio_wrapper_report_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "audio_wrapper_report_matches_the_one_shot_transport",
        ) {
            return;
        }
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/sdk-fixtures/sdk-backwards/SDK_Backwards.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping audio wrapper A/B: build aex_render_worker.exe and SDK_Backwards.aex \
                 (tools/build-sdk-backwards.ps1) first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-audio-ab-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();

        // A deterministic mono f32le input. SDK_Backwards reverses the audio
        // and adds its default tone, so the output is a stable, non-trivial
        // function of the input on either route.
        let input_path = scratch.join("input.f32");
        let samples: Vec<f32> = (0..512).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
        let mut input_bytes = Vec::with_capacity(samples.len() * 4);
        for sample in &samples {
            input_bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&input_path, &input_bytes).unwrap();

        // Run A: default routing goes through the length-1 audio session. The
        // report's render_path == "audio_session" proves the session actually
        // carried it; a silent fallback to one-shot would set no render_path
        // and make this comparison vacuous.
        let output_a = scratch.join("out-a.f32");
        let report_a = render_experimental_audio(&root, &aex, &sha, &input_path, &output_a, &[])
            .expect("session-route audio render");
        assert_eq!(
            report_a.get("render_path"),
            Some(&serde_json::json!("audio_session")),
            "run A did not go through the audio session"
        );

        // Run B: the escape hatch forces the one-shot --render-audio transport,
        // which sets no render_path.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let output_b = scratch.join("out-b.f32");
        let report_b = render_experimental_audio(&root, &aex, &sha, &input_path, &output_b, &[]);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot audio render");
        assert!(
            report_b.get("render_path").is_none(),
            "run B should be the one-shot transport, which sets no render_path"
        );

        // The core guarantee: byte-identical f32 output between the two routes.
        let bytes_a = std::fs::read(&output_a).unwrap();
        let bytes_b = std::fs::read(&output_b).unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "audio output bytes differ between the session and one-shot routes"
        );
        // Non-vacuous: the effect actually transformed the input.
        assert_ne!(
            bytes_a, input_bytes,
            "SDK_Backwards did not change the audio; the A/B would be vacuous"
        );

        // Every public contract field the one-shot audio path validates must be
        // present and identical on the session route.
        let contract = [
            "status",
            "sample_rate",
            "channels",
            "sample_format",
            "guard_bytes_intact",
            "samples_finite",
            "audio_setup_error",
            "audio_render_error",
            "audio_setdown_error",
            "setup_range_valid",
            "output_samples",
            "output_created",
            "input_sha256",
            "output_sha256",
            "output_transport",
        ];
        let object_a = report_a.as_object().expect("report A object");
        let object_b = report_b.as_object().expect("report B object");
        for key in contract {
            let value_a = object_a
                .get(key)
                .unwrap_or_else(|| panic!("session report missing contract field {key}"));
            let value_b = object_b
                .get(key)
                .unwrap_or_else(|| panic!("one-shot report missing contract field {key}"));
            assert_eq!(
                value_a, value_b,
                "audio contract field {key} differs between the session and one-shot routes"
            );
        }
        // Beyond the named contract, no field shared by both reports may
        // diverge. Excluded: render_path (the session marker under test);
        // worker_diagnostics/session_close (stderr-derived); and module_audit,
        // whose phase_count legitimately differs (the session runs more
        // lifecycle capture phases than one-shot) while its module sets match
        // and each route's audit is validated independently by the secure
        // dispatch.
        let route_specific = [
            "render_path",
            "worker_diagnostics",
            "session_close",
            "module_audit",
        ];
        for (key, value_a) in object_a {
            if route_specific.contains(&key.as_str()) {
                continue;
            }
            if let Some(value_b) = object_b.get(key) {
                assert_eq!(
                    value_a, value_b,
                    "shared audio report field {key} differs between routes"
                );
            }
        }

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A/B equivalence for render-path custom UI (issue #242): a click driven
    /// during render must produce the same public `custom_ui_*` report fields
    /// and the same PNG bytes on the length-1 session route (default) as on the
    /// one-shot argv transport, for a real custom-UI AEX. The fixture is the
    /// authored pf_custom_ui_probe (instruments/pf-custom-ui-probe): its
    /// DO_CLICK opens the color picker once, invalidates once, sets
    /// PF_ChangeFlag_CHANGED_VALUE on the color param (index 1), and RENDER
    /// fills from that param, so the click deterministically changes the
    /// output. Requires the render worker and the probe fixture
    /// (tools/build-pf-custom-ui-probe.ps1); skips when missing.
    #[test]
    fn custom_ui_click_report_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "custom_ui_click_report_matches_the_one_shot_transport",
        ) {
            return;
        }
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-custom-ui-probe-build/Release/pf_custom_ui_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping custom UI A/B: build aex_render_worker.exe and pf_custom_ui_probe.aex \
                 (tools/build-pf-custom-ui-probe.ps1) first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-customui-ab-{}-{:032x}",
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

        // A click at this point drives DO_CLICK; the host picker returns this
        // color, which the probe stores in its color param and RENDER fills the
        // whole frame from (the click coordinates do not affect the output).
        let click = || RenderUiAction::Click {
            point: [20, 16],
            color: [0.85, 0.2, 0.6, 1.0],
        };

        // Run A: default routing through the length-1 image session. The
        // diagnostic counter proves the session carried it (a silent one-shot
        // fallback would make the comparison vacuous).
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_a = scratch.join("out-a.png");
        let report_a = render_experimental_image_at_time_with_format_context_and_ui_action(
            &root,
            &aex,
            &sha,
            &input,
            &output_a,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            None,
            Some(click()),
        )
        .expect("session-route custom UI click render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the session wrapper did not carry run A"
        );
        // The click was actually dispatched and changed the color param.
        assert_eq!(
            report_a.get("custom_ui_click_dispatched"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            report_a.get("custom_ui_click_changed_value"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            report_a.get("custom_ui_context_closed"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            report_a.get("custom_ui_lifecycle_errors"),
            Some(&serde_json::json!([0, 0, 0, 0]))
        );

        // Run B: the escape hatch forces the one-shot argv transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let report_b = render_experimental_image_at_time_with_format_context_and_ui_action(
            &root,
            &aex,
            &sha,
            &input,
            &output_b,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            None,
            Some(click()),
        );
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot custom UI click render");
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            after_a,
            "the escape hatch did not force the one-shot transport"
        );

        // Reports must match field-for-field except the volatile output/stderr
        // fields (the custom_ui_* fields are covered by this same comparison).
        let volatile = ["output_png", "output_raw", "worker_diagnostics"];
        let mut flattened_a = report_a.as_object().expect("report A object").clone();
        let mut flattened_b = report_b.as_object().expect("report B object").clone();
        for key in volatile {
            flattened_a.remove(key);
            flattened_b.remove(key);
        }
        assert_eq!(
            flattened_a.keys().collect::<Vec<_>>(),
            flattened_b.keys().collect::<Vec<_>>(),
            "custom UI report key sets diverge"
        );
        for (key, value_a) in &flattened_a {
            assert_eq!(
                Some(value_a),
                flattened_b.get(key),
                "custom UI report field {key} differs between the session and one-shot routes"
            );
        }
        assert_eq!(
            std::fs::read(&output_a).unwrap(),
            std::fs::read(&output_b).unwrap(),
            "PNG bytes differ between the session and one-shot routes"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A/B equivalence for a render-path custom UI *draw* (issue #242): a draw
    /// event driven during render must produce the same public custom_ui_draw_*
    /// report fields and the same PNG bytes on the session route (default) as on
    /// the one-shot argv transport. pf_custom_ui_probe's DRAW paints one drawbot
    /// rectangle onto the control surface and flags the event handled.
    #[test]
    fn custom_ui_draw_report_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "custom_ui_draw_report_matches_the_one_shot_transport",
        ) {
            return;
        }
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-custom-ui-probe-build/Release/pf_custom_ui_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping custom UI draw A/B: build aex_render_worker.exe and \
                 pf_custom_ui_probe.aex (tools/build-pf-custom-ui-probe.ps1) first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-customui-draw-ab-{}-{:032x}",
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

        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_a = scratch.join("out-a.png");
        let report_a = render_experimental_image_at_time_with_format_context_and_ui_action(
            &root,
            &aex,
            &sha,
            &input,
            &output_a,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            None,
            Some(RenderUiAction::Draw),
        )
        .expect("session-route custom UI draw render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the session wrapper did not carry run A"
        );
        assert_eq!(
            report_a.get("custom_ui_draw_dispatched"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            report_a.get("custom_ui_draw_error"),
            Some(&serde_json::json!(0))
        );
        assert_eq!(
            report_a.get("custom_ui_context_closed"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(
            report_a.get("custom_ui_lifecycle_errors"),
            Some(&serde_json::json!([0, 0, 0, 0]))
        );

        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let report_b = render_experimental_image_at_time_with_format_context_and_ui_action(
            &root,
            &aex,
            &sha,
            &input,
            &output_b,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            None,
            Some(RenderUiAction::Draw),
        );
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot custom UI draw render");
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            after_a,
            "the escape hatch did not force the one-shot transport"
        );

        let volatile = ["output_png", "output_raw", "worker_diagnostics"];
        let mut flattened_a = report_a.as_object().expect("report A object").clone();
        let mut flattened_b = report_b.as_object().expect("report B object").clone();
        for key in volatile {
            flattened_a.remove(key);
            flattened_b.remove(key);
        }
        assert_eq!(
            flattened_a.keys().collect::<Vec<_>>(),
            flattened_b.keys().collect::<Vec<_>>(),
            "custom UI draw report key sets diverge"
        );
        for (key, value_a) in &flattened_a {
            assert_eq!(
                Some(value_a),
                flattened_b.get(key),
                "custom UI draw report field {key} differs between routes"
            );
        }
        assert_eq!(
            std::fs::read(&output_a).unwrap(),
            std::fs::read(&output_b).unwrap(),
            "PNG bytes differ between the session and one-shot routes"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// An effect that expands its output past the launch slot grows the shared
    /// section in place (#262). Verified by the lifecycle the grow must not
    /// replay, not by agreeing with the one-shot (#361).
    #[test]
    fn expand_output_grows_in_place_without_replaying_the_lifecycle() {
        if crate::common::skip_without_restricted_token_launch(
            "expand_output_grows_in_place_without_replaying_the_lifecycle",
        ) {
            return;
        }
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

        // Eligible render with the escape hatch unset, so the session IS
        // attempted; the fault injection then forces it to report a Fallback.
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        unsafe { std::env::set_var(FORCE_SESSION_FALLBACK_ENV, "1") };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output = scratch.join("out.png");
        let result = render_experimental_image(&root, &aex, &sha, &input, &output, &[]);
        unsafe { std::env::remove_var(FORCE_SESSION_FALLBACK_ENV) };

        let error = result.expect_err("a forced session fallback must fail closed, not fall back");
        let message = error.to_string();
        assert!(
            message.contains(DISABLE_SESSION_WRAPPER_ENV),
            "the fail-closed error must name the one-shot override; got: {message}"
        );
        // Secondary sanity check: the session wrapper did not carry a render.
        // (This counter only tracks the session path, so it alone does not
        // distinguish fail-closed from a silent one-shot; the discriminators are
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
        if crate::common::skip_without_restricted_token_launch(
            "oversized_layer_is_carried_at_its_own_dimensions",
        ) {
            return;
        }
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
        if crate::common::skip_without_restricted_token_launch(
            "zero_duration_render_produces_the_single_frame_on_the_session",
        ) {
            return;
        }
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
        if crate::common::skip_without_restricted_token_launch(
            "conformance_render_settings_change_the_render_and_do_not_leak",
        ) {
            return;
        }
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

        // What this test can and cannot see: the flattened public report carries
        // no render_settings block -- that field lives in the worker report the
        // session close returns, not in interactive_image_render (measured
        // against the real report). So the worker-side arrival of the trailer is
        // not observable here; it is the session protocol's own tests that cover
        // it. What is observable is the effect the setting is asked for: the
        // broker's alpha pre-transform rewrites the input the plug-in sees.
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
    /// is attempted (escape hatch unset) but cannot carry the render, the render
    /// must surface an explicit error instead of silently rerunning the one-shot
    /// `--render-audio` transport. The fault-injection env forces the audio
    /// session wrapper to report a Fallback without a real failure; the caller
    /// must then error (naming the override) and write no output. On the pre-#264
    /// code the forced Fallback fell through to a successful one-shot render.
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

        // Eligible audio render with the escape hatch unset, so the session is
        // attempted; the fault injection forces it to report a Fallback.
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        unsafe { std::env::set_var(FORCE_SESSION_FALLBACK_ENV, "1") };
        let output = scratch.join("out.f32");
        let result = render_experimental_audio(&root, &aex, &sha, &input_path, &output, &[]);
        unsafe { std::env::remove_var(FORCE_SESSION_FALLBACK_ENV) };

        let error =
            result.expect_err("a forced audio session fallback must fail closed, not fall back");
        let message = error.to_string();
        assert!(
            message.contains(DISABLE_SESSION_WRAPPER_ENV),
            "the fail-closed error must name the one-shot override; got: {message}"
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

    /// Image render + audio sidecar (#98 W4, issue #339): the classic session
    /// carries the audio source through its `session-audio:v1|` launch trailer,
    /// so the public report and PNG must match the one-shot `--render-image-audio`
    /// transport field for field.
    ///
    /// This is the A/B that covers the two divergences the session route had:
    /// `audio_present` and `audio_input_sha256` were both hardcoded on the
    /// session side, so the audio gate never applied and the whole audio_*
    /// block vanished from the report. Reverting either one fails the key-set
    /// comparison below.
    #[test]
    fn image_audio_sidecar_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "image_audio_sidecar_matches_the_one_shot_transport",
        ) {
            return;
        }
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let Some(aex) = visual_audio_probe(&root, "pf_visual_audio_sidecar_probe") else {
            eprintln!("skipping image+audio A/B: run tools/build-pf-visual-audio-probe.ps1 first");
            return;
        };
        if !worker.is_file() {
            eprintln!("skipping image+audio A/B: build aex_render_worker.exe first");
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
        // pf_visual_audio_sidecar_probe checks out samples 4..9 at 44100 and
        // returns PF_Err_NONE only when it reads back 0.25 at the window start
        // and 0.5625 at its last in-range sample, so a render that reaches this
        // test's assertions is itself proof the audio arrived.
        let mut samples = [0.0f32; 10];
        samples[4] = 0.25;
        samples[9] = 0.5625;
        let sidecar = scratch.join("audio.f32");
        std::fs::write(
            &sidecar,
            samples
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<u8>>(),
        )
        .unwrap();

        // Run A: default routing carries the audio render on the session.
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_a = scratch.join("out-a.png");
        let report_a = render_experimental_image_with_audio_sidecar(
            &root,
            &aex,
            &sha,
            &input,
            &sidecar,
            &output_a,
            &[],
            RenderTiming::default(),
        )
        .expect("session-route audio render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the audio-sidecar render must now be carried by the session"
        );

        // Run B: the escape hatch forces the one-shot argv transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let report_b = render_experimental_image_with_audio_sidecar(
            &root,
            &aex,
            &sha,
            &input,
            &sidecar,
            &output_b,
            &[],
            RenderTiming::default(),
        );
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot audio render");
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            after_a,
            "the escape hatch did not force the one-shot transport"
        );

        // The audio telemetry must be present on both routes, not just equal:
        // two reports that both dropped the block would compare equal.
        for (label, report) in [("session", &report_a), ("one-shot", &report_b)] {
            assert_eq!(
                report.get("audio_sidecar_transport"),
                Some(&serde_json::json!("mono_f32le_44100")),
                "the {label} route lost the audio sidecar transport"
            );
            assert_eq!(
                report.get("audio_usage_advertised"),
                Some(&serde_json::json!(true)),
                "the {label} route lost the audio telemetry"
            );
            assert_eq!(
                report.get("audio_checkout_calls"),
                Some(&serde_json::json!(1)),
                "the {label} route did not observe the plug-in's audio checkout"
            );
        }

        let volatile = ["output_png", "output_raw", "worker_diagnostics"];
        let mut flat_a = report_a.as_object().expect("report A object").clone();
        let mut flat_b = report_b.as_object().expect("report B object").clone();
        for key in volatile {
            flat_a.remove(key);
            flat_b.remove(key);
        }
        assert_eq!(
            flat_a.keys().collect::<Vec<_>>(),
            flat_b.keys().collect::<Vec<_>>(),
            "audio report key sets diverge between the routes"
        );
        for (key, value_a) in &flat_a {
            assert_eq!(
                Some(value_a),
                flat_b.get(key),
                "audio report field {key} differs between the session and one-shot routes"
            );
        }
        assert_eq!(
            std::fs::read(&output_a).unwrap(),
            std::fs::read(&output_b).unwrap(),
            "the audio render PNG differs between the session and one-shot routes"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Combined classic audio + secondary layer (#341): unlike the legacy
    /// `--render-image-audio` command, the session has independent transports
    /// for its audio trailer and inherited layer handles. The fixture folds the
    /// checked-out audio window and the layer pixels into the PNG, so changing
    /// either input independently must change the output. This is deliberately
    /// session-canonical per #264/#291; the final assertion keeps the one-shot
    /// 16-argv limitation visible instead of pretending the routes are equal.
    #[test]
    fn image_audio_and_secondary_layer_are_jointly_consumed_by_session() {
        if crate::common::skip_without_restricted_token_launch(
            "image_audio_and_secondary_layer_are_jointly_consumed_by_session",
        ) {
            return;
        }
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
            unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
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

        // The fixed-arity one-shot remains intentionally unchanged. Disabling
        // the canonical session must expose that old limitation, not silently
        // drop either input or count as another session render.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let before_one_shot = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let one_shot_output = scratch.join("forced-one-shot.png");
        let parameters = vec![layer_parameter(1, &layer_a)];
        let one_shot = render_experimental_image_with_audio_sidecar(
            &root,
            &aex,
            &sha,
            &input,
            &audio_a,
            &one_shot_output,
            &parameters,
            RenderTiming::default(),
        );
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        one_shot.expect_err("the 16-argv one-shot cannot carry audio and a layer");
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            before_one_shot,
            "the forced one-shot unexpectedly entered the session"
        );
        assert!(!one_shot_output.exists());

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
    fn an_unadvertised_plugin_with_a_sidecar_is_refused_on_both_routes() {
        if crate::common::skip_without_restricted_token_launch(
            "an_unadvertised_plugin_with_a_sidecar_is_refused_on_both_routes",
        ) {
            return;
        }
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

        for (label, force_one_shot) in [("session", false), ("one-shot", true)] {
            if force_one_shot {
                unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
            } else {
                unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
            }
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
            unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
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
