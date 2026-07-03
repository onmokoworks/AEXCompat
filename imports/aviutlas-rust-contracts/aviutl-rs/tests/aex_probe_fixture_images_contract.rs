#[allow(dead_code)]
#[path = "../examples/aex_probe_fixture_images.rs"]
mod aex_probe_fixture_images;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

fn target_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-probe-fixtures")
        .join(name)
}

fn unique_target_root(label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    target_root(&format!(
        "contract-{}-{}-{stamp}",
        std::process::id(),
        label
    ))
}

static CWD_LOCK: Mutex<()> = Mutex::new(());

struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    fn enter(path: &Path) -> Self {
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

#[cfg(windows)]
fn create_dir_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(original, link)
}

#[cfg(unix)]
fn create_dir_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

fn repo_schema() -> Value {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .join("analysis")
        .join("AEX_PROBE_SYNTHETIC_IMAGE_FIXTURES_SCHEMA_2026-06-01.json");
    let text = std::fs::read_to_string(&schema_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", schema_path.display()));
    serde_json::from_str(&text).expect("fixture schema should parse")
}

fn string_array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item == expected)
}

fn assert_manifest_matches_schema(manifest: &Value, schema: &Value) {
    for field in schema["required_fields"].as_array().unwrap() {
        let field = field.as_str().unwrap();
        assert!(
            manifest.get(field).is_some(),
            "manifest missing required field {field}"
        );
    }
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(
            manifest.get(field).unwrap_or(&Value::Null),
            expected,
            "manifest value mismatch for {field}"
        );
    }
    assert!(manifest["output_root"]
        .as_str()
        .unwrap_or_default()
        .contains("target/aex-probe-fixtures"));
    let images = manifest["images"].as_array().unwrap();
    assert_eq!(images.len(), 3);
    for expected in schema["required_images"].as_array().unwrap() {
        let id = expected["id"].as_str().unwrap();
        let image = images
            .iter()
            .find(|image| image["id"] == id)
            .unwrap_or_else(|| panic!("missing image {id}"));
        for field in schema["image_required_fields"].as_array().unwrap() {
            let field = field.as_str().unwrap();
            assert!(image.get(field).is_some(), "image {id} missing {field}");
        }
        assert_eq!(image["file_name"], expected["file_name"]);
        assert_eq!(image["relative_path"], expected["file_name"]);
        assert_eq!(image["pattern"], expected["pattern"]);
        assert_eq!(image["pixel_format"], "rgba8");
        assert_eq!(image["width"], manifest["width"]);
        assert_eq!(image["height"], manifest["height"]);
    }
    let checks = manifest["checks"].as_array().unwrap();
    for check in schema["required_check_names"].as_array().unwrap() {
        let check = check.as_str().unwrap();
        assert!(
            checks
                .iter()
                .any(|item| item["name"] == check && item["status"] == "passed"),
            "missing passed check {check}"
        );
    }
    let serialized = serde_json::to_string(manifest)
        .unwrap()
        .to_ascii_lowercase();
    for token in schema["forbidden_manifest_tokens"].as_array().unwrap() {
        let token = token.as_str().unwrap();
        assert!(
            !serialized.contains(token),
            "manifest should not contain forbidden token {token}"
        );
    }
    for note in schema["required_notes"].as_array().unwrap() {
        let note = note.as_str().unwrap();
        assert!(
            string_array_contains(&manifest["notes"], note),
            "manifest missing note {note}"
        );
    }
}

