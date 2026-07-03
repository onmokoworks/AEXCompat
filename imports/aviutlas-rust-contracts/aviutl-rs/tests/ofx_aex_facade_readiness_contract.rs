#[allow(dead_code)]
#[path = "../examples/aex_native_stage_plan.rs"]
mod aex_native_stage_plan;
#[allow(dead_code)]
#[path = "../examples/ofx_aex_facade_readiness.rs"]
mod ofx_aex_facade_readiness;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CONTRACT: &str = include_str!("../../analysis/OFX_AEX_FACADE_CONTRACT_2026-05-31.json");
const FIXTURE_GATE: &str = include_str!("../../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json");
const READINESS_REPORT_SCHEMA: &str =
    include_str!("../../analysis/OFX_AEX_FACADE_READINESS_REPORT_SCHEMA_2026-06-01.json");
const BOUNDARY_SCHEMA: &str =
    include_str!("../../analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json");

fn report(loader_gate: &str, capabilities: &[&str]) -> Value {
    let output = ofx_aex_facade_readiness::plan_ofx_facade_readiness_json(
        CONTRACT,
        FIXTURE_GATE,
        loader_gate,
        capabilities,
    )
    .expect("OFX facade readiness should run");
    serde_json::from_str(&output).expect("OFX facade readiness should emit JSON")
}

fn report_with_native_stage_plan(
    loader_gate: &str,
    native_stage_plan: &str,
    capabilities: &[&str],
) -> Value {
    let output = ofx_aex_facade_readiness::plan_ofx_facade_readiness_json_with_native_stage_plan(
        CONTRACT,
        FIXTURE_GATE,
        loader_gate,
        Some(native_stage_plan),
        capabilities,
    )
    .expect("OFX facade readiness with native stage plan should run");
    serde_json::from_str(&output).expect("OFX facade readiness should emit JSON")
}

fn report_with_loader_slice_review(
    loader_gate: &str,
    loader_slice_review: &str,
    capabilities: &[&str],
) -> Value {
    let output = ofx_aex_facade_readiness::
        plan_ofx_facade_readiness_json_with_native_stage_plan_and_loader_slice_review(
            CONTRACT,
            FIXTURE_GATE,
            loader_gate,
            None,
            Some(loader_slice_review),
            capabilities,
        )
        .expect("OFX facade readiness with loader slice review should run");
    serde_json::from_str(&output).expect("OFX facade readiness should emit JSON")
}

fn report_with_loader_evidence(
    loader_gate: &str,
    loader_slice_review: Option<&str>,
    loader_approval_report: Option<&str>,
    capabilities: &[&str],
) -> Value {
    let output = ofx_aex_facade_readiness::
        plan_ofx_facade_readiness_json_with_native_stage_plan_and_loader_evidence(
            CONTRACT,
            FIXTURE_GATE,
            loader_gate,
            None,
            loader_slice_review,
            loader_approval_report,
            capabilities,
        )
        .expect("OFX facade readiness with loader evidence should run");
    serde_json::from_str(&output).expect("OFX facade readiness should emit JSON")
}

fn report_with_aex_metadata_gate(
    loader_gate: &str,
    aex_metadata_gate_report: &str,
    capabilities: &[&str],
) -> Value {
    let output = ofx_aex_facade_readiness::plan_ofx_facade_readiness_json_with_all_evidence(
        ofx_aex_facade_readiness::OfxFacadeReadinessJsonInputs {
            contract_json: CONTRACT,
            fixture_gate_json: FIXTURE_GATE,
            loader_gate_json: loader_gate,
            native_stage_plan_json: None,
            loader_slice_review_json: None,
            loader_approval_report_json: None,
            aex_metadata_gate_report_json: Some(aex_metadata_gate_report),
            capability_jsons: capabilities,
        },
    )
    .expect("OFX facade readiness with AEX metadata gate should run");
    serde_json::from_str(&output).expect("OFX facade readiness should emit JSON")
}

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-ofx-facade-readiness")
        .join(format!("{}-{name}", std::process::id()))
}

fn unique_target_path(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    target_path(&format!("{stamp}-{name}"))
}

