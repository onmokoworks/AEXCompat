//! Validate explicit human approval receipts for AEX loader implementation review.
//!
//! This validator reads JSON artifacts only. It does not open, hash, copy,
//! load, execute, describe, or render `.aex` binaries. An accepted receipt
//! approves only the next separate loader implementation review slice; it does
//! not enable native loading, worker plug-in loading, render, or OFX routing.

use serde::Serialize;
use serde_json::json;
use serde_json::Value;
use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct LoaderApprovalValidationReport {
    schema_version: u32,
    report_kind: String,
    validation_status: String,
    approval_accepted: bool,
    loader_review_approved: bool,
    native_load_performed: bool,
    loader_enabled: bool,
    real_aex_load_enabled: bool,
    native_loader_calls_allowed: bool,
    worker_may_load_plugin: bool,
    broker_may_load_aex: bool,
    render_performed: bool,
    ofx_route_allowed: bool,
    input_contains_forbidden_tokens: bool,
    loader_slice_review_packet_checked: bool,
    loader_slice_review_packet_status: Option<String>,
    loader_slice_review_packet_checksum_algorithm: String,
    loader_slice_review_packet_checksum_hex: String,
    fixture_gate_status: Option<String>,
    fixture_selected_for_review: bool,
    fixture_candidate_review_status: Option<String>,
    fixture_candidate_fixture_status: Option<String>,
    fixture_candidate_plugin_class: Option<String>,
    runtime_and_cleanroom_ready: bool,
    receipt_kind: Option<String>,
    receipt_approval_status: Option<String>,
    receipt_allows_separate_loader_implementation_review: bool,
    receipt_allows_native_aex_load: bool,
    receipt_allows_worker_plugin_load: bool,
    receipt_allows_render_png: bool,
    receipt_allows_ofx_route: bool,
    blocked_reasons: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ApprovalCliCommand {
    Validate {
        packet: PathBuf,
        receipt: PathBuf,
        out: PathBuf,
    },
    DraftTemplate {
        packet: PathBuf,
        out: PathBuf,
    },
}

pub fn validate_loader_approval_receipt_json(
    loader_slice_review_packet_json: &str,
    approval_receipt_json: &str,
) -> Result<String, Box<dyn Error>> {
    let packet_text = strip_json_bom(loader_slice_review_packet_json);
    let receipt_text = strip_json_bom(approval_receipt_json);
    let packet: Value = serde_json::from_str(packet_text)?;
    let receipt: Value = serde_json::from_str(receipt_text)?;
    let report =
        validate_loader_approval_receipt_values(&packet, packet_text, &receipt, receipt_text);
    Ok(serde_json::to_string_pretty(&report)?)
}

pub fn draft_loader_approval_receipt_template_json(
    loader_slice_review_packet_json: &str,
) -> Result<String, Box<dyn Error>> {
    let packet_text = strip_json_bom(loader_slice_review_packet_json);
    let packet: Value = serde_json::from_str(packet_text)?;
    let mut blocked_reasons = Vec::new();
    validate_packet_ready(&packet, &mut blocked_reasons);
    if contains_forbidden_packet_evidence(&packet, packet_text) {
        blocked_reasons.push(
            "loader slice review packet contains forbidden private path, payload, output, or native-load evidence"
                .to_string(),
        );
    }
    if !blocked_reasons.is_empty() {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            format!(
                "loader approval receipt template requires a ready sanitized packet: {}",
                blocked_reasons.join("; ")
            ),
        )
        .into());
    }

    let packet_checksum = fnv1a64_hex(packet_text.as_bytes());
    let template = json!({
        "schema_version": 1,
        "receipt_kind": "aex_loader_manual_approval_receipt",
        "template_status": "draft_unapproved_template",
        "approval_receipt_id": null,
        "approval_status": "draft_unapproved",
        "approved_by": null,
        "approved_at_utc": null,
        "approval_scope": {
            "loader_slice_review_packet_status": "ready_for_manual_loader_slice_review_no_load",
            "loader_slice_review_packet_checksum_algorithm": "fnv1a64-v1-noncryptographic",
            "loader_slice_review_packet_checksum_hex": packet_checksum,
            "fixture_gate_status_required": "review_queue_approved_local_only",
            "selected_candidate_review_status_required": "approved-local-only",
            "selected_candidate_fixture_status_required": "local-build-candidate",
            "selected_candidate_plugin_class_required": "classic-effect-candidate",
            "operation_scope": "render_png_first_loader_review",
            "ofx_route_scope": "deferred_not_approved"
        },
        "prerequisite_reports": {
            "loader_slice_review_packet_reviewed": false,
            "fixture_gate_reviewed": false,
            "loader_manifest_reviewed": false,
            "provenance_audit_reviewed": false,
            "worker_identity_revalidation_reviewed": false,
            "sandbox_preflight_reviewed": false,
            "worker_attestation_reviewed": false,
            "job_object_reviewed": false,
            "handle_inheritance_reviewed": false,
            "license_reviewed": false,
            "cleanroom_reviewed": false,
            "generated_target_exclusion_reviewed": false
        },
        "approval_effect": {
            "allow_separate_loader_implementation_review": false,
            "allow_native_aex_load": false,
            "allow_worker_plugin_load": false,
            "allow_broker_may_load_aex": false,
            "allow_render_png": false,
            "allow_ofx_route": false,
            "allow_aex_sdk_or_abi_import": false,
            "allow_private_path_publication": false
        },
        "safety_acknowledgements": {
            "local_fixture_reviewed": false,
            "license_reviewed": false,
            "cleanroom_reviewed": false,
            "worker_isolation_reviewed": false,
            "ofx_deferred_acknowledged": false,
            "native_load_still_forbidden": true,
            "render_still_forbidden": true,
            "private_path_publication_forbidden": true
        },
        "template_generated_by_tool": true,
        "native_load_performed": false,
        "loader_enabled": false,
        "real_aex_load_enabled": false,
        "worker_may_load_plugin": false,
        "render_performed": false,
        "ofx_route_allowed": false,
        "notes": [
            "This template is not approval.",
            "A human must fill receipt id, approver, timestamp, review acknowledgements, and approval effect before validation can accept it.",
            "The filled receipt may approve only a separate loader implementation review slice.",
            "Native AEX loading, worker plug-in loading, render, and OFX routing must remain false in the receipt."
        ]
    });
    Ok(serde_json::to_string_pretty(&template)?)
}

