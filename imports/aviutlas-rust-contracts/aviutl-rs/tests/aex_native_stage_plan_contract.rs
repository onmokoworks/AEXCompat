#[allow(dead_code)]
#[path = "../examples/aex_native_stage_plan.rs"]
mod aex_native_stage_plan;

use serde_json::json;
use serde_json::Value;

const NATIVE_STAGE_PLAN_SCHEMA: &str =
    include_str!("../../analysis/AEX_NATIVE_STAGE_PLAN_SCHEMA_2026-06-01.json");
const BOUNDARY_SCHEMA: &str =
    include_str!("../../analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json");

fn plan(manifest: &str, ticket: &str) -> Value {
    let output = aex_native_stage_plan::plan_native_stage_contract_json(manifest, ticket)
        .expect("native stage plan should run");
    serde_json::from_str(&output).expect("native stage plan should emit JSON")
}

fn plan_with_boundary(manifest: &str, ticket: &str, boundary: &str) -> Value {
    let output = aex_native_stage_plan::plan_native_stage_contract_json_with_boundary_schema(
        manifest,
        ticket,
        Some(boundary),
    )
    .expect("native stage plan with boundary should run");
    serde_json::from_str(&output).expect("native stage plan should emit JSON")
}

fn schema() -> Value {
    serde_json::from_str(NATIVE_STAGE_PLAN_SCHEMA).expect("native stage plan schema should parse")
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

fn assert_plan_matches_schema(report: &Value, schema: &Value) {
    assert_object_has_fields(
        report,
        &json_string_array(&schema["required_fields"]),
        "plan",
    );
    assert_object_has_fields(
        &report["manifest_summary"],
        &json_string_array(&schema["manifest_summary_required_fields"]),
        "manifest_summary",
    );
    assert_object_has_fields(
        &report["manifest_readiness_summary"],
        &json_string_array(&schema["manifest_readiness_summary_required_fields"]),
        "manifest_readiness_summary",
    );
    assert_object_has_fields(
        &report["manifest_fixture_refresh_audit_summary"],
        &json_string_array(&schema["manifest_fixture_refresh_audit_summary_required_fields"]),
        "manifest_fixture_refresh_audit_summary",
    );
    if report["manifest_fixture_refresh_audit_summary"]["provided"]
        .as_bool()
        .expect("manifest fixture refresh provided should be bool")
    {
        for (field, expected) in schema
            ["manifest_fixture_refresh_audit_summary_ready_values_when_provided"]
            .as_object()
            .expect("fixture refresh ready values should be object")
        {
            assert_eq!(
                &report["manifest_fixture_refresh_audit_summary"][field], expected,
                "manifest fixture refresh summary field {field} diverged"
            );
        }
    }
    if let Some(boundary_summary) = report.get("cleanroom_boundary_summary") {
        assert_object_has_fields(
            boundary_summary,
            &json_string_array(&schema["cleanroom_boundary_summary_required_fields_when_present"]),
            "cleanroom_boundary_summary",
        );
        for (field, expected) in schema["cleanroom_boundary_summary_required_values_when_present"]
            .as_object()
            .unwrap()
        {
            assert_eq!(
                &boundary_summary[field], expected,
                "cleanroom boundary summary field {field} diverged from schema"
            );
        }
    }
    assert_object_has_fields(
        &report["ticket_summary"],
        &json_string_array(&schema["ticket_summary_required_fields"]),
        "ticket_summary",
    );
    assert_object_has_fields(
        &report["ticket_runtime_evidence_summary"],
        &json_string_array(&schema["ticket_runtime_evidence_summary_required_fields"]),
        "ticket_runtime_evidence_summary",
    );
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(
            &report[field], expected,
            "native stage plan field {field} diverged from schema"
        );
    }
    if report["status"] == schema["ready_status"] {
        for (field, expected) in schema["ticket_runtime_evidence_summary_required_values"]
            .as_object()
            .unwrap()
        {
            assert_eq!(
                &report["ticket_runtime_evidence_summary"][field], expected,
                "ready native stage plan runtime evidence field {field} diverged from schema"
            );
        }
    }
    assert!(
        json_string_array(&schema["allowed_statuses"])
            .contains(&report["status"].as_str().expect("status should be string")),
        "unexpected plan status"
    );
    for (field, expected) in schema["promotion_gate_required_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["promotion_gate"][field], expected,
            "promotion gate field {field} diverged from schema"
        );
    }
    for host in report["host_struct_plan"]
        .as_array()
        .expect("host_struct_plan should be an array")
    {
        assert_object_has_fields(
            host,
            &json_string_array(&schema["host_struct_plan_required_fields"]),
            "host_struct",
        );
        assert_eq!(
            host["allocation_status"],
            schema["host_struct_required_values"]["allocation_status"]
        );
        assert_eq!(
            host["mutation_status"],
            schema["host_struct_required_values"]["mutation_status"]
        );
    }
    for stage in report["native_stage_order"]
        .as_array()
        .expect("native_stage_order should be an array")
    {
        assert_object_has_fields(
            stage,
            &json_string_array(&schema["native_stage_required_fields"]),
            "native_stage",
        );
        assert_eq!(stage["status"], schema["native_stage_status"]);
    }
    for name in json_string_array(&schema["required_check_names"]) {
        assert!(
            check_status(report, name, "passed") || check_status(report, name, "blocked"),
            "plan missing required check {name}"
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
    for note in json_string_array(&schema["required_notes"]) {
        assert!(
            report["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == note),
            "plan missing required note {note}"
        );
    }
    let serialized = serde_json::to_string(report)
        .expect("plan should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "plan should not contain serialized token {token}"
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

fn stage_named<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["native_stage_order"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["name"] == name)
        .unwrap_or_else(|| panic!("missing native stage {name}"))
}

#[test]
fn ready_plan_maps_ticket_to_pf_selectors_but_runs_nothing() {
    let report = plan(&ready_manifest_json(), &accepted_ticket_json());
    let schema = schema();

    assert_plan_matches_schema(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["selectors_executed"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(report["worker_may_load_plugin"], false);
    assert_eq!(report["ofx_may_route_to_loader"], false);
    assert_eq!(
        report["promotion_gate"]["worker_selector_calls_allowed"],
        false
    );
    assert_eq!(report["selected_effect_id"], json!("adaptivefilter-local"));
    assert_eq!(report["manifest_readiness_summary"]["provided"], false);
    assert_eq!(
        report["manifest_fixture_refresh_audit_summary"]["provided"],
        false
    );
    assert_eq!(
        report["ticket_runtime_evidence_summary"]["worker_identity_revalidation_required"],
        "passed"
    );
    assert_eq!(
        report["ticket_runtime_evidence_summary"]["worker_attestation_required"],
        "passed"
    );
    assert_eq!(
        report["ticket_runtime_evidence_summary"]["sandbox_preflight_required"],
        "passed"
    );
    assert_eq!(
        report["ticket_runtime_evidence_summary"]["job_object_required"],
        "assigned-with-kill-on-close"
    );
    assert_eq!(
        report["ticket_runtime_evidence_summary"]["handle_inheritance_required"],
        "sentinel_not_inherited-with-explicit-handle-list"
    );
    assert!(
        report.get("cleanroom_boundary_summary").is_none(),
        "host boundary summary should be absent unless explicitly supplied"
    );
    assert_eq!(
        stage_named(&report, "global_setup")["pf_selector"],
        "PF_Cmd_GLOBAL_SETUP"
    );
    assert_eq!(
        stage_named(&report, "render")["pf_selector"],
        "PF_Cmd_RENDER"
    );
    assert_eq!(stage_named(&report, "render")["status"], "planned_not_run");
    assert_eq!(
        stage_named(&report, "frame_setup")["coarse_ticket_stage"],
        "render"
    );
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));
}

#[test]
fn ready_plan_preserves_manifest_fixture_refresh_audit_summary_when_provided() {
    let report = plan(
        &ready_manifest_with_fixture_refresh_audit_json(),
        &accepted_ticket_json(),
    );
    let schema = schema();

    assert_plan_matches_schema(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["selectors_executed"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["ofx_may_route_to_loader"], false);
    let summary = &report["manifest_fixture_refresh_audit_summary"];
    assert_eq!(summary["provided"], true);
    assert_eq!(summary["status"], "fixture_gate_refresh_ready_no_load");
    assert_eq!(summary["native_load_performed"], false);
    assert_eq!(summary["render_performed"], false);
    assert_eq!(summary["fixture_selected"], false);
    assert_eq!(summary["loader_enabled"], false);
    assert_eq!(summary["fixture_gate_candidate_count"], 2);
    assert_eq!(summary["wiztree_total_aex_count"], 119);
    assert_eq!(summary["wiztree_canonical_non_generated_count"], 40);
    assert_eq!(summary["wiztree_generated_target_artifact_count"], 79);
    assert_eq!(summary["generated_target_artifacts_excluded"], true);
    assert_eq!(summary["candidates_present_in_refresh"], true);
    assert_eq!(summary["input_contains_forbidden_tokens"], false);
    assert_eq!(summary["blocked_reason_count"], 0);
    assert!(check_status(
        &report,
        "manifest_fixture_refresh_audit_ready_no_load",
        "passed"
    ));
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn manifest_fixture_refresh_audit_summary_blocks_only_when_provided_and_invalid() {
    let mut manifest: Value =
        serde_json::from_str(&ready_manifest_with_fixture_refresh_audit_json()).unwrap();
    manifest["preflight_summary"]["fixture_refresh_audit_summary"]["loader_enabled"] = json!(true);
    manifest["preflight_summary"]["fixture_refresh_audit_summary"]
        ["input_contains_forbidden_tokens"] = json!(true);

    let report = plan(&manifest.to_string(), &accepted_ticket_json());

    assert_eq!(report["status"], "blocked_loader_manifest");
    assert!(check_status(
        &report,
        "manifest_fixture_refresh_audit_ready_no_load",
        "blocked"
    ));
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(
        report["promotion_gate"]["native_loader_calls_allowed"],
        false
    );
}

#[test]
fn manifest_fixture_refresh_audit_summary_requires_manifest_check_passed() {
    let mut manifest: Value =
        serde_json::from_str(&ready_manifest_with_fixture_refresh_audit_json()).unwrap();
    manifest["checks"] = json!(manifest["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|check| check["name"] != "fixture_gate_refresh_audit_ready_no_load")
        .cloned()
        .collect::<Vec<_>>());

    let report = plan(&manifest.to_string(), &accepted_ticket_json());

    assert_eq!(report["status"], "blocked_loader_manifest");
    assert!(check_status(
        &report,
        "manifest_fixture_refresh_audit_ready_no_load",
        "blocked"
    ));
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["ofx_may_route_to_loader"], false);
}

#[test]
fn ready_plan_carries_manifest_readiness_semantic_gate_when_provided() {
    let report = plan(
        &ready_manifest_with_readiness_json(),
        &accepted_ticket_json(),
    );
    let schema = schema();

    assert_plan_matches_schema(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert!(check_status(
        &report,
        "manifest_readiness_pipl_gate",
        "passed"
    ));
    assert_eq!(report["manifest_readiness_summary"]["provided"], true);
    assert_eq!(
        report["manifest_readiness_summary"]["matched_entry_count"],
        1
    );
    assert_eq!(
        report["manifest_readiness_summary"]["status"],
        schema["required_ready_manifest_readiness_values_when_provided"]["status"]
    );
    assert_eq!(
        report["manifest_readiness_summary"]["entry_status"],
        schema["required_ready_manifest_readiness_values_when_provided"]["entry_status"]
    );
    assert_eq!(
        report["manifest_readiness_summary"]["pipl_content_scan_status"],
        schema["required_ready_manifest_readiness_values_when_provided"]
            ["pipl_content_scan_status"]
    );
    assert_eq!(
        report["manifest_readiness_summary"]["pipl_content_scan_ready"],
        schema["required_ready_manifest_readiness_values_when_provided"]["pipl_content_scan_ready"]
    );
    assert!(report["manifest_readiness_summary"]["allowed_operations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|operation| {
            operation.as_str()
                == schema["required_ready_manifest_readiness_values_when_provided"]
                    ["allowed_operation"]
                    .as_str()
        }));
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(stage_named(&report, "render")["status"], "planned_not_run");
}

#[test]
fn ready_plan_with_host_boundary_schema_carries_cleanroom_boundary_summary() {
    let report = plan_with_boundary(
        &ready_manifest_json(),
        &accepted_ticket_json(),
        BOUNDARY_SCHEMA,
    );
    let schema = schema();

    assert_plan_matches_schema(&report, &schema);
    assert_eq!(report["status"], schema["ready_status"]);
    assert!(check_status(
        &report,
        "host_vocabulary_boundary_no_loader_or_sdk",
        "passed"
    ));
    let summary = &report["cleanroom_boundary_summary"];
    assert_eq!(
        summary["schema_name"],
        "AEX host vocabulary cleanroom boundary"
    );
    assert_eq!(summary["schema_version"], 1);
    assert_eq!(summary["publication_status"], "local-only design artifact");
    assert_eq!(
        summary["compatibility_classification"],
        "Cleanroom planning vocabulary guard"
    );
    assert_eq!(summary["source_file_count"], 16);
    assert_eq!(summary["allowed_planning_label_count"], 13);
    assert_eq!(summary["allowed_metadata_label_count"], 5);
    assert_eq!(summary["source_forbidden_substring_count"], 30);
    assert_eq!(summary["required_source_boundary_notes_count"], 4);
    assert_eq!(summary["native_loader_calls_allowed"], false);
    assert_eq!(summary["adobe_sdk_headers_allowed"], false);
    assert_eq!(summary["abi_generator_allowed"], false);
    assert_eq!(summary["third_party_effect_host_crate_allowed"], false);
    assert_eq!(summary["third_party_pipl_crate_allowed"], false);
    assert_eq!(
        summary["reuse_existing_aviutl_dynamic_loader_for_aex_allowed"],
        false
    );
    assert_eq!(summary["pf_names_are_planning_labels_only"], true);
    assert_eq!(summary["metadata_labels_do_not_define_abi"], true);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(stage_named(&report, "render")["status"], "planned_not_run");
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn host_boundary_schema_allowing_native_loader_blocks_stage_plan() {
    let mut boundary: Value = serde_json::from_str(BOUNDARY_SCHEMA).unwrap();
    boundary["allowed_policy"]["native_loader_calls_allowed"] = json!(true);
    let report = plan_with_boundary(
        &ready_manifest_json(),
        &accepted_ticket_json(),
        &boundary.to_string(),
    );

    assert_eq!(report["status"], "blocked_no_load_invariants");
    assert!(check_status(
        &report,
        "host_vocabulary_boundary_no_loader_or_sdk",
        "blocked"
    ));
    assert_eq!(
        report["cleanroom_boundary_summary"]["native_loader_calls_allowed"],
        true
    );
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(
        report["promotion_gate"]["native_loader_calls_allowed"],
        false
    );
}

#[test]
fn manifest_readiness_summary_without_semantic_gate_blocks_stage_plan() {
    let mut manifest: Value = serde_json::from_str(&ready_manifest_with_readiness_json()).unwrap();
    manifest["readiness_summary"]["pipl_content_scan_status"] = json!("partial_semantic_matches");
    manifest["readiness_summary"]["pipl_content_scan_ready"] = json!(false);
    manifest["readiness_summary"]["allowed_operations"] = json!([]);
    let report = plan(&manifest.to_string(), &accepted_ticket_json());

    assert_eq!(report["status"], "blocked_loader_manifest");
    assert!(check_status(
        &report,
        "manifest_readiness_pipl_gate",
        "blocked"
    ));
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(
        report["promotion_gate"]["native_loader_calls_allowed"],
        false
    );
}

#[test]
fn blocked_manifest_cannot_produce_ready_stage_contract() {
    let mut manifest: Value = serde_json::from_str(&ready_manifest_json()).unwrap();
    manifest["status"] = json!("blocked_capability_draft");
    manifest["implementation_gate"]["ready_for_separate_loader_slice_review"] = json!(false);
    let report = plan(&manifest.to_string(), &accepted_ticket_json());

    assert_eq!(report["status"], "blocked_loader_manifest");
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(
        report["promotion_gate"]["native_loader_calls_allowed"],
        false
    );
    assert!(check_status(
        &report,
        "loader_implementation_manifest_ready_no_load",
        "blocked"
    ));
}

#[test]
fn worker_ticket_claiming_load_or_selector_execution_is_blocked() {
    let mut ticket: Value = serde_json::from_str(&accepted_ticket_json()).unwrap();
    ticket["worker_may_load_plugin"] = json!(true);
    ticket["planned_stages"][4]["status"] = json!("ran");
    let report = plan(&ready_manifest_json(), &ticket.to_string());

    assert_eq!(report["status"], "blocked_worker_ticket");
    assert_eq!(report["worker_may_load_plugin"], false);
    assert!(check_status(
        &report,
        "worker_loader_ticket_accepted_no_load",
        "blocked"
    ));
    assert!(check_status(
        &report,
        "ticket_planned_stages_not_run",
        "blocked"
    ));
}

#[test]
fn worker_ticket_missing_runtime_evidence_is_blocked() {
    let mut ticket: Value = serde_json::from_str(&accepted_ticket_json()).unwrap();
    ticket["required_runtime_evidence"]["sandbox_preflight_required"] = json!("not_run");
    ticket["required_runtime_evidence"]["handle_inheritance_required"] = json!(null);
    let report = plan(&ready_manifest_json(), &ticket.to_string());

    assert_eq!(report["status"], "blocked_worker_ticket");
    assert!(check_status(
        &report,
        "worker_loader_ticket_runtime_evidence",
        "blocked"
    ));
    assert_eq!(
        report["ticket_runtime_evidence_summary"]["sandbox_preflight_required"],
        "not_run"
    );
    assert!(report["ticket_runtime_evidence_summary"]["handle_inheritance_required"].is_null());
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["worker_may_load_plugin"], false);
    assert_eq!(
        report["promotion_gate"]["worker_selector_calls_allowed"],
        false
    );
}

#[test]
fn manifest_ticket_identity_mismatch_is_blocked() {
    let mut ticket: Value = serde_json::from_str(&accepted_ticket_json()).unwrap();
    ticket["selected_loader_entry"]["normalized_plugin_path"] =
        json!("d:\\aviutlas\\local\\otherfilter.aex");
    let report = plan(&ready_manifest_json(), &ticket.to_string());

    assert_eq!(report["status"], "blocked_identity_mismatch");
    assert!(check_status(
        &report,
        "manifest_ticket_identity_match",
        "blocked"
    ));
    assert_eq!(
        report["promotion_gate"]["ofx_facade_may_route_to_loader"],
        false
    );
}

#[test]
fn contaminated_inputs_are_blocked_without_echoing_payload_fields() {
    let mut ticket: Value = serde_json::from_str(&accepted_ticket_json()).unwrap();
    ticket["binary_payload"] = json!("redacted");
    let report = plan(&ready_manifest_json(), &ticket.to_string());

    assert_eq!(report["status"], "blocked_no_load_invariants");
    assert!(check_status(
        &report,
        "evidence_anti_contamination",
        "blocked"
    ));
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("binary_payload"));
}

#[test]
fn contaminated_runtime_evidence_is_blocked_without_echoing_forbidden_value() {
    let mut ticket: Value = serde_json::from_str(&accepted_ticket_json()).unwrap();
    ticket["required_runtime_evidence"]["sandbox_preflight_required"] = json!("output_png");
    let report = plan(&ready_manifest_json(), &ticket.to_string());

    assert_eq!(report["status"], "blocked_worker_ticket");
    assert!(check_status(
        &report,
        "worker_loader_ticket_runtime_evidence",
        "blocked"
    ));
    assert!(check_status(
        &report,
        "evidence_anti_contamination",
        "blocked"
    ));
    assert!(report["ticket_runtime_evidence_summary"]["sandbox_preflight_required"].is_null());
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("output_png"));
    assert_eq!(report["native_load_performed"], false);
}

fn ready_manifest_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "ready_for_separate_loader_implementation_review_no_load",
        "native_load_performed": false,
        "broker_may_load_plugin": false,
        "loader_may_load_plugin": false,
        "ofx_may_route_to_loader": false,
        "selected_fixture": "adaptive-filter-local",
        "selected_effect_id": "adaptivefilter-local",
        "selected_plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
        "normalized_plugin_path": "d:\\aviutlas\\local\\adaptivefilter.aex",
        "preflight_summary": {
            "status": "preflight_passed_no_load",
            "preflight_passed": true,
            "selected_candidate_id": "adaptive-filter-local",
            "selected_loader_entry_effect_id": "adaptivefilter-local",
            "selected_loader_entry_ready": true
        },
        "capability_summary": {
            "matched_capability_count": 1,
            "effect_id": "adaptivefilter-local",
            "plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
            "evidence_mode": "static-classifier-metadata-only",
            "load_status": "not_loaded",
            "broker_may_load_plugin": false,
            "aex_worker_supported": false,
            "ofx_facade_supported": false,
            "selector_statuses": ["not_run", "not_run"]
        },
        "implementation_gate": {
            "ready_for_separate_loader_slice_review": true,
            "native_loader_calls_allowed": false,
            "broker_may_load_aex": false,
            "ofx_facade_may_route_to_loader": false,
            "requires_explicit_user_approval": true,
            "requires_code_review": true,
            "requires_local_fixture_only": true
        },
        "checks": [
            {"name": "loader_preflight_core_no_load", "status": "passed", "evidence": "metadata"},
            {"name": "evidence_anti_contamination", "status": "passed", "evidence": "metadata"}
        ],
        "blocked_reasons": [],
        "next_action": "Open a separate reviewed loader implementation slice; this manifest still permits no native loading.",
        "notes": [
            "Manifest reads JSON metadata only.",
            "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.",
            "A ready manifest is permission to review a separate loader implementation slice, not permission to load a plugin."
        ]
    }))
    .unwrap()
}

