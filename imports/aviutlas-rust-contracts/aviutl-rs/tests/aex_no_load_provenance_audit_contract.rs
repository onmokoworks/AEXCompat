#[allow(dead_code)]
#[path = "../examples/aex_native_stage_plan.rs"]
mod aex_native_stage_plan;
#[allow(dead_code)]
#[path = "../examples/aex_no_load_provenance_audit.rs"]
mod aex_no_load_provenance_audit;
#[allow(dead_code)]
#[path = "../examples/ofx_aex_facade_readiness.rs"]
mod ofx_aex_facade_readiness;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const AUDIT_SCHEMA: &str =
    include_str!("../../analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json");
const BOUNDARY_SCHEMA: &str =
    include_str!("../../analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json");
const CONTRACT: &str = include_str!("../../analysis/OFX_AEX_FACADE_CONTRACT_2026-05-31.json");
const FIXTURE_GATE: &str = include_str!("../../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json");

fn audit(loader_manifest: &str, native_stage_plan: &str, ofx_readiness: &str) -> Value {
    let output = aex_no_load_provenance_audit::audit_no_load_provenance_json(
        loader_manifest,
        native_stage_plan,
        ofx_readiness,
    )
    .expect("no-load provenance audit should run");
    serde_json::from_str(&output).expect("audit should emit JSON")
}

fn audit_with_fixture_identity_smoke(
    loader_manifest: &str,
    native_stage_plan: &str,
    ofx_readiness: &str,
    fixture_identity_smoke: &str,
) -> Value {
    let output =
        aex_no_load_provenance_audit::audit_no_load_provenance_json_with_fixture_identity_smoke(
            loader_manifest,
            native_stage_plan,
            ofx_readiness,
            Some(fixture_identity_smoke),
        )
        .expect("no-load provenance audit should run with fixture identity smoke");
    serde_json::from_str(&output).expect("audit should emit JSON")
}

fn schema() -> Value {
    serde_json::from_str(AUDIT_SCHEMA).expect("audit schema should parse")
}

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-no-load-provenance-audit")
        .join(format!("{}-{name}", std::process::id()))
}