fn validate_loader_approval_receipt_values(
    packet: &Value,
    packet_text: &str,
    receipt: &Value,
    receipt_text: &str,
) -> LoaderApprovalValidationReport {
    let mut blocked_reasons = Vec::new();
    let packet_checksum = fnv1a64_hex(packet_text.as_bytes());
    let packet_status = safe_string_at(packet, &["status"]);
    let fixture_gate_status = safe_string_at(packet, &["fixture_gate_summary", "status"]);
    let fixture_candidate_review_status = safe_string_at(
        packet,
        &["fixture_gate_summary", "selected_candidate_review_status"],
    );
    let fixture_candidate_fixture_status = safe_string_at(
        packet,
        &["fixture_gate_summary", "selected_candidate_fixture_status"],
    );
    let fixture_candidate_plugin_class = safe_string_at(
        packet,
        &["fixture_gate_summary", "selected_candidate_plugin_class"],
    );
    let runtime_and_cleanroom_ready = bool_at(
        packet,
        &["provenance_summary", "runtime_and_cleanroom_ready"],
    );

    validate_packet_ready(packet, &mut blocked_reasons);
    validate_receipt_shape(receipt, &mut blocked_reasons);
    validate_receipt_scope(receipt, packet, &packet_checksum, &mut blocked_reasons);
    validate_receipt_prerequisites(receipt, &mut blocked_reasons);
    validate_receipt_effect(receipt, &mut blocked_reasons);
    validate_safety_acknowledgements(receipt, &mut blocked_reasons);

    let input_contains_forbidden_tokens = contains_forbidden_packet_evidence(packet, packet_text)
        || contains_forbidden_receipt_evidence(receipt, receipt_text);
    if input_contains_forbidden_tokens {
        blocked_reasons.push(
            "loader approval inputs contain forbidden private path, payload, output, or native-load evidence"
                .to_string(),
        );
    }

    let approval_accepted = blocked_reasons.is_empty();
    LoaderApprovalValidationReport {
        schema_version: 1,
        report_kind: "aex_loader_approval_receipt_validation".to_string(),
        validation_status: if approval_accepted {
            "approved_for_loader_implementation_review_no_load".to_string()
        } else {
            "invalid_receipt".to_string()
        },
        approval_accepted,
        loader_review_approved: approval_accepted,
        native_load_performed: false,
        loader_enabled: false,
        real_aex_load_enabled: false,
        native_loader_calls_allowed: false,
        worker_may_load_plugin: false,
        broker_may_load_aex: false,
        render_performed: false,
        ofx_route_allowed: false,
        input_contains_forbidden_tokens,
        loader_slice_review_packet_checked: true,
        loader_slice_review_packet_status: packet_status,
        loader_slice_review_packet_checksum_algorithm: "fnv1a64-v1-noncryptographic".to_string(),
        loader_slice_review_packet_checksum_hex: packet_checksum,
        fixture_gate_status,
        fixture_selected_for_review: bool_at(
            packet,
            &["fixture_gate_summary", "selected_fixture_present"],
        ),
        fixture_candidate_review_status,
        fixture_candidate_fixture_status,
        fixture_candidate_plugin_class,
        runtime_and_cleanroom_ready,
        receipt_kind: safe_string_at(receipt, &["receipt_kind"]),
        receipt_approval_status: safe_string_at(receipt, &["approval_status"]),
        receipt_allows_separate_loader_implementation_review: bool_at(
            receipt,
            &[
                "approval_effect",
                "allow_separate_loader_implementation_review",
            ],
        ),
        receipt_allows_native_aex_load: bool_at(
            receipt,
            &["approval_effect", "allow_native_aex_load"],
        ),
        receipt_allows_worker_plugin_load: bool_at(
            receipt,
            &["approval_effect", "allow_worker_plugin_load"],
        ),
        receipt_allows_render_png: bool_at(receipt, &["approval_effect", "allow_render_png"]),
        receipt_allows_ofx_route: bool_at(receipt, &["approval_effect", "allow_ofx_route"]),
        blocked_reasons,
        notes: vec![
            "AEX loader approval receipt validation reads JSON metadata only.".to_string(),
            "An accepted receipt approves only a separate loader implementation review slice.".to_string(),
            "This validator does not enable native AEX loading, worker plug-in loading, render, or OFX routing.".to_string(),
            "Private plug-in paths and AEX static metadata labels must not be serialized in approval receipts or reports.".to_string(),
        ],
    }
}

