#[allow(dead_code)]
#[path = "../examples/aex_loader_implementation_manifest.rs"]
mod aex_loader_implementation_manifest;

use serde_json::json;
use serde_json::Value;

const IMPLEMENTATION_MANIFEST_SCHEMA: &str =
    include_str!("../../analysis/AEX_LOADER_IMPLEMENTATION_MANIFEST_SCHEMA_2026-06-01.json");

fn manifest(preflight: &str, capabilities: &[String]) -> Value {
    let refs = capabilities.iter().map(String::as_str).collect::<Vec<_>>();
    let output = aex_loader_implementation_manifest::plan_loader_implementation_manifest_json(
        preflight, &refs,
    )
    .expect("implementation manifest should run");
    serde_json::from_str(&output).expect("manifest should emit JSON")
}

fn manifest_with_readiness(preflight: &str, capabilities: &[String], readiness: &str) -> Value {
    let refs = capabilities.iter().map(String::as_str).collect::<Vec<_>>();
    let output =
        aex_loader_implementation_manifest::plan_loader_implementation_manifest_json_with_readiness(
            preflight,
            &refs,
            Some(readiness),
        )
        .expect("implementation manifest should run with readiness evidence");
    serde_json::from_str(&output).expect("manifest should emit JSON")
}

fn schema() -> Value {
    serde_json::from_str(IMPLEMENTATION_MANIFEST_SCHEMA)
        .expect("implementation manifest schema should parse")
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

fn assert_manifest_matches_schema(report: &Value, schema: &Value) {
    assert_object_has_fields(
        report,
        &json_string_array(&schema["required_fields"]),
        "manifest",
    );
    assert_object_has_fields(
        &report["preflight_summary"],
        &json_string_array(&schema["preflight_summary_required_fields"]),
        "preflight_summary",
    );
    assert_object_has_fields(
        &report["preflight_summary"]["fixture_refresh_audit_summary"],
        &json_string_array(&schema["fixture_refresh_audit_summary_required_fields"]),
        "preflight_summary.fixture_refresh_audit_summary",
    );
    if report["preflight_summary"]["fixture_refresh_audit_summary"]["provided"]
        .as_bool()
        .expect("fixture refresh provided should be bool")
    {
        for (field, expected) in schema["fixture_refresh_audit_summary_ready_values_when_provided"]
            .as_object()
            .expect("fixture refresh ready values should be object")
        {
            assert_eq!(
                &report["preflight_summary"]["fixture_refresh_audit_summary"][field], expected,
                "fixture refresh summary field {field} diverged"
            );
        }
    }
    assert_object_has_fields(
        &report["capability_summary"],
        &json_string_array(&schema["capability_summary_required_fields"]),
        "capability_summary",
    );
    assert_object_has_fields(
        &report["readiness_summary"],
        &json_string_array(&schema["readiness_summary_required_fields"]),
        "readiness_summary",
    );
    assert_object_has_fields(
        &report["implementation_gate"],
        &json_string_array(&schema["implementation_gate_required_fields"]),
        "implementation_gate",
    );
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(
            &report[field], expected,
            "manifest field {field} diverged from schema"
        );
    }
    for (field, expected) in schema["implementation_gate_required_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["implementation_gate"][field], expected,
            "implementation gate field {field} diverged from schema"
        );
    }
    assert!(
        json_string_array(&schema["allowed_statuses"]).contains(
            &report["status"]
                .as_str()
                .expect("manifest status should be string")
        ),
        "unexpected manifest status"
    );
    for name in json_string_array(&schema["required_check_names"]) {
        assert!(
            check_status(report, name, "passed") || check_status(report, name, "blocked"),
            "manifest missing required check {name}"
        );
    }
    for name in json_string_array(&schema["conditional_check_names"]) {
        if check_status(report, name, "passed") || check_status(report, name, "blocked") {
            continue;
        }
        if name == "fixture_gate_refresh_audit_ready_no_load" {
            assert_eq!(
                report["preflight_summary"]["fixture_refresh_audit_summary"]["provided"], false,
                "fixture refresh audit check may be absent only when summary is absent"
            );
            continue;
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
            "manifest missing required note {note}"
        );
    }
    let serialized = serde_json::to_string(report)
        .expect("manifest should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "manifest should not contain serialized token {token}"
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

#[test]
fn ready_manifest_is_still_no_load_and_only_opens_review() {
    let report = manifest(&passing_preflight_json(), &[matching_capability_json()]);
    let schema = schema();

    assert_manifest_matches_schema(&report, &schema);
    assert_eq!(
        report["status"],
        "ready_for_separate_loader_implementation_review_no_load"
    );
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert_eq!(report["loader_may_load_plugin"], false);
    assert_eq!(report["ofx_may_route_to_loader"], false);
    assert_eq!(
        report["implementation_gate"]["ready_for_separate_loader_slice_review"],
        true
    );
    assert_eq!(
        report["implementation_gate"]["native_loader_calls_allowed"],
        false
    );
    assert_eq!(report["selected_fixture"], "adaptive-filter-local");
    assert_eq!(report["selected_effect_id"], "adaptivefilter-local");
    assert_eq!(
        report["selected_plugin_path"],
        "D:\\AviUtlas\\local\\AdaptiveFilter.aex"
    );
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert_eq!(
        report["capability_summary"]["evidence_mode"],
        schema["required_ready_capability_values"]["evidence_mode"]
    );
    assert_eq!(
        report["capability_summary"]["load_status"],
        schema["required_ready_capability_values"]["load_status"]
    );
    assert_eq!(
        report["capability_summary"]["aex_worker_supported"],
        schema["required_ready_capability_values"]["aex_worker_supported"]
    );
    assert_eq!(report["readiness_summary"]["provided"], false);
    assert_eq!(report["readiness_summary"]["matched_entry_count"], 0);
    assert_eq!(
        report["preflight_summary"]["fixture_refresh_audit_summary"]["provided"],
        false
    );
}

#[test]
fn ready_manifest_preserves_fixture_refresh_audit_summary_when_preflight_provides_it() {
    let report = manifest(
        &passing_preflight_with_fixture_refresh_audit_json(),
        &[matching_capability_json()],
    );
    let schema = schema();

    assert_manifest_matches_schema(&report, &schema);
    assert_eq!(
        report["status"],
        "ready_for_separate_loader_implementation_review_no_load"
    );
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["loader_may_load_plugin"], false);
    assert_eq!(report["ofx_may_route_to_loader"], false);
    let summary = &report["preflight_summary"]["fixture_refresh_audit_summary"];
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
        "fixture_gate_refresh_audit_ready_no_load",
        "passed"
    ));
}