fn unique_target_path(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    target_path(&format!("{stamp}-{name}"))
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
        &report["loader_manifest_summary"],
        &json_string_array(&schema["loader_manifest_summary_required_fields"]),
        "loader_manifest_summary",
    );
    assert_object_has_fields(
        &report["native_stage_plan_summary"],
        &json_string_array(&schema["native_stage_plan_summary_required_fields"]),
        "native_stage_plan_summary",
    );
    assert_object_has_fields(
        &report["ofx_readiness_summary"],
        &json_string_array(&schema["ofx_readiness_summary_required_fields"]),
        "ofx_readiness_summary",
    );
    assert_object_has_fields(
        &report["fixture_identity_smoke_summary"],
        &json_string_array(&schema["fixture_identity_smoke_summary_required_fields"]),
        "fixture_identity_smoke_summary",
    );
    if report["loader_manifest_summary"]["fixture_refresh_audit_provided"]
        .as_bool()
        .expect("loader fixture refresh provided should be bool")
    {
        for (field, expected) in schema["fixture_refresh_audit_summary_ready_values_when_provided"]
            .as_object()
            .expect("fixture refresh ready values should be object")
        {
            assert_eq!(
                &report["loader_manifest_summary"][field], expected,
                "loader fixture refresh summary field {field} diverged"
            );
            assert_eq!(
                &report["native_stage_plan_summary"][field], expected,
                "native fixture refresh summary field {field} diverged"
            );
        }
    }
    if report["fixture_identity_smoke_summary"]["provided"]
        .as_bool()
        .expect("fixture identity smoke provided should be bool")
    {
        for (field, expected) in schema["fixture_identity_smoke_summary_ready_values_when_provided"]
            .as_object()
            .expect("fixture identity smoke ready values should be object")
        {
            assert_eq!(
                &report["fixture_identity_smoke_summary"][field], expected,
                "fixture identity smoke summary field {field} diverged"
            );
        }
    }

    for (field, expected) in schema["required_values"]
        .as_object()
        .expect("required_values should be object")
    {
        assert_eq!(
            &report[field], expected,
            "audit field {field} diverged from schema"
        );
    }
    assert!(
        json_string_array(&schema["allowed_statuses"]).contains(
            &report["status"]
                .as_str()
                .expect("audit status should be string")
        ),
        "unexpected audit status"
    );
    for name in json_string_array(&schema["required_check_names"]) {
        assert!(
            check_status(report, name, "passed") || check_status(report, name, "blocked"),
            "audit missing required check {name}"
        );
    }
    for name in json_string_array(&schema["conditional_check_names"]) {
        if check_status(report, name, "passed") || check_status(report, name, "blocked") {
            continue;
        }
        if name == "fixture_refresh_audit_preserved_no_load" {
            assert_eq!(
                report["loader_manifest_summary"]["fixture_refresh_audit_provided"], false,
                "fixture refresh audit check may be absent only when summary is absent"
            );
            assert_eq!(
                report["native_stage_plan_summary"]["fixture_refresh_audit_provided"], false,
                "fixture refresh audit check may be absent only when summary is absent"
            );
        } else if name == "fixture_identity_smoke_ready_no_load" {
            assert_eq!(
                report["fixture_identity_smoke_summary"]["provided"], false,
                "fixture identity smoke check may be absent only when summary is absent"
            );
        }
    }
    for check in report["checks"].as_array().expect("checks should be array") {
        assert_object_has_fields(check, &["name", "status", "evidence"], "check");
        let status = check["status"].as_str().unwrap();
        assert!(
            json_string_array(&schema["check_statuses"]).contains(&status),
            "unexpected check status {status}"
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
    for (field, expected) in schema["loader_manifest_summary_required_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["loader_manifest_summary"][field], expected,
            "loader manifest summary field {field} diverged"
        );
    }
    for (field, expected) in schema["native_stage_plan_summary_required_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["native_stage_plan_summary"][field], expected,
            "native stage summary field {field} diverged"
        );
    }
    for (field, expected) in schema["ofx_readiness_summary_required_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["ofx_readiness_summary"][field], expected,
            "OFX readiness summary field {field} diverged"
        );
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
fn ready_provenance_chain_is_metadata_only_and_schema_bound() {
    let loader_manifest = ready_loader_manifest_json();
    let native_stage_plan = ready_native_stage_plan_json(&loader_manifest);
    let ofx_readiness = ready_ofx_readiness_json(&native_stage_plan);
    let report = audit(&loader_manifest, &native_stage_plan, &ofx_readiness);
    let schema = schema();

    assert_audit_matches_schema(&report, &schema);
    assert_ready_summary_values(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["selectors_executed"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["ofx_route_allowed"], false);
    assert_eq!(report["evidence_contains_forbidden_tokens"], false);
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));
    assert_eq!(
        report["native_stage_plan_summary"]["native_stage_count"],
        report["native_stage_plan_summary"]["planned_not_run_count"]
    );
    assert!(
        report["native_stage_plan_summary"]["native_stage_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        report["loader_manifest_summary"]["fixture_refresh_audit_provided"],
        false
    );
    assert_eq!(
        report["native_stage_plan_summary"]["fixture_refresh_audit_provided"],
        false
    );
    assert_eq!(report["fixture_identity_smoke_summary"]["provided"], false);
}

#[test]
fn audit_preserves_fixture_refresh_audit_when_present() {
    let loader_manifest = ready_loader_manifest_with_fixture_refresh_audit_json();
    let native_stage_plan = ready_native_stage_plan_json(&loader_manifest);
    let ofx_readiness = ready_ofx_readiness_json(&native_stage_plan);
    let report = audit(&loader_manifest, &native_stage_plan, &ofx_readiness);
    let schema = schema();

    assert_audit_matches_schema(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert_eq!(
        report["loader_manifest_summary"]["fixture_refresh_audit_provided"],
        true
    );
    assert_eq!(
        report["native_stage_plan_summary"]["fixture_refresh_audit_provided"],
        true
    );
    assert_eq!(
        report["loader_manifest_summary"]["fixture_refresh_audit_status"],
        "fixture_gate_refresh_ready_no_load"
    );
    assert_eq!(
        report["native_stage_plan_summary"]["fixture_refresh_audit_status"],
        "fixture_gate_refresh_ready_no_load"
    );
    assert_eq!(
        report["loader_manifest_summary"]
            ["fixture_refresh_audit_wiztree_canonical_non_generated_count"],
        40
    );
    assert_eq!(
        report["native_stage_plan_summary"]
            ["fixture_refresh_audit_wiztree_generated_target_artifact_count"],
        79
    );
    assert!(check_status(
        &report,
        "fixture_refresh_audit_preserved_no_load",
        "passed"
    ));
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["ofx_route_allowed"], false);
}

