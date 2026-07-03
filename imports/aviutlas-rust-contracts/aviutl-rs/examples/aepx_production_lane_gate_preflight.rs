//! Metadata-only preflight for the AEPX production-lane gate.
//!
//! This checker reads the local production-lane gate schema and verifies that
//! write-enabled `aepx_patch_probe apply` remains closed. It does not read XML
//! bodies, accept real `.aepx` input, launch After Effects, invoke external
//! processes, or write AEPX files.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

const REPORT_KIND: &str = "aepx_production_lane_gate_preflight";
const READY_STATUS: &str = "apply_gate_closed_ready";
const BLOCKED_STATUS: &str = "blocked_gate_drift";

const CURRENT_FALSE_FIELDS: &[&str] = &[
    "aepx_patch_probe_apply_enabled",
    "real_aepx_input_enabled",
    "production_xml_writer_enabled",
    "binary_aep_writer_enabled",
    "source_overwrite_enabled",
    "after_effects_tool_launch_enabled",
    "external_process_invocation_in_writer_enabled",
];

const MUST_REMAIN_FALSE_FIELDS: &[&str] = &[
    "aepx_patch_probe_apply_enabled",
    "real_aepx_input_enabled",
    "production_xml_writer_enabled",
    "xml_body_write_performed",
    "output_write_performed",
];

const REQUIRED_EVIDENCE: &[&str] = &[
    "synthetic_preservation_proof_contract_green",
    "synthetic_fixture_matrix_covers_scanner_encoding_path_and_multiop_boundaries",
    "production_writer_path_boundary_contract_green",
    "production_writer_exact_byte_diff_contract_green",
    "production_writer_all_or_nothing_contract_green",
    "production_report_privacy_contract_green",
    "direct_xml_dependency_license_audit_green_or_no_new_dependency",
    "real_aepx_fixture_review_receipt_local_only",
    "manual_ae_smoke_protocol_reviewed_if_claiming_ae_acceptance",
    "parent_approval_receipt_for_apply_slice",
];

const FAIL_CLOSED_STATUSES: &[&str] = &[
    "invalid_request",
    "source_not_found",
    "output_exists",
    "output_same_as_source",
    "unsupported_operation",
    "ambiguous_target",
    "target_not_found",
    "expected_value_mismatch",
    "parse_error",
    "preservation_failed",
    "write_failed",
];

const HARD_FORBIDDEN: &[&str] = &[
    "binary .aep parsing",
    "binary .aep writing",
    "source overwrite",
    "automated After Effects launch",
    "external process invocation inside writer",
    "name-only selector apply",
    "ambiguous selector apply",
    "text-node edits",
    "CDATA edits",
    "node insertion",
    "node deletion",
    "namespace rewriting",
    "entity normalization",
    ".aex loading",
    "OFX routing",
];

