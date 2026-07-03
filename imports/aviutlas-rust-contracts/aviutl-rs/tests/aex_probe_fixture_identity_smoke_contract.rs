#[allow(dead_code)]
#[path = "../examples/aex_probe_fixture_images.rs"]
mod aex_probe_fixture_images;

#[allow(dead_code)]
#[path = "../examples/aex_probe_fixture_identity_smoke.rs"]
mod aex_probe_fixture_identity_smoke;

use serde_json::Value;
use std::path::{Path, PathBuf};

fn fixture_target_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-probe-fixtures")
        .join(name)
}

fn smoke_target_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-image-probe")
        .join("fixture-identity-smoke")
        .join(name)
}

fn unique_label(label: &str) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("contract-{}-{label}-{stamp}", std::process::id())
}

fn repo_schema() -> Value {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .join("analysis")
        .join("AEX_PROBE_FIXTURE_IDENTITY_SMOKE_SCHEMA_2026-06-01.json");
    let text = std::fs::read_to_string(&schema_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", schema_path.display()));
    serde_json::from_str(&text).expect("fixture identity smoke schema should parse")
}

fn string_array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item == expected)
}

fn make_fixture_manifest(label: &str, size: u32) -> (PathBuf, PathBuf) {
    let root = fixture_target_root(&unique_label(label));
    let manifest = root.join("manifest.local.json");
    aex_probe_fixture_images::generate_fixture_images(&root, size, &manifest).unwrap();
    (root, manifest)
}

fn assert_report_matches_schema(report: &Value, schema: &Value) {
    for field in schema["required_fields"].as_array().unwrap() {
        let field = field.as_str().unwrap();
        assert!(
            report.get(field).is_some(),
            "smoke report missing required field {field}"
        );
    }
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(
            report.get(field).unwrap_or(&Value::Null),
            expected,
            "smoke report value mismatch for {field}"
        );
    }
    assert!(report["fixture_manifest"]
        .as_str()
        .unwrap_or_default()
        .contains("target/aex-probe-fixtures"));
    assert!(report["output_root"]
        .as_str()
        .unwrap_or_default()
        .contains("target/aex-image-probe"));

    let entries = report["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    for id in schema["required_entry_ids"].as_array().unwrap() {
        let id = id.as_str().unwrap();
        let entry = entries
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap_or_else(|| panic!("missing smoke entry {id}"));
        for field in schema["entry_required_fields"].as_array().unwrap() {
            let field = field.as_str().unwrap();
            assert!(entry.get(field).is_some(), "entry {id} missing {field}");
        }
        for (field, expected) in schema["required_entry_values"].as_object().unwrap() {
            assert_eq!(
                entry.get(field).unwrap_or(&Value::Null),
                expected,
                "entry {id} value mismatch for {field}"
            );
        }
    }

    let checks = report["checks"].as_array().unwrap();
    for check in schema["required_check_names"].as_array().unwrap() {
        let check = check.as_str().unwrap();
        assert!(
            checks
                .iter()
                .any(|item| item["name"] == check && item["status"] == "passed"),
            "missing passed smoke check {check}"
        );
    }

    let serialized = serde_json::to_string(report).unwrap().to_ascii_lowercase();
    for token in schema["forbidden_report_tokens"].as_array().unwrap() {
        let token = token.as_str().unwrap();
        assert!(
            !serialized.contains(token),
            "smoke report should not contain forbidden token {token}"
        );
    }
    for note in schema["required_notes"].as_array().unwrap() {
        let note = note.as_str().unwrap();
        assert!(
            string_array_contains(&report["notes"], note),
            "smoke report missing note {note}"
        );
    }
}

