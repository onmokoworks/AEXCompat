#[allow(dead_code)]
#[path = "../examples/aex_loader_slice_review_packet.rs"]
mod aex_loader_slice_review_packet;

use serde_json::{json, Value};

const REVIEW_SCHEMA: &str =
    include_str!("../../analysis/AEX_LOADER_SLICE_REVIEW_SCHEMA_2026-06-01.json");
const UNAPPROVED_FIXTURE_GATE: &str =
    include_str!("../../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json");

fn review_packet(manifest: &str, provenance: &str, fixture_gate: &str) -> Value {
    let output = aex_loader_slice_review_packet::plan_loader_slice_review_json(
        manifest,
        provenance,
        fixture_gate,
    )
    .expect("loader slice review packet should run");
    serde_json::from_str(&output).expect("review packet should emit JSON")
}

fn schema() -> Value {
    serde_json::from_str(REVIEW_SCHEMA).expect("review schema should parse")
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

fn assert_packet_matches_schema(packet: &Value, schema: &Value) {
    assert_object_has_fields(
        packet,
        &json_string_array(&schema["required_fields"]),
        "review packet",
    );
    assert_object_has_fields(
        &packet["fixture_gate_summary"],
        &json_string_array(&schema["fixture_gate_summary_required_fields"]),
        "fixture_gate_summary",
    );
    assert_object_has_fields(
        &packet["manifest_summary"],
        &json_string_array(&schema["manifest_summary_required_fields"]),
        "manifest_summary",
    );
    assert_object_has_fields(
        &packet["provenance_summary"],
        &json_string_array(&schema["provenance_summary_required_fields"]),
        "provenance_summary",
    );
    assert_object_has_fields(
        &packet["review_requirements"],
        &json_string_array(&schema["review_requirements_required_fields"]),
        "review_requirements",
    );
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(
            &packet[field], expected,
            "packet field {field} diverged from schema"
        );
    }
    for (field, expected) in schema["review_requirements_required_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &packet["review_requirements"][field], expected,
            "review requirement {field} diverged from schema"
        );
    }
    assert!(
        json_string_array(&schema["allowed_statuses"]).contains(
            &packet["status"]
                .as_str()
                .expect("packet status should be string")
        ),
        "unexpected packet status"
    );
    for name in json_string_array(&schema["required_check_names"]) {
        assert!(
            check_status(packet, name, "passed") || check_status(packet, name, "blocked"),
            "packet missing required check {name}"
        );
    }
    for name in json_string_array(&schema["conditional_check_names"]) {
        if check_status(packet, name, "passed") || check_status(packet, name, "blocked") {
            continue;
        }
        if name == "fixture_identity_smoke_preserved_no_load" {
            assert_eq!(
                packet["provenance_summary"]["fixture_identity_smoke_provided"], false,
                "fixture identity smoke check may be absent only when summary is absent"
            );
        }
    }
    for check in packet["checks"].as_array().expect("checks should be array") {
        assert_object_has_fields(check, &["name", "status", "evidence"], "check");
        let status = check["status"].as_str().unwrap();
        assert!(
            json_string_array(&schema["check_statuses"]).contains(&status),
            "unexpected check status {status}"
        );
    }
    for note in json_string_array(&schema["required_notes"]) {
        assert!(
            packet["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == note),
            "packet missing required note {note}"
        );
    }
    let serialized = serde_json::to_string(packet)
        .expect("packet should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "packet should not serialize forbidden token {token}"
        );
    }
    for marker in json_string_array(&schema["forbidden_private_path_markers"]) {
        assert!(
            !serde_json::to_string(packet).unwrap().contains(marker),
            "packet should not serialize private path marker {marker}"
        );
    }
}

fn assert_ready_values(packet: &Value, schema: &Value) {
    for (field, expected) in schema["fixture_gate_summary_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &packet["fixture_gate_summary"][field], expected,
            "fixture gate summary field {field} diverged"
        );
    }
    for (field, expected) in schema["manifest_summary_ready_values"].as_object().unwrap() {
        assert_eq!(
            &packet["manifest_summary"][field], expected,
            "manifest summary field {field} diverged"
        );
    }
    for (field, expected) in schema["provenance_summary_ready_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &packet["provenance_summary"][field], expected,
            "provenance summary field {field} diverged"
        );
    }
    for (field, expected) in schema["fixture_identity_smoke_ready_values_when_provided"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &packet["provenance_summary"][field], expected,
            "fixture identity smoke field {field} diverged"
        );
    }
}

