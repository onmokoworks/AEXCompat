#[allow(dead_code)]
#[path = "../examples/aex_loader_preflight.rs"]
mod aex_loader_preflight;

#[allow(dead_code)]
#[path = "../examples/aex_fixture_gate_refresh_audit.rs"]
mod aex_fixture_gate_refresh_audit;

use serde_json::json;
use serde_json::Value;

const PREFLIGHT_REPORT_SCHEMA: &str =
    include_str!("../../analysis/AEX_LOADER_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json");
const FIXTURE_GATE: &str = include_str!("../../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json");
const WIZTREE_REFRESH: &str =
    include_str!("../../analysis/AEX_WIZTREE_AEX_REFRESH_2026-06-01.json");

fn preflight(fixture_gate: &str, loader_gate: &str) -> Value {
    let output = aex_loader_preflight::plan_loader_preflight_json(fixture_gate, loader_gate)
        .expect("loader preflight should run");
    serde_json::from_str(&output).expect("preflight should emit JSON")
}

fn preflight_with_fixture_refresh_audit(
    fixture_gate: &str,
    loader_gate: &str,
    fixture_refresh_audit: Option<&str>,
) -> Value {
    let output = aex_loader_preflight::plan_loader_preflight_json_with_fixture_refresh_audit(
        fixture_gate,
        loader_gate,
        fixture_refresh_audit,
    )
    .expect("loader preflight with refresh audit should run");
    serde_json::from_str(&output).expect("preflight should emit JSON")
}

fn ready_fixture_refresh_audit_json() -> String {
    aex_fixture_gate_refresh_audit::audit_fixture_gate_refresh_json(FIXTURE_GATE, WIZTREE_REFRESH)
        .expect("fixture gate refresh audit should run")
}

