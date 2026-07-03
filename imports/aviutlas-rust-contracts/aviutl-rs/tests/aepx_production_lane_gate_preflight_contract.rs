#[allow(dead_code)]
#[path = "../examples/aepx_production_lane_gate_preflight.rs"]
mod aepx_production_lane_gate_preflight;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const GATE_SCHEMA: &str =
    include_str!("../../analysis/AEPX_PRODUCTION_LANE_GATE_SCHEMA_2026-06-01.json");
const REPORT_SCHEMA: &str = include_str!(
    "../../analysis/AEPX_PRODUCTION_LANE_GATE_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json"
);

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-production-lane-gate-preflight")
        .join(format!("{}-{name}", std::process::id()))
}

fn run(value: &Value) -> Value {
    let report = aepx_production_lane_gate_preflight::run_production_lane_gate_preflight_json(
        &serde_json::to_string(value).unwrap(),
    )
    .expect("preflight should return JSON");
    serde_json::from_str(&report).expect("preflight report should parse")
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

#[test]
fn production_lane_gate_preflight_reports_apply_gate_closed_ready() {
    let gate_schema: Value = serde_json::from_str(GATE_SCHEMA).unwrap();
    let report_schema: Value = serde_json::from_str(REPORT_SCHEMA).unwrap();
    let report = run(&gate_schema);

    assert_report_matches_schema(&report, &report_schema);
    assert_fields_equal(&report, &report_schema["ready_required_values"], "ready");
    assert_fields_equal(
        &report["allowed_first_candidate"],
        &report_schema["allowed_first_candidate_required_values"],
        "allowed_first_candidate",
    );
    assert_fields_equal(
        &report["evidence_gate"],
        &report_schema["evidence_gate_required_values"],
        "evidence_gate",
    );
    assert_fields_equal(
        &report["io_gate"],
        &report_schema["io_gate_required_values"],
        "io_gate",
    );
    assert_fields_equal(
        &report["write_gate"],
        &report_schema["write_gate_required_values"],
        "write_gate",
    );
    assert_fields_equal(
        &report["privacy_gate"],
        &report_schema["privacy_gate_required_values"],
        "privacy_gate",
    );
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));
    assert!(report["evidence_gate"]["missing_required_green_evidence"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(report["evidence_gate"]["missing_fail_closed_statuses"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(report["evidence_gate"]["missing_hard_forbidden_items"]
        .as_array()
        .unwrap()
        .is_empty());

    let serialized = serde_json::to_string(&report).unwrap();
    for forbidden in [
        "D:/private",
        "C:\\Users\\",
        "<xmp:",
        "comp-main",
        "Main Reviewed",
        "SPIKE_COMMENT_SENTINEL",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "preflight report should not echo private/synthetic payload token {forbidden}"
        );
    }
}

#[test]
fn production_lane_gate_preflight_blocks_if_apply_or_real_input_flags_drift_open() {
    let mut gate_schema: Value = serde_json::from_str(GATE_SCHEMA).unwrap();
    gate_schema["current_state"]["aepx_patch_probe_apply_enabled"] = json!(true);
    gate_schema["current_state"]["real_aepx_input_enabled"] = json!(true);

    let report = run(&gate_schema);

    assert_eq!(report["status"], "blocked_gate_drift");
    assert_eq!(report["production_apply_enabled"], true);
    assert_eq!(report["real_aepx_input_enabled"], true);
    assert_eq!(report["binary_aep_writer_enabled"], false);
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert_eq!(report["io_gate"]["binary_aep_read_performed"], false);
    assert_eq!(report["io_gate"]["binary_aep_write_performed"], false);
    assert_eq!(report["io_gate"]["xml_body_read_performed"], false);
    assert!(report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason
            .as_str()
            .unwrap()
            .contains("aepx_patch_probe_apply_enabled must remain false")));
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |check| check["name"] == "current_state.aepx_patch_probe_apply_enabled"
                && check["status"] == "failed"
        ));
}

