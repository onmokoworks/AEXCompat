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
        render_experimental_audio, render_experimental_image, render_experimental_image_at_time,
        render_experimental_image_at_time_with_format_and_context,
        render_experimental_image_at_time_with_format_context_and_ui_action,
        render_experimental_image_with_parameter_animation, AnimationInterpolation, AnimationTime,
        AnimationValue, InteractiveParameter, ParameterAnimation, ParameterAnimationKey,
        RenderPixelFormat, RenderTiming, RenderUiAction, DISABLE_SESSION_WRAPPER_ENV,
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

    /// A/B equivalence for the length-1 *audio* session wrapper (issue #251):
    /// the resident audio-session route must produce the same public
    /// `render_experimental_audio` report contract and the same f32 output
    /// bytes as the one-shot `--render-audio` transport, for a real audio AEX
    /// (SDK_Backwards). Requires the render worker and the SDK_Backwards
    /// fixture from this checkout; skips (with a message) when either is
    /// missing.
    #[test]
    fn audio_wrapper_report_matches_the_one_shot_transport() {
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
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex =
            root.join("target/pf-custom-ui-probe-build/Release/pf_custom_ui_probe.aex");
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
        assert_eq!(report_a.get("custom_ui_click_dispatched"), Some(&serde_json::json!(true)));
        assert_eq!(report_a.get("custom_ui_click_changed_value"), Some(&serde_json::json!(true)));
        assert_eq!(report_a.get("custom_ui_context_closed"), Some(&serde_json::json!(true)));
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
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex =
            root.join("target/pf-custom-ui-probe-build/Release/pf_custom_ui_probe.aex");
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
        assert_eq!(report_a.get("custom_ui_draw_dispatched"), Some(&serde_json::json!(true)));
        assert_eq!(report_a.get("custom_ui_draw_error"), Some(&serde_json::json!(0)));
        assert_eq!(report_a.get("custom_ui_context_closed"), Some(&serde_json::json!(true)));
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

    /// A/B equivalence for an expand-output effect (issue #261): the length-1
    /// session route re-opens with a larger output slot and renders the
    /// expanded output byte-identically to the one-shot argv transport, instead
    /// of falling back. The fixture is pf_expand_allowed_probe (FRAME_SETUP
    /// grows the output by 4px with PF_OutFlag_I_EXPAND_BUFFER), which overruns
    /// the initial slot and drives the resize_needed re-open. Requires the
    /// render worker and the resize probe (tools/build-pf-frame-resize-probe.ps1).
    #[test]
    fn expand_output_matches_the_one_shot_transport() {
        let _env_guard = SESSION_ROUTE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex = root
            .join("target/pf-frame-resize-probe-build/Release/pf_expand_allowed_probe.aex");
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

        // Run A: default routing. The effect expands 64x48 -> 68x52, overruns
        // the initial 64x48 slot, and the wrapper re-opens at 68x52 on the
        // session route. The counter proves the session carried it (a silent
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

        // Run B: the escape hatch forces the one-shot argv transport.
        unsafe { std::env::set_var(DISABLE_SESSION_WRAPPER_ENV, "1") };
        let after_a = RENDER_SESSION_WRAPPER_RENDERS.load(Ordering::SeqCst);
        let output_b = scratch.join("out-b.png");
        let report_b = render_experimental_image(&root, &aex, &sha, &input, &output_b, &[]);
        unsafe { std::env::remove_var(DISABLE_SESSION_WRAPPER_ENV) };
        let report_b = report_b.expect("one-shot expand render");
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
}