fn preflight_report_schema() -> Value {
    serde_json::from_str(PREFLIGHT_REPORT_SCHEMA).expect("preflight report schema should parse")
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

fn assert_preflight_report_matches_schema(report: &Value, schema: &Value) {
    assert_object_has_fields(
        report,
        &json_string_array(&schema["required_fields"]),
        "report",
    );

    for (field, expected) in schema["required_values"]
        .as_object()
        .expect("required_values should be an object")
    {
        assert_eq!(
            &report[field], expected,
            "preflight report field {field} diverged from schema"
        );
    }
    assert_eq!(
        report["native_load_performed"],
        schema["no_load_invariants"]["native_load_performed"]
    );
    assert_eq!(
        report["broker_may_load_plugin"],
        schema["no_load_invariants"]["broker_may_load_plugin"]
    );

    let status = report["status"].as_str().expect("status should be string");
    assert!(
        json_string_array(&schema["allowed_statuses"]).contains(&status),
        "unexpected status {status}"
    );
    assert_eq!(
        report["preflight_passed"].as_bool().unwrap(),
        status == schema["passing_status"].as_str().unwrap()
    );

    assert_object_has_fields(
        &report["fixture_gate"],
        &json_string_array(&schema["fixture_gate_required_fields"]),
        "fixture_gate",
    );
    assert_object_has_fields(
        &report["loader_gate"],
        &json_string_array(&schema["loader_gate_required_fields"]),
        "loader_gate",
    );
    assert_object_has_fields(
        &report["fixture_refresh_audit_summary"],
        &json_string_array(&schema["fixture_refresh_audit_summary_required_fields"]),
        "fixture_refresh_audit_summary",
    );
    if report["fixture_refresh_audit_summary"]["provided"]
        .as_bool()
        .expect("fixture refresh audit provided should be bool")
    {
        for (field, expected) in schema
            ["fixture_refresh_audit_summary_required_values_when_provided"]
            .as_object()
            .expect("fixture refresh provided values should be object")
        {
            assert_eq!(
                &report["fixture_refresh_audit_summary"][field], expected,
                "fixture refresh audit summary field {field} diverged"
            );
        }
    }
    if !report["selected_candidate"].is_null() {
        assert_object_has_fields(
            &report["selected_candidate"],
            &json_string_array(&schema["selected_candidate_required_fields"]),
            "selected_candidate",
        );
    }
    if !report["selected_loader_entry"].is_null() {
        assert_object_has_fields(
            &report["selected_loader_entry"],
            &json_string_array(&schema["selected_loader_entry_required_fields"]),
            "selected_loader_entry",
        );
    }

    let checks = report["checks"]
        .as_array()
        .expect("checks should be an array");
    let allowed_check_statuses = json_string_array(&schema["check_statuses"]);
    for check in checks {
        assert_object_has_fields(check, &["name", "status", "evidence"], "check");
        let check_status = check["status"]
            .as_str()
            .expect("check status should be string");
        assert!(
            allowed_check_statuses.contains(&check_status),
            "unexpected check status {check_status}"
        );
    }
    for name in json_string_array(&schema["required_check_names"]) {
        assert!(
            check_status(report, name, "passed") || check_status(report, name, "blocked"),
            "report missing required preflight check {name}"
        );
    }
    for name in json_string_array(&schema["conditional_check_names"]) {
        if check_status(report, name, "passed") || check_status(report, name, "blocked") {
            continue;
        }
        if name == "fixture_gate_refresh_audit_ready_no_load" {
            assert_eq!(
                report["fixture_refresh_audit_summary"]["provided"], false,
                "fixture refresh audit check may be absent only when the audit is absent"
            );
            continue;
        }
        assert!(
            report["selected_fixture"].is_null(),
            "conditional check {name} may be absent only before fixture selection"
        );
    }

    let notes = report["notes"]
        .as_array()
        .expect("notes should be an array");
    for expected_note in json_string_array(&schema["required_notes"]) {
        assert!(
            notes.iter().any(|note| note == expected_note),
            "report missing required note {expected_note}"
        );
    }
    let next_action = report["next_action"]
        .as_str()
        .expect("next_action should be a string");
    if report["preflight_passed"].as_bool().unwrap() {
        assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
        assert!(next_action.contains(schema["passing_next_action_contains"].as_str().unwrap()));
    } else {
        assert!(!report["blocked_reasons"].as_array().unwrap().is_empty());
        assert!(next_action.contains(schema["blocked_next_action_contains"].as_str().unwrap()));
    }

    let serialized = serde_json::to_string(report)
        .expect("report should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "preflight report should not contain serialized token {token}"
        );
    }
}

#[test]
fn preflight_reports_match_dedicated_schema() {
    let schema = preflight_report_schema();
    let current_fixture_gate =
        include_str!("../../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json");
    let blocked = preflight(current_fixture_gate, &closed_loader_gate_json());
    assert_preflight_report_matches_schema(&blocked, &schema);

    let passing = preflight(
        &fixture_gate_json(Some("adaptive-filter-local"), true),
        &open_loader_gate_json(),
    );
    assert_preflight_report_matches_schema(&passing, &schema);
}

#[test]
fn current_fixture_review_gate_blocks_without_selected_fixture() {
    let loader_gate = closed_loader_gate_json();
    let report = preflight(FIXTURE_GATE, &loader_gate);

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "blocked_no_selected_fixture");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(report["selected_fixture"], Value::Null);
    assert_eq!(report["selected_loader_entry"], Value::Null);
    assert_eq!(report["fixture_gate"]["candidate_count"], 2);
    assert_eq!(report["loader_gate"]["status"], "loader_gate_not_opened");
    assert_eq!(report["loader_gate"]["approved"], false);
    assert_eq!(report["loader_gate"]["loader_enabled"], false);
    assert_eq!(report["loader_gate"]["real_aex_load_enabled"], false);
    assert_eq!(report["fixture_refresh_audit_summary"]["provided"], false);
    assert!(array_contains(
        &report["blocked_reasons"],
        "fixture_review_gate.selected_fixture is null"
    ));
    assert!(array_contains(
        &report["blocked_reasons"],
        "readiness loader gate is not open for exactly one candidate"
    ));
    assert!(check_status(&report, "selected_fixture", "blocked"));

    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("sha256"));
    assert!(!serialized.contains("base64"));
}

