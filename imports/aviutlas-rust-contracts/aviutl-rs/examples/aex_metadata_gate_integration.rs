//! Parent-level AEX metadata gate integration report.
//!
//! This joins the closed fixture gate, readiness planner, loader gate, synthetic
//! fixture manifest, identity smoke, and OFX facade contract as metadata only.
//! It does not open, copy, hash, load, describe, execute, render, or route any
//! `.aex` file.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

const REPORT_KIND: &str = "aex_metadata_gate_integration";
const READY_STATUS: &str = "aex_metadata_gate_ready_no_load";
const BLOCKED_STATUS: &str = "blocked_aex_metadata_gate";

#[derive(Debug, Serialize)]
struct AexMetadataGateIntegrationReport {
    schema_version: u32,
    report_kind: String,
    publication_status: String,
    status: String,
    native_load_performed: bool,
    render_performed: bool,
    aex_loaded: bool,
    worker_started: bool,
    broker_load_or_render_allowed: bool,
    ofx_route_allowed: bool,
    ae_invoked: bool,
    private_payload_copied: bool,
    fixture_gate_summary: FixtureGateSummary,
    readiness_summary: ReadinessSummary,
    loader_gate_summary: LoaderGateSummary,
    synthetic_fixture_summary: SyntheticFixtureSummary,
    identity_smoke_summary: IdentitySmokeSummary,
    ofx_contract_summary: OfxContractSummary,
    checks: Vec<IntegrationCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FixtureGateSummary {
    provided: bool,
    schema_version: Option<u64>,
    status: Option<String>,
    selected_fixture_present: bool,
    approval_approved: Option<bool>,
    approval_loader_enabled: Option<bool>,
    approval_real_aex_load_enabled: Option<bool>,
    approval_render_png_enabled: Option<bool>,
    approval_describe_enabled_for_real_aex: Option<bool>,
    candidate_count: usize,
    not_approved_candidate_count: usize,
}

#[derive(Debug, Serialize)]
struct ReadinessSummary {
    provided: bool,
    schema_version: Option<u64>,
    status: Option<String>,
    candidate_count: Option<u64>,
    allowlist_entry_count: Option<u64>,
    draft_describe_entry_count: usize,
    blocked_or_deferred_entry_count: usize,
}

#[derive(Debug, Serialize)]
struct LoaderGateSummary {
    provided: bool,
    schema_version: Option<u64>,
    status: Option<String>,
    approved: Option<bool>,
    loader_enabled: Option<bool>,
    real_aex_load_enabled: Option<bool>,
    open_candidate_count: Option<u64>,
    entry_count: usize,
    not_approved_entry_count: usize,
    describe_only_entry_count: usize,
    deferred_ofx_entry_count: usize,
}

#[derive(Debug, Serialize)]
struct SyntheticFixtureSummary {
    provided: bool,
    schema_version: Option<u64>,
    status: Option<String>,
    pixel_format: Option<String>,
    image_count: Option<u64>,
    native_load_performed: Option<bool>,
    render_performed: Option<bool>,
    aex_loaded: Option<bool>,
    worker_started: Option<bool>,
    broker_invoked: Option<bool>,
    ofx_route_invoked: Option<bool>,
    ae_invoked: Option<bool>,
    private_payload_copied: Option<bool>,
}

#[derive(Debug, Serialize)]
struct IdentitySmokeSummary {
    provided: bool,
    schema_version: Option<u64>,
    status: Option<String>,
    fixture_manifest_status: Option<String>,
    transport_operation: Option<String>,
    image_count: Option<u64>,
    transport_count: Option<u64>,
    identity_pixels_checked_count: Option<u64>,
    native_load_performed: Option<bool>,
    render_performed: Option<bool>,
    aex_loaded: Option<bool>,
    worker_started: Option<bool>,
    broker_invoked: Option<bool>,
    ofx_route_invoked: Option<bool>,
    ae_invoked: Option<bool>,
    private_payload_copied: Option<bool>,
    aex_render_correctness_evidence: Option<bool>,
}

#[derive(Debug, Serialize)]
struct OfxContractSummary {
    provided: bool,
    schema_version: Option<u64>,
    status: Option<String>,
    ofx_host_may_load_aex: Option<bool>,
    ofx_adapter_may_load_aex: Option<bool>,
    broker_may_load_aex: Option<bool>,
    review_approved: Option<bool>,
    may_point_to_broker: Option<bool>,
    may_issue_describe: Option<bool>,
    may_issue_render_png: Option<bool>,
}

#[derive(Debug, Serialize)]
struct IntegrationCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn integrate_aex_metadata_gate_json(
    fixture_gate_json: &str,
    readiness_json: &str,
    loader_gate_json: &str,
    synthetic_fixture_manifest_json: &str,
    identity_smoke_json: &str,
    ofx_contract_json: &str,
) -> Result<String, Box<dyn Error>> {
    let fixture_gate: Value = serde_json::from_str(fixture_gate_json)?;
    let readiness: Value = serde_json::from_str(readiness_json)?;
    let loader_gate: Value = serde_json::from_str(loader_gate_json)?;
    let synthetic_fixture_manifest: Value = serde_json::from_str(synthetic_fixture_manifest_json)?;
    let identity_smoke: Value = serde_json::from_str(identity_smoke_json)?;
    let ofx_contract: Value = serde_json::from_str(ofx_contract_json)?;
    let report = integrate_aex_metadata_gate(
        &fixture_gate,
        &readiness,
        &loader_gate,
        &synthetic_fixture_manifest,
        &identity_smoke,
        &ofx_contract,
    );
    Ok(serde_json::to_string_pretty(&report)?)
}

fn integrate_aex_metadata_gate(
    fixture_gate: &Value,
    readiness: &Value,
    loader_gate: &Value,
    synthetic_fixture_manifest: &Value,
    identity_smoke: &Value,
    ofx_contract: &Value,
) -> AexMetadataGateIntegrationReport {
    let fixture_gate_summary = fixture_gate_summary(fixture_gate);
    let readiness_summary = readiness_summary(readiness);
    let loader_gate_summary = loader_gate_summary(loader_gate);
    let synthetic_fixture_summary = synthetic_fixture_summary(synthetic_fixture_manifest);
    let identity_smoke_summary = identity_smoke_summary(identity_smoke);
    let ofx_contract_summary = ofx_contract_summary(ofx_contract);

    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "fixture_gate_closed_no_selection",
        fixture_gate_summary.schema_version == Some(1)
            && fixture_gate_summary.status.as_deref() == Some("review_queue_not_approved")
            && !fixture_gate_summary.selected_fixture_present
            && fixture_gate_summary.approval_approved == Some(false)
            && fixture_gate_summary.approval_loader_enabled == Some(false)
            && fixture_gate_summary.approval_real_aex_load_enabled == Some(false)
            && fixture_gate_summary.approval_render_png_enabled == Some(false)
            && fixture_gate_summary.approval_describe_enabled_for_real_aex == Some(false)
            && fixture_gate_summary.candidate_count > 0
            && fixture_gate_summary.not_approved_candidate_count == fixture_gate_summary.candidate_count,
        format!(
            "status={}, selected_fixture_present={}, approval_approved={}, candidate_count={}, not_approved_candidate_count={}",
            display_optional(&fixture_gate_summary.status),
            fixture_gate_summary.selected_fixture_present,
            display_optional_bool(fixture_gate_summary.approval_approved),
            fixture_gate_summary.candidate_count,
            fixture_gate_summary.not_approved_candidate_count
        ),
        "fixture gate is not a closed, unselected, not-approved review queue",
    );

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "readiness_metadata_only_describe_planning",
        readiness_summary.schema_version == Some(1)
            && readiness_summary.status.as_deref() == Some("probe_readiness_planned")
            && readiness_summary.draft_describe_entry_count > 0
            && readiness_summary.candidate_count.unwrap_or(0)
                >= readiness_summary.draft_describe_entry_count as u64,
        format!(
            "status={}, candidate_count={}, draft_describe_entry_count={}, blocked_or_deferred_entry_count={}",
            display_optional(&readiness_summary.status),
            display_optional_u64(readiness_summary.candidate_count),
            readiness_summary.draft_describe_entry_count,
            readiness_summary.blocked_or_deferred_entry_count
        ),
        "readiness report is not metadata-only describe planning",
    );

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "loader_gate_closed_no_load",
        loader_gate_summary.schema_version == Some(1)
            && loader_gate_summary.status.as_deref() == Some("loader_gate_not_opened")
            && loader_gate_summary.approved == Some(false)
            && loader_gate_summary.loader_enabled == Some(false)
            && loader_gate_summary.real_aex_load_enabled == Some(false)
            && loader_gate_summary.open_candidate_count == Some(0)
            && loader_gate_summary.entry_count > 0
            && loader_gate_summary.not_approved_entry_count == loader_gate_summary.entry_count
            && loader_gate_summary.describe_only_entry_count == loader_gate_summary.entry_count
            && loader_gate_summary.deferred_ofx_entry_count == loader_gate_summary.entry_count,
        format!(
            "status={}, approved={}, loader_enabled={}, real_aex_load_enabled={}, open_candidate_count={}, entry_count={}",
            display_optional(&loader_gate_summary.status),
            display_optional_bool(loader_gate_summary.approved),
            display_optional_bool(loader_gate_summary.loader_enabled),
            display_optional_bool(loader_gate_summary.real_aex_load_enabled),
            display_optional_u64(loader_gate_summary.open_candidate_count),
            loader_gate_summary.entry_count
        ),
        "loader gate is not closed for every metadata candidate",
    );

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "synthetic_fixture_manifest_no_load",
        synthetic_fixture_summary.schema_version == Some(1)
            && synthetic_fixture_summary.status.as_deref()
                == Some("synthetic_fixture_images_ready_no_load")
            && synthetic_fixture_summary.pixel_format.as_deref() == Some("rgba8")
            && synthetic_fixture_summary.image_count == Some(3)
            && synthetic_fixture_summary.native_load_performed == Some(false)
            && synthetic_fixture_summary.render_performed == Some(false)
            && synthetic_fixture_summary.aex_loaded == Some(false)
            && synthetic_fixture_summary.worker_started == Some(false)
            && synthetic_fixture_summary.broker_invoked == Some(false)
            && synthetic_fixture_summary.ofx_route_invoked == Some(false)
            && synthetic_fixture_summary.ae_invoked == Some(false)
            && synthetic_fixture_summary.private_payload_copied == Some(false),
        format!(
            "status={}, image_count={}, native_load={}, render={}, broker_invoked={}, ofx_route={}",
            display_optional(&synthetic_fixture_summary.status),
            display_optional_u64(synthetic_fixture_summary.image_count),
            display_optional_bool(synthetic_fixture_summary.native_load_performed),
            display_optional_bool(synthetic_fixture_summary.render_performed),
            display_optional_bool(synthetic_fixture_summary.broker_invoked),
            display_optional_bool(synthetic_fixture_summary.ofx_route_invoked)
        ),
        "synthetic fixture manifest is not a no-load/no-render input manifest",
    );

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "identity_smoke_transport_only_no_load",
        identity_smoke_summary.schema_version == Some(1)
            && identity_smoke_summary.status.as_deref()
                == Some("fixture_identity_smoke_ready_no_load")
            && identity_smoke_summary.fixture_manifest_status.as_deref()
                == Some("synthetic_fixture_images_ready_no_load")
            && identity_smoke_summary.transport_operation.as_deref() == Some("identity_transport")
            && identity_smoke_summary.image_count == Some(3)
            && identity_smoke_summary.transport_count == Some(3)
            && identity_smoke_summary.identity_pixels_checked_count == Some(3)
            && identity_smoke_summary.native_load_performed == Some(false)
            && identity_smoke_summary.render_performed == Some(false)
            && identity_smoke_summary.aex_loaded == Some(false)
            && identity_smoke_summary.worker_started == Some(false)
            && identity_smoke_summary.broker_invoked == Some(true)
            && identity_smoke_summary.ofx_route_invoked == Some(false)
            && identity_smoke_summary.ae_invoked == Some(false)
            && identity_smoke_summary.private_payload_copied == Some(false)
            && identity_smoke_summary.aex_render_correctness_evidence == Some(false),
        format!(
            "status={}, transport={}, broker_invoked={}, aex_loaded={}, render_correctness_evidence={}",
            display_optional(&identity_smoke_summary.status),
            display_optional(&identity_smoke_summary.transport_operation),
            display_optional_bool(identity_smoke_summary.broker_invoked),
            display_optional_bool(identity_smoke_summary.aex_loaded),
            display_optional_bool(identity_smoke_summary.aex_render_correctness_evidence)
        ),
        "identity smoke is no longer transport-only no-load evidence",
    );

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "ofx_facade_deferred_no_bypass",
        ofx_contract_summary.schema_version == Some(1)
            && ofx_contract_summary.status.as_deref() == Some("deferred-contract-only")
            && ofx_contract_summary.ofx_host_may_load_aex == Some(false)
            && ofx_contract_summary.ofx_adapter_may_load_aex == Some(false)
            && ofx_contract_summary.broker_may_load_aex == Some(false)
            && ofx_contract_summary.review_approved == Some(false)
            && ofx_contract_summary.may_point_to_broker == Some(false)
            && ofx_contract_summary.may_issue_describe == Some(false)
            && ofx_contract_summary.may_issue_render_png == Some(false),
        format!(
            "status={}, host_load={}, adapter_load={}, broker_load={}, review_approved={}, may_render={}",
            display_optional(&ofx_contract_summary.status),
            display_optional_bool(ofx_contract_summary.ofx_host_may_load_aex),
            display_optional_bool(ofx_contract_summary.ofx_adapter_may_load_aex),
            display_optional_bool(ofx_contract_summary.broker_may_load_aex),
            display_optional_bool(ofx_contract_summary.review_approved),
            display_optional_bool(ofx_contract_summary.may_issue_render_png)
        ),
        "OFX facade contract is not deferred/no-bypass",
    );

    let ready = blocked_reasons.is_empty();
    AexMetadataGateIntegrationReport {
        schema_version: 1,
        report_kind: REPORT_KIND.to_string(),
        publication_status: "local-only".to_string(),
        status: if ready {
            READY_STATUS.to_string()
        } else {
            BLOCKED_STATUS.to_string()
        },
        native_load_performed: false,
        render_performed: false,
        aex_loaded: false,
        worker_started: false,
        broker_load_or_render_allowed: false,
        ofx_route_allowed: false,
        ae_invoked: false,
        private_payload_copied: false,
        fixture_gate_summary,
        readiness_summary,
        loader_gate_summary,
        synthetic_fixture_summary,
        identity_smoke_summary,
        ofx_contract_summary,
        checks,
        blocked_reasons,
        next_action: if ready {
            "Keep parent integration at metadata gate boundary; do not open native AEX load, render claims, or OFX routing without separate approval.".to_string()
        } else {
            "Fix or re-review AEX metadata gate inputs before using them for any loader, render, or OFX planning.".to_string()
        },
        notes: vec![
            "This report integrates metadata-only AEX gate evidence for parent orchestration.".to_string(),
            "It does not open, copy, hash, load, describe, execute, render, or route any .aex file.".to_string(),
            "Identity transport over synthetic PNG fixtures is not AEX render correctness evidence.".to_string(),
            "OFX remains deferred and cannot bypass the AEX fixture, loader, worker, or sandbox gates.".to_string(),
        ],
    }
}

