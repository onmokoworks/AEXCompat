#[allow(dead_code)]
#[path = "../examples/aex_metadata_gate_integration.rs"]
mod aex_metadata_gate_integration;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const REPORT_SCHEMA: &str =
    include_str!("../../analysis/AEX_METADATA_GATE_INTEGRATION_REPORT_SCHEMA_2026-06-01.json");

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-metadata-gate-integration")
        .join(format!("{}-{name}", std::process::id()))
}

fn run(
    fixture_gate: &Value,
    readiness: &Value,
    loader_gate: &Value,
    manifest: &Value,
    smoke: &Value,
    ofx: &Value,
) -> Value {
    let report = aex_metadata_gate_integration::integrate_aex_metadata_gate_json(
        &serde_json::to_string(fixture_gate).unwrap(),
        &serde_json::to_string(readiness).unwrap(),
        &serde_json::to_string(loader_gate).unwrap(),
        &serde_json::to_string(manifest).unwrap(),
        &serde_json::to_string(smoke).unwrap(),
        &serde_json::to_string(ofx).unwrap(),
    )
    .expect("metadata gate integration should return JSON");
    serde_json::from_str(&report).expect("integration report should parse")
}

fn string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("expected array")
        .iter()
        .map(|item| item.as_str().expect("expected string"))
        .collect()
}

fn assert_object_has_fields(value: &Value, fields: &[&str], label: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{label} should be object"));
    for field in fields {
        assert!(
            object.contains_key(*field),
            "{label} should contain field {field}"
        );
    }
}

fn assert_fields_equal(value: &Value, expected: &Value, label: &str) {
    for (field, expected_value) in expected.as_object().unwrap() {
        assert_eq!(
            &value[field], expected_value,
            "{label} field {field} diverged"
        );
    }
}

fn assert_report_matches_schema(report: &Value, schema: &Value) {
    assert_object_has_fields(report, &string_array(&schema["required_fields"]), "report");
    assert_fields_equal(report, &schema["required_values"], "report");
    assert!(
        string_array(&schema["allowed_statuses"])
            .contains(&report["status"].as_str().expect("status should be string")),
        "unexpected report status"
    );
    for check in string_array(&schema["required_check_names"]) {
        assert!(
            report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["name"] == check),
            "missing check {check}"
        );
    }
    for note in string_array(&schema["required_notes"]) {
        assert!(
            report["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == note),
            "missing note {note}"
        );
    }
}

fn fixture_gate_json() -> Value {
    json!({
        "schema_version": 1,
        "status": "review_queue_not_approved",
        "selected_fixture": null,
        "approval": {
            "approved": false,
            "loader_enabled": false,
            "real_aex_load_enabled": false,
            "render_png_enabled": false,
            "describe_enabled_for_real_aex": false
        },
        "candidates": [
            {"id": "adaptive-filter-local", "review_status": "not-approved"},
            {"id": "median-pro-local", "review_status": "not-approved"}
        ]
    })
}

fn readiness_json() -> Value {
    json!({
        "schema_version": 1,
        "status": "probe_readiness_planned",
        "candidate_count": 2,
        "allowlist_entry_count": 1,
        "entries": [
            {
                "effect_id": "adaptivefilter-local",
                "status": "draft_allowlisted",
                "allowed_operations": ["describe"]
            },
            {
                "effect_id": "deferred-local",
                "status": "blocked_or_deferred",
                "allowed_operations": []
            }
        ]
    })
}

fn loader_gate_json() -> Value {
    json!({
        "schema_version": 1,
        "status": "loader_gate_not_opened",
        "approved": false,
        "loader_enabled": false,
        "real_aex_load_enabled": false,
        "open_candidate_count": 0,
        "entries": [
            {
                "effect_id": "adaptivefilter-local",
                "loader_approval_status": "not-approved",
                "allowlist_operation_status": "describe-only",
                "ofx_facade_status": "deferred-same-broker-worker-contract"
            },
            {
                "effect_id": "medianpro-local",
                "loader_approval_status": "not-approved",
                "allowlist_operation_status": "describe-only",
                "ofx_facade_status": "deferred-same-broker-worker-contract"
            }
        ]
    })
}