#[test]
fn current_preflight_carries_ready_fixture_refresh_audit_when_provided() {
    let refresh_audit = ready_fixture_refresh_audit_json();
    let report = preflight_with_fixture_refresh_audit(
        FIXTURE_GATE,
        &closed_loader_gate_json(),
        Some(&refresh_audit),
    );
    let schema = preflight_report_schema();

    assert_preflight_report_matches_schema(&report, &schema);
    assert_eq!(report["status"], "blocked_no_selected_fixture");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(report["fixture_refresh_audit_summary"]["provided"], true);
    assert_eq!(
        report["fixture_refresh_audit_summary"]["status"],
        "fixture_gate_refresh_ready_no_load"
    );
    assert_eq!(
        report["fixture_refresh_audit_summary"]["wiztree_canonical_non_generated_count"],
        40
    );
    assert_eq!(
        report["fixture_refresh_audit_summary"]["wiztree_generated_target_artifact_count"],
        79
    );
    assert_eq!(
        report["fixture_refresh_audit_summary"]["candidates_present_in_refresh"],
        true
    );
    assert!(check_status(
        &report,
        "fixture_gate_refresh_audit_ready_no_load",
        "passed"
    ));
    assert!(!array_contains(
        &report["blocked_reasons"],
        "fixture gate refresh audit"
    ));
}

#[test]
fn fixture_refresh_audit_regression_blocks_preflight_evidence() {
    let mut refresh_audit: Value = serde_json::from_str(&ready_fixture_refresh_audit_json())
        .expect("ready refresh audit should parse");
    refresh_audit["status"] = json!("blocked_fixture_gate_refresh");

    let report = preflight_with_fixture_refresh_audit(
        FIXTURE_GATE,
        &closed_loader_gate_json(),
        Some(&refresh_audit.to_string()),
    );

    assert_eq!(report["status"], "blocked_no_selected_fixture");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(report["fixture_refresh_audit_summary"]["provided"], true);
    assert_eq!(
        report["fixture_refresh_audit_summary"]["status"],
        "blocked_fixture_gate_refresh"
    );
    assert!(check_status(
        &report,
        "fixture_gate_refresh_audit_ready_no_load",
        "blocked"
    ));
    assert!(array_contains(
        &report["blocked_reasons"],
        "fixture gate refresh audit is not a ready no-load queue-hygiene receipt"
    ));
}

#[test]
fn fixture_refresh_audit_contamination_blocks_without_echo() {
    let mut refresh_audit: Value = serde_json::from_str(&ready_fixture_refresh_audit_json())
        .expect("ready refresh audit should parse");
    refresh_audit["output_png"] = json!("private-output");

    let report = preflight_with_fixture_refresh_audit(
        FIXTURE_GATE,
        &closed_loader_gate_json(),
        Some(&refresh_audit.to_string()),
    );

    assert_eq!(report["status"], "blocked_no_selected_fixture");
    assert_eq!(
        report["fixture_refresh_audit_summary"]["input_contains_forbidden_tokens"],
        true
    );
    assert!(check_status(
        &report,
        "fixture_gate_refresh_audit_ready_no_load",
        "blocked"
    ));
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("output_png"));
    assert!(!serialized.contains("private-output"));
}

#[test]
fn selected_fixture_not_in_gate_is_blocked_before_loader_use() {
    let fixture_gate = fixture_gate_json(Some("not-in-gate"), true);
    let loader_gate = open_loader_gate_json();
    let report = preflight(&fixture_gate, &loader_gate);

    assert_eq!(report["status"], "blocked_selected_fixture_not_in_gate");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(report["selected_fixture"], "not-in-gate");
    assert_eq!(report["selected_candidate"], Value::Null);
    assert!(array_contains(
        &report["blocked_reasons"],
        "fixture_review_gate.selected_fixture is not present in candidates"
    ));
    assert!(check_status(&report, "selected_fixture", "blocked"));
    assert!(check_status(&report, "fixture_gate_approval", "passed"));
    assert!(check_status(&report, "loader_gate_open", "passed"));
    assert!(check_status(
        &report,
        "selected_fixture_in_loader_gate",
        "blocked"
    ));
}