#[derive(Debug, Serialize)]
struct ProductionLaneGatePreflightReport {
    schema_version: u32,
    report_kind: String,
    publication_status: String,
    status: String,
    production_apply_enabled: bool,
    real_aepx_input_enabled: bool,
    production_xml_writer_enabled: bool,
    binary_aep_writer_enabled: bool,
    source_overwrite_enabled: bool,
    after_effects_tool_launch_enabled: bool,
    external_process_invocation_in_writer_enabled: bool,
    allowed_first_candidate: AllowedFirstCandidateSummary,
    evidence_gate: EvidenceGateSummary,
    io_gate: IoGate,
    write_gate: WriteGate,
    privacy_gate: PrivacyGate,
    checks: Vec<PreflightCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AllowedFirstCandidateSummary {
    operation_kind: Option<String>,
    selector_kind: Option<String>,
    target_count: Option<u64>,
    expected_old_value_required: Option<bool>,
    replacement_class: Option<String>,
    create_new_output_only: Option<bool>,
    source_overwrite_allowed: Option<bool>,
}

#[derive(Debug, Serialize)]
struct EvidenceGateSummary {
    required_green_evidence_count: usize,
    missing_required_green_evidence: Vec<String>,
    fail_closed_status_count: usize,
    missing_fail_closed_statuses: Vec<String>,
    hard_forbidden_count: usize,
    missing_hard_forbidden_items: Vec<String>,
    dry_run_evidence_accepted_as_apply_permission: Option<bool>,
    synthetic_proof_accepted_as_ae_compatibility: Option<bool>,
    requires_parent_approval: Option<bool>,
    requires_code_review: Option<bool>,
    requires_targeted_contract_tests: Option<bool>,
    requires_real_aepx_fixture_receipt: Option<bool>,
}

#[derive(Debug, Serialize)]
struct IoGate {
    xml_body_read_performed: bool,
    xml_body_write_performed: bool,
    binary_aep_read_performed: bool,
    binary_aep_write_performed: bool,
    external_process_invoked: bool,
    after_effects_invoked: bool,
}

#[derive(Debug, Serialize)]
struct WriteGate {
    output_write_performed: bool,
    source_overwrite_performed: bool,
    production_apply_enabled: bool,
    real_aepx_input_enabled: bool,
    production_xml_writer_enabled: bool,
    binary_aep_writer_enabled: bool,
}

#[derive(Debug, Serialize)]
struct PrivacyGate {
    xml_body_embedded: bool,
    private_patch_payloads_embedded: bool,
    selector_values_embedded: bool,
    expected_old_values_embedded: bool,
    new_values_embedded: bool,
    sentinel_values_embedded: bool,
    metadata_only_binding_required: Option<bool>,
}

#[derive(Debug, Serialize)]
struct PreflightCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn run_production_lane_gate_preflight_json(
    gate_schema_json: &str,
) -> Result<String, Box<dyn Error>> {
    let gate_schema: Value = serde_json::from_str(gate_schema_json)?;
    let report = run_preflight(&gate_schema);
    Ok(serde_json::to_string_pretty(&report)?)
}

fn run_preflight(gate_schema: &Value) -> ProductionLaneGatePreflightReport {
    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "schema_version",
        gate_schema["schema_version"].as_u64() == Some(1),
        format!(
            "schema_version={}",
            display_json(&gate_schema["schema_version"])
        ),
        "production lane gate schema_version must be 1",
    );
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "publication_status",
        gate_schema["publication_status"].as_str() == Some("local-only gate artifact"),
        format!(
            "publication_status={}",
            display_json(&gate_schema["publication_status"])
        ),
        "production lane gate must remain a local-only gate artifact",
    );

    let current_state = &gate_schema["current_state"];
    for field in CURRENT_FALSE_FIELDS {
        push_false_field_check(
            &mut checks,
            &mut blocked_reasons,
            "current_state",
            current_state,
            field,
        );
    }

    let must_remain_false = &gate_schema["must_remain_false_until_all_required_evidence_is_green"];
    for field in MUST_REMAIN_FALSE_FIELDS {
        push_false_field_check(
            &mut checks,
            &mut blocked_reasons,
            "must_remain_false_until_all_required_evidence_is_green",
            must_remain_false,
            field,
        );
    }

    let allowed_first_candidate = allowed_first_candidate_summary(gate_schema);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "allowed_first_candidate_scope",
        allowed_first_candidate.operation_kind.as_deref() == Some("rename_comp")
            && allowed_first_candidate.selector_kind.as_deref() == Some("xml_id")
            && allowed_first_candidate.target_count == Some(1)
            && allowed_first_candidate.expected_old_value_required == Some(true)
            && allowed_first_candidate.create_new_output_only == Some(true)
            && allowed_first_candidate.source_overwrite_allowed == Some(false),
        format!(
            "operation={}, selector={}, target_count={}, create_new_only={}, source_overwrite_allowed={}",
            display_optional(&allowed_first_candidate.operation_kind),
            display_optional(&allowed_first_candidate.selector_kind),
            allowed_first_candidate
                .target_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "missing".to_string()),
            display_optional_bool(allowed_first_candidate.create_new_output_only),
            display_optional_bool(allowed_first_candidate.source_overwrite_allowed),
        ),
        "first apply candidate scope is wider than exact xml_id rename_comp create-new only",
    );

    let evidence_gate = evidence_gate_summary(gate_schema);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "required_green_evidence_catalog",
        evidence_gate.missing_required_green_evidence.is_empty(),
        format!(
            "required_green_evidence_count={}, missing_count={}",
            evidence_gate.required_green_evidence_count,
            evidence_gate.missing_required_green_evidence.len()
        ),
        "production lane gate is missing required evidence items",
    );
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "fail_closed_status_catalog",
        evidence_gate.missing_fail_closed_statuses.is_empty(),
        format!(
            "fail_closed_status_count={}, missing_count={}",
            evidence_gate.fail_closed_status_count,
            evidence_gate.missing_fail_closed_statuses.len()
        ),
        "production lane gate is missing fail-closed statuses",
    );
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "hard_forbidden_catalog",
        evidence_gate.missing_hard_forbidden_items.is_empty(),
        format!(
            "hard_forbidden_count={}, missing_count={}",
            evidence_gate.hard_forbidden_count,
            evidence_gate.missing_hard_forbidden_items.len()
        ),
        "production lane gate is missing hard-forbidden first-slice items",
    );
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "transition_rule",
        evidence_gate.requires_parent_approval == Some(true)
            && evidence_gate.requires_code_review == Some(true)
            && evidence_gate.requires_targeted_contract_tests == Some(true)
            && evidence_gate.requires_real_aepx_fixture_receipt == Some(true)
            && evidence_gate.dry_run_evidence_accepted_as_apply_permission == Some(false)
            && evidence_gate.synthetic_proof_accepted_as_ae_compatibility == Some(false),
        format!(
            "parent_approval={}, code_review={}, targeted_tests={}, real_fixture_receipt={}, dry_run_as_apply={}, synthetic_as_ae={}",
            display_optional_bool(evidence_gate.requires_parent_approval),
            display_optional_bool(evidence_gate.requires_code_review),
            display_optional_bool(evidence_gate.requires_targeted_contract_tests),
            display_optional_bool(evidence_gate.requires_real_aepx_fixture_receipt),
            display_optional_bool(evidence_gate.dry_run_evidence_accepted_as_apply_permission),
            display_optional_bool(evidence_gate.synthetic_proof_accepted_as_ae_compatibility),
        ),
        "transition rule would allow apply without the required parent-approved production slice",
    );

    let privacy_gate = privacy_gate(gate_schema);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "report_privacy_boundary",
        !privacy_gate.xml_body_embedded
            && !privacy_gate.private_patch_payloads_embedded
            && !privacy_gate.selector_values_embedded
            && !privacy_gate.expected_old_values_embedded
            && !privacy_gate.new_values_embedded
            && !privacy_gate.sentinel_values_embedded
            && privacy_gate.metadata_only_binding_required == Some(true),
        format!(
            "xml_body_embedded={}, private_payloads_embedded={}, metadata_only_binding_required={}",
            privacy_gate.xml_body_embedded,
            privacy_gate.private_patch_payloads_embedded,
            display_optional_bool(privacy_gate.metadata_only_binding_required)
        ),
        "production lane gate privacy boundary would allow private payload reporting",
    );

    let ready = blocked_reasons.is_empty();
    ProductionLaneGatePreflightReport {
        schema_version: 1,
        report_kind: REPORT_KIND.to_string(),
        publication_status: "local-only".to_string(),
        status: if ready {
            READY_STATUS.to_string()
        } else {
            BLOCKED_STATUS.to_string()
        },
        production_apply_enabled: bool_at(current_state, "aepx_patch_probe_apply_enabled"),
        real_aepx_input_enabled: bool_at(current_state, "real_aepx_input_enabled"),
        production_xml_writer_enabled: bool_at(current_state, "production_xml_writer_enabled"),
        binary_aep_writer_enabled: bool_at(current_state, "binary_aep_writer_enabled"),
        source_overwrite_enabled: bool_at(current_state, "source_overwrite_enabled"),
        after_effects_tool_launch_enabled: bool_at(
            current_state,
            "after_effects_tool_launch_enabled",
        ),
        external_process_invocation_in_writer_enabled: bool_at(
            current_state,
            "external_process_invocation_in_writer_enabled",
        ),
        allowed_first_candidate,
        evidence_gate,
        io_gate: IoGate {
            xml_body_read_performed: false,
            xml_body_write_performed: false,
            binary_aep_read_performed: false,
            binary_aep_write_performed: false,
            external_process_invoked: false,
            after_effects_invoked: false,
        },
        write_gate: WriteGate {
            output_write_performed: false,
            source_overwrite_performed: false,
            production_apply_enabled: bool_at(current_state, "aepx_patch_probe_apply_enabled"),
            real_aepx_input_enabled: bool_at(current_state, "real_aepx_input_enabled"),
            production_xml_writer_enabled: bool_at(current_state, "production_xml_writer_enabled"),
            binary_aep_writer_enabled: bool_at(current_state, "binary_aep_writer_enabled"),
        },
        privacy_gate,
        checks,
        blocked_reasons,
        next_action: if ready {
            "Keep production AEPX apply closed until a separate parent-approved write-enabled slice satisfies every required evidence item.".to_string()
        } else {
            "Fix the production-lane gate artifact before using it to plan any write-enabled AEPX apply slice.".to_string()
        },
        notes: vec![
            "This preflight reads the local production-lane gate schema only.".to_string(),
            "It does not read XML bodies, accept real .aepx input, write AEPX output, launch AE, or invoke external processes.".to_string(),
            "A ready report means the apply gate is still closed and well-specified; it is not permission to enable apply.".to_string(),
            "Dry-run and synthetic preservation proof evidence remain review inputs only.".to_string(),
            "Binary .aep remains no-read/no-write in this AEPX production-lane preflight.".to_string(),
        ],
    }
}

