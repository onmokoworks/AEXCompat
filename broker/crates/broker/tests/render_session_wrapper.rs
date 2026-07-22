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

    #[test]
    fn smart_single_image_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "smart_single_image_matches_the_one_shot_transport",
        ) {
            return;
        }
        // A CPU SmartFX single-image render (no layers, no context) is now
        // carried by the length-1 smart session (#278 stage 1). Prove it renders
        // byte-identically to the one-shot --smart-image route, for both a normal
        // frame (t=0) and a legally empty result (the probe answers an empty
        // result_rect at current_time % 4 == 3), where both routes skip the PNG.
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex =
            root.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping smart A/B: build aex_smart_worker.exe and \
                 pf_smart_geometry_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-smart-ab-{}-{:032x}",
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

        let volatile = ["output_png", "output_raw", "worker_diagnostics"];
        let compare = |report_a: &serde_json::Value, report_b: &serde_json::Value, label: &str| {
            let mut a = report_a.as_object().expect("report A").clone();
            let mut b = report_b.as_object().expect("report B").clone();
            for key in volatile {
                a.remove(key);
                b.remove(key);
            }
            assert_eq!(
                a.keys().collect::<Vec<_>>(),
                b.keys().collect::<Vec<_>>(),
                "{label}: smart report key sets diverge between the routes"
            );
            for (key, value_a) in &a {
                assert_eq!(
                    Some(value_a),
                    b.get(key),
                    "{label}: smart report field {key} differs between the routes"
                );
            }
        };

        // Case 1: a normal (non-empty) smart frame at t=0.
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_a = scratch.join("normal-a.png");
        let report_a = render_experimental_smart_image(&root, &aex, &sha, &input, &out_a, &[])
            .expect("session-route smart render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the smart render must now be carried by the session"
        );
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let out_b = scratch.join("normal-b.png");
        let report_b = render_experimental_smart_image(&root, &aex, &sha, &input, &out_b, &[]);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot smart render");
        compare(&report_a, &report_b, "normal");
        assert_eq!(
            std::fs::read(&out_a).unwrap(),
            std::fs::read(&out_b).unwrap(),
            "the smart PNG differs between the routes"
        );

        // Case 2: a legally empty smart result at t=3 (probe mode EmptyResult).
        // Neither route writes a PNG; the reports must still match.
        let empty_timing = RenderTiming {
            current_time: 3,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
        };
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let empty_before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let empty_a = scratch.join("empty-a.png");
        let empty_report_a = render_experimental_smart_image_at_time(
            &root,
            &aex,
            &sha,
            &input,
            &empty_a,
            &[],
            empty_timing,
        )
        .expect("session-route empty smart render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > empty_before,
            "the empty smart render must be carried by the session"
        );
        assert!(
            !empty_a.exists(),
            "an empty smart result writes no PNG on the session route"
        );
        assert_eq!(
            empty_report_a.get("empty_result_rect"),
            Some(&serde_json::Value::Bool(true)),
            "the session must report the empty result: {empty_report_a}"
        );
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let empty_b = scratch.join("empty-b.png");
        let empty_report_b = render_experimental_smart_image_at_time(
            &root,
            &aex,
            &sha,
            &input,
            &empty_b,
            &[],
            empty_timing,
        );
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let empty_report_b = empty_report_b.expect("one-shot empty smart render");
        compare(&empty_report_a, &empty_report_b, "empty");
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
                RenderPixelFormat::Argb16,
            )
        };

        // Run A: default routing carries the smart layered render on the session.
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let out_a = scratch.join("a.png");
        let report_a = render(&out_a).expect("session-route smart timed-multilayer render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the smart timed-multilayer render must be carried by the session"
        );

        // Run B: escape hatch forces the one-shot --smart-image16-layer transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let out_b = scratch.join("b.png");
        let report_b = render(&out_b);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot smart timed-multilayer render");

        assert_eq!(
            report_a.get("output_sha256"),
            report_b.get("output_sha256"),
            "the smart timed-multilayer output must match between the routes"
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
            "the smart timed-multilayer PNG differs between the routes"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn wrapper_report_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "wrapper_report_matches_the_one_shot_transport",
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
                "skipping wrapper A/B: build aex_render_worker.exe and pf_sampling_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-wrapper-ab-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        let gradient = image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        });
        gradient.save(&input).unwrap();

        // Run A: default routing; the diagnostic counter proves the session
        // wrapper actually carried it (a silent fallback would make this
        // comparison vacuous).
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_a = scratch.join("out-a.png");
        let report_a = render_experimental_image(&root, &aex, &sha, &input, &output_a, &[])
            .expect("session-route render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the session wrapper did not carry run A"
        );

        // Run B: the escape hatch forces the one-shot argv transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let report_b = render_experimental_image(&root, &aex, &sha, &input, &output_b, &[])
            .expect("one-shot render");
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            after_a,
            "the escape hatch did not force the one-shot transport"
        );

        // The reports must match field-for-field except where the routes
        // legitimately differ: the output paths and the stderr-derived
        // process diagnostics (the session's stage traces include its frame
        // loop, and elapsed timings are volatile either way).
        let volatile = ["output_png", "output_raw", "worker_diagnostics"];
        let mut flattened_a = report_a.as_object().expect("report A object").clone();
        let mut flattened_b = report_b.as_object().expect("report B object").clone();
        for key in volatile {
            flattened_a.remove(key);
            flattened_b.remove(key);
        }
        let keys_a: Vec<_> = flattened_a.keys().collect();
        let keys_b: Vec<_> = flattened_b.keys().collect();
        assert_eq!(keys_a, keys_b, "report key sets diverge");
        for (key, value_a) in &flattened_a {
            assert_eq!(
                Some(value_a),
                flattened_b.get(key),
                "report field {key} differs between the session and one-shot routes"
            );
        }
        assert_eq!(
            std::fs::read(&output_a).unwrap(),
            std::fs::read(&output_b).unwrap(),
            "PNG bytes differ between the session and one-shot routes"
        );

        // A nonzero render time exercises the deferred SEQUENCE_SETUP
        // seeding: effects reading in_data->current_time during setup must
        // observe the requested time on both routes.
        let timing = RenderTiming {
            current_time: 7,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
        };
        let timed_before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let timed_a = scratch.join("timed-a.png");
        let timed_report_a =
            render_experimental_image_at_time(&root, &aex, &sha, &input, &timed_a, &[], timing)
                .expect("session-route timed render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > timed_before,
            "the session wrapper did not carry the timed render"
        );
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let timed_b = scratch.join("timed-b.png");
        let timed_report_b =
            render_experimental_image_at_time(&root, &aex, &sha, &input, &timed_b, &[], timing)
                .expect("one-shot timed render");
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let mut timed_flat_a = timed_report_a.as_object().expect("timed A").clone();
        let mut timed_flat_b = timed_report_b.as_object().expect("timed B").clone();
        for key in volatile {
            timed_flat_a.remove(key);
            timed_flat_b.remove(key);
        }
        for (key, value_a) in &timed_flat_a {
            assert_eq!(
                Some(value_a),
                timed_flat_b.get(key),
                "timed report field {key} differs between the routes"
            );
        }
        assert_eq!(
            std::fs::read(&timed_a).unwrap(),
            std::fs::read(&timed_b).unwrap(),
            "timed PNG bytes differ between the routes"
        );

        // A spatial + render-environment host context (issue #98 W1-3) must
        // travel through the session launch argv and produce the identical
        // report on both routes. A mask-free, aux-free, coverage-free context
        // stays session-representable.
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
        let ctx_before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let ctx_a = scratch.join("ctx-a.png");
        let ctx_report_a = render_experimental_image_at_time_with_format_and_context(
            &root,
            &aex,
            &sha,
            &input,
            &ctx_a,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            Some(&context),
        )
        .expect("session-route context render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > ctx_before,
            "the session wrapper did not carry the context render"
        );
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let ctx_b = scratch.join("ctx-b.png");
        let ctx_report_b = render_experimental_image_at_time_with_format_and_context(
            &root,
            &aex,
            &sha,
            &input,
            &ctx_b,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            Some(&context),
        )
        .expect("one-shot context render");
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let mut ctx_flat_a = ctx_report_a.as_object().expect("context A").clone();
        let mut ctx_flat_b = ctx_report_b.as_object().expect("context B").clone();
        for key in volatile {
            ctx_flat_a.remove(key);
            ctx_flat_b.remove(key);
        }
        // The spatial and render-environment contract fields must be present
        // and equal: this is the whole point of carrying the context.
        assert_eq!(
            ctx_flat_a.get("spatial_contract_ok"),
            Some(&serde_json::json!(true))
        );
        for (key, value_a) in &ctx_flat_a {
            assert_eq!(
                Some(value_a),
                ctx_flat_b.get(key),
                "context report field {key} differs between the routes"
            );
        }
        assert_eq!(
            std::fs::read(&ctx_a).unwrap(),
            std::fs::read(&ctx_b).unwrap(),
            "context PNG bytes differ between the routes"
        );

        // A non-empty mask scene (issue #98 W1-3b) travels through the session
        // launch argv as the v2| trailer and must match on both routes.
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
        let mask_before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let mask_a = scratch.join("mask-a.png");
        let mask_report_a = render_experimental_image_at_time_with_format_and_context(
            &root,
            &aex,
            &sha,
            &input,
            &mask_a,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            Some(&mask_context),
        )
        .expect("session-route mask render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > mask_before,
            "the session wrapper did not carry the mask render"
        );
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let mask_b = scratch.join("mask-b.png");
        let mask_report_b = render_experimental_image_at_time_with_format_and_context(
            &root,
            &aex,
            &sha,
            &input,
            &mask_b,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            Some(&mask_context),
        )
        .expect("one-shot mask render");
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let mut mask_flat_a = mask_report_a.as_object().expect("mask A").clone();
        let mut mask_flat_b = mask_report_b.as_object().expect("mask B").clone();
        for key in volatile {
            mask_flat_a.remove(key);
            mask_flat_b.remove(key);
        }
        for (key, value_a) in &mask_flat_a {
            assert_eq!(
                Some(value_a),
                mask_flat_b.get(key),
                "mask report field {key} differs between the routes"
            );
        }
        assert_eq!(
            std::fs::read(&mask_a).unwrap(),
            std::fs::read(&mask_b).unwrap(),
            "mask PNG bytes differ between the routes"
        );
        // An alpha-as-coverage host context (issue #98 W1-4c) rides the
        // session launch argv as the `--alpha-as-coverage-v1` auxiliary option
        // (published once at launch), and must match the one-shot route
        // field-for-field and byte-for-byte. The eligibility gate no longer
        // forces such a context onto the one-shot path.
        let coverage_context: HostContext = serde_json::from_value(serde_json::json!({
            "mask_scene": {"masks": []},
            "alpha_as_coverage_params": [0],
        }))
        .expect("alpha-as-coverage host context fixture");
        let cov_before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let cov_a = scratch.join("cov-a.png");
        let cov_report_a = render_experimental_image_at_time_with_format_and_context(
            &root,
            &aex,
            &sha,
            &input,
            &cov_a,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            Some(&coverage_context),
        )
        .expect("session-route alpha-as-coverage render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > cov_before,
            "the session wrapper did not carry the alpha-as-coverage render"
        );
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let cov_b = scratch.join("cov-b.png");
        let cov_report_b = render_experimental_image_at_time_with_format_and_context(
            &root,
            &aex,
            &sha,
            &input,
            &cov_b,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            Some(&coverage_context),
        )
        .expect("one-shot alpha-as-coverage render");
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let mut cov_flat_a = cov_report_a.as_object().expect("coverage A").clone();
        let mut cov_flat_b = cov_report_b.as_object().expect("coverage B").clone();
        for key in volatile {
            cov_flat_a.remove(key);
            cov_flat_b.remove(key);
        }
        for (key, value_a) in &cov_flat_a {
            assert_eq!(
                Some(value_a),
                cov_flat_b.get(key),
                "alpha-as-coverage report field {key} differs between the routes"
            );
        }
        assert_eq!(
            std::fs::read(&cov_a).unwrap(),
            std::fs::read(&cov_b).unwrap(),
            "alpha-as-coverage PNG bytes differ between the routes"
        );

        // An aux-channel host context (issue #211) rides the session launch
        // argv as the `--aux-manifest-v1` auxiliary option, the same
        // broker-built manifest the one-shot path emits. The eligibility gate no
        // longer forces an aux-carrying context onto the one-shot path, so both
        // routes must produce the identical report and PNG bytes.
        //
        // This A/B was dead-on-arrival until issue #231: the worker's aux loader
        // rejects any manifest/sidecar path carrying the Windows `\\?\` verbatim
        // prefix (its `absolute().lexically_normal() == canonical()` gate fails
        // because MSVC's `canonical` drops the prefix and `absolute` keeps it),
        // and `repository_root()`'s `canonicalize()` yields exactly such a path,
        // so both routes exited 3. prepare_aux_transport now de-verbatims the
        // transport root before writing the manifest, matching the broker's
        // existing minidump/trace path handling, so the real worker accepts the
        // aux render on both routes.
        //
        // pf_sampling_probe does not read the depth channel, but that is exactly
        // the point of this A/B: the worker must accept and load the manifest on
        // both routes, and an effect that ignores it must still render
        // byte-for-byte identically. A full-resolution depth plane (matching the
        // 64x32 input at downsample 1/1) is the least ambiguous shape for the
        // worker's aux loader. The aux sample source must live under the
        // repository root (prepare_aux_transport bounds it there); target/ is
        // repository-local and git-ignored.
        let depth_type = i32::from_be_bytes(*b"DPTH");
        let aux_source_dir = root.join(format!(
            "target/aux-ab-source-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&aux_source_dir).unwrap();
        let aux_source = aux_source_dir.join("depth.f32");
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
        let aux_before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let aux_a = scratch.join("aux-a.png");
        let aux_report_a = render_experimental_image_at_time_with_format_and_context(
            &root,
            &aex,
            &sha,
            &input,
            &aux_a,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            Some(&aux_context),
        )
        .expect("session-route aux render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > aux_before,
            "the session wrapper did not carry the aux render"
        );
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let aux_b = scratch.join("aux-b.png");
        let aux_report_b = render_experimental_image_at_time_with_format_and_context(
            &root,
            &aex,
            &sha,
            &input,
            &aux_b,
            &[],
            RenderTiming::default(),
            false,
            RenderPixelFormat::Argb8,
            Some(&aux_context),
        )
        .expect("one-shot aux render");
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let mut aux_flat_a = aux_report_a.as_object().expect("aux A").clone();
        let mut aux_flat_b = aux_report_b.as_object().expect("aux B").clone();
        for key in volatile {
            aux_flat_a.remove(key);
            aux_flat_b.remove(key);
        }
        for (key, value_a) in &aux_flat_a {
            assert_eq!(
                Some(value_a),
                aux_flat_b.get(key),
                "aux report field {key} differs between the routes"
            );
        }
        assert_eq!(
            std::fs::read(&aux_a).unwrap(),
            std::fs::read(&aux_b).unwrap(),
            "aux PNG bytes differ between the routes"
        );
        let _ = std::fs::remove_dir_all(&aux_source_dir);

        // Secondary layer A/B equivalence needs an AEX declaring a layer
        // parameter, which pf_sampling_probe does not; the session layer
        // transport is covered by the render_session fixture integration test,
        // and the real-AEX equivalence is tracked separately.
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
    #[test]
    fn wrapper_layer_and_slider_match_across_routes() {
        if crate::common::skip_without_restricted_token_launch(
            "wrapper_layer_and_slider_match_across_routes",
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
                "skipping layer+slider A/B: build aex_render_worker.exe and \
                 pf_layer_param_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-wrapper-layer-ab-{}-{:032x}",
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
        // The secondary layer carries a pattern unrelated to the input so the
        // green (layer-derived) and blue (input+layer) output channels differ
        // from a pass-through of either source.
        let secondary = scratch.join("layer.png");
        image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x + y) as u8, (x * 7) as u8, (y * 9) as u8, 255])
        })
        .save(&secondary)
        .unwrap();

        let volatile = ["output_png", "output_raw", "worker_diagnostics"];
        let render = |output: &Path,
                      params: &[InteractiveParameter],
                      timing: RenderTiming,
                      disable_session: bool| {
            if disable_session {
                unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
            }
            let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
            let report = render_experimental_image_at_time(
                &root, &aex, &sha, &input, output, params, timing,
            )
            .expect("layer+slider render");
            let carried = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before;
            if disable_session {
                unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
            }
            (report, carried)
        };

        let params = vec![layer_parameter(1, &secondary), float_parameter(2, 200.0)];
        for (label, timing) in [
            ("time0", RenderTiming::default()),
            (
                "timed",
                RenderTiming {
                    current_time: 7,
                    time_step: 1,
                    total_time: 300,
                    time_scale: 30,
                },
            ),
        ] {
            let out_a = scratch.join(format!("{label}-a.png"));
            let (report_a, carried_a) = render(&out_a, &params, timing, false);
            assert!(
                carried_a,
                "the session wrapper did not carry run A ({label})"
            );
            let out_b = scratch.join(format!("{label}-b.png"));
            let (report_b, carried_b) = render(&out_b, &params, timing, true);
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
            // A declared layer slot filled with a secondary layer must render
            // cleanly (pf_sampling_probe's missing slot faulted the render). The
            // probe must see all three slots, and slot 1 must carry the secondary.
            assert_eq!(
                flat_a.get("passed"),
                Some(&serde_json::json!(true)),
                "layer render did not pass ({label}): {report_a}"
            );
            assert_eq!(
                flat_a.get("in_data_num_params"),
                Some(&serde_json::json!(3)),
                "probe did not expose input + layer + slider ({label})"
            );
            assert_eq!(
                flat_a.get("secondary_layers"),
                Some(&serde_json::json!([{"slot": 1, "width": 64, "height": 32}])),
                "the secondary layer was not delivered to slot 1 ({label})"
            );
            assert_eq!(
                std::fs::read(&out_a).unwrap(),
                std::fs::read(&out_b).unwrap(),
                "PNG bytes differ between the session and one-shot routes ({label})"
            );
        }

        // Consumption checks (session route): the match above is only meaningful
        // if the probe actually reads both inputs. Changing the slider value, and
        // separately the secondary-layer pixels, must each change the output.
        let base = scratch.join("consume-base.png");
        render(&base, &params, RenderTiming::default(), false);
        let base_bytes = std::fs::read(&base).unwrap();

        let other_slider = vec![layer_parameter(1, &secondary), float_parameter(2, 40.0)];
        let slider_out = scratch.join("consume-slider.png");
        render(&slider_out, &other_slider, RenderTiming::default(), false);
        assert_ne!(
            base_bytes,
            std::fs::read(&slider_out).unwrap(),
            "changing the slider did not change the output; the user parameter was not consumed"
        );

        let secondary2 = scratch.join("layer2.png");
        image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(y * 5) as u8, (x * 2) as u8, (x * y) as u8, 255])
        })
        .save(&secondary2)
        .unwrap();
        let other_layer = vec![layer_parameter(1, &secondary2), float_parameter(2, 200.0)];
        let layer_out = scratch.join("consume-layer.png");
        render(&layer_out, &other_layer, RenderTiming::default(), false);
        assert_ne!(
            base_bytes,
            std::fs::read(&layer_out).unwrap(),
            "changing the secondary layer did not change the output; the layer was not consumed"
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

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

    /// A/B equivalence for an expand-output effect (issue #262): the length-1
    /// session route grows the shared output slot in place and renders the
    /// expanded output byte-identically to the one-shot argv transport, instead
    /// of falling back. The fixture is pf_expand_allowed_probe (FRAME_SETUP
    /// grows the output by 4px with PF_OutFlag_I_EXPAND_BUFFER), which overruns
    /// the initial slot and drives the resize_needed in-session grow. Requires
    /// the render worker and the resize probe (tools/build-pf-frame-resize-probe.ps1).
    #[test]
    fn expand_output_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "expand_output_matches_the_one_shot_transport",
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
            eprintln!(
                "skipping expand A/B: build aex_render_worker.exe and pf_expand_allowed_probe.aex \
                 (tools/build-pf-frame-resize-probe.ps1) first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-expand-ab-{}-{:032x}",
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

        // The probe appends one byte per lifecycle selector to this file across
        // every worker it is spawned into: 'S' for FRAME_SETUP, 'R' for RENDER.
        // The counts are the only cross-process observable of a re-opened
        // worker's lifecycle: an expand that overran the launch slot and re-opened
        // the session (the pre-#262 behaviour) runs FRAME_SETUP and RENDER once in
        // the discarded worker and again in the re-opened worker (two 'S', two
        // 'R'); the in-session grow keeps the same worker, so each runs exactly
        // once (one 'S', one 'R'), matching the one-shot route. This is the direct
        // evidence that SEQUENCE/FRAME setup+setdown are not replayed (#262
        // finding 3617631908).
        let render_log = scratch.join("selector-dispatches.bin");
        // The probe appends (mode "ab"); start from a clean slate so a stale file
        // can never inflate the counts into a false negative.
        std::fs::remove_file(&render_log).ok();
        unsafe { std::env::set_var("AEXCOMPAT_RESIZE_RENDER_LOG", &render_log) };
        let count_marker = |marker: u8| -> usize {
            std::fs::read(&render_log)
                .map(|bytes| bytes.iter().filter(|byte| **byte == marker).count())
                .unwrap_or(0)
        };

        // Run A: default routing. The effect expands 64x48 -> 68x52, overruns the
        // initial 64x48 slot, and the worker grows the shared section in place on
        // the session route. The counter proves the session carried it (a silent
        // one-shot fallback would leave it unchanged).
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_a = scratch.join("out-a.png");
        let report_a = render_experimental_image(&root, &aex, &sha, &input, &output_a, &[])
            .expect("session-route expand render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the session wrapper did not carry the expand render"
        );
        // The output really expanded past the input dimensions.
        assert_eq!(report_a.get("width"), Some(&serde_json::json!(68)));
        assert_eq!(report_a.get("height"), Some(&serde_json::json!(52)));
        // Exactly-once for the whole lifecycle: the in-session grow must not
        // replay FRAME_SETUP or RENDER (a re-open would run each twice).
        let (session_setups, session_renders) = (count_marker(b'S'), count_marker(b'R'));
        assert_eq!(
            (session_setups, session_renders),
            (1, 1),
            "the session expand ran FRAME_SETUP {session_setups}x and RENDER {session_renders}x \
             (expected 1/1; a re-open would replay the lifecycle in a second worker)"
        );
        std::fs::remove_file(&render_log).ok();

        // Run B: the escape hatch forces the one-shot argv transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let report_b = render_experimental_image(&root, &aex, &sha, &input, &output_b, &[]);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot expand render");
        unsafe { std::env::remove_var("AEXCOMPAT_RESIZE_RENDER_LOG") };
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            after_a,
            "the escape hatch did not force the one-shot transport"
        );
        // The one-shot route runs the lifecycle exactly once; this is the
        // baseline the session route's single-worker grow above is matched
        // against.
        let (one_shot_setups, one_shot_renders) = (count_marker(b'S'), count_marker(b'R'));
        assert_eq!(
            (one_shot_setups, one_shot_renders),
            (1, 1),
            "the one-shot expand ran FRAME_SETUP {one_shot_setups}x and RENDER {one_shot_renders}x \
             (expected 1/1)"
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
            "expand report key sets diverge"
        );
        for (key, value_a) in &flattened_a {
            assert_eq!(
                Some(value_a),
                flattened_b.get(key),
                "expand report field {key} differs between the session and one-shot routes"
            );
        }
        assert_eq!(
            std::fs::read(&output_a).unwrap(),
            std::fs::read(&output_b).unwrap(),
            "expanded PNG bytes differ between the session and one-shot routes"
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

    /// Variable-sized layer slots (#264): a secondary layer larger (in total
    /// pixels) than the primary input is now carried by the SESSION (its slot is
    /// sized to the layer's own dimensions), not routed to one-shot. Run A (the
    /// session route) must carry it (the counter advances) and produce the same
    /// PNG as Run B (the one-shot route via the escape hatch), proving the
    /// variable-slot transport is byte-equivalent. Before this change the layer
    /// overran the uniform primary-sized slot and the render was routed to
    /// one-shot instead.
    #[test]
    fn oversized_layer_renders_on_the_session_matching_one_shot() {
        if crate::common::skip_without_restricted_token_launch(
            "oversized_layer_renders_on_the_session_matching_one_shot",
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
                "skipping oversized-layer test: build aex_render_worker.exe and \
                 pf_layer_param_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-oversized-layer-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        // Primary input smaller than the secondary layer: the layer's pixel count
        // (64*48) exceeds the primary (40*30). The session now sizes the layer
        // slot to the layer's own dimensions, so it carries this render.
        let input = scratch.join("input.png");
        image::RgbaImage::from_fn(40, 30, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        let secondary = scratch.join("layer.png");
        image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x + y) as u8, (x * 7) as u8, (y * 9) as u8, 255])
        })
        .save(&secondary)
        .unwrap();
        let params = vec![layer_parameter(1, &secondary)];

        // Run A: default routing, oversized layer now carried by the session.
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_a = scratch.join("out-a.png");
        let _report_a = render_experimental_image(&root, &aex, &sha, &input, &output_a, &params)
            .expect("session-route oversized-layer render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the oversized-layer render must now be carried by the session"
        );

        // Run B: escape hatch forces the one-shot layered transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let result_b = render_experimental_image(&root, &aex, &sha, &input, &output_b, &params);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        result_b.expect("one-shot oversized-layer render");
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            after_a,
            "the escape hatch did not force the one-shot transport"
        );

        assert_eq!(
            std::fs::read(&output_a).unwrap(),
            std::fs::read(&output_b).unwrap(),
            "the oversized-layer PNG differs between the session and one-shot routes"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn zero_duration_render_matches_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "zero_duration_render_matches_the_one_shot_transport",
        ) {
            return;
        }
        // A zero-duration render (total_time == 0) is the single t=0 frame the
        // one-shot worker produces; the session now carries it too (#272), so it
        // is no longer routed to one-shot. Prove the session renders it (the
        // wrapper counter advances) byte-for-byte identically to the one-shot
        // escape-hatch route.
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping zero-duration A/B: build aex_render_worker.exe and \
                 pf_sampling_probe.aex first"
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
        let timing = RenderTiming {
            current_time: 0,
            time_step: 1,
            total_time: 0,
            time_scale: 30,
        };

        // Run A: default routing, the zero-duration render now carried by the
        // session (the counter must advance, or the comparison is vacuous).
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_a = scratch.join("out-a.png");
        render_experimental_image_at_time(&root, &aex, &sha, &input, &output_a, &[], timing)
            .expect("session-route zero-duration render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the zero-duration render must now be carried by the session"
        );

        // Run B: the escape hatch forces the one-shot argv transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let result_b =
            render_experimental_image_at_time(&root, &aex, &sha, &input, &output_b, &[], timing);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        result_b.expect("one-shot zero-duration render");
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            after_a,
            "the escape hatch did not force the one-shot transport"
        );
        assert_eq!(
            std::fs::read(&output_a).unwrap(),
            std::fs::read(&output_b).unwrap(),
            "the zero-duration PNG differs between the session and one-shot routes"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn conformance_render_settings_match_the_one_shot_transport() {
        if crate::common::skip_without_restricted_token_launch(
            "conformance_render_settings_match_the_one_shot_transport",
        ) {
            return;
        }
        // A conformance render (AEXCOMPAT_CONFORMANCE_RENDER_SETTINGS set) is now
        // carried by the session, which forwards the --conformance-render-settings-v1
        // trailer so the worker reports the same render_settings block the
        // one-shot route does (#275). The broker pre-transforms the input for the
        // alpha mode on both routes, so the pixels match too.
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root.join("target/pf-sampling-probe-build/Release/pf_sampling_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping conformance A/B: build aex_render_worker.exe and \
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
        image::RgbaImage::from_fn(48, 32, |x, y| {
            image::Rgba([(x * 5) as u8, (y * 3) as u8, (x + y) as u8, 200])
        })
        .save(&input)
        .unwrap();

        // A valid v1 conformance trailer: premultiplied alpha (a non-trivial
        // input pre-transform, so the broker actually rewrites the input on both
        // routes), software renderer. The input above has alpha 200 < 255 so the
        // premultiply changes the color channels. The conformance env changes
        // both the pre-transform and the report, so a leak past this test (e.g.
        // a panic before the explicit remove) would corrupt later tests; restore
        // the prior value (or its absence) on drop rather than relying on
        // reaching the end.
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
        let _conformance_guard = EnvVarGuard::set(
            "AEXCOMPAT_CONFORMANCE_RENDER_SETTINGS",
            "v1|premultiplied|0|-|0|software",
        );

        // Run A: default routing now carries the conformance render on the
        // session (the counter must advance).
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let before = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_a = scratch.join("out-a.png");
        let report_a = render_experimental_image(&root, &aex, &sha, &input, &output_a, &[])
            .expect("session-route conformance render");
        assert!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before,
            "the conformance render must now be carried by the session"
        );

        // Run B: the escape hatch forces the one-shot argv transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let report_b = render_experimental_image(&root, &aex, &sha, &input, &output_b, &[]);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        // The conformance env is cleared by `_conformance_guard` on drop.
        let report_b = report_b.expect("one-shot conformance render");
        assert_eq!(
            RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst),
            after_a,
            "the escape hatch did not force the one-shot transport"
        );

        // The reports must match field-for-field except the output paths and the
        // stderr-derived process diagnostics (the session's stage traces include
        // its frame loop; elapsed timings are volatile). Forwarding the trailer
        // keeps every conformance-affected report field consistent across routes.
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
            "conformance report key sets diverge between the routes"
        );
        for (key, value_a) in &flat_a {
            assert_eq!(
                Some(value_a),
                flat_b.get(key),
                "conformance report field {key} differs between the session and one-shot routes"
            );
        }
        assert_eq!(
            std::fs::read(&output_a).unwrap(),
            std::fs::read(&output_b).unwrap(),
            "the conformance PNG differs between the session and one-shot routes"
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