#[test]
fn fixture_identity_smoke_runs_identity_transport_over_all_synthetic_images() {
    let (fixture_root, fixture_manifest) = make_fixture_manifest("identity-ready", 16);
    let smoke_root = smoke_target_root(&unique_label("identity-ready"));
    let report_path = smoke_root.join("smoke.local.json");

    let report = aex_probe_fixture_identity_smoke::run_fixture_identity_smoke(
        &fixture_manifest,
        &smoke_root,
        &report_path,
    )
    .unwrap();

    assert_eq!(report.status, "fixture_identity_smoke_ready_no_load");
    assert_eq!(report.transport_count, 3);
    assert_eq!(report.identity_pixels_checked_count, 3);
    assert!(!report.native_load_performed);
    assert!(!report.render_performed);
    assert!(!report.aex_loaded);
    assert!(!report.worker_started);
    assert!(report.broker_invoked);
    assert!(!report.ofx_route_invoked);
    assert!(!report.ae_invoked);
    assert!(!report.private_payload_copied);
    assert!(!report.aex_render_correctness_evidence);

    let report_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_report_matches_schema(&report_json, &repo_schema());

    for entry in report.entries {
        let input = fixture_root.join(format!("{}_rgba8.png", entry.id));
        let output = smoke_root.join(format!("{}_identity_rgba8.png", entry.id));
        assert!(output.exists());
        let input_rgba = image::open(input).unwrap().to_rgba8();
        let output_rgba = image::open(output).unwrap().to_rgba8();
        assert_eq!(input_rgba.as_raw(), output_rgba.as_raw());
    }
}

#[test]
fn fixture_identity_smoke_rejects_non_synthetic_or_contaminated_manifest() {
    let (_fixture_root, fixture_manifest) = make_fixture_manifest("bad-manifest", 8);
    let mut manifest_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture_manifest).unwrap()).unwrap();
    manifest_json["render_performed"] = Value::Bool(true);
    let bad_manifest = fixture_manifest
        .parent()
        .unwrap()
        .join("manifest.contaminated.json");
    std::fs::write(
        &bad_manifest,
        serde_json::to_string_pretty(&manifest_json).unwrap(),
    )
    .unwrap();
    let smoke_root = smoke_target_root(&unique_label("bad-manifest"));

    let err = aex_probe_fixture_identity_smoke::run_fixture_identity_smoke(
        &bad_manifest,
        &smoke_root,
        &smoke_root.join("smoke.local.json"),
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("no-load/no-render boundary"));
    assert!(!smoke_root.join("gradient_identity_rgba8.png").exists());
    assert!(!smoke_root.join("smoke.local.json").exists());
}

#[test]
fn fixture_identity_smoke_rejects_forged_image_set() {
    let (_fixture_root, fixture_manifest) = make_fixture_manifest("forged-image-set", 8);
    let mut manifest_json: Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture_manifest).unwrap()).unwrap();
    manifest_json["images"][0]["id"] = Value::String("sha256".to_string());
    manifest_json["images"][0]["file_name"] = Value::String("sha256.png".to_string());
    manifest_json["images"][0]["relative_path"] = Value::String("sha256.png".to_string());
    manifest_json["images"][0]["pattern"] = Value::String("sha256-payload".to_string());
    let bad_manifest = fixture_manifest
        .parent()
        .unwrap()
        .join("manifest.forged.json");
    std::fs::write(
        &bad_manifest,
        serde_json::to_string_pretty(&manifest_json).unwrap(),
    )
    .unwrap();
    let smoke_root = smoke_target_root(&unique_label("forged-image-set"));

    let err = aex_probe_fixture_identity_smoke::run_fixture_identity_smoke(
        &bad_manifest,
        &smoke_root,
        &smoke_root.join("smoke.local.json"),
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("generated synthetic set"),
        "expected synthetic set rejection, got {err}"
    );
    assert!(!smoke_root.join("sha256_identity_rgba8.png").exists());
    assert!(!smoke_root.join("smoke.local.json").exists());
}