#[test]
fn fixture_generator_writes_three_synthetic_rgba8_pngs_and_manifest() {
    let root = unique_target_root("ready");
    let manifest_path = root.join("manifest.local.json");
    let manifest =
        aex_probe_fixture_images::generate_fixture_images(&root, 16, &manifest_path).unwrap();
    assert_eq!(manifest.status, "synthetic_fixture_images_ready_no_load");

    let gradient = image::open(root.join("gradient_rgba8.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(gradient.dimensions(), (16, 16));
    assert_eq!(gradient.get_pixel(0, 0).0, [0, 0, 0, 255]);
    assert_eq!(gradient.get_pixel(15, 15).0, [255, 255, 255, 255]);
    assert!(gradient.get_pixel(15, 0).0[0] > gradient.get_pixel(0, 0).0[0]);
    assert!(gradient.get_pixel(0, 15).0[1] > gradient.get_pixel(0, 0).0[1]);

    let checker = image::open(root.join("checker_rgba8.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(checker.dimensions(), (16, 16));
    assert_eq!(checker.get_pixel(0, 0).0, [232, 232, 232, 255]);
    assert_eq!(checker.get_pixel(2, 0).0, [32, 32, 32, 255]);
    assert_eq!(checker.get_pixel(0, 2).0, [32, 32, 32, 255]);

    let solid_alpha = image::open(root.join("solid_alpha_rgba8.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(solid_alpha.dimensions(), (16, 16));
    assert_eq!(solid_alpha.get_pixel(0, 0).0, [96, 168, 255, 128]);
    assert_eq!(solid_alpha.get_pixel(15, 15).0, [96, 168, 255, 128]);

    let manifest_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_manifest_matches_schema(&manifest_json, &repo_schema());
}

#[test]
fn fixture_generator_rejects_paths_outside_generated_fixture_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let err = aex_probe_fixture_images::generate_fixture_images(
        &root,
        8,
        &root.join("manifest.local.json"),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("target/aex-probe-fixtures"));

    let sibling_prefix = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-probe-fixtures-evil");
    let err = aex_probe_fixture_images::generate_fixture_images(
        &sibling_prefix,
        8,
        &sibling_prefix.join("manifest.local.json"),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("target/aex-probe-fixtures"));

    let outside_crate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target")
        .join("aex-probe-fixtures")
        .join("outside-crate");
    let err = aex_probe_fixture_images::generate_fixture_images(
        &outside_crate,
        8,
        &outside_crate.join("manifest.local.json"),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("target/aex-probe-fixtures"));

    let root = unique_target_root("bad-manifest");
    let err = aex_probe_fixture_images::generate_fixture_images(
        &root,
        8,
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("outside-manifest.local.json"),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("target/aex-probe-fixtures"));
}

#[test]
fn fixture_generator_rejects_existing_outputs_without_overwrite() {
    let root = unique_target_root("existing-output");
    let manifest_path = root.join("manifest.local.json");
    aex_probe_fixture_images::generate_fixture_images(&root, 8, &manifest_path).unwrap();

    let err = aex_probe_fixture_images::generate_fixture_images(&root, 8, &manifest_path)
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("generated output already exists"),
        "expected output preflight failure, got {err}"
    );
    let manifest_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(
        manifest_json["status"],
        "synthetic_fixture_images_ready_no_load"
    );
}

#[test]
fn fixture_generator_preflights_all_outputs_before_writing() {
    let root = unique_target_root("preflight-existing-later-output");
    std::fs::create_dir_all(&root).unwrap();
    let existing_checker = root.join("checker_rgba8.png");
    std::fs::write(&existing_checker, b"already here").unwrap();
    let manifest_path = root.join("manifest.local.json");

    let err = aex_probe_fixture_images::generate_fixture_images(&root, 8, &manifest_path)
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("generated output already exists"),
        "expected output preflight failure, got {err}"
    );
    assert!(!root.join("gradient_rgba8.png").exists());
    assert!(!root.join("solid_alpha_rgba8.png").exists());
    assert!(!manifest_path.exists());
    assert_eq!(std::fs::read(&existing_checker).unwrap(), b"already here");
}

#[test]
fn fixture_generator_rolls_back_pngs_when_manifest_parent_fails_late() {
    let root = unique_target_root("manifest-parent-file");
    std::fs::create_dir_all(&root).unwrap();
    let manifest_parent_file = root.join("manifest-parent-file");
    std::fs::write(&manifest_parent_file, b"not a directory").unwrap();
    let manifest_path = manifest_parent_file.join("manifest.local.json");

    let err = aex_probe_fixture_images::generate_fixture_images(&root, 8, &manifest_path)
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("failed to create manifest dir"),
        "expected manifest parent failure, got {err}"
    );
    assert!(!root.join("gradient_rgba8.png").exists());
    assert!(!root.join("checker_rgba8.png").exists());
    assert!(!root.join("solid_alpha_rgba8.png").exists());
    assert_eq!(
        std::fs::read(&manifest_parent_file).unwrap(),
        b"not a directory"
    );
}

#[test]
fn fixture_generator_rejects_symlinked_generated_root_escape() {
    let escape_root = unique_target_root("symlink-escape-outside");
    let link = unique_target_root("symlink-escape-link");
    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir_all(&escape_root);
    std::fs::create_dir_all(&escape_root).unwrap();
    if create_dir_symlink(&escape_root, &link).is_err() {
        return;
    }

    let err =
        aex_probe_fixture_images::generate_fixture_images(&link, 8, &link.join("manifest.json"))
            .unwrap_err()
            .to_string();

    assert!(err.contains("target/aex-probe-fixtures"));
    assert!(!escape_root.join("gradient_rgba8.png").exists());

    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_file(&link);
}

#[test]
fn fixture_generator_rejects_symlinked_output_ancestor_escape() {
    let outside = unique_target_root("ancestor-link-outside");
    let parent = unique_target_root("ancestor-link-parent");
    let link = parent.join("link");
    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(&parent).unwrap();
    if create_dir_symlink(&outside, &link).is_err() {
        return;
    }
    let output_root = link.join("nested");

    let err = aex_probe_fixture_images::generate_fixture_images(
        &output_root,
        8,
        &output_root.join("manifest.json"),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("target/aex-probe-fixtures"));
    assert!(!outside.join("nested").join("gradient_rgba8.png").exists());

    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_file(&link);
}

#[test]
fn fixture_generator_rejects_relative_paths_from_non_crate_cwd() {
    let _guard = CWD_LOCK.lock().unwrap();
    let foreign_cwd = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target")
        .join(format!(
            "fixture-cwd-drift-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&foreign_cwd).unwrap();
    let _cwd_guard = CurrentDirGuard::enter(&foreign_cwd);
    let relative_root = Path::new("target")
        .join("aex-probe-fixtures")
        .join("cwd-drift");
    let relative_manifest = relative_root.join("manifest.local.json");

    let err =
        aex_probe_fixture_images::generate_fixture_images(&relative_root, 8, &relative_manifest)
            .unwrap_err()
            .to_string();

    assert!(err.contains("target/aex-probe-fixtures"));
    assert!(!foreign_cwd
        .join("target")
        .join("aex-probe-fixtures")
        .join("cwd-drift")
        .join("gradient_rgba8.png")
        .exists());
}

#[test]
fn fixture_schema_pins_no_load_no_render_boundaries() {
    let schema = repo_schema();
    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["compatibility_classification"],
        "Synthetic RGBA8 fixture inputs only"
    );
    assert_eq!(schema["required_values"]["native_load_performed"], false);
    assert_eq!(schema["required_values"]["render_performed"], false);
    assert_eq!(schema["required_values"]["aex_loaded"], false);
    assert_eq!(schema["required_values"]["worker_started"], false);
    assert_eq!(schema["required_values"]["broker_invoked"], false);
    assert_eq!(schema["required_values"]["ofx_route_invoked"], false);
    assert_eq!(schema["required_values"]["ae_invoked"], false);
    assert_eq!(schema["required_values"]["private_payload_copied"], false);

    for file_name in [
        "gradient_rgba8.png",
        "checker_rgba8.png",
        "solid_alpha_rgba8.png",
    ] {
        assert!(
            schema["required_images"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["file_name"] == file_name),
            "schema missing required fixture image {file_name}"
        );
    }
    for token in [
        "sha256",
        "base64",
        "binary_payload",
        "loadlibrary",
        "libloading",
        "effectmain",
        "input_png",
        "output_png",
    ] {
        assert!(string_array_contains(
            &schema["forbidden_manifest_tokens"],
            token
        ));
    }
}