#[test]
fn audit_preserves_fixture_identity_smoke_as_sanitized_optional_evidence() {
    let loader_manifest = ready_loader_manifest_json();
    let native_stage_plan = ready_native_stage_plan_json(&loader_manifest);
    let ofx_readiness = ready_ofx_readiness_json(&native_stage_plan);
    let fixture_identity_smoke = ready_fixture_identity_smoke_json();
    let report = audit_with_fixture_identity_smoke(
        &loader_manifest,
        &native_stage_plan,
        &ofx_readiness,
        &fixture_identity_smoke,
    );
    let schema = schema();

    assert_audit_matches_schema(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert_eq!(report["fixture_identity_smoke_summary"]["provided"], true);
    assert_eq!(
        report["fixture_identity_smoke_summary"]["status"],
        "fixture_identity_smoke_ready_no_load"
    );
    assert_eq!(
        report["fixture_identity_smoke_summary"]["transport_operation"],
        "identity_transport"
    );
    assert_eq!(
        report["fixture_identity_smoke_summary"]["expected_synthetic_image_set"],
        true
    );
    assert_eq!(
        report["fixture_identity_smoke_summary"]["all_entries_identity_pixels_match"],
        true
    );
    assert_eq!(
        report["fixture_identity_smoke_summary"]["aex_render_correctness_evidence"],
        false
    );
    assert!(check_status(
        &report,
        "fixture_identity_smoke_ready_no_load",
        "passed"
    ));
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("input_png"));
    assert!(!serialized.contains("output_png"));
}

#[test]
fn audit_blocks_fixture_identity_smoke_render_or_entry_drift_without_echo() {
    let loader_manifest = ready_loader_manifest_json();
    let native_stage_plan = ready_native_stage_plan_json(&loader_manifest);
    let ofx_readiness = ready_ofx_readiness_json(&native_stage_plan);
    let mut fixture_identity_smoke: Value =
        serde_json::from_str(&ready_fixture_identity_smoke_json()).unwrap();
    fixture_identity_smoke["aex_render_correctness_evidence"] = json!(true);
    fixture_identity_smoke["entries"][0]["identity_pixels_match"] = json!(false);
    fixture_identity_smoke["checks"][0]["status"] = json!("blocked");
    fixture_identity_smoke["worker_exe"] = json!("target/aex-image-probe/test-workers/stub.exe");
    let fixture_identity_smoke = fixture_identity_smoke.to_string();
    let report = audit_with_fixture_identity_smoke(
        &loader_manifest,
        &native_stage_plan,
        &ofx_readiness,
        &fixture_identity_smoke,
    );

    assert_eq!(report["status"], "blocked_no_load_provenance_chain");
    assert!(check_status(
        &report,
        "fixture_identity_smoke_ready_no_load",
        "blocked"
    ));
    assert_eq!(
        report["fixture_identity_smoke_summary"]["aex_render_correctness_evidence"],
        true
    );
    assert_eq!(
        report["fixture_identity_smoke_summary"]["all_entries_identity_pixels_match"],
        false
    );
    assert_eq!(
        report["fixture_identity_smoke_summary"]["all_checks_passed"],
        false
    );
    assert_eq!(
        report["fixture_identity_smoke_summary"]["input_contains_forbidden_tokens"],
        true
    );
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["ofx_route_allowed"], false);
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("input_png"));
    assert!(!serialized.contains("output_png"));
    assert!(!serialized.contains("worker_exe"));
}

#[test]
fn audit_blocks_fixture_refresh_audit_drift_without_opening_boundaries() {
    let loader_manifest = ready_loader_manifest_with_fixture_refresh_audit_json();
    let mut native_stage_plan: Value =
        serde_json::from_str(&ready_native_stage_plan_json(&loader_manifest)).unwrap();
    native_stage_plan["manifest_fixture_refresh_audit_summary"]["loader_enabled"] = json!(true);
    native_stage_plan["manifest_fixture_refresh_audit_summary"]
        ["input_contains_forbidden_tokens"] = json!(true);
    let native_stage_plan = native_stage_plan.to_string();
    let ofx_readiness = ready_ofx_readiness_json(&native_stage_plan);
    let report = audit(&loader_manifest, &native_stage_plan, &ofx_readiness);

    assert_eq!(report["status"], "blocked_no_load_provenance_chain");
    assert!(check_status(
        &report,
        "fixture_refresh_audit_preserved_no_load",
        "blocked"
    ));
    assert_eq!(
        report["native_stage_plan_summary"]["fixture_refresh_audit_loader_enabled"],
        true
    );
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["selectors_executed"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["ofx_route_allowed"], false);
}