fn check_status(packet: &Value, name: &str, status: &str) -> bool {
    packet["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == name && check["status"] == status)
}

fn cli_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

#[test]
fn ready_loader_slice_review_packet_is_no_load_and_sanitized() {
    let packet = review_packet(
        &ready_manifest_json(),
        &ready_provenance_json(),
        &approved_fixture_gate_json(),
    );
    let schema = schema();

    assert_packet_matches_schema(&packet, &schema);
    assert_ready_values(&packet, &schema);
    assert_eq!(packet["status"], schema["ready_status"]);
    assert_eq!(packet["native_load_performed"], false);
    assert_eq!(packet["loader_slice_approved"], false);
    assert_eq!(packet["loader_enabled"], false);
    assert_eq!(packet["real_aex_load_enabled"], false);
    assert_eq!(packet["native_loader_calls_allowed"], false);
    assert_eq!(packet["broker_may_load_aex"], false);
    assert_eq!(packet["worker_may_load_plugin"], false);
    assert_eq!(packet["render_performed"], false);
    assert_eq!(packet["ofx_route_allowed"], false);
    assert_eq!(
        packet["manifest_summary"]["selected_plugin_path_redacted"],
        true
    );
    assert_eq!(
        packet["fixture_gate_summary"]["selected_fixture_present"],
        true
    );
    assert_eq!(packet["fixture_gate_summary"]["approval_approved"], true);
    assert_eq!(
        packet["fixture_gate_summary"]["approval_loader_enabled"],
        true
    );
    assert_eq!(
        packet["fixture_gate_summary"]["approval_real_aex_load_enabled"],
        true
    );
    assert!(packet["blocked_reasons"].as_array().unwrap().is_empty());
    assert!(packet["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));
}

#[test]
fn cli_requires_fixture_gate_argument() {
    let error = aex_loader_slice_review_packet::parse_args_from(cli_args(&[
        "--manifest",
        "target/aex-loader-implementation/loader-implementation.local.json",
        "--provenance-audit",
        "target/aex-no-load-provenance-audit/provenance-audit.local.json",
    ]))
    .expect_err("fixture gate should be mandatory");

    assert_eq!(error, "--fixture-gate is required");
}

#[test]
fn cli_accepts_fixture_gate_argument_without_enabling_loader() {
    let (manifest, provenance, fixture_gate, out) =
        aex_loader_slice_review_packet::parse_args_from(cli_args(&[
            "--manifest",
            "target/aex-loader-implementation/loader-implementation.local.json",
            "--provenance-audit",
            "target/aex-no-load-provenance-audit/provenance-audit.local.json",
            "--fixture-gate",
            "../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json",
            "--out",
            "target/aex-loader-slice-review/custom.local.json",
        ]))
        .expect("fixture gate argument should parse");

    assert!(manifest.ends_with("loader-implementation.local.json"));
    assert!(provenance.ends_with("provenance-audit.local.json"));
    assert!(fixture_gate.ends_with("AEX_FIXTURE_REVIEW_GATE_2026-05-31.json"));
    assert!(out.ends_with("custom.local.json"));
}

#[test]
fn loader_permission_claims_block_review_packet() {
    let mut manifest: Value = serde_json::from_str(&ready_manifest_json()).unwrap();
    manifest["implementation_gate"]["native_loader_calls_allowed"] = json!(true);
    manifest["implementation_gate"]["broker_may_load_aex"] = json!(true);
    let packet = review_packet(
        &manifest.to_string(),
        &ready_provenance_json(),
        &approved_fixture_gate_json(),
    );

    assert_eq!(packet["status"], "blocked_loader_slice_review_packet");
    assert!(check_status(
        &packet,
        "loader_manifest_ready_no_load",
        "blocked"
    ));
    assert!(check_status(
        &packet,
        "no_execution_or_route_permission",
        "blocked"
    ));
    assert_eq!(packet["native_loader_calls_allowed"], false);
    assert_eq!(packet["broker_may_load_aex"], false);
    assert_eq!(packet["loader_enabled"], false);
    assert_eq!(packet["real_aex_load_enabled"], false);
}

#[test]
fn provenance_execution_or_ofx_claims_block_review_packet() {
    let mut provenance: Value = serde_json::from_str(&ready_provenance_json()).unwrap();
    provenance["render_performed"] = json!(true);
    provenance["ofx_route_allowed"] = json!(true);
    provenance["checks"][4]["status"] = json!("blocked");
    let packet = review_packet(
        &ready_manifest_json(),
        &provenance.to_string(),
        &approved_fixture_gate_json(),
    );

    assert_eq!(packet["status"], "blocked_loader_slice_review_packet");
    assert!(check_status(
        &packet,
        "provenance_chain_ready_no_load",
        "blocked"
    ));
    assert!(check_status(
        &packet,
        "no_execution_or_route_permission",
        "blocked"
    ));
    assert_eq!(packet["render_performed"], false);
    assert_eq!(packet["ofx_route_allowed"], false);
}

#[test]
fn fixture_identity_smoke_drift_blocks_handoff_without_echo() {
    let mut provenance: Value = serde_json::from_str(&ready_provenance_json()).unwrap();
    provenance["fixture_identity_smoke_summary"]["aex_render_correctness_evidence"] = json!(true);
    provenance["fixture_identity_smoke_summary"]["input_contains_forbidden_tokens"] = json!(true);
    provenance["checks"][6]["status"] = json!("blocked");
    let packet = review_packet(
        &ready_manifest_json(),
        &provenance.to_string(),
        &approved_fixture_gate_json(),
    );

    assert_eq!(packet["status"], "blocked_loader_slice_review_packet");
    assert!(check_status(
        &packet,
        "fixture_identity_smoke_preserved_no_load",
        "blocked"
    ));
    assert_eq!(
        packet["provenance_summary"]["fixture_identity_smoke_aex_render_correctness_evidence"],
        true
    );
    assert_eq!(
        packet["provenance_summary"]["fixture_identity_smoke_input_contains_forbidden_tokens"],
        true
    );
    let serialized = serde_json::to_string(&packet).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("input_png"));
    assert!(!serialized.contains("output_png"));
}

#[test]
fn contaminated_manifest_input_blocks_without_private_path_echo() {
    let mut manifest: Value = serde_json::from_str(&ready_manifest_json()).unwrap();
    manifest["binary_payload"] = json!("redacted");
    let packet = review_packet(
        &manifest.to_string(),
        &ready_provenance_json(),
        &approved_fixture_gate_json(),
    );

    assert_eq!(packet["status"], "blocked_loader_slice_review_packet");
    assert_eq!(packet["input_contains_forbidden_tokens"], true);
    assert!(check_status(
        &packet,
        "evidence_anti_contamination",
        "blocked"
    ));
    let serialized = serde_json::to_string(&packet).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("binary_payload"));
    assert!(!serialized.contains("adaptivefilter.aex"));
}

#[test]
fn current_unapproved_fixture_gate_blocks_loader_slice_review() {
    let packet = review_packet(
        &ready_manifest_json(),
        &ready_provenance_json(),
        UNAPPROVED_FIXTURE_GATE,
    );

    assert_eq!(packet["status"], "blocked_loader_slice_review_packet");
    assert!(check_status(
        &packet,
        "fixture_gate_manual_approval_ready_no_load",
        "blocked"
    ));
    assert!(check_status(
        &packet,
        "loader_manifest_ready_no_load",
        "blocked"
    ));
    assert_eq!(
        packet["fixture_gate_summary"]["status"],
        "review_queue_not_approved"
    );
    assert_eq!(
        packet["fixture_gate_summary"]["selected_fixture_present"],
        false
    );
    assert_eq!(packet["fixture_gate_summary"]["approval_approved"], false);
    assert_eq!(packet["loader_slice_approved"], false);
    assert_eq!(packet["loader_enabled"], false);
    assert_eq!(packet["real_aex_load_enabled"], false);
    let serialized = serde_json::to_string(&packet).unwrap();
    assert!(!serialized.contains("AdaptiveFilter.aex"));
    assert!(!serialized.contains("MedianPro.aex"));
    assert!(!serialized.contains("D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins"));
    assert!(!serialized.contains("EffectMain"));
    assert!(!serialized.contains("AEEffect"));
}

#[test]
fn contaminated_fixture_gate_blocks_without_static_metadata_echo() {
    let mut gate: Value = serde_json::from_str(&approved_fixture_gate_json()).unwrap();
    gate["candidates"][0]["binary_payload"] = json!("redacted");
    gate["candidates"][0]["optional_static_evidence"]["pe_export_entrypoint"] = json!("EffectMain");
    let packet = review_packet(
        &ready_manifest_json(),
        &ready_provenance_json(),
        &gate.to_string(),
    );

    assert_eq!(packet["status"], "blocked_loader_slice_review_packet");
    assert_eq!(packet["input_contains_forbidden_tokens"], true);
    assert_eq!(
        packet["fixture_gate_summary"]["input_contains_forbidden_tokens"],
        true
    );
    assert!(check_status(
        &packet,
        "fixture_gate_manual_approval_ready_no_load",
        "blocked"
    ));
    assert!(check_status(
        &packet,
        "evidence_anti_contamination",
        "blocked"
    ));
    let serialized = serde_json::to_string(&packet).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("binary_payload"));
    assert!(!serialized.contains("effectmain"));
    assert!(!serialized.contains("aeeffect"));
    assert!(!serialized.contains("adaptivefilter.aex"));
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
            "selected_loader_entry_ready": true,
            "fixture_refresh_audit_summary": {
                "provided": true,
                "status": "fixture_gate_refresh_ready_no_load"
            }
        },
        "capability_summary": {
            "matched_capability_count": 1,
            "effect_id": "adaptivefilter-local",
            "evidence_mode": "static-classifier-metadata-only",
            "load_status": "not_loaded",
            "broker_may_load_plugin": false,
            "aex_worker_supported": false,
            "ofx_facade_supported": false,
            "selector_statuses": ["not_run"]
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
            {"name": "loader_preflight_core_no_load", "status": "passed"},
            {"name": "readiness_pipl_semantic_gate", "status": "passed"},
            {"name": "evidence_anti_contamination", "status": "passed"}
        ],
        "blocked_reasons": [],
        "notes": [
            "Manifest reads JSON metadata only.",
            "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.",
            "A ready manifest is permission to review a separate loader implementation slice, not permission to load a plugin."
        ]
    }))
    .unwrap()
}

