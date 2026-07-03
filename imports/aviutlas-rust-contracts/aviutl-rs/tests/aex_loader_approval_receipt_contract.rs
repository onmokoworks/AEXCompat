#[allow(dead_code)]
#[path = "../examples/aex_loader_approval_receipt.rs"]
mod aex_loader_approval_receipt;

use serde_json::{json, Value};

const APPROVAL_SCHEMA: &str =
    include_str!("../../analysis/AEX_LOADER_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json");

fn validate(packet: &str, receipt: &str) -> Value {
    let output =
        aex_loader_approval_receipt::validate_loader_approval_receipt_json(packet, receipt)
            .expect("approval receipt validator should run");
    serde_json::from_str(&output).expect("validator should emit JSON")
}

fn draft_template(packet: &str) -> Value {
    let output = aex_loader_approval_receipt::draft_loader_approval_receipt_template_json(packet)
        .expect("draft template should run for ready packets");
    serde_json::from_str(&output).expect("template should emit JSON")
}

fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn check_schema(report: &Value) {
    let schema: Value = serde_json::from_str(APPROVAL_SCHEMA).expect("schema should parse");
    for field in schema["validator_report"]["required_fields"]
        .as_array()
        .unwrap()
    {
        let field = field.as_str().unwrap();
        assert!(
            report.as_object().unwrap().contains_key(field),
            "report missing field {field}"
        );
    }
    for (field, expected) in schema["validator_report"]["required_no_load_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(report[field], *expected, "report field {field} diverged");
    }
    for note in schema["validator_report"]["required_notes"]
        .as_array()
        .unwrap()
    {
        assert!(
            report["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == note),
            "report missing required note {note}"
        );
    }
}

fn cli_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

#[test]
fn approved_receipt_validates_review_only_and_keeps_loader_disabled() {
    let packet = ready_packet_json();
    let receipt = approved_receipt_json(&packet);
    let report = validate(&packet, &receipt);

    check_schema(&report);
    assert_eq!(
        report["validation_status"],
        "approved_for_loader_implementation_review_no_load"
    );
    assert_eq!(report["approval_accepted"], true);
    assert_eq!(report["loader_review_approved"], true);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["loader_enabled"], false);
    assert_eq!(report["real_aex_load_enabled"], false);
    assert_eq!(report["native_loader_calls_allowed"], false);
    assert_eq!(report["worker_may_load_plugin"], false);
    assert_eq!(report["broker_may_load_aex"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["ofx_route_allowed"], false);
    assert_eq!(
        report["fixture_gate_status"],
        "review_queue_approved_local_only"
    );
    assert_eq!(
        report["fixture_candidate_review_status"],
        "approved-local-only"
    );
    assert_eq!(
        report["loader_slice_review_packet_checksum_hex"],
        fnv1a64_hex(packet.as_bytes())
    );
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
}

#[test]
fn draft_template_is_unapproved_bound_to_ready_packet_and_not_accepted() {
    let packet = ready_packet_json();
    let template = draft_template(&packet);

    assert_eq!(template["template_status"], "draft_unapproved_template");
    assert_eq!(template["approval_status"], "draft_unapproved");
    assert!(template["approval_receipt_id"].is_null());
    assert!(template["approved_by"].is_null());
    assert!(template["approved_at_utc"].is_null());
    assert_eq!(
        template["approval_scope"]["loader_slice_review_packet_checksum_hex"],
        fnv1a64_hex(packet.as_bytes())
    );
    assert_eq!(
        template["approval_scope"]["fixture_gate_status_required"],
        "review_queue_approved_local_only"
    );
    assert_eq!(
        template["approval_effect"]["allow_separate_loader_implementation_review"],
        false
    );
    assert_eq!(template["approval_effect"]["allow_native_aex_load"], false);
    assert_eq!(
        template["approval_effect"]["allow_worker_plugin_load"],
        false
    );
    assert_eq!(template["approval_effect"]["allow_render_png"], false);
    assert_eq!(template["approval_effect"]["allow_ofx_route"], false);
    assert_eq!(template["native_load_performed"], false);
    assert_eq!(template["loader_enabled"], false);
    assert_eq!(template["real_aex_load_enabled"], false);
    assert_eq!(template["worker_may_load_plugin"], false);
    assert_eq!(template["render_performed"], false);
    assert_eq!(template["ofx_route_allowed"], false);

    let report = validate(&packet, &template.to_string());
    assert_eq!(report["validation_status"], "invalid_receipt");
    assert_eq!(report["approval_accepted"], false);
    assert!(report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("approval_status")));

    let serialized = serde_json::to_string(&template)
        .unwrap()
        .to_ascii_lowercase();
    assert!(!serialized.contains(".aex"));
    assert!(!serialized.contains("effectmain"));
    assert!(!serialized.contains("plugin_path"));
}

