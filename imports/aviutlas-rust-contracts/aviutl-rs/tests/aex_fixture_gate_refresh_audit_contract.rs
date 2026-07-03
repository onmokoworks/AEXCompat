#[allow(dead_code)]
#[path = "../examples/aex_fixture_gate_refresh_audit.rs"]
mod aex_fixture_gate_refresh_audit;

use serde_json::{json, Value};

const FIXTURE_GATE: &str = include_str!("../../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json");
const WIZTREE_REFRESH: &str =
    include_str!("../../analysis/AEX_WIZTREE_AEX_REFRESH_2026-06-01.json");
const AUDIT_SCHEMA: &str =
    include_str!("../../analysis/AEX_FIXTURE_GATE_REFRESH_AUDIT_SCHEMA_2026-06-01.json");

fn audit(fixture_gate: &str, wiztree_refresh: &str) -> Value {
    let output = aex_fixture_gate_refresh_audit::audit_fixture_gate_refresh_json(
        fixture_gate,
        wiztree_refresh,
    )
    .expect("fixture gate refresh audit should run");
    serde_json::from_str(&output).expect("audit should emit JSON")
}

fn schema() -> Value {
    serde_json::from_str(AUDIT_SCHEMA).expect("fixture gate refresh audit schema should parse")
}

fn json_string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("expected JSON array")
        .iter()
        .map(|item| item.as_str().expect("expected string array item"))
        .collect()
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

fn assert_audit_matches_schema(report: &Value, schema: &Value) {
    assert_object_has_fields(
        report,
        &json_string_array(&schema["required_fields"]),
        "audit",
    );
    assert_object_has_fields(
        &report["fixture_gate_summary"],
        &json_string_array(&schema["fixture_gate_summary_required_fields"]),
        "fixture_gate_summary",
    );
    assert_object_has_fields(
        &report["wiztree_refresh_summary"],
        &json_string_array(&schema["wiztree_refresh_summary_required_fields"]),
        "wiztree_refresh_summary",
    );
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(
            &report[field], expected,
            "audit field {field} diverged from schema"
        );
    }
    assert!(
        json_string_array(&schema["allowed_statuses"])
            .contains(&report["status"].as_str().expect("status should be string")),
        "unexpected audit status"
    );
    for candidate in report["candidate_crosscheck"]
        .as_array()
        .expect("candidate_crosscheck should be an array")
    {
        assert_object_has_fields(
            candidate,
            &json_string_array(&schema["candidate_crosscheck_required_fields"]),
            "candidate_crosscheck entry",
        );
    }
    for check in report["checks"].as_array().expect("checks should be array") {
        assert_object_has_fields(check, &["name", "status", "evidence"], "check");
        let status = check["status"].as_str().unwrap();
        assert!(
            json_string_array(&schema["check_statuses"]).contains(&status),
            "unexpected check status {status}"
        );
    }
    for name in json_string_array(&schema["required_check_names"]) {
        assert!(
            check_status(report, name, "passed") || check_status(report, name, "blocked"),
            "audit missing required check {name}"
        );
    }
    for note in json_string_array(&schema["required_notes"]) {
        assert!(
            report["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == note),
            "audit missing required note {note}"
        );
    }
    let serialized = serde_json::to_string(report)
        .expect("audit should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "audit report should not contain serialized token {token}"
        );
    }
}

fn assert_ready_summary_values(report: &Value, schema: &Value) {
    for (field, expected) in schema["fixture_gate_summary_required_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["fixture_gate_summary"][field], expected,
            "fixture gate summary field {field} diverged"
        );
    }
    for (field, expected) in schema["wiztree_refresh_summary_required_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["wiztree_refresh_summary"][field], expected,
            "WizTree refresh summary field {field} diverged"
        );
    }
    for candidate in report["candidate_crosscheck"].as_array().unwrap() {
        assert!(
            json_string_array(&schema["candidate_crosscheck_required_ids"])
                .contains(&candidate["id"].as_str().unwrap())
        );
        for (field, expected) in schema["candidate_crosscheck_required_ready_values"]
            .as_object()
            .unwrap()
        {
            assert_eq!(
                &candidate[field], expected,
                "candidate crosscheck field {field} diverged"
            );
        }
        assert!(candidate["blocked_reason_count"].as_u64().unwrap() > 0);
    }
}

fn check_status(report: &Value, name: &str, status: &str) -> bool {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == name && check["status"] == status)
}

fn contains_reason(report: &Value, expected: &str) -> bool {
    report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains(expected))
}