fn validate_packet_ready(packet: &Value, blocked: &mut Vec<String>) {
    require(
        packet["schema_version"].as_u64() == Some(1),
        "loader slice review packet schema_version must be 1",
        blocked,
    );
    require(
        str_at(packet, &["status"]) == Some("ready_for_manual_loader_slice_review_no_load"),
        "loader slice review packet must be ready",
        blocked,
    );
    for field in [
        "native_load_performed",
        "loader_slice_approved",
        "loader_enabled",
        "real_aex_load_enabled",
        "native_loader_calls_allowed",
        "broker_may_load_aex",
        "worker_may_load_plugin",
        "render_performed",
        "ofx_route_allowed",
        "input_contains_forbidden_tokens",
    ] {
        require(
            !bool_at(packet, &[field]),
            &format!("loader slice review packet field {field} must be false"),
            blocked,
        );
    }
    require(
        json_array_len(&packet["blocked_reasons"]) == 0,
        "loader slice review packet must have no blocked reasons",
        blocked,
    );
    for check in [
        "fixture_gate_manual_approval_ready_no_load",
        "loader_manifest_ready_no_load",
        "provenance_chain_ready_no_load",
        "manual_review_requirements_closed",
        "no_execution_or_route_permission",
        "evidence_anti_contamination",
    ] {
        require(
            report_check_passed(&packet["checks"], check),
            &format!("loader slice review packet check {check} must be passed"),
            blocked,
        );
    }
    if bool_at(
        packet,
        &["provenance_summary", "fixture_identity_smoke_provided"],
    ) {
        require(
            report_check_passed(
                &packet["checks"],
                "fixture_identity_smoke_preserved_no_load",
            ),
            "fixture identity smoke check must be passed when smoke evidence is provided",
            blocked,
        );
    }

    let fixture = &packet["fixture_gate_summary"];
    require(
        str_at(fixture, &["status"]) == Some("review_queue_approved_local_only"),
        "fixture gate summary must be approved local-only",
        blocked,
    );
    for field in [
        "selected_fixture_present",
        "approval_approved",
        "approval_loader_enabled",
        "approval_real_aex_load_enabled",
        "approval_render_png_enabled",
        "approval_describe_enabled_for_real_aex",
        "selected_candidate_source_license_evidence_present",
        "single_fixture_policy_ready",
        "runtime_evidence_ready",
    ] {
        require(
            bool_at(fixture, &[field]),
            &format!("fixture gate summary field {field} must be true"),
            blocked,
        );
    }
    require(
        fixture["generated_target_candidate_count"].as_u64() == Some(0),
        "fixture gate summary must exclude generated target candidates",
        blocked,
    );
    require(
        str_at(fixture, &["selected_candidate_review_status"]) == Some("approved-local-only"),
        "selected fixture candidate must be approved-local-only",
        blocked,
    );
    require(
        str_at(fixture, &["selected_candidate_fixture_status"]) == Some("local-build-candidate"),
        "selected fixture candidate must be local-build-candidate",
        blocked,
    );
    require(
        str_at(fixture, &["selected_candidate_plugin_class"]) == Some("classic-effect-candidate"),
        "selected fixture candidate must be classic-effect-candidate",
        blocked,
    );
    require(
        !bool_at(fixture, &["input_contains_forbidden_tokens"]),
        "fixture gate summary must be sanitized",
        blocked,
    );

    require(
        str_at(packet, &["manifest_summary", "status"])
            == Some("ready_for_separate_loader_implementation_review_no_load"),
        "loader implementation manifest summary must be ready",
        blocked,
    );
    require(
        str_at(packet, &["provenance_summary", "status"]) == Some("no_load_provenance_chain_ready"),
        "no-load provenance summary must be ready",
        blocked,
    );
    require(
        bool_at(
            packet,
            &["provenance_summary", "runtime_and_cleanroom_ready"],
        ),
        "runtime and cleanroom provenance must be ready",
        blocked,
    );
    require(
        bool_at(packet, &["provenance_summary", "ofx_readiness_ready"]),
        "OFX readiness provenance must be ready and deferred",
        blocked,
    );
    for field in [
        "explicit_user_approval_required",
        "code_review_required",
        "local_build_classic_effect_fixture_required",
        "cleanroom_boundary_required",
        "license_review_required",
        "worker_isolation_evidence_required",
        "ofx_facade_review_deferred",
        "generated_target_fixtures_forbidden",
    ] {
        require(
            bool_at(packet, &["review_requirements", field]),
            &format!("review requirement {field} must be true"),
            blocked,
        );
    }
}

