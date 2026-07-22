//! End-to-end coverage for the resident interactive render session
//! (issue #107): the harness's live-render adapter drives the REAL render
//! worker with per-frame parameter updates (protocol v:2) and every frame's
//! output must reflect that frame's values. Requires the real worker and the
//! pf_parameter_echo_probe fixture from this checkout; skips (with a message)
//! when either is not built.

mod common;

#[cfg(test)]
#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{
        InteractiveParameter, InteractiveRenderSession, InteractiveSessionOpen, RenderPixelFormat,
    };
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root")
    }

    fn echo_parameter(value: f64) -> InteractiveParameter {
        serde_json::from_value(serde_json::json!({
            "slot": 1, "name": "Echo", "kind": "float",
            "minimum": 0.0, "maximum": 255.0, "value": value,
            "choices": [], "color": [0, 0, 0, 0], "components": [0.0, 0.0, 0.0],
            "component_count": 0, "layer_path": null,
            "enabled": true, "visible": true, "supervised": false,
        }))
        .expect("echo parameter fixture")
    }

    fn built_artifacts() -> Option<(PathBuf, PathBuf, String)> {
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_render_worker.exe");
        let aex =
            root.join("target/pf-parameter-echo-probe-build/Release/pf_parameter_echo_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping resident-session live test: build aex_render_worker.exe and \
                 pf_parameter_echo_probe.aex first"
            );
            return None;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        Some((root, aex, sha))
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-resident-live-{tag}-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        scratch
    }

    #[test]
    fn per_frame_parameter_updates_change_the_real_render() {
        if crate::common::skip_without_restricted_token_launch(
            "per_frame_parameter_updates_change_the_real_render",
        ) {
            return;
        }
        let Some((root, aex, sha)) = built_artifacts() else {
            return;
        };
        let scratch = scratch_dir("echo");
        let (width, height) = (64u32, 32u32);
        let rgba = vec![0u8; (width * height * 4) as usize];
        let declared = [echo_parameter(0.0)];
        let mut session = InteractiveRenderSession::open(InteractiveSessionOpen {
            repository: &root,
            plugin_id: "experimental",
            plugin_path: &aex,
            plugin_sha256: &sha,
            parameters: Some(&declared),
            dependencies: Vec::new(),
            width,
            height,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            timeout_ms: 30_000,
        })
        .expect("open resident session against the real worker");

        // The echo probe fills every pixel with (value, 255-value, 128, 255):
        // each live update must land in exactly that frame's output.
        for (index, value) in [(0i32, 32u8), (1, 200), (2, 91)] {
            let update = [echo_parameter(f64::from(value))];
            let output = scratch.join(format!("frame-{index}.png"));
            let report = session
                .render(&rgba, index, Some(&update), &output)
                .expect("live frame renders");
            assert_eq!(report["passed"], true, "report: {report}");
            let decoded = image::open(&output).expect("output PNG decodes").to_rgba8();
            assert_eq!(
                decoded.get_pixel(0, 0).0,
                [value, 255 - value, 128, 255],
                "frame {index} must carry the value {value}"
            );
        }
        let close = session.close();
        assert_eq!(close["frames_ok"], 3);
        assert_eq!(close["parameter_update_frames"], 3);
        assert_eq!(close["session_clean"], true, "close: {close}");
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Manual latency comparison for docs/RENDER_SESSION_INVESTIGATION notes
    /// (`cargo test ... resident_session_latency -- --ignored --nocapture`).
    /// Reference values only, machine dependent, never frozen evidence.
    #[test]
    #[ignore = "manual latency measurement; prints medians"]
    fn resident_session_latency_versus_one_shot() {
        let Some((root, aex, sha)) = built_artifacts() else {
            return;
        };
        let scratch = scratch_dir("latency");
        let (width, height) = (1920u32, 1080u32);
        let input = scratch.join("input.png");
        image::RgbaImage::from_pixel(width, height, image::Rgba([8, 16, 32, 255]))
            .save(&input)
            .unwrap();
        let rgba = vec![0u8; (width * height * 4) as usize];
        let runs = 12usize;

        let median = |mut samples: Vec<f64>| {
            samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
            samples[samples.len() / 2]
        };

        // One-shot per parameter change: the pre-#107 GUI behavior (each
        // render pays worker start + AEX load + lifecycle + staging).
        let mut one_shot = Vec::new();
        for index in 0..runs {
            let parameters = [echo_parameter((index % 200) as f64)];
            let output = scratch.join(format!("oneshot-{index}.png"));
            let started = std::time::Instant::now();
            aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                &root,
                &aex,
                &sha,
                &input,
                &output,
                &parameters,
                aexcompat_broker::image_render::RenderTiming {
                    current_time: 0,
                    time_step: 1,
                    total_time: 300,
                    time_scale: 30,
                },
                false,
                RenderPixelFormat::Argb8,
                None,
                None,
                aexcompat_broker::image_render::RenderGpuBackend::Auto,
                Vec::new(),
            )
            .expect("one-shot render");
            one_shot.push(started.elapsed().as_secs_f64() * 1000.0);
        }

        // Resident session: open once, per-frame parameter updates.
        let declared = [echo_parameter(0.0)];
        let open_started = std::time::Instant::now();
        let mut session = InteractiveRenderSession::open(InteractiveSessionOpen {
            repository: &root,
            plugin_id: "experimental",
            plugin_path: &aex,
            plugin_sha256: &sha,
            parameters: Some(&declared),
            dependencies: Vec::new(),
            width,
            height,
            pixel_format: RenderPixelFormat::Argb8,
            time_step: 1,
            total_time: 300,
            time_scale: 30,
            timeout_ms: 30_000,
        })
        .expect("open resident session");
        let open_ms = open_started.elapsed().as_secs_f64() * 1000.0;
        let mut resident = Vec::new();
        for index in 0..runs {
            let update = [echo_parameter((index % 200) as f64)];
            let output = scratch.join(format!("resident-{index}.png"));
            let started = std::time::Instant::now();
            session
                .render(&rgba, index as i32, Some(&update), &output)
                .expect("resident render");
            resident.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let close = session.close();
        assert_eq!(close["session_clean"], true, "close: {close}");

        println!(
            "one_shot_ms median={:.1} min={:.1} max={:.1}",
            median(one_shot.clone()),
            one_shot.iter().cloned().fold(f64::INFINITY, f64::min),
            one_shot.iter().cloned().fold(0.0, f64::max),
        );
        println!("resident_open_ms={open_ms:.1}");
        println!(
            "resident_frame_ms median={:.1} min={:.1} max={:.1}",
            median(resident.clone()),
            resident.iter().cloned().fold(f64::INFINITY, f64::min),
            resident.iter().cloned().fold(0.0, f64::max),
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