fn readiness_report_schema() -> Value {
    serde_json::from_str(READINESS_REPORT_SCHEMA)
        .expect("OFX facade readiness report schema should parse")
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

fn assert_readiness_report_matches_schema(report: &Value, schema: &Value) {
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
            "OFX readiness report field {field} diverged from schema"
        );
    }

    let status = report["status"].as_str().expect("status should be string");
    assert!(
        json_string_array(&schema["allowed_statuses"]).contains(&status),
        "unexpected OFX readiness status {status}"
    );
    assert!(
        json_string_array(&schema["allowed_describe_exposures"]).contains(
            &report["describe_exposure"]
                .as_str()
                .expect("describe_exposure should be string")
        )
    );
    assert!(
        json_string_array(&schema["allowed_render_png_exposures"]).contains(
            &report["render_png_exposure"]
                .as_str()
                .expect("render_png_exposure should be string")
        )
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
        &report["native_stage_plan_summary"],
        &json_string_array(&schema["native_stage_plan_summary_required_fields"]),
        "native_stage_plan_summary",
    );
    if report["native_stage_plan_summary"]["provided"] == true
        && report["status"] == schema["deferred_status"]
    {
        for (field, expected) in schema["native_stage_plan_summary_required_values_when_provided"]
            .as_object()
            .expect("native_stage_plan_summary_required_values_when_provided should be object")
        {
            assert_eq!(
                &report["native_stage_plan_summary"][field], expected,
                "native stage plan summary field {field} diverged from schema"
            );
        }
    }
    assert_object_has_fields(
        &report["loader_slice_review_summary"],
        &json_string_array(&schema["loader_slice_review_summary_required_fields"]),
        "loader_slice_review_summary",
    );
    if report["loader_slice_review_summary"]["provided"] == true
        && report["status"] == schema["deferred_status"]
    {
        for (field, expected) in schema["loader_slice_review_summary_required_values_when_provided"]
            .as_object()
            .expect("loader_slice_review_summary_required_values_when_provided should be object")
        {
            assert_eq!(
                &report["loader_slice_review_summary"][field], expected,
                "loader slice review summary field {field} diverged from schema"
            );
        }
    }
    assert_object_has_fields(
        &report["loader_approval_summary"],
        &json_string_array(&schema["loader_approval_summary_required_fields"]),
        "loader_approval_summary",
    );
    if report["loader_approval_summary"]["provided"] == true
        && report["status"] == schema["deferred_status"]
    {
        for (field, expected) in schema["loader_approval_summary_required_values_when_provided"]
            .as_object()
            .expect("loader_approval_summary_required_values_when_provided should be object")
        {
            assert_eq!(
                &report["loader_approval_summary"][field], expected,
                "loader approval summary field {field} diverged from schema"
            );
        }
    }
    assert_object_has_fields(
        &report["aex_metadata_gate_summary"],
        &json_string_array(&schema["aex_metadata_gate_summary_required_fields"]),
        "aex_metadata_gate_summary",
    );
    if report["aex_metadata_gate_summary"]["provided"] == true
        && report["status"] == schema["deferred_status"]
    {
        for (field, expected) in schema["aex_metadata_gate_summary_required_values_when_provided"]
            .as_object()
            .expect("aex_metadata_gate_summary_required_values_when_provided should be object")
        {
            assert_eq!(
                &report["aex_metadata_gate_summary"][field], expected,
                "AEX metadata gate summary field {field} diverged from schema"
            );
        }
    }
    assert_object_has_fields(
        &report["ofx_facade_review_gate"],
        &json_string_array(&schema["ofx_facade_review_gate_required_fields"]),
        "ofx_facade_review_gate",
    );
    for (field, expected) in schema["ofx_facade_review_gate_required_values"]
        .as_object()
        .expect("ofx_facade_review_gate_required_values should be an object")
    {
        assert_eq!(
            &report["ofx_facade_review_gate"][field], expected,
            "OFX facade review gate field {field} diverged from schema"
        );
    }

    for entry in report["entries"]
        .as_array()
        .expect("entries should be an array")
    {
        assert_object_has_fields(
            entry,
            &json_string_array(&schema["entry_required_fields"]),
            "entry",
        );
        for (field, expected) in schema["entry_required_values"]
            .as_object()
            .expect("entry_required_values should be an object")
        {
            assert_eq!(
                &entry[field], expected,
                "OFX readiness entry field {field} diverged from schema"
            );
        }
    }

    for required_path in json_string_array(&schema["required_forbidden_paths"]) {
        assert!(
            report["forbidden_paths"]
                .as_array()
                .expect("forbidden_paths should be an array")
                .iter()
                .any(|path| path == required_path),
            "report missing required forbidden path {required_path}"
        );
    }
    for expected_note in json_string_array(&schema["required_notes"]) {
        assert!(
            report["notes"]
                .as_array()
                .expect("notes should be an array")
                .iter()
                .any(|note| note == expected_note),
            "report missing required note {expected_note}"
        );
    }

    let serialized = serde_json::to_string(report)
        .expect("report should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "OFX readiness report should not contain serialized token {token}"
        );
    }
}

#[test]
fn ofx_facade_readiness_reports_match_dedicated_schema() {
    let schema = readiness_report_schema();

    let deferred = report(&closed_loader_gate_json(), &[closed_capability_json()]);
    assert_readiness_report_matches_schema(&deferred, &schema);

    let premature = report(&closed_loader_gate_json(), &[premature_capability_json()]);
    assert_readiness_report_matches_schema(&premature, &schema);

    let open_loader = report(&open_loader_gate_json(), &[closed_capability_json()]);
    assert_readiness_report_matches_schema(&open_loader, &schema);
}