fn fixture_gate_summary(gate: &Value) -> FixtureGateSummary {
    let candidates = gate["candidates"].as_array().cloned().unwrap_or_default();
    FixtureGateSummary {
        provided: true,
        schema_version: gate["schema_version"].as_u64(),
        status: string_at(gate, "status"),
        selected_fixture_present: !gate["selected_fixture"].is_null(),
        approval_approved: gate["approval"]["approved"].as_bool(),
        approval_loader_enabled: gate["approval"]["loader_enabled"].as_bool(),
        approval_real_aex_load_enabled: gate["approval"]["real_aex_load_enabled"].as_bool(),
        approval_render_png_enabled: gate["approval"]["render_png_enabled"].as_bool(),
        approval_describe_enabled_for_real_aex: gate["approval"]["describe_enabled_for_real_aex"]
            .as_bool(),
        candidate_count: candidates.len(),
        not_approved_candidate_count: candidates
            .iter()
            .filter(|candidate| candidate["review_status"].as_str() == Some("not-approved"))
            .count(),
    }
}

fn readiness_summary(readiness: &Value) -> ReadinessSummary {
    let entries = readiness["entries"].as_array().cloned().unwrap_or_default();
    ReadinessSummary {
        provided: true,
        schema_version: readiness["schema_version"].as_u64(),
        status: string_at(readiness, "status"),
        candidate_count: readiness["candidate_count"].as_u64(),
        allowlist_entry_count: readiness["allowlist_entry_count"].as_u64(),
        draft_describe_entry_count: entries
            .iter()
            .filter(|entry| {
                entry["status"].as_str() == Some("draft_allowlisted")
                    && string_array_contains(&entry["allowed_operations"], "describe")
                    && !string_array_contains(&entry["allowed_operations"], "render_png")
            })
            .count(),
        blocked_or_deferred_entry_count: entries
            .iter()
            .filter(|entry| entry["status"].as_str() == Some("blocked_or_deferred"))
            .count(),
    }
}