fn push_false_field_check(
    checks: &mut Vec<PreflightCheck>,
    blocked_reasons: &mut Vec<String>,
    object_name: &str,
    object: &Value,
    field: &str,
) {
    push_check(
        checks,
        blocked_reasons,
        &format!("{object_name}.{field}"),
        object[field].as_bool() == Some(false),
        format!("{object_name}.{field}={}", display_json(&object[field])),
        &format!("{object_name}.{field} must remain false"),
    );
}

fn push_check(
    checks: &mut Vec<PreflightCheck>,
    blocked_reasons: &mut Vec<String>,
    name: &str,
    passed: bool,
    evidence: String,
    blocked_reason: &str,
) {
    checks.push(PreflightCheck {
        name: name.to_string(),
        status: if passed { "passed" } else { "failed" }.to_string(),
        evidence,
    });
    if !passed {
        blocked_reasons.push(blocked_reason.to_string());
    }
}

fn allowed_first_candidate_summary(gate_schema: &Value) -> AllowedFirstCandidateSummary {
    let candidate = &gate_schema["allowed_first_candidate"];
    AllowedFirstCandidateSummary {
        operation_kind: string_at(candidate, "operation_kind"),
        selector_kind: string_at(candidate, "selector_kind"),
        target_count: candidate["target_count"].as_u64(),
        expected_old_value_required: candidate["expected_old_value_required"].as_bool(),
        replacement_class: string_at(candidate, "replacement_class"),
        create_new_output_only: candidate["create_new_output_only"].as_bool(),
        source_overwrite_allowed: candidate["source_overwrite_allowed"].as_bool(),
    }
}

