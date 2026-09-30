#![cfg(windows)]

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

fn run(
    exe: &str,
    repository: &Path,
    plugin: &Path,
    input: &Path,
    output: &Path,
    command: &str,
    format: &str,
) -> std::process::Output {
    Command::new(exe)
        .current_dir(repository)
        .args(["--headless", command])
        .arg(plugin)
        .arg(input)
        .arg(output)
        .args([format, "smart", "0", "300", "30"])
        .output()
        .unwrap()
}

#[test]
fn built_cli_dispatches_raw_and_exr_and_rejects_wrong_exr_depth() {
    let repository = repository_root();
    let plugin =
        repository.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
    let render_worker = repository.join("target/minihost-build/aex_worker.exe");
    let smart_worker = repository.join("target/minihost-build/aex_worker.exe");
    if !plugin.is_file() || !render_worker.is_file() || !smart_worker.is_file() {
        eprintln!("skipping built artifact CLI test: native fixtures are not built");
        return;
    }
    let scratch = std::env::temp_dir().join(format!(
        "aexcompat-artifact-cli-{}-{:x}",
        std::process::id(),
        Sha256::digest(plugin.as_os_str().as_encoded_bytes())
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let input = scratch.join("input.png");
    image::RgbaImage::from_pixel(64, 48, image::Rgba([17, 23, 31, 255]))
        .save(&input)
        .unwrap();
    let exe = env!("CARGO_BIN_EXE_aexcompat-harness");

    let raw_dir = scratch.join("raw");
    let raw = run(
        exe,
        &repository,
        &plugin,
        &input,
        &raw_dir,
        "--render-raw",
        "argb16",
    );
    assert!(
        raw.status.success(),
        "raw stderr: {}",
        String::from_utf8_lossy(&raw.stderr)
    );
    let raw_report: serde_json::Value = serde_json::from_slice(&raw.stdout).unwrap();
    assert_eq!(raw_report["passed"], true);
    assert!(raw_dir.join("output.bin").is_file());
    assert!(raw_dir.join("output.json").is_file());

    let exr_dir = scratch.join("exr");
    let exr = run(
        exe,
        &repository,
        &plugin,
        &input,
        &exr_dir,
        "--render-exr",
        "argb32f",
    );
    assert!(
        exr.status.success(),
        "EXR stderr: {}",
        String::from_utf8_lossy(&exr.stderr)
    );
    let exr_report: serde_json::Value = serde_json::from_slice(&exr.stdout).unwrap();
    assert_eq!(exr_report["passed"], true);
    assert!(exr_dir.join("output.exr").is_file());
    assert!(exr_dir.join("output.json").is_file());

    let rejected_dir = scratch.join("rejected");
    let rejected = run(
        exe,
        &repository,
        &plugin,
        &input,
        &rejected_dir,
        "--render-exr",
        "argb16",
    );
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("render-exr requires argb32f"));
    assert!(!rejected_dir.exists());
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn built_cli_renders_through_a_target_directory_junction() {
    let repository = repository_root();
    let plugin =
        repository.join("target/pf-smart-geometry-probe-build/Release/pf_smart_geometry_probe.aex");
    if !plugin.is_file()
        || !repository
            .join("target/minihost-build/aex_worker.exe")
            .is_file()
    {
        eprintln!("skipping built artifact CLI test: native fixtures are not built");
        return;
    }
    let scratch = std::env::temp_dir().join(format!(
        "aexcompat-artifact-cli-junction-{}-{:x}",
        std::process::id(),
        Sha256::digest(repository.as_os_str().as_encoded_bytes())
    ));
    std::fs::create_dir(&scratch).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            // Remove the junction itself before deleting our scratch tree.
            let target = self.0.join("target");
            if std::fs::symlink_metadata(&target).is_ok() && std::fs::remove_dir(&target).is_err() {
                return;
            }
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(scratch.clone());
    for name in ["guest", "broker"] {
        std::fs::create_dir(scratch.join(name)).unwrap();
        std::fs::write(scratch.join(name).join("Cargo.toml"), []).unwrap();
    }
    let created = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(scratch.join("target"))
        .arg(repository.join("target"))
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let input = scratch.join("input.png");
    image::RgbaImage::from_pixel(64, 48, image::Rgba([17, 23, 31, 255]))
        .save(&input)
        .unwrap();
    let output = scratch.join("raw");
    let rendered = run(
        env!("CARGO_BIN_EXE_aexcompat-harness"),
        &scratch,
        &plugin,
        &input,
        &output,
        "--render-raw",
        "argb16",
    );
    assert!(
        rendered.status.success(),
        "junction render stderr: {}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&rendered.stdout).unwrap();
    assert_eq!(report["passed"], true);
    assert_eq!(
        std::fs::metadata(output.join("output.bin")).unwrap().len(),
        64 * 48 * 8
    );
}