#[test]
fn selected_fixture_still_requires_fixture_approval() {
    let fixture_gate = fixture_gate_json(Some("adaptive-filter-local"), false);
    let loader_gate = closed_loader_gate_json();
    let report = preflight(&fixture_gate, &loader_gate);

    assert_eq!(report["status"], "blocked_fixture_not_approved");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["selected_fixture"], "adaptive-filter-local");
    assert_eq!(report["selected_candidate"]["id"], "adaptive-filter-local");
    assert_eq!(
        report["selected_candidate"]["plugin_path"],
        "D:\\AviUtlas\\local\\AdaptiveFilter.aex"
    );
    assert_eq!(
        report["selected_candidate"]["normalized_plugin_path"],
        "d:\\aviutlas\\local\\adaptivefilter.aex"
    );
    assert_eq!(
        report["selected_candidate"]["loader_gate_plugin_path"],
        "D:\\AviUtlas\\local\\AdaptiveFilter.aex"
    );
    assert_eq!(
        report["selected_candidate"]["loader_gate_effect_id"],
        "adaptivefilter-local"
    );
    assert_eq!(
        report["selected_candidate"]["path_match_status"],
        "matched_normalized_path"
    );
    assert_eq!(
        report["selected_loader_entry"]["effect_id"],
        "adaptivefilter-local"
    );
    assert_eq!(
        report["selected_loader_entry"]["plugin_path"],
        "D:\\AviUtlas\\local\\AdaptiveFilter.aex"
    );
    assert_eq!(
        report["selected_loader_entry"]["normalized_plugin_path"],
        "d:\\aviutlas\\local\\adaptivefilter.aex"
    );
    assert_eq!(
        report["selected_loader_entry"]["path_match_status"],
        "matched_normalized_path"
    );
    assert_eq!(
        report["selected_loader_entry"]["allowlist_operation_status"],
        "describe-only"
    );
    assert_eq!(report["selected_loader_entry"]["entry_ready"], false);
    assert!(check_status(&report, "selected_fixture", "passed"));
    assert!(check_status(&report, "fixture_gate_approval", "blocked"));
    assert!(check_status(&report, "loader_gate_open", "blocked"));
    assert_eq!(report["native_load_performed"], false);
}

#[test]
fn selected_fixture_requires_describe_approval_before_real_aex_preflight() {
    let mut fixture_gate: Value =
        serde_json::from_str(&fixture_gate_json(Some("adaptive-filter-local"), true)).unwrap();
    fixture_gate["approval"]["describe_enabled_for_real_aex"] = json!(false);
    let loader_gate = open_loader_gate_json();

    let report = preflight(&fixture_gate.to_string(), &loader_gate);

    assert_eq!(report["status"], "blocked_fixture_not_approved");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(
        report["fixture_gate"]["approval_describe_enabled_for_real_aex"],
        false
    );
    assert!(check_status(&report, "fixture_gate_approval", "blocked"));
    assert!(array_contains(
        &report["blocked_reasons"],
        "fixture review gate approval flags are not all enabled"
    ));
}