#[test]
fn audit_blocks_ofx_readiness_without_native_stage_summary() {
    let loader_manifest = ready_loader_manifest_json();
    let native_stage_plan = ready_native_stage_plan_json(&loader_manifest);
    let ofx_readiness = ofx_readiness_without_native_stage_plan_json();
    let report = audit(&loader_manifest, &native_stage_plan, &ofx_readiness);

    assert_eq!(report["status"], "blocked_no_load_provenance_chain");
    assert!(check_status(
        &report,
        "ofx_readiness_deferred_no_bypass",
        "passed"
    ));
    assert!(check_status(
        &report,
        "ofx_readiness_consumed_native_stage_plan",
        "blocked"
    ));
    assert_eq!(
        report["ofx_readiness_summary"]["native_stage_plan_summary_provided"],
        false
    );
    assert!(contains_reason(
        &report,
        "OFX readiness did not consume the ready native stage plan summary"
    ));
}

#[test]
fn audit_blocks_native_selector_or_route_claims() {
    let loader_manifest = ready_loader_manifest_json();
    let mut native_stage_plan: Value =
        serde_json::from_str(&ready_native_stage_plan_json(&loader_manifest)).unwrap();
    native_stage_plan["selectors_executed"] = json!(true);
    native_stage_plan["promotion_gate"]["worker_selector_calls_allowed"] = json!(true);
    native_stage_plan["ofx_may_route_to_loader"] = json!(true);
    native_stage_plan["promotion_gate"]["ofx_facade_may_route_to_loader"] = json!(true);
    let native_stage_plan = native_stage_plan.to_string();
    let ofx_readiness = ready_ofx_readiness_json(&native_stage_plan);
    let report = audit(&loader_manifest, &native_stage_plan, &ofx_readiness);

    assert_eq!(report["status"], "blocked_no_load_provenance_chain");
    assert!(check_status(
        &report,
        "native_stage_plan_ready_no_load",
        "blocked"
    ));
    assert!(check_status(
        &report,
        "ofx_readiness_deferred_no_bypass",
        "blocked"
    ));
    assert_eq!(
        report["native_stage_plan_summary"]["selectors_executed"],
        true
    );
    assert_eq!(
        report["ofx_readiness_summary"]["native_stage_selector_execution_blocked"],
        false
    );
    assert_eq!(
        report["ofx_readiness_summary"]["native_stage_ofx_route_blocked"],
        false
    );
}

#[test]
fn audit_blocks_contaminated_inputs_without_echo() {
    let loader_manifest = ready_loader_manifest_json();
    let mut native_stage_plan: Value =
        serde_json::from_str(&ready_native_stage_plan_json(&loader_manifest)).unwrap();
    native_stage_plan["status"] = json!("output_png");
    let native_stage_plan = native_stage_plan.to_string();
    let ofx_readiness = ready_ofx_readiness_json(&native_stage_plan);
    let report = audit(&loader_manifest, &native_stage_plan, &ofx_readiness);

    assert_eq!(report["status"], "blocked_no_load_provenance_chain");
    assert_eq!(report["evidence_contains_forbidden_tokens"], true);
    assert!(check_status(
        &report,
        "evidence_anti_contamination",
        "blocked"
    ));
    assert!(report["native_stage_plan_summary"]["status"].is_null());
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("output_png"));
}