#[test]
fn fixture_refresh_audit_summary_requires_preflight_check_passed() {
    let mut preflight: Value =
        serde_json::from_str(&passing_preflight_with_fixture_refresh_audit_json()).unwrap();
    preflight["checks"] = json!(preflight["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|check| check["name"] != "fixture_gate_refresh_audit_ready_no_load")
        .cloned()
        .collect::<Vec<_>>());

    let report = manifest(&preflight.to_string(), &[matching_capability_json()]);

    assert_eq!(report["status"], "blocked_fixture_refresh_evidence");
    assert!(check_status(
        &report,
        "fixture_gate_refresh_audit_ready_no_load",
        "blocked"
    ));
    assert_eq!(
        report["implementation_gate"]["ready_for_separate_loader_slice_review"],
        false
    );
    assert_eq!(report["loader_may_load_plugin"], false);
}

#[test]
fn fixture_refresh_audit_summary_not_ready_blocks_loader_review() {
    let mut preflight: Value =
        serde_json::from_str(&passing_preflight_with_fixture_refresh_audit_json()).unwrap();
    preflight["fixture_refresh_audit_summary"]["loader_enabled"] = json!(true);
    preflight["fixture_refresh_audit_summary"]["input_contains_forbidden_tokens"] = json!(true);

    let report = manifest(&preflight.to_string(), &[matching_capability_json()]);

    assert_eq!(report["status"], "blocked_fixture_refresh_evidence");
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["broker_may_load_plugin"], false);
    assert!(check_status(
        &report,
        "fixture_gate_refresh_audit_ready_no_load",
        "blocked"
    ));
    assert_eq!(
        report["implementation_gate"]["ready_for_separate_loader_slice_review"],
        false
    );
}