fn ready_manifest_with_readiness_json() -> String {
    let mut manifest: Value = serde_json::from_str(&ready_manifest_json()).unwrap();
    manifest["readiness_summary"] = json!({
        "provided": true,
        "matched_entry_count": 1,
        "status": "probe_readiness_planned",
        "effect_id": "adaptivefilter-local",
        "plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
        "entry_status": "draft_allowlisted",
        "pipl_content_scan_status": "semantic_matches",
        "pipl_content_scan_ready": true,
        "allowed_operations": ["describe"]
    });
    serde_json::to_string_pretty(&manifest).unwrap()
}

fn ready_manifest_with_fixture_refresh_audit_json() -> String {
    let mut manifest: Value = serde_json::from_str(&ready_manifest_json()).unwrap();
    manifest["preflight_summary"]["fixture_refresh_audit_summary"] = json!({
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
    });
    manifest["checks"]
        .as_array_mut()
        .expect("checks should be array")
        .push(json!({
            "name": "fixture_gate_refresh_audit_ready_no_load",
            "status": "passed",
            "evidence": "provided=true, status=fixture_gate_refresh_ready_no_load"
        }));
    serde_json::to_string_pretty(&manifest).unwrap()
}

fn accepted_ticket_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "ticket_protocol_version": 1,
        "generated_by": "aex_image_probe",
        "generated_unix_ms": 1234,
        "max_ticket_age_ms": 30000,
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
        ],
        "denied_surfaces": [
            "AEGP",
            "AEIO",
            "SmartFX-only",
            "GPU",
            "custom UI",
            "audio",
            "layer checkout",
            "file/network APIs"
        ],
        "notes": [
            "Worker validated loader ticket metadata only; no native AEX load was performed."
        ]
    }))
    .unwrap()
}