fn manifest_json() -> Value {
    json!({
        "schema_version": 1,
        "status": "synthetic_fixture_images_ready_no_load",
        "pixel_format": "rgba8",
        "image_count": 3,
        "native_load_performed": false,
        "render_performed": false,
        "aex_loaded": false,
        "worker_started": false,
        "broker_invoked": false,
        "ofx_route_invoked": false,
        "ae_invoked": false,
        "private_payload_copied": false
    })
}

fn identity_smoke_json() -> Value {
    json!({
        "schema_version": 1,
        "status": "fixture_identity_smoke_ready_no_load",
        "fixture_manifest_status": "synthetic_fixture_images_ready_no_load",
        "transport_operation": "identity_transport",
        "image_count": 3,
        "transport_count": 3,
        "identity_pixels_checked_count": 3,
        "native_load_performed": false,
        "render_performed": false,
        "aex_loaded": false,
        "worker_started": false,
        "broker_invoked": true,
        "ofx_route_invoked": false,
        "ae_invoked": false,
        "private_payload_copied": false,
        "aex_render_correctness_evidence": false
    })
}

fn ofx_contract_json() -> Value {
    json!({
        "schema_version": 1,
        "status": "deferred-contract-only",
        "route": {
            "ofx_host_may_load_aex": false,
            "ofx_adapter_may_load_aex": false,
            "broker_may_load_aex": false
        },
        "ofx_facade_review_gate": {
            "approved": false,
            "may_point_to_broker": false,
            "may_issue_describe": false,
            "may_issue_render_png": false
        }
    })
}

fn ready_report() -> Value {
    run(
        &fixture_gate_json(),
        &readiness_json(),
        &loader_gate_json(),
        &manifest_json(),
        &identity_smoke_json(),
        &ofx_contract_json(),
    )
}

#[test]
fn aex_metadata_gate_integration_reports_ready_without_echoing_private_paths() {
    let report = ready_report();
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).unwrap();

    assert_report_matches_schema(&report, &schema);
    assert_fields_equal(&report, &schema["ready_required_values"], "ready");
    assert_fields_equal(
        &report["fixture_gate_summary"],
        &schema["fixture_gate_summary_required_values"],
        "fixture_gate_summary",
    );
    assert_fields_equal(
        &report["readiness_summary"],
        &schema["readiness_summary_required_values"],
        "readiness_summary",
    );
    assert_fields_equal(
        &report["loader_gate_summary"],
        &schema["loader_gate_summary_required_values"],
        "loader_gate_summary",
    );
    assert_fields_equal(
        &report["synthetic_fixture_summary"],
        &schema["synthetic_fixture_summary_required_values"],
        "synthetic_fixture_summary",
    );
    assert_fields_equal(
        &report["identity_smoke_summary"],
        &schema["identity_smoke_summary_required_values"],
        "identity_smoke_summary",
    );
    assert_fields_equal(
        &report["ofx_contract_summary"],
        &schema["ofx_contract_summary_required_values"],
        "ofx_contract_summary",
    );
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));

    let serialized = serde_json::to_string(&report).unwrap();
    for forbidden in string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(forbidden),
            "integration report should not echo forbidden token {forbidden}"
        );
    }
}

#[test]
fn aex_metadata_gate_integration_blocks_fixture_selection_or_loader_approval() {
    let mut fixture_gate = fixture_gate_json();
    fixture_gate["selected_fixture"] = json!("adaptive-filter-local");
    fixture_gate["approval"]["approved"] = json!(true);
    fixture_gate["approval"]["loader_enabled"] = json!(true);

    let report = run(
        &fixture_gate,
        &readiness_json(),
        &loader_gate_json(),
        &manifest_json(),
        &identity_smoke_json(),
        &ofx_contract_json(),
    );

    assert_eq!(report["status"], "blocked_aex_metadata_gate");
    assert_eq!(
        report["fixture_gate_summary"]["selected_fixture_present"],
        true
    );
    assert_eq!(report["fixture_gate_summary"]["approval_approved"], true);
    assert_eq!(report["native_load_performed"], false);
    assert!(report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("fixture gate")));
}

