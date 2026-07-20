//! A/B equivalence for the length-1 session wrapper (issue #98 stage W2):
//! the resident-session route must produce the same public
//! `interactive_image_render` report and PNG bytes as the one-shot argv
//! transport for a plain classic render. Requires the real render worker
//! executable and the pf_sampling_probe fixture from this checkout; skips
//! (with a message) when either is not built.

#[cfg(test)]
#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{
        render_experimental_image, render_experimental_image_at_time,
        render_experimental_image_at_time_with_format_and_context,
        render_experimental_image_with_parameter_animation, AnimationInterpolation, AnimationTime,
        AnimationValue, InteractiveParameter, ParameterAnimation, ParameterAnimationKey,
        RenderPixelFormat, RenderTiming, DISABLE_SESSION_WRAPPER_ENV,
        RENDER_SESSION_WRAPPER_RENDERS,
    };
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
    fn wrapper_report_matches_the_one_shot_transport() {
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
            image::Rgba([
                (x * 3) as u8,
                (y * 5) as u8,
                (x + y) as u8,
                255,
            ])
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
        assert_eq!(ctx_flat_a.get("spatial_contract_ok"), Some(&serde_json::json!(true)));
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

        // Aux-channel A/B equivalence (issue #211): the broker now carries aux
        // channels through the session as the `--aux-manifest-v1` auxiliary
        // option, sharing the same broker-built manifest the one-shot path
        // emits. A successful byte-equal A/B cannot run here yet: the real
        // worker's sealed classic-render path currently rejects an
        // aux-carrying render with a bare `exit_code 3` (empty stderr, no stage
        // events) on BOTH routes, even though the standalone aux transport
        // self-test (`--self-test-pf-ae-channel-transport`) accepts the same
        // manifest. That gap is pre-existing and independent of #211 (the
        // one-shot aux path predates it), tracked in issue #231.
        //
        // The session/one-shot equivalence #211 delivers was verified manually
        // against the real worker: both routes fail identically for an
        // aux-carrying render (same exit_code, same diagnostics), proving the
        // session wrapper carries aux exactly as the one-shot path does. Since
        // the render fails, the session route falls back and
        // RENDER_SESSION_WRAPPER_RENDERS does not advance, so a counter-based
        // A/B is not meaningful until #231 lands a working aux render path. The
        // broker-side wiring (aux_channels -> manifest -> SessionOpenRequest) is
        // guarded machine-portably by
        // `prepare_aux_transport_output_satisfies_the_session_aux_manifest_contract`
        // in image_render.rs.

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
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex =
            root.join("target/pf-layer-param-probe-build/Release/pf_layer_param_probe.aex");
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
            let report =
                render_experimental_image_at_time(&root, &aex, &sha, &input, output, params, timing)
                    .expect("layer+slider render");
            let carried = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst) > before;
            if disable_session {
                unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
            }
            (report, carried)
        };

        let params = vec![
            layer_parameter(1, &secondary),
            float_parameter(2, 200.0),
        ];
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
            assert!(carried_a, "the session wrapper did not carry run A ({label})");
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

        let other_slider = vec![
            layer_parameter(1, &secondary),
            float_parameter(2, 40.0),
        ];
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
        let other_layer = vec![
            layer_parameter(1, &secondary2),
            float_parameter(2, 200.0),
        ];
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
}