#[test]
fn passing_preflight_is_still_no_load_and_requires_separate_loader_slice() {
    let fixture_gate = fixture_gate_json(Some("adaptive-filter-local"), true);
    let loader_gate = open_loader_gate_json();
    let report = preflight(&fixture_gate, &loader_gate);

    assert_eq!(report["status"], "preflight_passed_no_load");
    assert_eq!(report["preflight_passed"], true);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(
        report["selected_loader_entry"]["effect_id"],
        "adaptivefilter-local"
    );
    assert_eq!(
        report["selected_loader_entry"]["plugin_path"],
        "D:\\AviUtlas\\local\\AdaptiveFilter.aex"
    );
    assert_eq!(
        report["selected_loader_entry"]["normalized_plugin_path"],
        "d:\\aviutlas\\local\\adaptivefilter.aex"
    );
    assert_eq!(
        report["selected_loader_entry"]["path_match_status"],
        "matched_normalized_path"
    );
    assert_eq!(
        report["selected_loader_entry"]["pre_loader_status"],
        "approved-local-only"
    );
    assert_eq!(
        report["selected_loader_entry"]["loader_approval_status"],
        "approved-local-only"
    );
    assert_eq!(
        report["selected_loader_entry"]["allowlist_operation_status"],
        "render_png"
    );
    assert_eq!(
        report["selected_loader_entry"]["handle_inheritance_required"],
        "sentinel_not_inherited-with-explicit-handle-list"
    );
    assert_eq!(
        report["selected_loader_entry"]["worker_identity_revalidation_required"],
        "passed"
    );
    assert_eq!(
        report["selected_loader_entry"]["worker_attestation_required"],
        "passed"
    );
    assert_eq!(
        report["selected_loader_entry"]["sandbox_preflight_required"],
        "passed"
    );
    assert_eq!(
        report["selected_loader_entry"]["job_object_required"],
        "assigned-with-kill-on-close"
    );
    assert_eq!(report["selected_loader_entry"]["entry_ready"], true);
    assert_eq!(report["loader_gate"]["approved"], true);
    assert_eq!(report["loader_gate"]["loader_enabled"], true);
    assert_eq!(report["loader_gate"]["real_aex_load_enabled"], true);
    assert_eq!(
        report["next_action"],
        "Open a separate explicit loader implementation slice; this preflight still performed no native loading."
    );
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));
}

#[test]
fn selected_fixture_path_matching_uses_lexical_normalization_only() {
    let mut fixture_gate: Value =
        serde_json::from_str(&fixture_gate_json(Some("adaptive-filter-local"), true)).unwrap();
    fixture_gate["candidates"][0]["path"] =
        json!("D:/AviUtlas/local/./staging/../AdaptiveFilter.aex");
    let mut loader_gate: Value = serde_json::from_str(&open_loader_gate_json()).unwrap();
    loader_gate["entries"][0]["plugin_path"] = json!("d:\\AVIUTLAS\\local\\AdaptiveFilter.aex");

    let report = preflight(&fixture_gate.to_string(), &loader_gate.to_string());

    assert_eq!(report["status"], "preflight_passed_no_load");
    assert_eq!(report["preflight_passed"], true);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(
        report["selected_candidate"]["plugin_path"],
        "D:/AviUtlas/local/./staging/../AdaptiveFilter.aex"
    );
    assert_eq!(
        report["selected_candidate"]["normalized_plugin_path"],
        "d:\\aviutlas\\local\\adaptivefilter.aex"
    );
    assert_eq!(
        report["selected_candidate"]["loader_gate_plugin_path"],
        "d:\\AVIUTLAS\\local\\AdaptiveFilter.aex"
    );
    assert_eq!(
        report["selected_candidate"]["path_match_status"],
        "matched_normalized_path"
    );
    assert_eq!(
        report["selected_loader_entry"]["plugin_path"],
        "d:\\AVIUTLAS\\local\\AdaptiveFilter.aex"
    );
    assert_eq!(
        report["selected_loader_entry"]["normalized_plugin_path"],
        "d:\\aviutlas\\local\\adaptivefilter.aex"
    );
    assert_eq!(
        report["selected_loader_entry"]["path_match_status"],
        "matched_normalized_path"
    );
    assert!(check_status(
        &report,
        "selected_fixture_in_loader_gate",
        "passed"
    ));
    assert!(check_status(
        &report,
        "loader_gate_single_ready_entry",
        "passed"
    ));
}