#[test]
fn ofx_facade_readiness_stays_deferred_and_no_bypass() {
    let report = report(&closed_loader_gate_json(), &[closed_capability_json()]);

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "deferred_contract_only");
    assert_eq!(report["contract_status"], "deferred-contract-only");
    assert_eq!(report["ofx_host_may_load_aex"], false);
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["aviutlas_may_route_through_ofx_to_reach_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
    assert_eq!(report["fixture_gate"]["selected_fixture"], Value::Null);
    assert_eq!(report["fixture_gate"]["candidate_count"], 2);
    assert_eq!(report["loader_gate"]["status"], "loader_gate_not_opened");
    assert_eq!(report["loader_gate"]["approved"], false);
    assert_eq!(report["loader_gate"]["loader_enabled"], false);
    assert_eq!(report["loader_gate"]["real_aex_load_enabled"], false);
    assert_eq!(report["native_stage_plan_summary"]["provided"], false);
    assert_eq!(report["loader_slice_review_summary"]["provided"], false);
    assert_eq!(
        report["loader_slice_review_summary"]["ready_for_manual_loader_slice_review_no_load"],
        false
    );
    assert_eq!(report["loader_approval_summary"]["provided"], false);
    assert_eq!(
        report["loader_approval_summary"]["approval_accepted"],
        false
    );
    assert_eq!(report["aex_metadata_gate_summary"]["provided"], false);
    assert_eq!(
        report["aex_metadata_gate_summary"]["ready_for_ofx_review_no_load"],
        false
    );
    assert_eq!(report["ofx_facade_review_gate"]["approved"], false);
    assert_eq!(
        report["ofx_facade_review_gate"]["may_point_to_broker"],
        false
    );
    assert_eq!(
        report["ofx_facade_review_gate"]["may_issue_render_png"],
        false
    );
    assert_eq!(report["capability_count"], 1);
    assert_eq!(report["supported_capability_count"], 0);
    assert!(report["current_supported_operations"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        report["describe_exposure"],
        "metadata_only_no_worker_describe"
    );
    assert_eq!(report["render_png_exposure"], "blocked_loader_gate_closed");
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert_eq!(report["entries"][0]["effect_id"], "adaptivefilter-local");
    assert_eq!(report["entries"][0]["ofx_facade_supported"], false);
    assert_eq!(
        report["entries"][0]["exposure_status"],
        "deferred_same_aex_worker_gate"
    );
    assert!(report["forbidden_paths"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path == "OFX adapter bypasses AEX allowlist"));

    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    for forbidden in ["loadlibrary", "libloading", "sha256", "base64"] {
        assert!(
            !serialized.contains(forbidden),
            "OFX readiness should not expose forbidden token {forbidden}"
        );
    }
}

#[test]
fn ofx_facade_readiness_summarizes_native_stage_plan_but_stays_deferred() {
    let native_stage_plan = ready_native_stage_plan_json();
    let report = report_with_native_stage_plan(
        &closed_loader_gate_json(),
        &native_stage_plan,
        &[closed_capability_json()],
    );
    let schema = readiness_report_schema();

    assert_readiness_report_matches_schema(&report, &schema);
    assert_eq!(report["status"], "deferred_contract_only");
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
    assert_eq!(
        report["native_stage_plan_summary"]["status"],
        "planned_native_stage_contract_no_load"
    );
    assert_eq!(
        report["native_stage_plan_summary"]["no_load_stage_plan_ready"],
        true
    );
    assert_eq!(
        report["native_stage_plan_summary"]["worker_runtime_evidence_ready"],
        true
    );
    assert_eq!(
        report["native_stage_plan_summary"]["cleanroom_boundary_no_loader_or_sdk"],
        true
    );
    assert_eq!(
        report["native_stage_plan_summary"]["native_stage_count"],
        report["native_stage_plan_summary"]["planned_not_run_count"]
    );
    assert_eq!(
        report["native_stage_plan_summary"]["ofx_route_blocked"],
        true
    );
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn ofx_facade_readiness_accepts_native_stage_plan_generated_by_stage_planner() {
    let native_stage_plan = native_stage_plan_generated_by_stage_planner_json();
    let native_stage_plan: Value =
        serde_json::from_str(&native_stage_plan).expect("native stage plan should parse");
    assert_eq!(
        native_stage_plan["status"],
        "planned_native_stage_contract_no_load"
    );
    assert_eq!(native_stage_plan["native_load_performed"], false);
    assert_eq!(
        native_stage_plan["cleanroom_boundary_summary"]["native_loader_calls_allowed"],
        false
    );

    let report = report_with_native_stage_plan(
        &closed_loader_gate_json(),
        &native_stage_plan.to_string(),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "deferred_contract_only");
    assert_eq!(report["native_stage_plan_summary"]["provided"], true);
    assert_eq!(
        report["native_stage_plan_summary"]["no_load_stage_plan_ready"],
        true
    );
    assert_eq!(
        report["native_stage_plan_summary"]["worker_runtime_evidence_ready"],
        true
    );
    assert_eq!(
        report["native_stage_plan_summary"]["cleanroom_boundary_no_loader_or_sdk"],
        true
    );
    assert_eq!(
        report["native_stage_plan_summary"]["native_stage_count"],
        report["native_stage_plan_summary"]["planned_not_run_count"]
    );
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
}

#[test]
fn ofx_facade_readiness_blocks_native_stage_route_or_execution_claims() {
    let mut native_stage_plan: Value =
        serde_json::from_str(&ready_native_stage_plan_json()).unwrap();
    native_stage_plan["ofx_may_route_to_loader"] = json!(true);
    native_stage_plan["promotion_gate"]["ofx_facade_may_route_to_loader"] = json!(true);
    native_stage_plan["selectors_executed"] = json!(true);
    let report = report_with_native_stage_plan(
        &closed_loader_gate_json(),
        &native_stage_plan.to_string(),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["native_stage_plan_summary"]["selector_execution_blocked"],
        false
    );
    assert_eq!(
        report["native_stage_plan_summary"]["ofx_route_blocked"],
        false
    );
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert!(contains_reason(
        &report,
        "native stage plan does not preserve no-load/runtime/cleanroom evidence for OFX facade review"
    ));
}

#[test]
fn ofx_facade_readiness_blocks_contaminated_native_stage_plan_without_echo() {
    let mut native_stage_plan: Value =
        serde_json::from_str(&ready_native_stage_plan_json()).unwrap();
    native_stage_plan["status"] = json!("output_png");
    let report = report_with_native_stage_plan(
        &closed_loader_gate_json(),
        &native_stage_plan.to_string(),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["native_stage_plan_summary"]["input_contains_forbidden_tokens"],
        true
    );
    assert!(report["native_stage_plan_summary"]["status"].is_null());
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("output_png"));
}

#[test]
fn ofx_facade_readiness_consumes_sanitized_loader_slice_review_packet_but_stays_deferred() {
    let loader_slice_review = ready_loader_slice_review_packet_json();
    let report = report_with_loader_slice_review(
        &closed_loader_gate_json(),
        &loader_slice_review,
        &[closed_capability_json()],
    );
    let schema = readiness_report_schema();

    assert_readiness_report_matches_schema(&report, &schema);
    assert_eq!(report["status"], "deferred_contract_only");
    assert_eq!(report["ofx_host_may_load_aex"], false);
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
    assert_eq!(
        report["render_png_exposure"],
        "blocked_pending_separate_ofx_review"
    );
    assert_eq!(report["loader_slice_review_summary"]["provided"], true);
    assert_eq!(
        report["loader_slice_review_summary"]["status"],
        "ready_for_manual_loader_slice_review_no_load"
    );
    assert_eq!(
        report["loader_slice_review_summary"]["ready_for_manual_loader_slice_review_no_load"],
        true
    );
    assert_eq!(
        report["loader_slice_review_summary"]["loader_slice_approved"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["loader_enabled"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["real_aex_load_enabled"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["native_loader_calls_allowed"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["broker_may_load_aex"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["worker_may_load_plugin"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["render_performed"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["ofx_route_allowed"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["input_contains_forbidden_tokens"],
        false
    );
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn ofx_facade_readiness_blocks_loader_slice_packet_with_ofx_route_allowed() {
    let mut loader_slice_review: Value =
        serde_json::from_str(&ready_loader_slice_review_packet_json()).unwrap();
    loader_slice_review["ofx_route_allowed"] = json!(true);
    let report = report_with_loader_slice_review(
        &closed_loader_gate_json(),
        &loader_slice_review.to_string(),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["loader_slice_review_summary"]["ready_for_manual_loader_slice_review_no_load"],
        false
    );
    assert_eq!(
        report["loader_slice_review_summary"]["ofx_route_allowed"],
        true
    );
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert!(contains_reason(
        &report,
        "loader slice review packet does not preserve sanitized no-load evidence for OFX facade review"
    ));
}

#[test]
fn ofx_facade_readiness_does_not_serialize_private_loader_slice_paths() {
    let mut loader_slice_review: Value =
        serde_json::from_str(&ready_loader_slice_review_packet_json()).unwrap();
    loader_slice_review["status"] =
        json!("D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins\\AdaptiveFilter.aex");
    let report = report_with_loader_slice_review(
        &closed_loader_gate_json(),
        &loader_slice_review.to_string(),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["loader_slice_review_summary"]["input_contains_forbidden_tokens"],
        true
    );
    assert!(report["loader_slice_review_summary"]["status"].is_null());
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    for forbidden in [
        "adaptivefilter.aex",
        "medianpro.aex",
        "d:\\projects\\01_project\\04_tools\\ae_plugins",
        "ae_plugins",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "OFX readiness should not echo private loader slice marker {forbidden}"
        );
    }
}

#[test]
fn ofx_facade_readiness_consumes_loader_approval_report_but_keeps_ofx_deferred() {
    let loader_approval = ready_loader_approval_report_json();
    let report = report_with_loader_evidence(
        &closed_loader_gate_json(),
        Some(&ready_loader_slice_review_packet_json()),
        Some(&loader_approval),
        &[closed_capability_json()],
    );
    let schema = readiness_report_schema();

    assert_readiness_report_matches_schema(&report, &schema);
    assert_eq!(report["status"], "deferred_contract_only");
    assert_eq!(report["ofx_host_may_load_aex"], false);
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
    assert_eq!(
        report["render_png_exposure"],
        "blocked_pending_separate_ofx_review"
    );
    assert_eq!(report["loader_approval_summary"]["provided"], true);
    assert_eq!(
        report["loader_approval_summary"]["validation_status"],
        "approved_for_loader_implementation_review_no_load"
    );
    assert_eq!(report["loader_approval_summary"]["approval_accepted"], true);
    assert_eq!(
        report["loader_approval_summary"]["loader_review_approved"],
        true
    );
    assert_eq!(
        report["loader_approval_summary"]["receipt_allows_separate_loader_implementation_review"],
        true
    );
    assert_eq!(
        report["loader_approval_summary"]["receipt_allows_ofx_route"],
        false
    );
    assert_eq!(
        report["loader_approval_summary"]["ofx_route_allowed"],
        false
    );
    assert_eq!(report["loader_approval_summary"]["blocked_reason_count"], 0);
    assert_eq!(
        report["loader_approval_summary"]["input_contains_forbidden_tokens"],
        false
    );
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn ofx_facade_readiness_blocks_loader_approval_report_with_route_or_render_claims() {
    let mut approval: Value = serde_json::from_str(&ready_loader_approval_report_json()).unwrap();
    approval["receipt_allows_ofx_route"] = json!(true);
    approval["receipt_allows_render_png"] = json!(true);
    approval["ofx_route_allowed"] = json!(true);
    let report = report_with_loader_evidence(
        &closed_loader_gate_json(),
        Some(&ready_loader_slice_review_packet_json()),
        Some(&approval.to_string()),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["loader_approval_summary"]["receipt_allows_ofx_route"],
        true
    );
    assert_eq!(
        report["loader_approval_summary"]["receipt_allows_render_png"],
        true
    );
    assert_eq!(report["loader_approval_summary"]["ofx_route_allowed"], true);
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert!(contains_reason(
        &report,
        "loader approval validation report does not preserve review-only no-load evidence for OFX facade review"
    ));
}

#[test]
fn ofx_facade_readiness_does_not_serialize_private_loader_approval_paths() {
    let mut approval: Value = serde_json::from_str(&ready_loader_approval_report_json()).unwrap();
    approval["validation_status"] =
        json!("D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins\\MedianPro.aex");
    let report = report_with_loader_evidence(
        &closed_loader_gate_json(),
        Some(&ready_loader_slice_review_packet_json()),
        Some(&approval.to_string()),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["loader_approval_summary"]["input_contains_forbidden_tokens"],
        true
    );
    assert!(report["loader_approval_summary"]["validation_status"].is_null());
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    for forbidden in [
        "medianpro.aex",
        "adaptivefilter.aex",
        "d:\\projects\\01_project\\04_tools\\ae_plugins",
        "plugin_path",
        "normalized_plugin_path",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "OFX readiness should not echo private approval marker {forbidden}"
        );
    }
}

#[test]
fn ofx_facade_readiness_consumes_aex_metadata_gate_but_keeps_ofx_deferred() {
    let metadata_gate = ready_aex_metadata_gate_report_json();
    let report = report_with_aex_metadata_gate(
        &closed_loader_gate_json(),
        &metadata_gate,
        &[closed_capability_json()],
    );
    let schema = readiness_report_schema();

    assert_readiness_report_matches_schema(&report, &schema);
    assert_eq!(report["status"], "deferred_contract_only");
    assert_eq!(report["ofx_host_may_load_aex"], false);
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["aviutlas_may_route_through_ofx_to_reach_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
    assert_eq!(report["render_png_exposure"], "blocked_loader_gate_closed");
    assert_eq!(report["aex_metadata_gate_summary"]["provided"], true);
    assert_eq!(
        report["aex_metadata_gate_summary"]["status"],
        "aex_metadata_gate_ready_no_load"
    );
    assert_eq!(
        report["aex_metadata_gate_summary"]["ready_for_ofx_review_no_load"],
        true
    );
    assert_eq!(
        report["aex_metadata_gate_summary"]["native_load_performed"],
        false
    );
    assert_eq!(report["aex_metadata_gate_summary"]["aex_loaded"], false);
    assert_eq!(
        report["aex_metadata_gate_summary"]["ofx_route_allowed"],
        false
    );
    assert_eq!(
        report["aex_metadata_gate_summary"]["fixture_gate_status"],
        "review_queue_not_approved"
    );
    assert_eq!(
        report["aex_metadata_gate_summary"]["loader_gate_status"],
        "loader_gate_not_opened"
    );
    assert_eq!(
        report["aex_metadata_gate_summary"]["identity_smoke_aex_render_correctness_evidence"],
        false
    );
    assert_eq!(
        report["aex_metadata_gate_summary"]["ofx_contract_status"],
        "deferred-contract-only"
    );
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn ofx_facade_readiness_blocks_aex_metadata_gate_route_or_render_claims() {
    let mut metadata_gate: Value =
        serde_json::from_str(&ready_aex_metadata_gate_report_json()).unwrap();
    metadata_gate["status"] = json!("blocked_aex_metadata_gate");
    metadata_gate["aex_loaded"] = json!(true);
    metadata_gate["ofx_route_allowed"] = json!(true);
    metadata_gate["identity_smoke_summary"]["aex_render_correctness_evidence"] = json!(true);
    metadata_gate["ofx_contract_summary"]["review_approved"] = json!(true);
    metadata_gate["blocked_reasons"] = json!(["drift toward OFX route"]);
    let report = report_with_aex_metadata_gate(
        &closed_loader_gate_json(),
        &metadata_gate.to_string(),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["aex_metadata_gate_summary"]["ready_for_ofx_review_no_load"],
        false
    );
    assert_eq!(report["aex_metadata_gate_summary"]["aex_loaded"], true);
    assert_eq!(
        report["aex_metadata_gate_summary"]["ofx_route_allowed"],
        true
    );
    assert_eq!(
        report["aex_metadata_gate_summary"]["identity_smoke_aex_render_correctness_evidence"],
        true
    );
    assert_eq!(
        report["aex_metadata_gate_summary"]["ofx_contract_review_approved"],
        true
    );
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert!(contains_reason(
        &report,
        "AEX metadata gate report does not preserve no-load/deferred evidence for OFX facade review"
    ));
}

#[test]
fn ofx_facade_readiness_does_not_serialize_private_aex_metadata_gate_paths() {
    let mut metadata_gate: Value =
        serde_json::from_str(&ready_aex_metadata_gate_report_json()).unwrap();
    metadata_gate["status"] =
        json!("D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins\\AdaptiveFilter.aex");
    metadata_gate["notes"] = json!(["plugin_path should never be echoed"]);
    let report = report_with_aex_metadata_gate(
        &closed_loader_gate_json(),
        &metadata_gate.to_string(),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["aex_metadata_gate_summary"]["input_contains_forbidden_tokens"],
        true
    );
    assert!(report["aex_metadata_gate_summary"]["status"].is_null());
    assert!(report["aex_metadata_gate_summary"]["report_kind"].is_null());
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    for forbidden in [
        "adaptivefilter.aex",
        "d:\\projects\\01_project\\04_tools",
        "ae_plugins",
        "plugin_path",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "OFX readiness should not echo private metadata-gate marker {forbidden}"
        );
    }
}

#[test]
fn ofx_facade_readiness_validates_report_output_policy() {
    let valid_report = target_path("ofx-facade.local.json");
    assert!(
        ofx_aex_facade_readiness::validate_ofx_facade_report_output_path(&valid_report).is_ok()
    );

    let non_json_report = target_path("ofx-facade.local.txt");
    assert!(
        ofx_aex_facade_readiness::validate_ofx_facade_report_output_path(&non_json_report).is_err()
    );

    let traversal_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-ofx-facade-readiness")
        .join("..")
        .join("private-ofx-readiness.json");
    assert!(
        ofx_aex_facade_readiness::validate_ofx_facade_report_output_path(&traversal_report)
            .is_err()
    );

    let outside_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("outside-aex-ofx-facade-readiness.json");
    assert!(
        ofx_aex_facade_readiness::validate_ofx_facade_report_output_path(&outside_report).is_err()
    );
    assert!(!traversal_report.exists());
    assert!(!outside_report.exists());
}

#[test]
fn ofx_facade_readiness_report_writer_uses_create_new() {
    let report = unique_target_path("ofx-create-new.local.json");
    ofx_aex_facade_readiness::write_ofx_facade_report_create_new(&report, "{\"first\":true}")
        .expect("first OFX readiness report write should succeed");

    let err =
        ofx_aex_facade_readiness::write_ofx_facade_report_create_new(&report, "{\"second\":true}")
            .expect_err("second OFX readiness report write should use create_new and fail");
    assert!(
        err.to_string().contains("already exists"),
        "unexpected create-new error: {err}"
    );

    let contents =
        std::fs::read_to_string(&report).expect("created OFX readiness report should be readable");
    assert!(contents.contains("\"first\":true"));
    assert!(!contents.contains("\"second\":true"));
}

#[test]
fn ofx_facade_readiness_blocks_premature_capability_support() {
    let report = report(&closed_loader_gate_json(), &[premature_capability_json()]);

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(report["supported_capability_count"], 1);
    assert!(contains_reason(
        &report,
        "capability says OFX facade is supported before loader approval"
    ));
    assert!(contains_reason(
        &report,
        "capability exposes operations before OFX facade is implemented"
    ));
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
}

#[test]
fn ofx_facade_readiness_blocks_capability_ids_outside_loader_gate() {
    let report = report(&closed_loader_gate_json(), &[mismatched_capability_json()]);

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(report["capability_count"], 1);
    assert_eq!(report["supported_capability_count"], 0);
    assert_eq!(report["entries"][0]["effect_id"], "not-in-loader-gate");
    assert!(contains_reason(
        &report,
        "capability effect_id not-in-loader-gate is not present in loader gate entries"
    ));
    assert_eq!(report["ofx_host_may_load_aex"], false);
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
    assert_eq!(
        report["ofx_facade_review_gate"]["requires_capability_ids_subset_of_loader_gate"],
        true
    );
}

#[test]
fn ofx_facade_readiness_blocks_open_loader_until_separate_review() {
    let report = report(&open_loader_gate_json(), &[closed_capability_json()]);

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(
        report["render_png_exposure"],
        "blocked_pending_separate_ofx_review"
    );
    assert!(contains_reason(
        &report,
        "AEX loader gate is open; OFX readiness must be reviewed separately"
    ));
    assert_eq!(report["ofx_host_may_load_aex"], false);
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(
        report["ofx_facade_review_gate"]["may_point_to_broker"],
        false
    );
}

#[test]
fn ofx_facade_readiness_blocks_contract_route_bypass_claims() {
    let report = ofx_aex_facade_readiness::plan_ofx_facade_readiness_json(
        bypass_contract_json(),
        FIXTURE_GATE,
        &closed_loader_gate_json(),
        &[closed_capability_json()],
    )
    .expect("OFX facade readiness should run on bypass contract");
    let report: Value = serde_json::from_str(&report).expect("OFX report should parse");

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(report["ofx_host_may_load_aex"], false);
    assert_eq!(report["ofx_adapter_may_load_aex"], false);
    assert_eq!(report["aviutlas_may_route_through_ofx_to_reach_aex"], false);
    assert_eq!(report["broker_may_load_aex"], false);
    assert!(contains_reason(
        &report,
        "OFX facade contract is not deferred-contract-only"
    ));
    assert!(contains_reason(
        &report,
        "OFX host route claims it may load .aex"
    ));
    assert!(contains_reason(
        &report,
        "OFX adapter route claims it may load .aex"
    ));
    assert!(contains_reason(
        &report,
        "broker route claims it may load .aex"
    ));
    assert!(contains_reason(
        &report,
        "AviUtlas AEX route is not direct broker first"
    ));
    assert!(contains_reason(
        &report,
        "OFX facade review gate is open before AEX worker support exists"
    ));
}

#[test]
fn ofx_facade_readiness_blocks_partial_open_loader_without_review_gate() {
    let report = report(
        &partial_open_loader_gate_json(),
        &[closed_capability_json()],
    );

    assert_eq!(report["status"], "blocked_contract_mismatch");
    assert_eq!(report["loader_gate"]["open_candidate_count"], 1);
    assert_eq!(report["loader_gate"]["loader_enabled"], false);
    assert_eq!(report["loader_gate"]["real_aex_load_enabled"], false);
    assert_eq!(
        report["render_png_exposure"],
        "blocked_pending_separate_ofx_review"
    );
    assert_eq!(
        report["ofx_facade_review_gate"]["may_point_to_broker"],
        false
    );
    assert_eq!(
        report["ofx_facade_review_gate"]["may_issue_describe"],
        false
    );
    assert!(contains_reason(
        &report,
        "OFX facade review is still required before pointing at the AEX broker"
    ));
}

fn contains_reason(report: &Value, expected: &str) -> bool {
    report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains(expected))
}

fn ready_native_stage_plan_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "planned_native_stage_contract_no_load",
        "native_load_performed": false,
        "selectors_executed": false,
        "render_performed": false,
        "broker_may_load_plugin": false,
        "worker_may_load_plugin": false,
        "ofx_may_route_to_loader": false,
        "ticket_runtime_evidence_summary": {
            "worker_identity_revalidation_required": "passed",
            "worker_attestation_required": "passed",
            "sandbox_preflight_required": "passed",
            "job_object_required": "assigned-with-kill-on-close",
            "handle_inheritance_required": "sentinel_not_inherited-with-explicit-handle-list"
        },
        "cleanroom_boundary_summary": {
            "native_loader_calls_allowed": false,
            "adobe_sdk_headers_allowed": false,
            "abi_generator_allowed": false,
            "third_party_effect_host_crate_allowed": false,
            "third_party_pipl_crate_allowed": false,
            "reuse_existing_aviutl_dynamic_loader_for_aex_allowed": false,
            "pf_names_are_planning_labels_only": true,
            "metadata_labels_do_not_define_abi": true
        },
        "native_stage_order": [
            {"name": "load", "status": "planned_not_run"},
            {"name": "global_setup", "status": "planned_not_run"},
            {"name": "params_setup", "status": "planned_not_run"},
            {"name": "sequence_setup", "status": "planned_not_run"},
            {"name": "frame_setup", "status": "planned_not_run"},
            {"name": "render", "status": "planned_not_run"},
            {"name": "frame_setdown", "status": "planned_not_run"},
            {"name": "sequence_setdown", "status": "planned_not_run"},
            {"name": "global_setdown", "status": "planned_not_run"}
        ],
        "promotion_gate": {
            "native_loader_calls_allowed": false,
            "worker_selector_calls_allowed": false,
            "worker_pixel_buffers_allowed": false,
            "ofx_facade_may_route_to_loader": false
        }
    }))
    .unwrap()
}

fn ready_loader_slice_review_packet_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "ready_for_manual_loader_slice_review_no_load",
        "loader_slice_approved": false,
        "loader_enabled": false,
        "real_aex_load_enabled": false,
        "native_loader_calls_allowed": false,
        "broker_may_load_aex": false,
        "worker_may_load_plugin": false,
        "render_performed": false,
        "ofx_route_allowed": false
    }))
    .unwrap()
}

fn ready_loader_approval_report_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "report_kind": "aex_loader_approval_receipt_validation",
        "validation_status": "approved_for_loader_implementation_review_no_load",
        "approval_accepted": true,
        "loader_review_approved": true,
        "native_load_performed": false,
        "loader_enabled": false,
        "real_aex_load_enabled": false,
        "native_loader_calls_allowed": false,
        "worker_may_load_plugin": false,
        "broker_may_load_aex": false,
        "render_performed": false,
        "ofx_route_allowed": false,
        "input_contains_forbidden_tokens": false,
        "loader_slice_review_packet_checked": true,
        "loader_slice_review_packet_status": "ready_for_manual_loader_slice_review_no_load",
        "loader_slice_review_packet_checksum_algorithm": "fnv1a64-v1-noncryptographic",
        "loader_slice_review_packet_checksum_hex": "0123456789abcdef",
        "fixture_gate_status": "review_queue_approved_local_only",
        "fixture_selected_for_review": true,
        "fixture_candidate_review_status": "approved-local-only",
        "fixture_candidate_fixture_status": "local-build-candidate",
        "fixture_candidate_plugin_class": "classic-effect-candidate",
        "runtime_and_cleanroom_ready": true,
        "receipt_kind": "aex_loader_manual_approval_receipt",
        "receipt_approval_status": "approved",
        "receipt_allows_separate_loader_implementation_review": true,
        "receipt_allows_native_aex_load": false,
        "receipt_allows_worker_plugin_load": false,
        "receipt_allows_render_png": false,
        "receipt_allows_ofx_route": false,
        "blocked_reasons": [],
        "notes": [
            "AEX loader approval receipt validation reads JSON metadata only.",
            "An accepted receipt approves only a separate loader implementation review slice.",
            "This validator does not enable native AEX loading, worker plug-in loading, render, or OFX routing.",
            "Private plug-in paths and AEX static metadata labels must not be serialized in approval receipts or reports."
        ]
    }))
    .unwrap()
}