#[test]
fn audit_blocks_loader_manifest_without_readiness_pipl_gate() {
    let mut loader_manifest: Value = serde_json::from_str(&ready_loader_manifest_json()).unwrap();
    loader_manifest["readiness_summary"]["provided"] = json!(false);
    loader_manifest["readiness_summary"]["matched_entry_count"] = json!(0);
    loader_manifest["readiness_summary"]["pipl_content_scan_ready"] = json!(false);
    let loader_manifest = loader_manifest.to_string();
    let native_stage_plan = ready_native_stage_plan_json(&ready_loader_manifest_json());
    let ofx_readiness = ready_ofx_readiness_json(&native_stage_plan);
    let report = audit(&loader_manifest, &native_stage_plan, &ofx_readiness);

    assert_eq!(report["status"], "blocked_no_load_provenance_chain");
    assert!(check_status(
        &report,
        "loader_manifest_ready_no_load",
        "blocked"
    ));
    assert_eq!(
        report["loader_manifest_summary"]["readiness_provided"],
        false
    );
    assert!(contains_reason(
        &report,
        "loader implementation manifest is not a complete ready no-load provenance packet"
    ));
}

#[test]
fn audit_validates_report_output_policy() {
    let valid_report = target_path("provenance-audit.local.json");
    assert!(
        aex_no_load_provenance_audit::validate_no_load_provenance_audit_report_output_path(
            &valid_report
        )
        .is_ok()
    );

    let non_json_report = target_path("provenance-audit.local.txt");
    assert!(
        aex_no_load_provenance_audit::validate_no_load_provenance_audit_report_output_path(
            &non_json_report
        )
        .is_err()
    );

    let traversal_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-no-load-provenance-audit")
        .join("..")
        .join("private-provenance-audit.json");
    assert!(
        aex_no_load_provenance_audit::validate_no_load_provenance_audit_report_output_path(
            &traversal_report
        )
        .is_err()
    );

    let outside_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("outside-aex-no-load-provenance-audit.json");
    assert!(
        aex_no_load_provenance_audit::validate_no_load_provenance_audit_report_output_path(
            &outside_report
        )
        .is_err()
    );
    assert!(!traversal_report.exists());
    assert!(!outside_report.exists());
}