fn evidence_gate_summary(gate_schema: &Value) -> EvidenceGateSummary {
    let required_green = string_array(&gate_schema["required_green_evidence_before_apply"]);
    let fail_closed = string_array(&gate_schema["fail_closed_statuses"]);
    let hard_forbidden = string_array(&gate_schema["hard_forbidden_in_first_apply_slice"]);
    let transition_rule = &gate_schema["transition_rule"];
    EvidenceGateSummary {
        required_green_evidence_count: required_green.len(),
        missing_required_green_evidence: missing_items(REQUIRED_EVIDENCE, &required_green),
        fail_closed_status_count: fail_closed.len(),
        missing_fail_closed_statuses: missing_items(FAIL_CLOSED_STATUSES, &fail_closed),
        hard_forbidden_count: hard_forbidden.len(),
        missing_hard_forbidden_items: missing_items(HARD_FORBIDDEN, &hard_forbidden),
        dry_run_evidence_accepted_as_apply_permission: transition_rule
            ["dry_run_evidence_accepted_as_apply_permission"]
            .as_bool(),
        synthetic_proof_accepted_as_ae_compatibility: transition_rule
            ["synthetic_proof_accepted_as_ae_compatibility"]
            .as_bool(),
        requires_parent_approval: transition_rule["requires_new_parent_approved_slice"].as_bool(),
        requires_code_review: transition_rule["requires_code_review"].as_bool(),
        requires_targeted_contract_tests: transition_rule["requires_targeted_contract_tests"]
            .as_bool(),
        requires_real_aepx_fixture_receipt: transition_rule
            ["requires_manual_receipt_for_real_aepx_fixture"]
            .as_bool(),
    }
}