fn ready_aex_metadata_gate_report_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "report_kind": "aex_metadata_gate_integration",
        "publication_status": "local-only",
        "status": "aex_metadata_gate_ready_no_load",
        "native_load_performed": false,
        "render_performed": false,
        "aex_loaded": false,
        "worker_started": false,
        "broker_load_or_render_allowed": false,
        "ofx_route_allowed": false,
        "ae_invoked": false,
        "private_payload_copied": false,
        "fixture_gate_summary": {
            "provided": true,
            "schema_version": 1,
            "status": "review_queue_not_approved",
            "selected_fixture_present": false,
            "approval_approved": false,
            "approval_loader_enabled": false,
            "approval_real_aex_load_enabled": false,
            "approval_render_png_enabled": false,
            "approval_describe_enabled_for_real_aex": false,
            "candidate_count": 2,
            "not_approved_candidate_count": 2
        },
        "readiness_summary": {
            "provided": true,
            "schema_version": 1,
            "status": "probe_readiness_planned",
            "candidate_count": 2,
            "allowlist_entry_count": 1,
            "draft_describe_entry_count": 1,
            "blocked_or_deferred_entry_count": 1
        },
        "loader_gate_summary": {
            "provided": true,
            "schema_version": 1,
            "status": "loader_gate_not_opened",
            "approved": false,
            "loader_enabled": false,
            "real_aex_load_enabled": false,
            "open_candidate_count": 0,
            "entry_count": 2,
            "not_approved_entry_count": 2,
            "describe_only_entry_count": 2,
            "deferred_ofx_entry_count": 2
        },
        "synthetic_fixture_summary": {
            "provided": true,
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
        },
        "identity_smoke_summary": {
            "provided": true,
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
        },
        "ofx_contract_summary": {
            "provided": true,
            "schema_version": 1,
            "status": "deferred-contract-only",
            "ofx_host_may_load_aex": false,
            "ofx_adapter_may_load_aex": false,
            "broker_may_load_aex": false,
            "review_approved": false,
            "may_point_to_broker": false,
            "may_issue_describe": false,
            "may_issue_render_png": false
        },
        "checks": [
            {"name": "fixture_gate_closed_no_selection", "status": "passed"},
            {"name": "readiness_metadata_only_describe_planning", "status": "passed"},
            {"name": "loader_gate_closed_no_load", "status": "passed"},
            {"name": "synthetic_fixture_manifest_no_load", "status": "passed"},
            {"name": "identity_smoke_transport_only_no_load", "status": "passed"},
            {"name": "ofx_facade_deferred_no_bypass", "status": "passed"}
        ],
        "blocked_reasons": [],
        "next_action": "metadata_only_parent_review",
        "notes": [
            "This report integrates metadata-only AEX gate evidence for parent orchestration.",
            "It does not open, copy, hash, load, describe, execute, render, or route any .aex file.",
            "Identity transport over synthetic PNG fixtures is not AEX render correctness evidence.",
            "OFX remains deferred and cannot bypass the AEX fixture, loader, worker, or sandbox gates."
        ]
    }))
    .unwrap()
}