fn loader_gate_summary(loader_gate: &Value) -> LoaderGateSummary {
    let entries = loader_gate["entries"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    LoaderGateSummary {
        provided: true,
        schema_version: loader_gate["schema_version"].as_u64(),
        status: string_at(loader_gate, "status"),
        approved: loader_gate["approved"].as_bool(),
        loader_enabled: loader_gate["loader_enabled"].as_bool(),
        real_aex_load_enabled: loader_gate["real_aex_load_enabled"].as_bool(),
        open_candidate_count: loader_gate["open_candidate_count"].as_u64(),
        entry_count: entries.len(),
        not_approved_entry_count: entries
            .iter()
            .filter(|entry| entry["loader_approval_status"].as_str() == Some("not-approved"))
            .count(),
        describe_only_entry_count: entries
            .iter()
            .filter(|entry| entry["allowlist_operation_status"].as_str() == Some("describe-only"))
            .count(),
        deferred_ofx_entry_count: entries
            .iter()
            .filter(|entry| {
                entry["ofx_facade_status"].as_str() == Some("deferred-same-broker-worker-contract")
            })
            .count(),
    }
}

fn synthetic_fixture_summary(manifest: &Value) -> SyntheticFixtureSummary {
    SyntheticFixtureSummary {
        provided: true,
        schema_version: manifest["schema_version"].as_u64(),
        status: string_at(manifest, "status"),
        pixel_format: string_at(manifest, "pixel_format"),
        image_count: manifest["image_count"].as_u64(),
        native_load_performed: manifest["native_load_performed"].as_bool(),
        render_performed: manifest["render_performed"].as_bool(),
        aex_loaded: manifest["aex_loaded"].as_bool(),
        worker_started: manifest["worker_started"].as_bool(),
        broker_invoked: manifest["broker_invoked"].as_bool(),
        ofx_route_invoked: manifest["ofx_route_invoked"].as_bool(),
        ae_invoked: manifest["ae_invoked"].as_bool(),
        private_payload_copied: manifest["private_payload_copied"].as_bool(),
    }
}

fn identity_smoke_summary(smoke: &Value) -> IdentitySmokeSummary {
    IdentitySmokeSummary {
        provided: true,
        schema_version: smoke["schema_version"].as_u64(),
        status: string_at(smoke, "status"),
        fixture_manifest_status: string_at(smoke, "fixture_manifest_status"),
        transport_operation: string_at(smoke, "transport_operation"),
        image_count: smoke["image_count"].as_u64(),
        transport_count: smoke["transport_count"].as_u64(),
        identity_pixels_checked_count: smoke["identity_pixels_checked_count"].as_u64(),
        native_load_performed: smoke["native_load_performed"].as_bool(),
        render_performed: smoke["render_performed"].as_bool(),
        aex_loaded: smoke["aex_loaded"].as_bool(),
        worker_started: smoke["worker_started"].as_bool(),
        broker_invoked: smoke["broker_invoked"].as_bool(),
        ofx_route_invoked: smoke["ofx_route_invoked"].as_bool(),
        ae_invoked: smoke["ae_invoked"].as_bool(),
        private_payload_copied: smoke["private_payload_copied"].as_bool(),
        aex_render_correctness_evidence: smoke["aex_render_correctness_evidence"].as_bool(),
    }
}

fn ofx_contract_summary(contract: &Value) -> OfxContractSummary {
    OfxContractSummary {
        provided: true,
        schema_version: contract["schema_version"].as_u64(),
        status: string_at(contract, "status"),
        ofx_host_may_load_aex: contract["route"]["ofx_host_may_load_aex"].as_bool(),
        ofx_adapter_may_load_aex: contract["route"]["ofx_adapter_may_load_aex"].as_bool(),
        broker_may_load_aex: contract["route"]["broker_may_load_aex"].as_bool(),
        review_approved: contract["ofx_facade_review_gate"]["approved"].as_bool(),
        may_point_to_broker: contract["ofx_facade_review_gate"]["may_point_to_broker"].as_bool(),
        may_issue_describe: contract["ofx_facade_review_gate"]["may_issue_describe"].as_bool(),
        may_issue_render_png: contract["ofx_facade_review_gate"]["may_issue_render_png"].as_bool(),
    }
}

fn push_check(
    checks: &mut Vec<IntegrationCheck>,
    blocked_reasons: &mut Vec<String>,
    name: &str,
    passed: bool,
    evidence: String,
    blocked_reason: &str,
) {
    checks.push(IntegrationCheck {
        name: name.to_string(),
        status: if passed { "passed" } else { "failed" }.to_string(),
        evidence,
    });
    if !passed {
        blocked_reasons.push(blocked_reason.to_string());
    }
}

fn string_at(value: &Value, field: &str) -> Option<String> {
    value[field].as_str().map(str::to_string)
}

fn string_array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item == expected)
}