#[test]
fn inconsistent_loader_gate_with_multiple_ready_entries_is_blocked() {
    let fixture_gate = fixture_gate_json(Some("adaptive-filter-local"), true);
    let mut loader_gate: Value = serde_json::from_str(&open_loader_gate_json()).unwrap();
    loader_gate["entries"][1]["pre_loader_status"] = json!("approved-local-only");
    loader_gate["entries"][1]["loader_approval_status"] = json!("approved-local-only");
    loader_gate["entries"][1]["allowlist_operation_status"] = json!("render_png");

    let report = preflight(&fixture_gate, &loader_gate.to_string());

    assert_eq!(report["status"], "blocked_loader_gate_closed");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert!(check_status(
        &report,
        "loader_gate_single_ready_entry",
        "blocked"
    ));
    assert!(array_contains(
        &report["blocked_reasons"],
        "exactly one entry-level approved render_png candidate"
    ));
}

#[test]
fn duplicate_fixture_candidate_ids_block_before_loader_permission() {
    let mut fixture_gate: Value =
        serde_json::from_str(&fixture_gate_json(Some("adaptive-filter-local"), true)).unwrap();
    fixture_gate["candidates"][1]["id"] = json!("adaptive-filter-local");
    let loader_gate = open_loader_gate_json();

    let report = preflight(&fixture_gate.to_string(), &loader_gate);

    assert_eq!(report["status"], "blocked_loader_gate_closed");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert!(check_status(
        &report,
        "fixture_gate_unique_candidates",
        "blocked"
    ));
    assert!(check_status(
        &report,
        "loader_gate_unique_entries",
        "passed"
    ));
    assert!(array_contains(
        &report["blocked_reasons"],
        "duplicate candidate ids or paths"
    ));
}

#[test]
fn duplicate_fixture_candidate_normalized_paths_block_before_loader_permission() {
    let mut fixture_gate: Value =
        serde_json::from_str(&fixture_gate_json(Some("adaptive-filter-local"), true)).unwrap();
    fixture_gate["candidates"][1]["path"] =
        json!("D:/AviUtlas/local/./staging/../AdaptiveFilter.aex");
    let loader_gate = open_loader_gate_json();

    let report = preflight(&fixture_gate.to_string(), &loader_gate);

    assert_eq!(report["status"], "blocked_loader_gate_closed");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert!(check_status(
        &report,
        "fixture_gate_unique_candidates",
        "blocked"
    ));
    assert!(check_status(
        &report,
        "loader_gate_single_ready_entry",
        "passed"
    ));
    assert!(array_contains(
        &report["blocked_reasons"],
        "duplicate candidate ids or paths"
    ));
}

#[test]
fn duplicate_loader_effect_ids_block_before_loader_permission() {
    let fixture_gate = fixture_gate_json(Some("adaptive-filter-local"), true);
    let mut loader_gate: Value = serde_json::from_str(&open_loader_gate_json()).unwrap();
    loader_gate["entries"][1]["effect_id"] = json!("adaptivefilter-local");

    let report = preflight(&fixture_gate, &loader_gate.to_string());

    assert_eq!(report["status"], "blocked_loader_gate_closed");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert!(check_status(
        &report,
        "fixture_gate_unique_candidates",
        "passed"
    ));
    assert!(check_status(
        &report,
        "loader_gate_unique_entries",
        "blocked"
    ));
    assert!(array_contains(
        &report["blocked_reasons"],
        "duplicate effect ids or paths"
    ));
}

#[test]
fn duplicate_loader_normalized_paths_block_before_loader_permission() {
    let fixture_gate = fixture_gate_json(Some("adaptive-filter-local"), true);
    let mut loader_gate: Value = serde_json::from_str(&open_loader_gate_json()).unwrap();
    loader_gate["entries"][1]["plugin_path"] =
        json!("D:/AviUtlas/local/./staging/../AdaptiveFilter.aex");

    let report = preflight(&fixture_gate, &loader_gate.to_string());

    assert_eq!(report["status"], "blocked_loader_gate_closed");
    assert_eq!(report["preflight_passed"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert!(check_status(
        &report,
        "fixture_gate_unique_candidates",
        "passed"
    ));
    assert!(check_status(
        &report,
        "loader_gate_unique_entries",
        "blocked"
    ));
    assert!(check_status(
        &report,
        "selected_fixture_in_loader_gate",
        "passed"
    ));
    assert!(array_contains(
        &report["blocked_reasons"],
        "duplicate effect ids or paths"
    ));
}

fn check_status(report: &Value, name: &str, status: &str) -> bool {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == name && check["status"] == status)
}

fn array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains(expected))
}