fn validate_receipt_shape(receipt: &Value, blocked: &mut Vec<String>) {
    require(
        receipt["schema_version"].as_u64() == Some(1),
        "approval receipt schema_version must be 1",
        blocked,
    );
    require(
        str_at(receipt, &["receipt_kind"]) == Some("aex_loader_manual_approval_receipt"),
        "approval receipt kind must be aex_loader_manual_approval_receipt",
        blocked,
    );
    require(
        str_at(receipt, &["approval_status"]) == Some("approved"),
        "approval_status must be approved",
        blocked,
    );
    for field in ["approval_receipt_id", "approved_by", "approved_at_utc"] {
        require(
            str_at(receipt, &[field])
                .map(|text| !text.trim().is_empty())
                .unwrap_or(false),
            &format!("{field} must be present for approved receipts"),
            blocked,
        );
    }
}

fn validate_receipt_scope(
    receipt: &Value,
    packet: &Value,
    packet_checksum: &str,
    blocked: &mut Vec<String>,
) {
    let scope = &receipt["approval_scope"];
    require(
        str_at(scope, &["loader_slice_review_packet_status"]) == str_at(packet, &["status"]),
        "approval scope must match loader slice review packet status",
        blocked,
    );
    require(
        str_at(scope, &["loader_slice_review_packet_checksum_algorithm"])
            == Some("fnv1a64-v1-noncryptographic"),
        "approval scope must use the current packet checksum algorithm",
        blocked,
    );
    require(
        str_at(scope, &["loader_slice_review_packet_checksum_hex"]) == Some(packet_checksum),
        "approval scope checksum must match the reviewed loader slice packet",
        blocked,
    );
    require(
        str_at(scope, &["fixture_gate_status_required"])
            == Some("review_queue_approved_local_only"),
        "approval scope must require approved local-only fixture gate",
        blocked,
    );
    require(
        str_at(scope, &["selected_candidate_review_status_required"])
            == Some("approved-local-only"),
        "approval scope must require approved-local-only selected candidate",
        blocked,
    );
    require(
        str_at(scope, &["selected_candidate_fixture_status_required"])
            == Some("local-build-candidate"),
        "approval scope must require local-build-candidate selected fixture",
        blocked,
    );
    require(
        str_at(scope, &["selected_candidate_plugin_class_required"])
            == Some("classic-effect-candidate"),
        "approval scope must require classic-effect-candidate selected fixture",
        blocked,
    );
    require(
        str_at(scope, &["operation_scope"]) == Some("render_png_first_loader_review"),
        "approval scope must be limited to the first render_png loader review",
        blocked,
    );
    require(
        str_at(scope, &["ofx_route_scope"]) == Some("deferred_not_approved"),
        "approval scope must keep OFX route deferred",
        blocked,
    );
}

