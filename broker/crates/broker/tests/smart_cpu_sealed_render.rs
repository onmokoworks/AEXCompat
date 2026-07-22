//! Regression coverage for issue #185: a pure CPU SmartFX render through the
//! sealed dispatch must not initialize any GPU backend context. Before the
//! fix, the smart worker unconditionally started a CUDA context, which pulled
//! nvcuda.dll and NVIDIA driver-store DLLs into the module audit as unknowns
//! and failed every sealed smart render on NVIDIA machines (exit 14,
//! module_audit_failed) while classic renders passed. The assertion is
//! machine-portable; its sensitivity is highest where an NVIDIA driver is
//! installed. Requires the real smart worker and the pf_smart_geometry_probe
//! fixture from this checkout; skips (with a message) when either is not
//! built.

#[cfg(test)]
#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::image_render::{
        RenderPixelFormat, RenderTiming, render_experimental_image_at_time_with_format,
    };
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root")
    }

    #[test]
    fn sealed_smart_cpu_render_passes_the_module_audit() {
        let root = repository_root();
        let worker = root.join("target/minihost-build/aex_smart_worker.exe");
        let aex =
            root.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
        if !worker.is_file() || !aex.is_file() {
            eprintln!(
                "skipping sealed smart CPU render: build aex_smart_worker.exe and \
                 pf_smart_geometry_probe.aex first"
            );
            return;
        }
        let sha = format!("{:x}", Sha256::digest(std::fs::read(&aex).unwrap()));
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-smart-cpu-audit-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&scratch).unwrap();
        let input = scratch.join("input.png");
        image::RgbaImage::from_pixel(64, 32, image::Rgba([32, 64, 96, 255]))
            .save(&input)
            .unwrap();
        let output = scratch.join("output.png");
        let report = render_experimental_image_at_time_with_format(
            &root,
            &aex,
            &sha,
            &input,
            &output,
            &[],
            RenderTiming::default(),
            true,
            RenderPixelFormat::Argb8,
        )
        .expect("sealed smart CPU render must not fail the module audit");
        assert_eq!(
            report["stage"], "interactive_image_render",
            "report: {report}"
        );
        assert_eq!(report["passed"], true, "report: {report}");
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