fn fixture_gate_json(selected_fixture: Option<&str>, approved: bool) -> String {
    let selected = selected_fixture
        .map(|id| format!(r#""{id}""#))
        .unwrap_or_else(|| "null".to_string());
    let review_status = if approved {
        "approved-local-only"
    } else {
        "not-approved"
    };
    format!(
        r#"{{
          "schema_version": 1,
          "status": "review_queue_not_approved",
          "selected_fixture": {selected},
          "recommended_first_review": "adaptive-filter-local",
          "recommendation_status": "queue-order-only-not-approval",
          "approval": {{
            "approved": {approved},
            "loader_enabled": {approved},
            "real_aex_load_enabled": {approved},
            "render_png_enabled": {approved},
            "describe_enabled_for_real_aex": {approved}
          }},
          "candidates": [
            {{
              "id": "adaptive-filter-local",
              "display_name": "AdaptiveFilter",
              "path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
              "review_status": "{review_status}",
              "fixture_status": "local-build-candidate",
              "plugin_class": "classic-effect-candidate"
            }},
            {{
              "id": "median-pro-local",
              "display_name": "MedianPro",
              "path": "D:\\AviUtlas\\local\\MedianPro.aex",
              "review_status": "{review_status}",
              "fixture_status": "local-build-candidate",
              "plugin_class": "classic-effect-candidate"
            }}
          ]
        }}"#
    )
}

fn closed_loader_gate_json() -> String {
    loader_gate_json(
        false,
        false,
        false,
        0,
        "blocked_pending_review",
        "not-approved",
        "describe-only",
    )
}

fn open_loader_gate_json() -> String {
    loader_gate_json(
        true,
        true,
        true,
        1,
        "approved-local-only",
        "approved-local-only",
        "render_png",
    )
}

fn loader_gate_json(
    approved: bool,
    loader_enabled: bool,
    real_aex_load_enabled: bool,
    open_candidate_count: u32,
    pre_loader_status: &str,
    loader_approval_status: &str,
    operation: &str,
) -> String {
    format!(
        r#"{{
          "schema_version": 1,
          "status": "loader_gate_not_opened",
          "approved": {approved},
          "loader_enabled": {loader_enabled},
          "real_aex_load_enabled": {real_aex_load_enabled},
          "open_candidate_count": {open_candidate_count},
          "entries": [
            {{
              "effect_id": "adaptivefilter-local",
              "plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
              "pre_loader_status": "{pre_loader_status}",
              "loader_approval_status": "{loader_approval_status}",
              "allowlist_operation_status": "{operation}",
              "sandbox_preflight_required": "passed",
              "job_object_required": "assigned-with-kill-on-close",
              "handle_inheritance_required": "sentinel_not_inherited-with-explicit-handle-list",
              "worker_identity_revalidation_required": "passed",
              "worker_attestation_required": "passed"
            }},
            {{
              "effect_id": "medianpro-local",
              "plugin_path": "D:\\AviUtlas\\local\\MedianPro.aex",
              "pre_loader_status": "blocked_pending_review",
              "loader_approval_status": "not-approved",
              "allowlist_operation_status": "describe-only",
              "sandbox_preflight_required": "passed",
              "job_object_required": "assigned-with-kill-on-close",
              "handle_inheritance_required": "sentinel_not_inherited-with-explicit-handle-list",
              "worker_identity_revalidation_required": "passed",
              "worker_attestation_required": "passed"
            }}
          ]
        }}"#
    )
}