fn native_stage_plan_generated_by_stage_planner_json() -> String {
    aex_native_stage_plan::plan_native_stage_contract_json_with_boundary_schema(
        &ready_loader_manifest_json(),
        &accepted_worker_loader_ticket_json(),
        Some(BOUNDARY_SCHEMA),
    )
    .expect("native stage planner should emit a ready no-load plan")
}

fn ready_loader_manifest_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "ready_for_separate_loader_implementation_review_no_load",
        "native_load_performed": false,
        "broker_may_load_plugin": false,
        "loader_may_load_plugin": false,
        "ofx_may_route_to_loader": false,
        "selected_effect_id": "adaptivefilter-local",
        "selected_plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
        "normalized_plugin_path": "d:\\aviutlas\\local\\adaptivefilter.aex",
        "implementation_gate": {
            "ready_for_separate_loader_slice_review": true,
            "native_loader_calls_allowed": false,
            "broker_may_load_aex": false,
            "ofx_facade_may_route_to_loader": false
        },
        "readiness_summary": {
            "provided": true,
            "matched_entry_count": 1,
            "status": "probe_readiness_planned",
            "entry_status": "draft_allowlisted",
            "pipl_content_scan_status": "semantic_matches",
            "pipl_content_scan_ready": true,
            "allowed_operations": ["describe"]
        },
        "blocked_reasons": []
    }))
    .unwrap()
}