#[test]
fn draft_template_rejects_blocked_or_contaminated_packet() {
    let mut packet: Value = serde_json::from_str(&ready_packet_json()).unwrap();
    packet["status"] = json!("blocked_loader_slice_review_packet");
    packet["blocked_reasons"] = json!(["blocked"]);
    let err = aex_loader_approval_receipt::draft_loader_approval_receipt_template_json(
        &packet.to_string(),
    )
    .expect_err("blocked packet must not produce a template");
    assert!(err
        .to_string()
        .contains("requires a ready sanitized packet"));

    let mut packet: Value = serde_json::from_str(&ready_packet_json()).unwrap();
    packet["binary_payload"] = json!("redacted");
    let err = aex_loader_approval_receipt::draft_loader_approval_receipt_template_json(
        &packet.to_string(),
    )
    .expect_err("contaminated packet must not produce a template");
    assert!(err.to_string().contains("forbidden"));
}

#[test]
fn current_or_blocked_packet_cannot_be_approved_by_receipt() {
    let mut packet: Value = serde_json::from_str(&ready_packet_json()).unwrap();
    packet["status"] = json!("blocked_loader_slice_review_packet");
    packet["blocked_reasons"] = json!(["fixture review gate has not selected and approved exactly one local-build classic effect candidate"]);
    packet["fixture_gate_summary"]["status"] = json!("review_queue_not_approved");
    packet["fixture_gate_summary"]["selected_fixture_present"] = json!(false);
    packet["fixture_gate_summary"]["approval_approved"] = json!(false);
    let packet = serde_json::to_string_pretty(&packet).unwrap();
    let receipt = approved_receipt_json(&packet);
    let report = validate(&packet, &receipt);

    assert_eq!(report["validation_status"], "invalid_receipt");
    assert_eq!(report["approval_accepted"], false);
    assert_eq!(report["loader_review_approved"], false);
    assert_eq!(report["loader_enabled"], false);
    assert!(report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("must be ready")));
}

