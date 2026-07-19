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
        render_experimental_image_at_time_with_format_and_context, RenderPixelFormat, RenderTiming,
        DISABLE_SESSION_WRAPPER_ENV, RENDER_SESSION_WRAPPER_RENDERS,
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

    #[test]
    fn wrapper_report_matches_the_one_shot_transport() {
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
        // Secondary layer A/B equivalence needs an AEX declaring a layer
        // parameter, which pf_sampling_probe does not; the session layer
        // transport is covered by the render_session fixture integration test,
        // and the real-AEX equivalence is tracked separately.
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
