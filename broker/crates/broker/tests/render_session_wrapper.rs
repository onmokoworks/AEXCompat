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
        render_experimental_image, DISABLE_SESSION_WRAPPER_ENV, RENDER_SESSION_WRAPPER_RENDERS,
    };
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
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