#[test]
fn checksum_mismatch_blocks_approval() {
    let packet = ready_packet_json();
    let mut receipt: Value = serde_json::from_str(&approved_receipt_json(&packet)).unwrap();
    receipt["approval_scope"]["loader_slice_review_packet_checksum_hex"] =
        json!("0000000000000000");
    let report = validate(&packet, &receipt.to_string());

    assert_eq!(report["validation_status"], "invalid_receipt");
    assert_eq!(report["approval_accepted"], false);
    assert!(report["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason.as_str().unwrap().contains("checksum")));
}

#[test]
fn unsafe_approval_effects_fail_closed() {
    let packet = ready_packet_json();
    let mut receipt: Value = serde_json::from_str(&approved_receipt_json(&packet)).unwrap();
    receipt["approval_effect"]["allow_native_aex_load"] = json!(true);
    receipt["approval_effect"]["allow_worker_plugin_load"] = json!(true);
    receipt["approval_effect"]["allow_render_png"] = json!(true);
    receipt["approval_effect"]["allow_ofx_route"] = json!(true);
    let report = validate(&packet, &receipt.to_string());

    assert_eq!(report["validation_status"], "invalid_receipt");
    assert_eq!(report["approval_accepted"], false);
    assert_eq!(report["native_load_performed"], false);
    assert_eq!(report["loader_enabled"], false);
    assert_eq!(report["worker_may_load_plugin"], false);
    assert_eq!(report["render_performed"], false);
    assert_eq!(report["ofx_route_allowed"], false);
}

#[test]
fn private_path_or_static_metadata_in_receipt_blocks_without_echo() {
    let packet = ready_packet_json();
    let mut receipt: Value = serde_json::from_str(&approved_receipt_json(&packet)).unwrap();
    receipt["approval_scope"]["plugin_path"] = json!("D:\\AviUtlas\\local\\AdaptiveFilter.aex");
    receipt["approval_scope"]["entrypoint"] = json!("EffectMain");
    let report = validate(&packet, &receipt.to_string());

    assert_eq!(report["validation_status"], "invalid_receipt");
    assert_eq!(report["input_contains_forbidden_tokens"], true);
    let serialized = serde_json::to_string(&report).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("adaptivefilter.aex"));
    assert!(!serialized.contains("effectmain"));
    assert!(!serialized.contains("plugin_path"));
}

#[test]
fn cli_requires_packet_and_receipt_arguments() {
    let error = aex_loader_approval_receipt::parse_args_from(cli_args(&[
        "--packet",
        "target/aex-loader-slice-review/loader-slice-review.local.json",
    ]))
    .expect_err("receipt should be required");
    assert_eq!(error, "--receipt is required");

    let command = aex_loader_approval_receipt::parse_args_from(cli_args(&[
        "--packet",
        "target/aex-loader-slice-review/loader-slice-review.local.json",
        "--receipt",
        "approval.local.json",
        "--out",
        "target/aex-loader-approval/approval-validation.local.json",
    ]))
    .expect("all arguments should parse");
    match command {
        aex_loader_approval_receipt::ApprovalCliCommand::Validate {
            packet,
            receipt,
            out,
        } => {
            assert!(packet.ends_with("loader-slice-review.local.json"));
            assert!(receipt.ends_with("approval.local.json"));
            assert!(out.ends_with("approval-validation.local.json"));
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn cli_supports_draft_template_without_receipt() {
    let command = aex_loader_approval_receipt::parse_args_from(cli_args(&[
        "--draft-template",
        "--packet",
        "target/aex-loader-slice-review/loader-slice-review.local.json",
        "--out",
        "target/aex-loader-approval/approval-receipt-template.local.json",
    ]))
    .expect("draft-template arguments should parse");
    match command {
        aex_loader_approval_receipt::ApprovalCliCommand::DraftTemplate { packet, out } => {
            assert!(packet.ends_with("loader-slice-review.local.json"));
            assert!(out.ends_with("approval-receipt-template.local.json"));
        }
        other => panic!("unexpected command: {other:?}"),
    }

    let error = aex_loader_approval_receipt::parse_args_from(cli_args(&[
        "--draft-template",
        "--packet",
        "target/aex-loader-slice-review/loader-slice-review.local.json",
        "--receipt",
        "approval.local.json",
    ]))
    .expect_err("draft template must not accept receipt input");
    assert_eq!(error, "--receipt cannot be used with --draft-template");
}

fn approved_receipt_json(packet: &str) -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "receipt_kind": "aex_loader_manual_approval_receipt",
        "approval_receipt_id": "aex-loader-review-local-001",
        "approval_status": "approved",
        "approved_by": "operator",
        "approved_at_utc": "2026-06-01T00:00:00Z",
        "approval_scope": {
            "loader_slice_review_packet_status": "ready_for_manual_loader_slice_review_no_load",
            "loader_slice_review_packet_checksum_algorithm": "fnv1a64-v1-noncryptographic",
            "loader_slice_review_packet_checksum_hex": fnv1a64_hex(packet.as_bytes()),
            "fixture_gate_status_required": "review_queue_approved_local_only",
            "selected_candidate_review_status_required": "approved-local-only",
            "selected_candidate_fixture_status_required": "local-build-candidate",
            "selected_candidate_plugin_class_required": "classic-effect-candidate",
            "operation_scope": "render_png_first_loader_review",
            "ofx_route_scope": "deferred_not_approved"
        },
        "prerequisite_reports": {
            "loader_slice_review_packet_reviewed": true,
            "fixture_gate_reviewed": true,
            "loader_manifest_reviewed": true,
            "provenance_audit_reviewed": true,
            "worker_identity_revalidation_reviewed": true,
            "sandbox_preflight_reviewed": true,
            "worker_attestation_reviewed": true,
            "job_object_reviewed": true,
            "handle_inheritance_reviewed": true,
            "license_reviewed": true,
            "cleanroom_reviewed": true,
            "generated_target_exclusion_reviewed": true
        },
        "approval_effect": {
            "allow_separate_loader_implementation_review": true,
            "allow_native_aex_load": false,
            "allow_worker_plugin_load": false,
            "allow_broker_may_load_aex": false,
            "allow_render_png": false,
            "allow_ofx_route": false,
            "allow_aex_sdk_or_abi_import": false,
            "allow_private_path_publication": false
        },
        "safety_acknowledgements": {
            "local_fixture_reviewed": true,
            "license_reviewed": true,
            "cleanroom_reviewed": true,
            "worker_isolation_reviewed": true,
            "ofx_deferred_acknowledged": true,
            "native_load_still_forbidden": true,
            "render_still_forbidden": true,
            "private_path_publication_forbidden": true
        }
    }))
    .unwrap()
}