#[test]
fn ready_fixture_gate_refresh_audit_is_metadata_only_and_schema_bound() {
    let report = audit(FIXTURE_GATE, WIZTREE_REFRESH);
    let schema = schema();

    assert_audit_matches_schema(&report, &schema);
    assert_ready_summary_values(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["fixture_selected"], false);
    assert_eq!(report["loader_enabled"], false);
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));
}

#[test]
fn audit_blocks_fixture_gate_selection_or_loader_enablement() {
    let mut gate: Value = serde_json::from_str(FIXTURE_GATE).unwrap();
    gate["selected_fixture"] = json!("adaptive-filter-local");
    gate["approval"]["approved"] = json!(true);
    gate["approval"]["loader_enabled"] = json!(true);
    let report = audit(&gate.to_string(), WIZTREE_REFRESH);

    assert_eq!(report["status"], "blocked_fixture_gate_refresh");
    assert_eq!(report["fixture_selected"], false);
    assert_eq!(report["loader_enabled"], false);
    assert!(check_status(
        &report,
        "fixture_gate_closed_no_selection",
        "blocked"
    ));
    assert!(check_status(
        &report,
        "evidence_anti_contamination",
        "blocked"
    ));
    assert!(contains_reason(
        &report,
        "fixture review gate is not a closed unselected review queue"
    ));
}

#[test]
fn audit_blocks_candidate_missing_from_refresh() {
    let mut refresh: Value = serde_json::from_str(WIZTREE_REFRESH).unwrap();
    refresh["fixture_gate_candidates_verified_present"] =
        json!([refresh["fixture_gate_candidates_verified_present"][0].clone()]);
    let report = audit(FIXTURE_GATE, &refresh.to_string());

    assert_eq!(report["status"], "blocked_fixture_gate_refresh");
    assert!(check_status(
        &report,
        "fixture_gate_candidates_present_in_refresh",
        "blocked"
    ));
    assert!(contains_reason(
        &report,
        "fixture review gate candidates are missing or size-mismatched in the WizTree refresh"
    ));
    assert!(report["candidate_crosscheck"]
        .as_array()
        .unwrap()
        .iter()
        .any(|candidate| candidate["id"] == "median-pro-local"
            && candidate["path_matches_refresh"] == false));
}

#[test]
fn audit_blocks_generated_target_candidate_in_gate() {
    let mut gate: Value = serde_json::from_str(FIXTURE_GATE).unwrap();
    gate["candidates"][0]["path"] =
        json!("D:\\Projects\\01_Project\\05_other\\AviUtlas\\aviutl-rs\\target\\aex-image-probe\\identity\\ready\\ClassicTest.aex");
    gate["candidates"][0]["observed_size_bytes"] = json!(32);
    let report = audit(&gate.to_string(), WIZTREE_REFRESH);

    assert_eq!(report["status"], "blocked_fixture_gate_refresh");
    assert!(check_status(
        &report,
        "fixture_gate_excludes_generated_target_artifacts",
        "blocked"
    ));
    assert!(report["candidate_crosscheck"]
        .as_array()
        .unwrap()
        .iter()
        .any(|candidate| candidate["id"] == "adaptive-filter-local"
            && candidate["generated_target_artifact"] == true));
}

#[test]
fn audit_blocks_refresh_generated_target_policy_regression() {
    let mut refresh: Value = serde_json::from_str(WIZTREE_REFRESH).unwrap();
    refresh["recommended_fixture_gate_delta"]
        ["do_not_expand_first_loader_queue_from_generated_target_artifacts"] = json!(false);
    refresh["generated_target_test_artifacts"]["policy"] =
        json!("generated artifacts may be reviewed");
    let report = audit(FIXTURE_GATE, &refresh.to_string());

    assert_eq!(report["status"], "blocked_fixture_gate_refresh");
    assert!(check_status(
        &report,
        "wiztree_refresh_generated_targets_excluded",
        "blocked"
    ));
}

#[test]
fn audit_blocks_contaminated_refresh_without_echo() {
    let mut refresh: Value = serde_json::from_str(WIZTREE_REFRESH).unwrap();
    refresh["payload_bytes"] = json!("private");
    let report = audit(FIXTURE_GATE, &refresh.to_string());

    assert_eq!(report["status"], "blocked_fixture_gate_refresh");
    assert!(check_status(
        &report,
        "evidence_anti_contamination",
        "blocked"
    ));
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("payload_bytes"));
}