#[test]
fn fixture_identity_smoke_rejects_forged_fixture_pixels() {
    let (fixture_root, fixture_manifest) = make_fixture_manifest("forged-pixels", 8);
    let forged = image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]));
    forged
        .save(fixture_root.join("gradient_rgba8.png"))
        .expect("test should overwrite generated target fixture");
    let smoke_root = smoke_target_root(&unique_label("forged-pixels"));

    let err = aex_probe_fixture_identity_smoke::run_fixture_identity_smoke(
        &fixture_manifest,
        &smoke_root,
        &smoke_root.join("smoke.local.json"),
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("pixels must match generated pattern"),
        "expected generated-pattern rejection, got {err}"
    );
    assert!(!smoke_root.join("gradient_identity_rgba8.png").exists());
    assert!(!smoke_root.join("smoke.local.json").exists());
}

#[test]
fn fixture_identity_smoke_rejects_forbidden_report_token_paths() {
    let fixture_root = fixture_target_root(&unique_label("sha256-path"));
    let fixture_manifest = fixture_root.join("manifest.local.json");
    aex_probe_fixture_images::generate_fixture_images(&fixture_root, 8, &fixture_manifest).unwrap();
    let smoke_root = smoke_target_root(&unique_label("safe-smoke-root"));

    let err = aex_probe_fixture_identity_smoke::run_fixture_identity_smoke(
        &fixture_manifest,
        &smoke_root,
        &smoke_root.join("smoke.local.json"),
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("forbidden report token sha256"),
        "expected forbidden token rejection, got {err}"
    );
    assert!(!smoke_root.join("gradient_identity_rgba8.png").exists());
    assert!(!smoke_root.join("smoke.local.json").exists());
}

#[test]
fn fixture_identity_smoke_preflights_outputs_before_transport() {
    let (_fixture_root, fixture_manifest) = make_fixture_manifest("existing-output", 8);
    let smoke_root = smoke_target_root(&unique_label("existing-output"));
    std::fs::create_dir_all(&smoke_root).unwrap();
    let existing_checker = smoke_root.join("checker_identity_rgba8.png");
    std::fs::write(&existing_checker, b"already here").unwrap();

    let err = aex_probe_fixture_identity_smoke::run_fixture_identity_smoke(
        &fixture_manifest,
        &smoke_root,
        &smoke_root.join("smoke.local.json"),
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("generated smoke output already exists"),
        "expected output preflight failure, got {err}"
    );
    assert!(!smoke_root.join("gradient_identity_rgba8.png").exists());
    assert!(!smoke_root.join("solid_alpha_identity_rgba8.png").exists());
    assert_eq!(std::fs::read(&existing_checker).unwrap(), b"already here");
}

#[test]
fn fixture_identity_smoke_schema_pins_no_load_boundary() {
    let schema = repo_schema();
    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["compatibility_classification"],
        "Synthetic fixture broker identity transport only"
    );
    assert_eq!(
        schema["required_values"]["generated_by"],
        "aex_probe_fixture_identity_smoke"
    );
    assert_eq!(
        schema["required_values"]["status"],
        "fixture_identity_smoke_ready_no_load"
    );
    assert_eq!(
        schema["required_values"]["transport_operation"],
        "identity_transport"
    );
    assert_eq!(schema["required_values"]["native_load_performed"], false);
    assert_eq!(schema["required_values"]["render_performed"], false);
    assert_eq!(schema["required_values"]["aex_loaded"], false);
    assert_eq!(schema["required_values"]["worker_started"], false);
    assert_eq!(schema["required_values"]["broker_invoked"], true);
    assert_eq!(schema["required_values"]["ofx_route_invoked"], false);
    assert_eq!(schema["required_values"]["ae_invoked"], false);
    assert_eq!(
        schema["required_values"]["aex_render_correctness_evidence"],
        false
    );
    for check in [
        "fixture_manifest_validated",
        "identity_transport_ok",
        "rgba_identity_pixels_match",
        "synthetic_fixture_pixels_match",
        "no_aex_input",
        "no_worker_or_host_invocation",
        "not_render_correctness_evidence",
    ] {
        assert!(string_array_contains(
            &schema["required_check_names"],
            check
        ));
    }
}