fn accepted_worker_loader_ticket_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "ticket_protocol_version": 1,
        "generated_by": "aex_image_probe",
        "publication_status": "local-only",
        "status": "accepted_no_load",
        "native_load_performed": false,
        "worker_may_load_plugin": false,
        "broker_may_load_plugin": false,
        "allowlist_id": "adaptivefilter-local",
        "operation": "render_png",
        "selected_loader_entry": {
            "effect_id": "adaptivefilter-local",
            "normalized_plugin_path": "d:\\aviutlas\\local\\adaptivefilter.aex",
            "path_match_status": "matched_normalized_path",
            "allowlist_operation_status": "render_png",
            "entry_ready": true
        },
        "required_runtime_evidence": {
            "worker_identity_revalidation_required": "passed",
            "worker_attestation_required": "passed",
            "sandbox_preflight_required": "passed",
            "job_object_required": "assigned-with-kill-on-close",
            "handle_inheritance_required": "sentinel_not_inherited-with-explicit-handle-list"
        },
        "planned_stages": [
            {"stage": "load", "status": "planned_not_run"},
            {"stage": "global_setup", "status": "planned_not_run"},
            {"stage": "params_setup", "status": "planned_not_run"},
            {"stage": "sequence_setup", "status": "planned_not_run"},
            {"stage": "render", "status": "planned_not_run"},
            {"stage": "sequence_teardown", "status": "planned_not_run"},
            {"stage": "global_teardown", "status": "planned_not_run"}
        ]
    }))
    .unwrap()
}