fn display_optional(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "missing".to_string())
}

fn display_optional_bool(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "missing".to_string())
}

fn display_optional_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "missing".to_string())
}

pub fn validate_metadata_gate_report_output_path(path: &Path) -> Result<(), String> {
    if path_has_traversal(path) {
        return Err("report path must not contain traversal components".to_string());
    }
    if !path_has_extension(path, "json") {
        return Err("report path must have .json extension".to_string());
    }
    if !output_is_under_target_root(path) {
        return Err("report path must be under target/aex-metadata-gate-integration".to_string());
    }
    Ok(())
}

fn write_report_create_new(path: &Path, report_json: &str) -> Result<(), Box<dyn Error>> {
    validate_metadata_gate_report_output_path(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !output_parent_canonical_is_under_target_root(path) {
        return Err("report parent must resolve under target/aex-metadata-gate-integration".into());
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
    file.write_all(report_json.as_bytes())?;
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
        .join("aex-metadata-gate-integration")
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

#[derive(Debug)]
struct Cli {
    fixture_gate: PathBuf,
    readiness: PathBuf,
    loader_gate: PathBuf,
    fixture_manifest: PathBuf,
    identity_smoke: PathBuf,
    ofx_contract: PathBuf,
    report: PathBuf,
}

fn parse_args() -> Result<Cli, String> {
    let mut fixture_gate = None;
    let mut readiness = None;
    let mut loader_gate = None;
    let mut fixture_manifest = None;
    let mut identity_smoke = None;
    let mut ofx_contract = None;
    let mut report = target_root().join("metadata-gate.local.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture-gate" => fixture_gate = args.next().map(PathBuf::from),
            "--readiness" => readiness = args.next().map(PathBuf::from),
            "--loader-gate" => loader_gate = args.next().map(PathBuf::from),
            "--fixture-manifest" => fixture_manifest = args.next().map(PathBuf::from),
            "--identity-smoke" => identity_smoke = args.next().map(PathBuf::from),
            "--ofx-contract" => ofx_contract = args.next().map(PathBuf::from),
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
    Ok(Cli {
        fixture_gate: fixture_gate.ok_or_else(|| "--fixture-gate is required".to_string())?,
        readiness: readiness.ok_or_else(|| "--readiness is required".to_string())?,
        loader_gate: loader_gate.ok_or_else(|| "--loader-gate is required".to_string())?,
        fixture_manifest: fixture_manifest
            .ok_or_else(|| "--fixture-manifest is required".to_string())?,
        identity_smoke: identity_smoke.ok_or_else(|| "--identity-smoke is required".to_string())?,
        ofx_contract: ofx_contract.ok_or_else(|| "--ofx-contract is required".to_string())?,
        report,
    })
}

fn usage() -> String {
    "usage: aex_metadata_gate_integration --fixture-gate <gate.json> --readiness <readiness.json> --loader-gate <loader-gate.json> --fixture-manifest <manifest.json> --identity-smoke <smoke.json> --ofx-contract <contract.json> [--report <report.json>]".to_string()
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli =
        parse_args().map_err(|message| std::io::Error::new(ErrorKind::InvalidInput, message))?;
    let fixture_gate = std::fs::read_to_string(&cli.fixture_gate)?;
    let readiness = std::fs::read_to_string(&cli.readiness)?;
    let loader_gate = std::fs::read_to_string(&cli.loader_gate)?;
    let fixture_manifest = std::fs::read_to_string(&cli.fixture_manifest)?;
    let identity_smoke = std::fs::read_to_string(&cli.identity_smoke)?;
    let ofx_contract = std::fs::read_to_string(&cli.ofx_contract)?;
    let report_json = integrate_aex_metadata_gate_json(
        &fixture_gate,
        &readiness,
        &loader_gate,
        &fixture_manifest,
        &identity_smoke,
        &ofx_contract,
    )?;
    write_report_create_new(&cli.report, &report_json)?;
    println!("{report_json}");
    Ok(())
}