#[test]
fn ready_manifest_with_readiness_requires_semantic_pipl_gate() {
    let report = manifest_with_readiness(
        &passing_preflight_json(),
        &[matching_capability_json()],
        &matching_readiness_json(),
    );
    let schema = schema();

    assert_manifest_matches_schema(&report, &schema);
    assert_eq!(
        report["status"],
        "ready_for_separate_loader_implementation_review_no_load"
    );
    assert!(check_status(
        &report,
        "readiness_pipl_semantic_gate",
        "passed"
    ));
    assert_eq!(report["readiness_summary"]["provided"], true);
    assert_eq!(report["readiness_summary"]["matched_entry_count"], 1);
    assert_eq!(
        report["readiness_summary"]["status"],
        schema["required_ready_readiness_values_when_provided"]["status"]
    );
    assert_eq!(
        report["readiness_summary"]["entry_status"],
        schema["required_ready_readiness_values_when_provided"]["entry_status"]
    );
    assert_eq!(
        report["readiness_summary"]["pipl_content_scan_status"],
        schema["required_ready_readiness_values_when_provided"]["pipl_content_scan_status"]
    );
    assert_eq!(
        report["readiness_summary"]["pipl_content_scan_ready"],
        schema["required_ready_readiness_values_when_provided"]["pipl_content_scan_ready"]
    );
    assert!(report["readiness_summary"]["allowed_operations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|operation| {
            operation.as_str()
                == schema["required_ready_readiness_values_when_provided"]["allowed_operation"]
                    .as_str()
        }));
    assert_eq!(
        report["implementation_gate"]["ready_for_separate_loader_slice_review"],
        true
    );
}

#[test]
fn readiness_without_semantic_pipl_gate_blocks_loader_review() {
    let mut readiness: Value = serde_json::from_str(&matching_readiness_json()).unwrap();
    readiness["entries"][0]["status"] = json!("blocked_or_deferred");
    readiness["entries"][0]["pipl_content_scan_status"] = json!("partial_semantic_matches");
    readiness["entries"][0]["pipl_content_scan_ready"] = json!(false);
    readiness["entries"][0]["allowed_operations"] = json!([]);

    let report = manifest_with_readiness(
        &passing_preflight_json(),
        &[matching_capability_json()],
        &readiness.to_string(),
    );

    assert_eq!(report["status"], "blocked_readiness_evidence");
    assert!(check_status(
        &report,
        "readiness_pipl_semantic_gate",
        "blocked"
    ));
    assert_eq!(
        report["implementation_gate"]["ready_for_separate_loader_slice_review"],
        false
    );
    assert_eq!(report["loader_may_load_plugin"], false);
}

#[test]
fn readiness_path_mismatch_blocks_loader_review() {
    let mut readiness: Value = serde_json::from_str(&matching_readiness_json()).unwrap();
    readiness["entries"][0]["plugin_path"] = json!("D:\\AviUtlas\\local\\OtherFilter.aex");

    let report = manifest_with_readiness(
        &passing_preflight_json(),
        &[matching_capability_json()],
        &readiness.to_string(),
    );

    assert_eq!(report["status"], "blocked_readiness_evidence");
    assert!(check_status(
        &report,
        "readiness_pipl_semantic_gate",
        "blocked"
    ));
    assert_eq!(report["readiness_summary"]["matched_entry_count"], 0);
    assert_eq!(report["native_load_performed"], false);
}