#[test]
fn aex_metadata_gate_integration_blocks_loader_gate_or_render_operation_drift() {
    let mut loader_gate = loader_gate_json();
    loader_gate["approved"] = json!(true);
    loader_gate["loader_enabled"] = json!(true);
    loader_gate["real_aex_load_enabled"] = json!(true);
    loader_gate["open_candidate_count"] = json!(1);
    loader_gate["entries"][0]["loader_approval_status"] = json!("approved-local-only");
    loader_gate["entries"][0]["allowlist_operation_status"] = json!("render_png");

    let report = run(
        &fixture_gate_json(),
        &readiness_json(),
        &loader_gate,
        &manifest_json(),
        &identity_smoke_json(),
        &ofx_contract_json(),
    );

    assert_eq!(report["status"], "blocked_aex_metadata_gate");
    assert_eq!(report["loader_gate_summary"]["loader_enabled"], true);
    assert_eq!(report["loader_gate_summary"]["real_aex_load_enabled"], true);
    assert_eq!(report["render_performed"], false);
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "loader_gate_closed_no_load" && check["status"] == "failed"));
}

#[test]
fn aex_metadata_gate_integration_blocks_identity_smoke_native_or_ofx_claims() {
    let mut smoke = identity_smoke_json();
    smoke["native_load_performed"] = json!(true);
    smoke["aex_loaded"] = json!(true);
    smoke["ofx_route_invoked"] = json!(true);
    smoke["aex_render_correctness_evidence"] = json!(true);

    let report = run(
        &fixture_gate_json(),
        &readiness_json(),
        &loader_gate_json(),
        &manifest_json(),
        &smoke,
        &ofx_contract_json(),
    );

    assert_eq!(report["status"], "blocked_aex_metadata_gate");
    assert_eq!(report["identity_smoke_summary"]["aex_loaded"], true);
    assert_eq!(
        report["identity_smoke_summary"]["aex_render_correctness_evidence"],
        true
    );
    assert_eq!(report["ofx_route_allowed"], false);
}

#[test]
fn aex_metadata_gate_integration_blocks_ofx_facade_opening() {
    let mut ofx = ofx_contract_json();
    ofx["status"] = json!("reviewed-open");
    ofx["route"]["ofx_adapter_may_load_aex"] = json!(true);
    ofx["ofx_facade_review_gate"]["approved"] = json!(true);
    ofx["ofx_facade_review_gate"]["may_issue_render_png"] = json!(true);

    let report = run(
        &fixture_gate_json(),
        &readiness_json(),
        &loader_gate_json(),
        &manifest_json(),
        &identity_smoke_json(),
        &ofx,
    );

    assert_eq!(report["status"], "blocked_aex_metadata_gate");
    assert_eq!(report["ofx_contract_summary"]["review_approved"], true);
    assert_eq!(
        report["ofx_contract_summary"]["ofx_adapter_may_load_aex"],
        true
    );
    assert_eq!(report["ofx_route_allowed"], false);
}

#[test]
fn aex_metadata_gate_integration_validates_report_output_policy() {
    let valid_report = target_path("metadata-gate.local.json");
    assert!(
        aex_metadata_gate_integration::validate_metadata_gate_report_output_path(&valid_report)
            .is_ok()
    );

    let non_json_report = target_path("metadata-gate.local.txt");
    assert!(
        aex_metadata_gate_integration::validate_metadata_gate_report_output_path(&non_json_report)
            .is_err()
    );

    let traversal_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-metadata-gate-integration")
        .join("..")
        .join("private-report.json");
    assert!(
        aex_metadata_gate_integration::validate_metadata_gate_report_output_path(&traversal_report)
            .is_err()
    );

    let outside_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("outside-aex-metadata-gate-integration.json");
    assert!(
        aex_metadata_gate_integration::validate_metadata_gate_report_output_path(&outside_report)
            .is_err()
    );
    assert!(!traversal_report.exists());
    assert!(!outside_report.exists());
}