#[test]
fn audit_report_writer_uses_create_new() {
    let report = unique_target_path("provenance-create-new.local.json");
    aex_no_load_provenance_audit::write_no_load_provenance_audit_report_create_new(
        &report,
        "{\"first\":true}",
    )
    .expect("first provenance audit report write should succeed");

    let err = aex_no_load_provenance_audit::write_no_load_provenance_audit_report_create_new(
        &report,
        "{\"second\":true}",
    )
    .expect_err("second provenance audit report write should use create_new and fail");
    assert!(
        err.to_string().contains("already exists"),
        "unexpected create-new error: {err}"
    );

    let contents = std::fs::read_to_string(&report)
        .expect("created provenance audit report should be readable");
    assert!(contents.contains("\"first\":true"));
    assert!(!contents.contains("\"second\":true"));
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

fn ready_loader_manifest_with_fixture_refresh_audit_json() -> String {
    let mut manifest: Value = serde_json::from_str(&ready_loader_manifest_json()).unwrap();
    manifest["preflight_summary"] = json!({
        "fixture_refresh_audit_summary": {
            "provided": true,
            "schema_version": 1,
            "publication_status": "local-only",
            "status": "fixture_gate_refresh_ready_no_load",
            "native_load_performed": false,
            "render_performed": false,
            "fixture_selected": false,
            "loader_enabled": false,
            "fixture_gate_candidate_count": 2,
            "wiztree_total_aex_count": 119,
            "wiztree_canonical_non_generated_count": 40,
            "wiztree_generated_target_artifact_count": 79,
            "generated_target_artifacts_excluded": true,
            "candidates_present_in_refresh": true,
            "input_contains_forbidden_tokens": false,
            "blocked_reason_count": 0
        }
    });
    manifest["checks"] = json!([
        {"name": "fixture_gate_refresh_audit_ready_no_load", "status": "passed", "evidence": "provided=true, status=fixture_gate_refresh_ready_no_load"}
    ]);
    serde_json::to_string_pretty(&manifest).unwrap()
}

fn ready_fixture_identity_smoke_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "generated_by": "aex_probe_fixture_identity_smoke",
        "generated_unix_ms": 1,
        "publication_status": "local-only",
        "status": "fixture_identity_smoke_ready_no_load",
        "fixture_manifest": "target/aex-probe-fixtures/manifest.local.json",
        "fixture_manifest_status": "synthetic_fixture_images_ready_no_load",
        "output_root": "target/aex-image-probe/fixture-identity-smoke",
        "transport_operation": "identity_transport",
        "pixel_format": "rgba8",
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
        "aex_render_correctness_evidence": false,
        "entries": [
            {
                "id": "gradient",
                "pattern": "xy-gradient-rgba8",
                "input_png": "target/aex-probe-fixtures/gradient_rgba8.png",
                "output_png": "target/aex-image-probe/fixture-identity-smoke/gradient_identity_rgba8.png",
                "width": 64,
                "height": 64,
                "pixel_format": "rgba8",
                "transport_status": "ok",
                "plugin_class": "identity-transport",
                "identity_pixels_match": true,
                "worker_started": false,
                "aex_loaded": false,
                "render_performed": false
            },
            {
                "id": "checker",
                "pattern": "checker-rgba8",
                "input_png": "target/aex-probe-fixtures/checker_rgba8.png",
                "output_png": "target/aex-image-probe/fixture-identity-smoke/checker_identity_rgba8.png",
                "width": 64,
                "height": 64,
                "pixel_format": "rgba8",
                "transport_status": "ok",
                "plugin_class": "identity-transport",
                "identity_pixels_match": true,
                "worker_started": false,
                "aex_loaded": false,
                "render_performed": false
            },
            {
                "id": "solid_alpha",
                "pattern": "solid-alpha-rgba8",
                "input_png": "target/aex-probe-fixtures/solid_alpha_rgba8.png",
                "output_png": "target/aex-image-probe/fixture-identity-smoke/solid_alpha_identity_rgba8.png",
                "width": 64,
                "height": 64,
                "pixel_format": "rgba8",
                "transport_status": "ok",
                "plugin_class": "identity-transport",
                "identity_pixels_match": true,
                "worker_started": false,
                "aex_loaded": false,
                "render_performed": false
            }
        ],
        "checks": [
            {"name": "fixture_manifest_validated", "status": "passed", "evidence": "synthetic fixture manifest accepted"},
            {"name": "identity_transport_ok", "status": "passed", "evidence": "all fixture images completed broker identity_transport"},
            {"name": "rgba_identity_pixels_match", "status": "passed", "evidence": "decoded input and output RGBA pixels matched for every image"},
            {"name": "synthetic_fixture_pixels_match", "status": "passed", "evidence": "decoded input RGBA pixels matched generated patterns"},
            {"name": "no_aex_input", "status": "passed", "evidence": "no AEX path is read or required"},
            {"name": "no_worker_or_host_invocation", "status": "passed", "evidence": "no worker, OFX, or AE process is started"},
            {"name": "not_render_correctness_evidence", "status": "passed", "evidence": "identity transport is not AEX render correctness evidence"}
        ],
        "notes": [
            "This smoke verifies broker PNG identity transport over synthetic inputs only.",
            "It is not AEX rendering, parameter discovery, loader approval, or OFX routing evidence.",
            "No AEX file is opened, copied, loaded, described, or rendered."
        ]
    }))
    .unwrap()
}

fn ready_native_stage_plan_json(loader_manifest: &str) -> String {
    aex_native_stage_plan::plan_native_stage_contract_json_with_boundary_schema(
        loader_manifest,
        &accepted_worker_loader_ticket_json(),
        Some(BOUNDARY_SCHEMA),
    )
    .expect("native stage planner should emit ready no-load JSON")
}

fn ready_ofx_readiness_json(native_stage_plan: &str) -> String {
    ofx_aex_facade_readiness::plan_ofx_facade_readiness_json_with_native_stage_plan(
        CONTRACT,
        FIXTURE_GATE,
        &closed_loader_gate_json(),
        Some(native_stage_plan),
        &[closed_capability_json()],
    )
    .expect("OFX readiness planner should emit JSON")
}

fn ofx_readiness_without_native_stage_plan_json() -> String {
    ofx_aex_facade_readiness::plan_ofx_facade_readiness_json(
        CONTRACT,
        FIXTURE_GATE,
        &closed_loader_gate_json(),
        &[closed_capability_json()],
    )
    .expect("OFX readiness planner should emit JSON")
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
    r#"{
      "schema_version": 1,
      "status": "loader_gate_not_opened",
      "approved": false,
      "loader_enabled": false,
      "real_aex_load_enabled": false,
      "open_candidate_count": 0,
      "entries": [
        {
          "effect_id": "adaptivefilter-local",
          "ofx_facade_status": "deferred-same-broker-worker-contract"
        }
      ]
    }"#
    .to_string()
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