fn validate_receipt_prerequisites(receipt: &Value, blocked: &mut Vec<String>) {
    let prerequisites = &receipt["prerequisite_reports"];
    for field in [
        "loader_slice_review_packet_reviewed",
        "fixture_gate_reviewed",
        "loader_manifest_reviewed",
        "provenance_audit_reviewed",
        "worker_identity_revalidation_reviewed",
        "sandbox_preflight_reviewed",
        "worker_attestation_reviewed",
        "job_object_reviewed",
        "handle_inheritance_reviewed",
        "license_reviewed",
        "cleanroom_reviewed",
        "generated_target_exclusion_reviewed",
    ] {
        require(
            bool_at(prerequisites, &[field]),
            &format!("prerequisite report acknowledgement {field} must be true"),
            blocked,
        );
    }
}

fn validate_receipt_effect(receipt: &Value, blocked: &mut Vec<String>) {
    let effect = &receipt["approval_effect"];
    require(
        bool_at(effect, &["allow_separate_loader_implementation_review"]),
        "approval effect must allow only a separate loader implementation review",
        blocked,
    );
    for field in [
        "allow_native_aex_load",
        "allow_worker_plugin_load",
        "allow_broker_may_load_aex",
        "allow_render_png",
        "allow_ofx_route",
        "allow_aex_sdk_or_abi_import",
        "allow_private_path_publication",
    ] {
        require(
            !bool_at(effect, &[field]),
            &format!("approval effect field {field} must remain false"),
            blocked,
        );
    }
}