fn closed_loader_gate_json() -> String {
    loader_gate_json(false, false, false, 0)
}

fn open_loader_gate_json() -> String {
    loader_gate_json(true, true, true, 1)
}

fn partial_open_loader_gate_json() -> String {
    loader_gate_json(false, false, false, 1)
}

fn bypass_contract_json() -> &'static str {
    r#"{
      "schema_version": 1,
      "status": "active",
      "route": {
        "aviutlas_to_aex_route": "aviutlas-through-ofx",
        "ofx_host_may_load_aex": true,
        "ofx_adapter_may_load_aex": true,
        "broker_may_load_aex": true,
        "worker_required": true,
        "worker_protocol": "aex-image-probe/broker-worker-v0",
        "first_loader_owner": "ofx-facade"
      },
      "ofx_facade_review_gate": {
        "status": "approved",
        "approved": true,
        "may_point_to_broker": true,
        "may_issue_describe": true,
        "may_issue_render_png": true,
        "requires_separate_review_after_aex_loader_gate": true,
        "requires_capability_ids_subset_of_loader_gate": true,
        "requires_same_shared_gates": true
      },
      "forbidden_paths": [
        "OFX host process loads .aex",
        "OFX adapter process loads .aex",
        "AviUtlas routes through OFX to reach AEX",
        "OFX adapter bypasses AEX allowlist"
      ],
      "current_supported_operations": []
    }"#
}