#[test]
fn blocked_preflight_receipt_cannot_open_review_packet() {
    let mut preflight: Value = serde_json::from_str(&passing_preflight_json()).unwrap();
    preflight["status"] = json!("blocked_loader_gate_closed");
    preflight["preflight_passed"] = json!(false);
    let report = manifest(&preflight.to_string(), &[matching_capability_json()]);

    assert_eq!(report["status"], "blocked_preflight_receipt");
    assert_eq!(
        report["implementation_gate"]["ready_for_separate_loader_slice_review"],
        false
    );
    assert_eq!(
        report["implementation_gate"]["native_loader_calls_allowed"],
        false
    );
    assert!(check_status(
        &report,
        "loader_preflight_core_no_load",
        "blocked"
    ));
    assert!(!report["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn capability_claiming_execution_is_blocked() {
    let mut capability: Value = serde_json::from_str(&matching_capability_json()).unwrap();
    capability["load_status"] = json!("loaded");
    capability["aex_worker"]["supported"] = json!(true);
    let report = manifest(&passing_preflight_json(), &[capability.to_string()]);

    assert_eq!(report["status"], "blocked_no_load_invariants");
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["loader_may_load_plugin"], false);
    assert!(check_status(
        &report,
        "capability_no_load_static_draft",
        "blocked"
    ));
    assert_eq!(
        report["implementation_gate"]["ready_for_separate_loader_slice_review"],
        false
    );
}

#[test]
fn mismatched_capability_path_is_blocked() {
    let mut capability: Value = serde_json::from_str(&matching_capability_json()).unwrap();
    capability["plugin_path"] = json!("D:\\AviUtlas\\local\\OtherFilter.aex");
    let report = manifest(&passing_preflight_json(), &[capability.to_string()]);

    assert_eq!(report["status"], "blocked_capability_draft");
    assert!(check_status(&report, "capability_path_identity", "blocked"));
    assert_eq!(report["loader_may_load_plugin"], false);
    assert_eq!(
        report["implementation_gate"]["ready_for_separate_loader_slice_review"],
        false
    );
}

#[test]
fn contaminated_evidence_is_blocked_without_echoing_private_token() {
    let mut capability: Value = serde_json::from_str(&matching_capability_json()).unwrap();
    capability["binary_payload"] = json!("redacted");
    let report = manifest(&passing_preflight_json(), &[capability.to_string()]);

    assert_eq!(report["status"], "blocked_no_load_invariants");
    assert!(check_status(
        &report,
        "evidence_anti_contamination",
        "blocked"
    ));
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("binary_payload"));
}

fn passing_preflight_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "preflight_passed_no_load",
        "preflight_passed": true,
        "native_load_performed": false,
        "broker_may_load_plugin": false,
        "selected_fixture": "adaptive-filter-local",
        "selected_candidate": {
            "id": "adaptive-filter-local",
            "display_name": "AdaptiveFilter",
            "plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
            "normalized_plugin_path": "d:\\aviutlas\\local\\adaptivefilter.aex",
            "loader_gate_plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
            "loader_gate_effect_id": "adaptivefilter-local",
            "path_match_status": "matched_normalized_path"
        },
        "selected_loader_entry": {
            "effect_id": "adaptivefilter-local",
            "plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
            "normalized_plugin_path": "d:\\aviutlas\\local\\adaptivefilter.aex",
            "path_match_status": "matched_normalized_path",
            "pre_loader_status": "approved-local-only",
            "loader_approval_status": "approved-local-only",
            "allowlist_operation_status": "render_png",
            "handle_inheritance_required": "sentinel_not_inherited-with-explicit-handle-list",
            "worker_identity_revalidation_required": "passed",
            "worker_attestation_required": "passed",
            "sandbox_preflight_required": "passed",
            "job_object_required": "assigned-with-kill-on-close",
            "entry_ready": true
        },
        "fixture_gate": {
            "status": "approved-local-only",
            "selected_fixture": "adaptive-filter-local",
            "approval_approved": true,
            "approval_loader_enabled": true,
            "approval_real_aex_load_enabled": true,
            "approval_render_png_enabled": true,
            "approval_describe_enabled_for_real_aex": true,
            "candidate_count": 1
        },
        "loader_gate": {
            "status": "loader_gate_opened_for_single_candidate",
            "approved": true,
            "loader_enabled": true,
            "real_aex_load_enabled": true,
            "open_candidate_count": 1,
            "entry_count": 1
        },
        "checks": [
            {"name": "fixture_gate_schema_version", "status": "passed"},
            {"name": "loader_gate_schema_version", "status": "passed"},
            {"name": "fixture_gate_unique_candidates", "status": "passed"},
            {"name": "loader_gate_unique_entries", "status": "passed"},
            {"name": "selected_fixture", "status": "passed"},
            {"name": "selected_fixture_metadata", "status": "passed"},
            {"name": "fixture_gate_approval", "status": "passed"},
            {"name": "loader_gate_single_ready_entry", "status": "passed"},
            {"name": "selected_fixture_in_loader_gate", "status": "passed"},
            {"name": "loader_gate_open", "status": "passed"},
            {"name": "selected_loader_entry", "status": "passed"}
        ],
        "blocked_reasons": [],
        "next_action": "Open a separate explicit loader implementation slice; this preflight still performed no native loading.",
        "notes": [
            "Preflight reads JSON metadata only.",
            "No .aex file is opened, hashed, loaded, executed, described, or rendered.",
            "A passing preflight is permission to start a separate loader slice, not proof that native loading is implemented."
        ]
    }))
    .unwrap()
}