fn validate_safety_acknowledgements(receipt: &Value, blocked: &mut Vec<String>) {
    let safety = &receipt["safety_acknowledgements"];
    for field in [
        "local_fixture_reviewed",
        "license_reviewed",
        "cleanroom_reviewed",
        "worker_isolation_reviewed",
        "ofx_deferred_acknowledged",
        "native_load_still_forbidden",
        "render_still_forbidden",
        "private_path_publication_forbidden",
    ] {
        require(
            bool_at(safety, &[field]),
            &format!("safety acknowledgement {field} must be true"),
            blocked,
        );
    }
}

fn contains_forbidden_packet_evidence(value: &Value, text: &str) -> bool {
    packet_text_contains_forbidden_tokens(text) || contains_forbidden_packet_fields(value)
}

fn contains_forbidden_receipt_evidence(value: &Value, text: &str) -> bool {
    receipt_text_contains_forbidden_tokens(text) || contains_forbidden_receipt_fields(value)
}

fn packet_text_contains_forbidden_tokens(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    forbidden_packet_tokens()
        .iter()
        .any(|token| text.contains(token))
}

fn receipt_text_contains_forbidden_tokens(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    forbidden_receipt_tokens()
        .iter()
        .any(|token| text.contains(token))
}

fn forbidden_packet_tokens() -> Vec<String> {
    vec![
        "sha256".to_string(),
        "binary_payload".to_string(),
        "base64".to_string(),
        ["load", "library"].concat(),
        ["lib", "loading"].concat(),
        "payload_bytes".to_string(),
        "copied_asset".to_string(),
        "native_load_result".to_string(),
        "rendered_pixels".to_string(),
        "worker_exe".to_string(),
        "input_png".to_string(),
        "output_png".to_string(),
        ["effect", "main"].concat(),
        "aeeffect".to_string(),
        "d:\\projects\\01_project\\04_tools\\ae_plugins".to_string(),
        "d:/projects/01_project/04_tools/ae_plugins".to_string(),
        "adaptivefilter.aex".to_string(),
        "medianpro.aex".to_string(),
    ]
}

fn forbidden_receipt_tokens() -> Vec<String> {
    vec![
        "sha256".to_string(),
        "binary_payload".to_string(),
        "base64".to_string(),
        ["load", "library"].concat(),
        ["lib", "loading"].concat(),
        "payload_bytes".to_string(),
        "copied_asset".to_string(),
        "native_load_result".to_string(),
        "rendered_pixels".to_string(),
        "worker_exe".to_string(),
        "input_png".to_string(),
        "output_png".to_string(),
        ".aex".to_string(),
        ["effect", "main"].concat(),
        "aeeffect".to_string(),
        "d:\\".to_string(),
        "d:/".to_string(),
    ]
}

fn contains_forbidden_packet_fields(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(field, child)| {
            forbidden_packet_field_names().contains(&field.as_str())
                || contains_forbidden_packet_fields(child)
        }),
        Value::Array(array) => array.iter().any(contains_forbidden_packet_fields),
        _ => false,
    }
}

fn contains_forbidden_receipt_fields(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(field, child)| {
            forbidden_receipt_field_names().contains(&field.as_str())
                || contains_forbidden_receipt_fields(child)
        }),
        Value::Array(array) => array.iter().any(contains_forbidden_receipt_fields),
        _ => false,
    }
}

fn forbidden_packet_field_names() -> Vec<&'static str> {
    vec![
        "binary_payload",
        "base64_payload",
        "payload_bytes",
        "copied_asset",
        "native_load_result",
        "rendered_pixels",
        "worker_exe",
        "input_png",
        "output_png",
    ]
}