#[test]
fn production_lane_gate_preflight_blocks_if_binary_aep_writer_flag_drifts_open() {
    let mut gate_schema: Value = serde_json::from_str(GATE_SCHEMA).unwrap();
    gate_schema["current_state"]["binary_aep_writer_enabled"] = json!(true);

    let report = run(&gate_schema);

    assert_eq!(report["status"], "blocked_gate_drift");
    assert_eq!(report["binary_aep_writer_enabled"], true);
    assert_eq!(report["io_gate"]["binary_aep_read_performed"], false);
    assert_eq!(report["io_gate"]["binary_aep_write_performed"], false);
    assert_eq!(report["write_gate"]["binary_aep_writer_enabled"], true);
    assert!(report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason
            .as_str()
            .unwrap()
            .contains("binary_aep_writer_enabled must remain false")));
    assert!(report["checks"].as_array().unwrap().iter().any(|check| {
        check["name"] == "current_state.binary_aep_writer_enabled" && check["status"] == "failed"
    }));
}

#[test]
fn production_lane_gate_preflight_blocks_if_required_evidence_or_transition_rule_drifts() {
    let mut gate_schema: Value = serde_json::from_str(GATE_SCHEMA).unwrap();
    let evidence = gate_schema["required_green_evidence_before_apply"]
        .as_array_mut()
        .unwrap();
    evidence.retain(|item| item != "parent_approval_receipt_for_apply_slice");
    gate_schema["transition_rule"]["dry_run_evidence_accepted_as_apply_permission"] = json!(true);

    let report = run(&gate_schema);

    assert_eq!(report["status"], "blocked_gate_drift");
    assert!(report["evidence_gate"]["missing_required_green_evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "parent_approval_receipt_for_apply_slice"));
    assert_eq!(
        report["evidence_gate"]["dry_run_evidence_accepted_as_apply_permission"],
        true
    );
    assert!(report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("required evidence")));
    assert!(report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("transition rule")));
}

#[test]
fn production_lane_gate_preflight_blocks_if_aex_or_ofx_forbidden_items_are_missing() {
    let mut gate_schema: Value = serde_json::from_str(GATE_SCHEMA).unwrap();
    let forbidden = gate_schema["hard_forbidden_in_first_apply_slice"]
        .as_array_mut()
        .unwrap();
    forbidden.retain(|item| item != ".aex loading" && item != "OFX routing");

    let report = run(&gate_schema);

    assert_eq!(report["status"], "blocked_gate_drift");
    let missing = report["evidence_gate"]["missing_hard_forbidden_items"]
        .as_array()
        .unwrap();
    assert!(missing.iter().any(|item| item == ".aex loading"));
    assert!(missing.iter().any(|item| item == "OFX routing"));
    assert_eq!(report["write_gate"]["production_apply_enabled"], false);
}

#[test]
fn production_lane_gate_preflight_validates_report_output_policy() {
    let valid_report = target_path("preflight.local.json");
    assert!(
        aepx_production_lane_gate_preflight::validate_preflight_report_output_path(&valid_report)
            .is_ok()
    );

    let non_json_report = target_path("preflight.local.txt");
    assert!(
        aepx_production_lane_gate_preflight::validate_preflight_report_output_path(
            &non_json_report
        )
        .is_err()
    );

    let traversal_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-production-lane-gate-preflight")
        .join("..")
        .join("private-report.json");
    assert!(
        aepx_production_lane_gate_preflight::validate_preflight_report_output_path(
            &traversal_report
        )
        .is_err()
    );

    let outside_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("outside-aepx-production-lane-gate-preflight.json");
    assert!(
        aepx_production_lane_gate_preflight::validate_preflight_report_output_path(&outside_report)
            .is_err()
    );
    assert!(!traversal_report.exists());
    assert!(!outside_report.exists());
}