fn passing_preflight_with_fixture_refresh_audit_json() -> String {
    let mut preflight: Value = serde_json::from_str(&passing_preflight_json()).unwrap();
    preflight["fixture_refresh_audit_summary"] = json!({
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
    preflight["checks"]
        .as_array_mut()
        .expect("checks should be array")
        .push(json!({
            "name": "fixture_gate_refresh_audit_ready_no_load",
            "status": "passed",
            "evidence": "provided=true, status=fixture_gate_refresh_ready_no_load"
        }));
    serde_json::to_string_pretty(&preflight).unwrap()
}

fn matching_capability_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "effect_id": "adaptivefilter-local",
        "display_name": "AdaptiveFilter",
        "plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
        "publication_status": "local-only",
        "evidence_mode": "static-classifier-metadata-only",
        "load_status": "not_loaded",
        "broker_may_load_plugin": false,
        "current_supported_operations": [],
        "params_status": "unknown",
        "params": [],
        "selectors": [
            {"name": "load", "status": "not_run"},
            {"name": "global_setup", "status": "not_run"},
            {"name": "params_setup", "status": "not_run"},
            {"name": "sequence_setup", "status": "not_run"},
            {"name": "render", "status": "not_run"},
            {"name": "sequence_teardown", "status": "not_run"},
            {"name": "global_teardown", "status": "not_run"}
        ],
        "aex_worker": {"supported": false, "status": "deferred_loader_gate_closed"},
        "ofx_facade": {"supported": false, "status": "deferred_same_aex_worker_gate"},
        "unsupported_or_deferred_surfaces": [
            "SmartFX",
            "GPU",
            "AEGP suites",
            "AEIO",
            "audio",
            "layer checkout",
            "custom UI",
            "arbitrary file or network APIs"
        ],
        "notes": [
            "Capability draft only: generated from static metadata without loading .aex.",
            "Selectors are not_run; no parameter descriptors or render pixels are claimed.",
            "Real loader, AEX worker support, and OFX facade support remain disabled."
        ]
    }))
    .unwrap()
}

fn matching_readiness_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "status": "probe_readiness_planned",
        "publication_status": "local-only",
        "candidate_count": 1,
        "allowlist_entry_count": 1,
        "blocked_count": 0,
        "generated_files": [
            "allowlist.local.draft.json",
            "readiness.local.json",
            "loader-gate.local.json",
            "requests/ONMK-AdaptiveFilter.describe.json",
            "capabilities/ONMK-AdaptiveFilter.capability.json"
        ],
        "entries": [
            {
                "effect_id": "adaptivefilter-local",
                "display_name": "AdaptiveFilter",
                "plugin_path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
                "status": "draft_allowlisted",
                "classifier_status": "candidate_for_contract_probe",
                "classifier_inferred_class": "classic-effect-candidate",
                "pipl_content_scan_status": "semantic_matches",
                "pipl_content_scan_ready": true,
                "allowed_operations": ["describe"],
                "blocked_reason": null
            }
        ],
        "notes": [
            "Draft readiness does not prove plugin loadability or render support.",
            "Worker execution still requires explicit reviewed worker_exe and allowlist review.",
            "Static-classifier catalog candidates require pipl_content_scan.status=semantic_matches with contents_emitted=false before readiness planning."
        ]
    }))
    .unwrap()
}