fn privacy_gate(gate_schema: &Value) -> PrivacyGate {
    let privacy = &gate_schema["report_privacy_boundary"];
    PrivacyGate {
        xml_body_embedded: bool_at(privacy, "xml_body_embedded"),
        private_patch_payloads_embedded: bool_at(privacy, "private_patch_payloads_embedded"),
        selector_values_embedded: bool_at(privacy, "selector_values_embedded"),
        expected_old_values_embedded: bool_at(privacy, "expected_old_values_embedded"),
        new_values_embedded: bool_at(privacy, "new_values_embedded"),
        sentinel_values_embedded: bool_at(privacy, "sentinel_values_embedded"),
        metadata_only_binding_required: privacy["metadata_only_binding_required"].as_bool(),
    }
}

fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect()
}

fn missing_items(required: &[&str], observed: &[String]) -> Vec<String> {
    required
        .iter()
        .filter(|item| !observed.iter().any(|observed| observed == **item))
        .map(|item| (*item).to_string())
        .collect()
}

fn bool_at(value: &Value, field: &str) -> bool {
    value[field].as_bool().unwrap_or(false)
}

fn string_at(value: &Value, field: &str) -> Option<String> {
    value[field].as_str().map(str::to_string)
}

fn display_json(value: &Value) -> String {
    if value.is_null() {
        "missing".to_string()
    } else {
        value.to_string()
    }
}

fn display_optional(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "missing".to_string())
}

fn display_optional_bool(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "missing".to_string())
}

pub fn validate_preflight_report_output_path(path: &Path) -> Result<(), String> {
    if path_has_traversal(path) {
        return Err("report path must not contain traversal components".to_string());
    }
    if !path_has_extension(path, "json") {
        return Err("report path must have .json extension".to_string());
    }
    if !output_is_under_target_root(path) {
        return Err(
            "report path must be under target/aepx-production-lane-gate-preflight".to_string(),
        );
    }
    Ok(())
}

fn write_report_create_new(
    path: &Path,
    report: &ProductionLaneGatePreflightReport,
) -> Result<(), Box<dyn Error>> {
    validate_preflight_report_output_path(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !output_parent_canonical_is_under_target_root(path) {
        return Err(
            "report parent must resolve under target/aepx-production-lane-gate-preflight".into(),
        );
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| {
            if err.kind() == ErrorKind::AlreadyExists {
                std::io::Error::new(ErrorKind::AlreadyExists, "report already exists")
            } else {
                err
            }
        })?;
    file.write_all(serde_json::to_string_pretty(report)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

fn output_parent_canonical_is_under_target_root(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(root) = std::fs::canonicalize(target_root()) else {
        return false;
    };
    let Ok(parent) = std::fs::canonicalize(parent) else {
        return false;
    };
    parent.starts_with(root)
}

fn target_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-production-lane-gate-preflight")
}

fn output_is_under_target_root(path: &Path) -> bool {
    absolute_like(path).starts_with(absolute_like(&target_root()))
}

fn absolute_like(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}

fn path_has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn path_has_traversal(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    })
}

fn default_gate_schema_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .join("analysis")
        .join("AEPX_PRODUCTION_LANE_GATE_SCHEMA_2026-06-01.json")
}

fn default_report_path() -> PathBuf {
    target_root().join("preflight.local.json")
}

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let mut gate_schema = default_gate_schema_path();
    let mut report = default_report_path();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--gate-schema" => {
                gate_schema = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--gate-schema requires a path".to_string())?;
            }
            "--report" => {
                report = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--report requires a path".to_string())?;
            }
            "--help" | "-h" => return Err(usage()),
            other => return Err(format!("unknown argument {other}\n{}", usage())),
        }
    }
    Ok((gate_schema, report))
}

fn usage() -> String {
    "usage: aepx_production_lane_gate_preflight [--gate-schema <schema.json>] [--report <report.json>]".to_string()
}

fn main() -> Result<(), Box<dyn Error>> {
    let (gate_schema_path, report_path) =
        parse_args().map_err(|message| std::io::Error::new(ErrorKind::InvalidInput, message))?;
    let gate_schema_json = std::fs::read_to_string(&gate_schema_path)?;
    let gate_schema: Value = serde_json::from_str(&gate_schema_json)?;
    let report = run_preflight(&gate_schema);
    write_report_create_new(&report_path, &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