fn loader_gate_json(
    approved: bool,
    loader_enabled: bool,
    real_aex_load_enabled: bool,
    open_candidate_count: u32,
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
              "ofx_facade_status": "deferred-same-broker-worker-contract"
            }}
          ]
        }}"#
    )
}

fn closed_capability_json() -> &'static str {
    r#"{
      "schema_version": 1,
      "effect_id": "adaptivefilter-local",
      "display_name": "AdaptiveFilter",
      "publication_status": "local-only",
      "evidence_mode": "static-classifier-metadata-only",
      "load_status": "not_loaded",
      "broker_may_load_plugin": false,
      "current_supported_operations": [],
      "params_status": "unknown",
      "params": [],
      "selectors": [],
      "aex_worker": {"supported": false, "status": "deferred_loader_gate_closed"},
      "ofx_facade": {"supported": false, "status": "deferred_same_aex_worker_gate"}
    }"#
}

fn premature_capability_json() -> &'static str {
    r#"{
      "schema_version": 1,
      "effect_id": "adaptivefilter-local",
      "display_name": "AdaptiveFilter",
      "publication_status": "local-only",
      "evidence_mode": "static-classifier-metadata-only",
      "load_status": "not_loaded",
      "broker_may_load_plugin": false,
      "current_supported_operations": ["render_png"],
      "params_status": "unknown",
      "params": [],
      "selectors": [],
      "aex_worker": {"supported": false, "status": "deferred_loader_gate_closed"},
      "ofx_facade": {"supported": true, "status": "premature"}
    }"#
}

fn mismatched_capability_json() -> &'static str {
    r#"{
      "schema_version": 1,
      "effect_id": "not-in-loader-gate",
      "display_name": "NotInLoaderGate",
      "publication_status": "local-only",
      "evidence_mode": "static-classifier-metadata-only",
      "load_status": "not_loaded",
      "broker_may_load_plugin": false,
      "current_supported_operations": [],
      "params_status": "unknown",
      "params": [],
      "selectors": [],
      "aex_worker": {"supported": false, "status": "deferred_loader_gate_closed"},
      "ofx_facade": {"supported": false, "status": "deferred_same_aex_worker_gate"}
    }"#
}
