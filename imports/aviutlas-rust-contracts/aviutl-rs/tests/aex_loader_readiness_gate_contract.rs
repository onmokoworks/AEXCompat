#[allow(dead_code)]
#[path = "../examples/aex_loader_readiness_gate.rs"]
mod aex_loader_readiness_gate;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const GATE_SCHEMA: &str =
    include_str!("../../analysis/AEX_LOADER_READINESS_GATE_SCHEMA_2026-06-03.json");

fn gate(preflight: &str, provenance: &str) -> Value {
    let output =
        aex_loader_readiness_gate::evaluate_loader_readiness_gate_json(preflight, provenance)
            .expect("loader readiness gate should run");
    serde_json::from_str(&output).expect("gate should emit JSON")
}

fn json_string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("expected JSON array")
        .iter()
        .map(|item| item.as_str().expect("expected string array item"))
        .collect()
}

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-loader-readiness-gate")
        .join(format!("{}-{name}", std::process::id()))
}

fn unique_target_path(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    target_path(&format!("{stamp}-{name}"))
}

fn assert_object_has_fields(value: &Value, fields: &[&str], label: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{label} should be an object"));
    for field in fields {
        assert!(
            object.contains_key(*field),
            "{label} missing required field {field}: {value:?}"
        );
    }
}

fn assert_gate_matches_schema(report: &Value, schema: &Value) {
    assert_object_has_fields(
        report,
        &json_string_array(&schema["required_fields"]),
        "readiness gate",
    );
    assert_object_has_fields(
        &report["preflight_summary"],
        &json_string_array(&schema["preflight_summary_required_fields"]),
        "preflight_summary",
    );
    assert_object_has_fields(
        &report["provenance_summary"],
        &json_string_array(&schema["provenance_summary_required_fields"]),
        "provenance_summary",
    );
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(&report[field], expected, "field {field} diverged");
    }
    let status = report["status"].as_str().unwrap();
    assert!(
        json_string_array(&schema["allowed_statuses"]).contains(&status),
        "unexpected gate status {status}"
    );
    for name in json_string_array(&schema["required_check_names"]) {
        assert!(
            report["checks"].as_array().unwrap().iter().any(|check| {
                check["name"] == name
                    && json_string_array(&schema["check_statuses"])
                        .contains(&check["status"].as_str().unwrap())
            }),
            "missing required check {name}"
        );
    }
    for note in json_string_array(&schema["required_notes"]) {
        assert!(
            report["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == note),
            "missing required note {note}"
        );
    }
}

fn ready_preflight_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "loader_preflight_ready_no_load",
        "preflight_passed": true,
        "native_load_performed": false,
        "broker_may_load_plugin": false,
        "selected_fixture": "adaptivefilter-local",
        "selected_loader_entry": {
            "effect_id": "adaptivefilter-local",
            "entry_ready": true
        },
        "blocked_reasons": []
    }))
    .unwrap()
}

fn ready_provenance_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "no_load_provenance_chain_ready",
        "native_load_performed": false,
        "selectors_executed": false,
        "render_performed": false,
        "ofx_route_allowed": false,
        "evidence_contains_forbidden_tokens": false,
        "fixture_identity_smoke_summary": {
            "provided": true,
            "status": "fixture_identity_smoke_ready_no_load",
            "native_load_performed": false,
            "render_performed": false,
            "aex_loaded": false
        },
        "blocked_reasons": []
    }))
    .unwrap()
}

#[test]
fn readiness_gate_reports_complete_evidence_while_native_gate_stays_closed() {
    let schema: Value = serde_json::from_str(GATE_SCHEMA).expect("schema should parse");
    let report = gate(&ready_preflight_json(), &ready_provenance_json());

    assert_eq!(
        schema["report_output_policy"]["required_extension"],
        ".json"
    );
    assert_eq!(
        schema["report_output_policy"]["required_root"],
        "target/aex-loader-readiness-gate"
    );
    assert_eq!(
        schema["report_output_policy"]["reject_traversal_components"],
        true
    );
    assert_eq!(schema["report_output_policy"]["create_new_only"], true);
    assert_eq!(
        schema["report_output_policy"]["canonical_parent_must_resolve_under_required_root"],
        true
    );

    assert_gate_matches_schema(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert_eq!(report["evidence_complete"], true);
    assert_eq!(report["final_gate_closed"], true);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["selectors_executed"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["may_load_aex"], false);
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());

    for (field, expected) in schema["preflight_summary_required_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(&report["preflight_summary"][field], expected);
    }
    for (field, expected) in schema["provenance_summary_required_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(&report["provenance_summary"][field], expected);
    }
}

#[test]
fn readiness_gate_blocks_when_no_load_evidence_is_missing_or_promoted() {
    let mut preflight: Value = serde_json::from_str(&ready_preflight_json()).unwrap();
    preflight["broker_may_load_plugin"] = json!(true);
    preflight["selected_loader_entry"] = Value::Null;
    let mut provenance: Value = serde_json::from_str(&ready_provenance_json()).unwrap();
    provenance["selectors_executed"] = json!(true);
    provenance["fixture_identity_smoke_summary"]["provided"] = json!(false);

    let report = gate(
        &serde_json::to_string_pretty(&preflight).unwrap(),
        &serde_json::to_string_pretty(&provenance).unwrap(),
    );

    assert_eq!(report["status"], "blocked_loader_readiness_gate");
    assert_eq!(report["evidence_complete"], false);
    assert_eq!(report["final_gate_closed"], true);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["may_load_aex"], false);
    assert!(
        report["blocked_reasons"].as_array().unwrap().len() >= 3,
        "promoted or missing evidence should produce explicit blockers"
    );
}

#[test]
fn readiness_gate_validates_report_output_policy() {
    let valid_report = target_path("loader-readiness-gate.local.json");
    assert!(
        aex_loader_readiness_gate::validate_loader_readiness_gate_report_output_path(&valid_report)
            .is_ok()
    );

    let non_json_report = target_path("loader-readiness-gate.local.txt");
    assert!(
        aex_loader_readiness_gate::validate_loader_readiness_gate_report_output_path(
            &non_json_report
        )
        .is_err()
    );

    let traversal_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-loader-readiness-gate")
        .join("..")
        .join("private-loader-readiness.json");
    assert!(
        aex_loader_readiness_gate::validate_loader_readiness_gate_report_output_path(
            &traversal_report
        )
        .is_err()
    );

    let outside_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("outside-aex-loader-readiness-gate.json");
    assert!(
        aex_loader_readiness_gate::validate_loader_readiness_gate_report_output_path(
            &outside_report
        )
        .is_err()
    );
    assert!(!traversal_report.exists());
    assert!(!outside_report.exists());
}

#[test]
fn readiness_gate_report_writer_uses_create_new() {
    let report = unique_target_path("loader-readiness-create-new.local.json");
    aex_loader_readiness_gate::write_loader_readiness_gate_report_create_new(
        &report,
        "{\"first\":true}",
    )
    .expect("first loader readiness report write should succeed");

    let err = aex_loader_readiness_gate::write_loader_readiness_gate_report_create_new(
        &report,
        "{\"second\":true}",
    )
    .expect_err("second loader readiness report write should use create_new and fail");
    assert!(
        err.to_string().contains("already exists"),
        "unexpected create-new error: {err}"
    );

    let contents = std::fs::read_to_string(&report)
        .expect("created loader readiness report should be readable");
    assert!(contents.contains("\"first\":true"));
    assert!(!contents.contains("\"second\":true"));
}