fn approved_fixture_gate_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "name": "AEX fixture review gate",
        "publication_status": "local-only design artifact",
        "status": "review_queue_approved_local_only",
        "metadata_mode": "path-and-size-only-no-hash-no-binary-payload",
        "purpose": "Choose exactly one local-build classic CPU effect candidate for a later, explicitly opened AEX worker loader slice without approving or loading it now.",
        "selected_fixture": "adaptive-filter-local",
        "recommended_first_review": "adaptive-filter-local",
        "recommendation_status": "manual-selection-recorded",
        "approval": {
            "approved": true,
            "loader_enabled": true,
            "real_aex_load_enabled": true,
            "render_png_enabled": true,
            "describe_enabled_for_real_aex": true
        },
        "single_fixture_policy": {
            "max_selected_fixtures": 1,
            "selection_requires_manual_user_approval": true,
            "selection_requires_local_only_license_review": true,
            "selection_requires_source_tree_review": true,
            "selection_requires_binary_redistribution_review": true,
            "selection_requires_loader_gate_opened_by_separate_slice": true,
            "no_parallel_first_loader_fixtures": true
        },
        "required_runtime_evidence_before_loader": {
            "allowlist_loader_approval_status": "approved-local-only",
            "request_loader_approval_status": "approved-local-only",
            "allowed_operation": "render_png",
            "worker_identity_revalidation": "passed",
            "sandbox_preflight": "passed",
            "worker_attestation": "passed",
            "job_object": "assigned-with-kill-on-close",
            "handle_inheritance": "sentinel_not_inherited-with-explicit-handle-list",
            "ofx_facade": "not-a-loader-and-not-a-bypass"
        },
        "candidates": [
            {
                "id": "adaptive-filter-local",
                "display_name": "AdaptiveFilter",
                "path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
                "source_tree": "D:\\AviUtlas\\local\\AdaptiveFilterRust",
                "observed_size_bytes": 207360,
                "generated_target_artifact": false,
                "fixture_status": "local-build-candidate",
                "classifier_status": "candidate_for_contract_probe",
                "plugin_class": "classic-effect-candidate",
                "legacy_effect_entrypoint_evidence": "CodeWin64X86(EffectMain) observed in adjacent build.rs; EffectMain export observed by optional read-only PE inspection",
                "optional_static_evidence": {
                    "evidence_status": "observed-read-only-no-load",
                    "pe_machine": "x86_64",
                    "pe_resource_summary": "present; top_level_entries=2; types=PIPL,id:16",
                    "pe_export_entrypoint": "EffectMain",
                    "adjacent_pipl_kind": "AEEffect",
                    "adjacent_pipl_name": "AdaptiveFilter",
                    "adjacent_pipl_category": "Filter",
                    "adjacent_pipl_match_name": "ONMK_AdaptiveFilter",
                    "adjacent_entrypoint": "EffectMain",
                    "smart_render_status": "declared-but-deferred"
                },
                "source_license_evidence": "LICENSE file observed; MIT license text noted in AEX_IMAGE_PROBE_LICENSE_NOTES_2026-05-31.md",
                "review_priority": 1,
                "review_status": "approved-local-only",
                "blocked_reasons": []
            },
            {
                "id": "median-pro-local",
                "display_name": "MedianPro",
                "path": "D:\\AviUtlas\\local\\MedianPro.aex",
                "source_tree": "D:\\AviUtlas\\local\\MedianProRust",
                "observed_size_bytes": 207360,
                "generated_target_artifact": false,
                "fixture_status": "local-build-candidate",
                "classifier_status": "candidate_for_contract_probe",
                "plugin_class": "classic-effect-candidate",
                "source_license_evidence": "LICENSE file observed; MIT license text noted in AEX_IMAGE_PROBE_LICENSE_NOTES_2026-05-31.md",
                "review_priority": 2,
                "review_status": "not-approved",
                "blocked_reasons": [
                    "manual user approval has not selected this fixture"
                ]
            }
        ],
        "rejected_first_loader_classes": [
            "aegp",
            "aeio",
            "smartfx-only",
            "gpu-only",
            "ml-or-onnx-heavy",
            "unknown"
        ],
        "notes": [
            "This artifact is a review queue, not permission to load .aex.",
            "No hash, binary payload, copied .aex, or private image fixture is embedded."
        ]
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
        "loader_manifest_summary": {
            "status": "ready_for_separate_loader_implementation_review_no_load",
            "native_load_performed": false,
            "readiness_provided": true,
            "blocked_reason_count": 0
        },
        "native_stage_plan_summary": {
            "status": "planned_native_stage_contract_no_load",
            "native_load_performed": false,
            "selectors_executed": false,
            "render_performed": false,
            "worker_runtime_evidence_ready": true,
            "cleanroom_boundary_no_loader_or_sdk": true,
            "blocked_reason_count": 0
        },
        "ofx_readiness_summary": {
            "status": "deferred_contract_only",
            "ofx_host_may_load_aex": false,
            "ofx_adapter_may_load_aex": false,
            "broker_may_load_aex": false,
            "aviutlas_may_route_through_ofx_to_reach_aex": false,
            "blocked_reason_count": 0
        },
        "fixture_identity_smoke_summary": {
            "provided": true,
            "status": "fixture_identity_smoke_ready_no_load",
            "broker_invoked": true,
            "aex_render_correctness_evidence": false,
            "input_contains_forbidden_tokens": false
        },
        "checks": [
            {"name": "loader_manifest_ready_no_load", "status": "passed"},
            {"name": "native_stage_plan_ready_no_load", "status": "passed"},
            {"name": "native_stage_runtime_and_cleanroom_ready", "status": "passed"},
            {"name": "ofx_readiness_deferred_no_bypass", "status": "passed"},
            {"name": "ofx_readiness_consumed_native_stage_plan", "status": "passed"},
            {"name": "evidence_anti_contamination", "status": "passed"},
            {"name": "fixture_identity_smoke_ready_no_load", "status": "passed"}
        ],
        "blocked_reasons": [],
        "notes": [
            "Provenance audit reads JSON metadata only.",
            "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.",
            "A ready audit is not loader approval and does not permit OFX routing."
        ]
    }))
    .unwrap()
}