fn forbidden_receipt_field_names() -> Vec<&'static str> {
    vec![
        "binary_payload",
        "base64_payload",
        "payload_bytes",
        "copied_asset",
        "native_load_result",
        "rendered_pixels",
        "worker_exe",
        "input_png",
        "output_png",
        "plugin_path",
        "normalized_plugin_path",
    ]
}

fn bool_at(value: &Value, path: &[&str]) -> bool {
    path.iter()
        .fold(value, |cursor, key| &cursor[*key])
        .as_bool()
        .unwrap_or(false)
}

fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .fold(value, |cursor, key| &cursor[*key])
        .as_str()
}

fn report_check_passed(checks: &Value, name: &str) -> bool {
    checks.as_array().into_iter().flatten().any(|check| {
        check["name"].as_str() == Some(name) && check["status"].as_str() == Some("passed")
    })
}

fn safe_string_at(value: &Value, path: &[&str]) -> Option<String> {
    let text = str_at(value, path)?;
    if text.trim().is_empty() || receipt_text_contains_forbidden_tokens(text) {
        return None;
    }
    Some(text.to_string())
}

fn json_array_len(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn require(condition: bool, reason: &str, blocked: &mut Vec<String>) {
    if !condition {
        blocked.push(reason.to_string());
    }
}

fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn strip_json_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

pub fn parse_args_from(
    args: impl IntoIterator<Item = String>,
) -> Result<ApprovalCliCommand, String> {
    let mut packet = None;
    let mut receipt = None;
    let mut draft_template = false;
    let mut out = PathBuf::from("target")
        .join("aex-loader-approval")
        .join("approval-validation.local.json");
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--packet" => {
                packet = args.next().map(PathBuf::from);
                if packet.is_none() {
                    return Err("--packet requires a JSON path".to_string());
                }
            }
            "--receipt" => {
                receipt = args.next().map(PathBuf::from);
                if receipt.is_none() {
                    return Err("--receipt requires a JSON path".to_string());
                }
            }
            "--draft-template" => {
                draft_template = true;
                out = PathBuf::from("target")
                    .join("aex-loader-approval")
                    .join("approval-receipt-template.local.json");
            }
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a JSON path".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: aex_loader_approval_receipt --packet target/aex-loader-slice-review/loader-slice-review.local.json --receipt path/to/aex-loader-approval-receipt.local.json [--out target/aex-loader-approval/approval-validation.local.json]\n       aex_loader_approval_receipt --draft-template --packet target/aex-loader-slice-review/loader-slice-review.local.json [--out target/aex-loader-approval/approval-receipt-template.local.json]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let packet = packet.ok_or_else(|| "--packet is required".to_string())?;
    if draft_template {
        if receipt.is_some() {
            return Err("--receipt cannot be used with --draft-template".to_string());
        }
        Ok(ApprovalCliCommand::DraftTemplate { packet, out })
    } else {
        Ok(ApprovalCliCommand::Validate {
            packet,
            receipt: receipt.ok_or_else(|| "--receipt is required".to_string())?,
            out,
        })
    }
}

fn parse_args() -> Result<ApprovalCliCommand, String> {
    parse_args_from(std::env::args().skip(1))
}

fn main() -> Result<(), Box<dyn Error>> {
    let command = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let (out_path, output) = match command {
        ApprovalCliCommand::Validate {
            packet,
            receipt,
            out,
        } => {
            let packet = std::fs::read_to_string(&packet)?;
            let receipt = std::fs::read_to_string(&receipt)?;
            (
                out,
                validate_loader_approval_receipt_json(&packet, &receipt)?,
            )
        }
        ApprovalCliCommand::DraftTemplate { packet, out } => {
            let packet = std::fs::read_to_string(&packet)?;
            (out, draft_loader_approval_receipt_template_json(&packet)?)
        }
    };
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, output)?;
    println!("{}", out_path.display());
    Ok(())
}