fn ready_packet_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "ready_for_manual_loader_slice_review_no_load",
        "native_load_performed": false,
        "loader_slice_approved": false,
        "loader_enabled": false,
        "real_aex_load_enabled": false,
        "native_loader_calls_allowed": false,
        "broker_may_load_aex": false,
        "worker_may_load_plugin": false,
        "render_performed": false,
        "ofx_route_allowed": false,
        "input_contains_forbidden_tokens": false,
        "fixture_gate_summary": {
            "schema_version": 1,
            "status": "review_queue_approved_local_only",
            "publication_status": "local-only design artifact",
            "selected_fixture_present": true,
            "recommended_first_review_present": true,
            "recommendation_status": "manual-selection-recorded",
            "approval_approved": true,
            "approval_loader_enabled": true,
            "approval_real_aex_load_enabled": true,
            "approval_render_png_enabled": true,
            "approval_describe_enabled_for_real_aex": true,
            "candidate_count": 2,
            "local_build_candidate_count": 2,
            "generated_target_candidate_count": 0,
            "selected_candidate_present": true,
            "selected_candidate_review_status": "approved-local-only",
            "selected_candidate_fixture_status": "local-build-candidate",
            "selected_candidate_plugin_class": "classic-effect-candidate",
            "selected_candidate_source_license_evidence_present": true,
            "selected_candidate_blocked_reason_count": 0,
            "rejected_first_loader_class_count": 6,
            "single_fixture_policy_ready": true,
            "runtime_evidence_ready": true,
            "input_contains_forbidden_tokens": false
        },
        "manifest_summary": {
            "status": "ready_for_separate_loader_implementation_review_no_load",
            "native_load_performed": false,
            "broker_may_load_plugin": false,
            "loader_may_load_plugin": false,
            "ofx_may_route_to_loader": false,
            "selected_fixture_present": true,
            "selected_effect_id_present": true,
            "selected_plugin_path_redacted": true,
            "ready_for_separate_loader_slice_review": true,
            "native_loader_calls_allowed": false,
            "broker_may_load_aex": false,
            "ofx_facade_may_route_to_loader": false,
            "requires_explicit_user_approval": true,
            "requires_code_review": true,
            "requires_local_fixture_only": true,
            "readiness_provided": true,
            "readiness_status": "probe_readiness_planned",
            "fixture_refresh_audit_provided": true,
            "fixture_refresh_audit_status": "fixture_gate_refresh_ready_no_load",
            "blocked_reason_count": 0,
            "input_contains_forbidden_tokens": false
        },
        "provenance_summary": {
            "status": "no_load_provenance_chain_ready",
            "native_load_performed": false,
            "selectors_executed": false,
            "render_performed": false,
            "ofx_route_allowed": false,
            "evidence_contains_forbidden_tokens": false,
            "loader_manifest_ready": true,
            "native_stage_plan_ready": true,
            "ofx_readiness_ready": true,
            "runtime_and_cleanroom_ready": true,
            "fixture_identity_smoke_provided": true,
            "fixture_identity_smoke_ready": true,
            "fixture_identity_smoke_broker_invoked": true,
            "fixture_identity_smoke_aex_render_correctness_evidence": false,
            "fixture_identity_smoke_input_contains_forbidden_tokens": false,
            "blocked_reason_count": 0,
            "input_contains_forbidden_tokens": false
        },
        "review_requirements": {
            "separate_loader_slice_required": true,
            "explicit_user_approval_required": true,
            "code_review_required": true,
            "local_build_classic_effect_fixture_required": true,
            "cleanroom_boundary_required": true,
            "license_review_required": true,
            "worker_isolation_evidence_required": true,
            "ofx_facade_review_deferred": true,
            "generated_target_fixtures_forbidden": true
        },
        "checks": [
            {"name": "fixture_gate_manual_approval_ready_no_load", "status": "passed"},
            {"name": "loader_manifest_ready_no_load", "status": "passed"},
            {"name": "provenance_chain_ready_no_load", "status": "passed"},
            {"name": "fixture_identity_smoke_preserved_no_load", "status": "passed"},
            {"name": "manual_review_requirements_closed", "status": "passed"},
            {"name": "no_execution_or_route_permission", "status": "passed"},
            {"name": "evidence_anti_contamination", "status": "passed"}
        ],
        "blocked_reasons": [],
        "next_action": "Use this as handoff evidence for a separate loader implementation review; do not enable native loading in this packet.",
        "notes": [
            "Loader slice review packet reads JSON metadata only.",
            "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.",
            "This packet is not loader approval and does not permit worker, broker, or OFX execution.",
            "Private plugin paths from upstream manifests are not serialized in this packet."
        ]
    }))
    .unwrap()
}
