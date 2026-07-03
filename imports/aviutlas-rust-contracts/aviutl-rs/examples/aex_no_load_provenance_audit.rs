//! Metadata-only audit for the AEX no-load provenance chain.
//!
//! This checker consumes the loader implementation manifest, native stage
//! plan, and OFX facade readiness report. It summarizes only derived no-load
//! facts and does not open, copy, hash, load, execute, describe, or render
//! `.aex` binaries.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct NoLoadProvenanceAudit {
    schema_version: u32,
    publication_status: String,
    status: String,
    native_load_performed: bool,
    selectors_executed: bool,
    render_performed: bool,
    ofx_route_allowed: bool,
    evidence_contains_forbidden_tokens: bool,
    loader_manifest_summary: LoaderManifestSummary,
    native_stage_plan_summary: NativeStagePlanAuditSummary,
    ofx_readiness_summary: OfxReadinessAuditSummary,
    fixture_identity_smoke_summary: FixtureIdentitySmokeAuditSummary,
    checks: Vec<AuditCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LoaderManifestSummary {
    status: Option<String>,
    native_load_performed: bool,
    broker_may_load_plugin: bool,
    loader_may_load_plugin: bool,
    ofx_may_route_to_loader: bool,
    ready_for_separate_loader_slice_review: bool,
    native_loader_calls_allowed: bool,
    broker_may_load_aex: bool,
    ofx_facade_may_route_to_loader: bool,
    readiness_provided: bool,
    readiness_matched_entry_count: usize,
    readiness_status: Option<String>,
    readiness_entry_status: Option<String>,
    readiness_pipl_content_scan_status: Option<String>,
    readiness_pipl_content_scan_ready: Option<bool>,
    readiness_allows_describe: bool,
    fixture_refresh_audit_provided: bool,
    fixture_refresh_audit_schema_version: Option<u32>,
    fixture_refresh_audit_publication_status: Option<String>,
    fixture_refresh_audit_status: Option<String>,
    fixture_refresh_audit_native_load_performed: Option<bool>,
    fixture_refresh_audit_render_performed: Option<bool>,
    fixture_refresh_audit_fixture_selected: Option<bool>,
    fixture_refresh_audit_loader_enabled: Option<bool>,
    fixture_refresh_audit_fixture_gate_candidate_count: usize,
    fixture_refresh_audit_wiztree_total_aex_count: u64,
    fixture_refresh_audit_wiztree_canonical_non_generated_count: u64,
    fixture_refresh_audit_wiztree_generated_target_artifact_count: u64,
    fixture_refresh_audit_generated_target_artifacts_excluded: bool,
    fixture_refresh_audit_candidates_present_in_refresh: bool,
    fixture_refresh_audit_input_contains_forbidden_tokens: bool,
    fixture_refresh_audit_blocked_reason_count: usize,
    blocked_reason_count: usize,
}

#[derive(Debug, Serialize)]
struct NativeStagePlanAuditSummary {
    status: Option<String>,
    native_load_performed: bool,
    selectors_executed: bool,
    render_performed: bool,
    broker_may_load_plugin: bool,
    worker_may_load_plugin: bool,
    ofx_may_route_to_loader: bool,
    native_loader_calls_allowed: bool,
    worker_selector_calls_allowed: bool,
    worker_pixel_buffers_allowed: bool,
    ofx_facade_may_route_to_loader: bool,
    native_stage_count: usize,
    planned_not_run_count: usize,
    worker_runtime_evidence_ready: bool,
    cleanroom_boundary_provided: bool,
    cleanroom_boundary_no_loader_or_sdk: bool,
    fixture_refresh_audit_provided: bool,
    fixture_refresh_audit_schema_version: Option<u32>,
    fixture_refresh_audit_publication_status: Option<String>,
    fixture_refresh_audit_status: Option<String>,
    fixture_refresh_audit_native_load_performed: Option<bool>,
    fixture_refresh_audit_render_performed: Option<bool>,
    fixture_refresh_audit_fixture_selected: Option<bool>,
    fixture_refresh_audit_loader_enabled: Option<bool>,
    fixture_refresh_audit_fixture_gate_candidate_count: usize,
    fixture_refresh_audit_wiztree_total_aex_count: u64,
    fixture_refresh_audit_wiztree_canonical_non_generated_count: u64,
    fixture_refresh_audit_wiztree_generated_target_artifact_count: u64,
    fixture_refresh_audit_generated_target_artifacts_excluded: bool,
    fixture_refresh_audit_candidates_present_in_refresh: bool,
    fixture_refresh_audit_input_contains_forbidden_tokens: bool,
    fixture_refresh_audit_blocked_reason_count: usize,
    blocked_reason_count: usize,
}

#[derive(Debug, Serialize)]
struct OfxReadinessAuditSummary {
    status: Option<String>,
    contract_status: Option<String>,
    ofx_host_may_load_aex: bool,
    ofx_adapter_may_load_aex: bool,
    broker_may_load_aex: bool,
    aviutlas_may_route_through_ofx_to_reach_aex: bool,
    current_supported_operation_count: usize,
    ofx_facade_review_gate_approved: bool,
    ofx_facade_may_point_to_broker: bool,
    ofx_facade_may_issue_describe: bool,
    ofx_facade_may_issue_render_png: bool,
    native_stage_plan_summary_provided: bool,
    native_stage_plan_no_load_ready: bool,
    native_stage_runtime_evidence_ready: bool,
    native_stage_cleanroom_boundary_no_loader_or_sdk: bool,
    native_stage_selector_execution_blocked: bool,
    native_stage_render_blocked: bool,
    native_stage_ofx_route_blocked: bool,
    native_stage_input_contains_forbidden_tokens: bool,
    blocked_reason_count: usize,
}

#[derive(Debug, Serialize)]
struct FixtureIdentitySmokeAuditSummary {
    provided: bool,
    schema_version: Option<u32>,
    publication_status: Option<String>,
    status: Option<String>,
    fixture_manifest_status: Option<String>,
    transport_operation: Option<String>,
    pixel_format: Option<String>,
    image_count: usize,
    transport_count: usize,
    identity_pixels_checked_count: usize,
    native_load_performed: Option<bool>,
    render_performed: Option<bool>,
    aex_loaded: Option<bool>,
    worker_started: Option<bool>,
    broker_invoked: Option<bool>,
    ofx_route_invoked: Option<bool>,
    ae_invoked: Option<bool>,
    private_payload_copied: Option<bool>,
    aex_render_correctness_evidence: Option<bool>,
    entry_count: usize,
    expected_synthetic_image_set: bool,
    all_entries_identity_transport_ok: bool,
    all_entries_identity_pixels_match: bool,
    all_entries_no_worker_or_aex_or_render: bool,
    required_check_pass_count: usize,
    required_checks_passed: bool,
    all_checks_passed: bool,
    synthetic_fixture_pixels_checked: bool,
    blocked_reason_count: usize,
    input_contains_forbidden_tokens: bool,
    sanitized_summary_contains_forbidden_tokens: bool,
}

#[derive(Debug, Serialize)]
struct AuditCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn audit_no_load_provenance_json(
    loader_manifest_json: &str,
    native_stage_plan_json: &str,
    ofx_readiness_json: &str,
) -> Result<String, Box<dyn Error>> {
    audit_no_load_provenance_json_with_fixture_identity_smoke(
        loader_manifest_json,
        native_stage_plan_json,
        ofx_readiness_json,
        None,
    )
}

pub fn audit_no_load_provenance_json_with_fixture_identity_smoke(
    loader_manifest_json: &str,
    native_stage_plan_json: &str,
    ofx_readiness_json: &str,
    fixture_identity_smoke_json: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let loader_manifest: Value = serde_json::from_str(loader_manifest_json)?;
    let native_stage_plan: Value = serde_json::from_str(native_stage_plan_json)?;
    let ofx_readiness: Value = serde_json::from_str(ofx_readiness_json)?;
    let fixture_identity_smoke: Option<Value> = fixture_identity_smoke_json
        .map(serde_json::from_str)
        .transpose()?;
    let audit = audit_no_load_provenance(
        &loader_manifest,
        loader_manifest_json,
        &native_stage_plan,
        native_stage_plan_json,
        &ofx_readiness,
        ofx_readiness_json,
        fixture_identity_smoke.as_ref(),
    );
    Ok(serde_json::to_string_pretty(&audit)?)
}

fn audit_no_load_provenance(
    loader_manifest: &Value,
    loader_manifest_text: &str,
    native_stage_plan: &Value,
    native_stage_plan_text: &str,
    ofx_readiness: &Value,
    ofx_readiness_text: &str,
    fixture_identity_smoke: Option<&Value>,
) -> NoLoadProvenanceAudit {
    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();
    let loader_manifest_summary = loader_manifest_summary(loader_manifest);
    let native_stage_plan_summary = native_stage_plan_summary(native_stage_plan);
    let ofx_readiness_summary = ofx_readiness_summary(ofx_readiness);
    let fixture_identity_smoke_summary = fixture_identity_smoke_summary(fixture_identity_smoke);
    let evidence_contains_forbidden_tokens = input_contains_forbidden_evidence(
        loader_manifest,
        loader_manifest_text,
        native_stage_plan,
        native_stage_plan_text,
        ofx_readiness,
        ofx_readiness_text,
    );

    let loader_manifest_ready = loader_manifest_ready_no_load(&loader_manifest_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "loader_manifest_ready_no_load",
        loader_manifest_ready,
        format!(
            "status={}, readiness_provided={}, readiness_pipl_content_scan_status={}, readiness_pipl_content_scan_ready={}",
            display_optional(&loader_manifest_summary.status),
            loader_manifest_summary.readiness_provided,
            display_optional(&loader_manifest_summary.readiness_pipl_content_scan_status),
            display_optional_bool(loader_manifest_summary.readiness_pipl_content_scan_ready)
        ),
        "loader implementation manifest is not a complete ready no-load provenance packet",
    );

    let native_stage_plan_ready = native_stage_plan_ready_no_load(&native_stage_plan_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "native_stage_plan_ready_no_load",
        native_stage_plan_ready,
        format!(
            "status={}, stage_count={}, planned_not_run_count={}, selectors_executed={}, render_performed={}, ofx_may_route_to_loader={}",
            display_optional(&native_stage_plan_summary.status),
            native_stage_plan_summary.native_stage_count,
            native_stage_plan_summary.planned_not_run_count,
            native_stage_plan_summary.selectors_executed,
            native_stage_plan_summary.render_performed,
            native_stage_plan_summary.ofx_may_route_to_loader
        ),
        "native stage plan does not preserve no-load stage ordering",
    );

    let runtime_and_cleanroom_ready =
        native_stage_runtime_and_cleanroom_ready(&native_stage_plan_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "native_stage_runtime_and_cleanroom_ready",
        runtime_and_cleanroom_ready,
        format!(
            "worker_runtime_evidence_ready={}, cleanroom_boundary_provided={}, cleanroom_boundary_no_loader_or_sdk={}",
            native_stage_plan_summary.worker_runtime_evidence_ready,
            native_stage_plan_summary.cleanroom_boundary_provided,
            native_stage_plan_summary.cleanroom_boundary_no_loader_or_sdk
        ),
        "native stage plan is missing worker runtime or cleanroom boundary evidence",
    );

    let fixture_refresh_preserved = fixture_refresh_audit_preserved_no_load(
        &loader_manifest_summary,
        &native_stage_plan_summary,
    );
    if loader_manifest_summary.fixture_refresh_audit_provided
        || native_stage_plan_summary.fixture_refresh_audit_provided
    {
        push_check(
            &mut checks,
            &mut blocked_reasons,
            "fixture_refresh_audit_preserved_no_load",
            fixture_refresh_preserved,
            format!(
                "loader_provided={}, native_stage_provided={}, loader_status={}, native_stage_status={}",
                loader_manifest_summary.fixture_refresh_audit_provided,
                native_stage_plan_summary.fixture_refresh_audit_provided,
                display_optional(&loader_manifest_summary.fixture_refresh_audit_status),
                display_optional(&native_stage_plan_summary.fixture_refresh_audit_status)
            ),
            "fixture refresh audit provenance was not preserved as ready no-load queue-hygiene evidence",
        );
    }

    if fixture_identity_smoke_summary.provided {
        let fixture_identity_ready =
            fixture_identity_smoke_ready_no_load(&fixture_identity_smoke_summary);
        push_check(
            &mut checks,
            &mut blocked_reasons,
            "fixture_identity_smoke_ready_no_load",
            fixture_identity_ready,
            format!(
                "status={}, transport_operation={}, image_count={}, transport_count={}, identity_pixels_checked_count={}, required_checks_passed={}, aex_render_correctness_evidence={}",
                display_optional(&fixture_identity_smoke_summary.status),
                display_optional(&fixture_identity_smoke_summary.transport_operation),
                fixture_identity_smoke_summary.image_count,
                fixture_identity_smoke_summary.transport_count,
                fixture_identity_smoke_summary.identity_pixels_checked_count,
                fixture_identity_smoke_summary.required_checks_passed,
                display_optional_bool(fixture_identity_smoke_summary.aex_render_correctness_evidence)
            ),
            "fixture identity smoke report is not a clean broker-only synthetic identity transport packet",
        );
    }

    let ofx_deferred_no_bypass = ofx_readiness_deferred_no_bypass(&ofx_readiness_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "ofx_readiness_deferred_no_bypass",
        ofx_deferred_no_bypass,
        format!(
            "status={}, ofx_host_may_load_aex={}, ofx_adapter_may_load_aex={}, broker_may_load_aex={}, aviutlas_may_route_through_ofx_to_reach_aex={}",
            display_optional(&ofx_readiness_summary.status),
            ofx_readiness_summary.ofx_host_may_load_aex,
            ofx_readiness_summary.ofx_adapter_may_load_aex,
            ofx_readiness_summary.broker_may_load_aex,
            ofx_readiness_summary.aviutlas_may_route_through_ofx_to_reach_aex
        ),
        "OFX facade readiness is not a closed deferred no-bypass report",
    );

    let ofx_consumed_native_stage_plan =
        ofx_readiness_consumed_native_stage_plan(&ofx_readiness_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "ofx_readiness_consumed_native_stage_plan",
        ofx_consumed_native_stage_plan,
        format!(
            "native_stage_plan_summary_provided={}, native_stage_plan_no_load_ready={}, native_stage_runtime_evidence_ready={}, native_stage_ofx_route_blocked={}",
            ofx_readiness_summary.native_stage_plan_summary_provided,
            ofx_readiness_summary.native_stage_plan_no_load_ready,
            ofx_readiness_summary.native_stage_runtime_evidence_ready,
            ofx_readiness_summary.native_stage_ofx_route_blocked
        ),
        "OFX readiness did not consume the ready native stage plan summary",
    );

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "evidence_anti_contamination",
        !evidence_contains_forbidden_tokens,
        "input reports scanned for forbidden payload, hash, loader, entrypoint, and rendered-output evidence".to_string(),
        "one or more provenance inputs contains forbidden evidence tokens or field names",
    );

    let ready = blocked_reasons.is_empty();
    NoLoadProvenanceAudit {
        schema_version: 1,
        publication_status: "local-only".to_string(),
        status: if ready {
            "no_load_provenance_chain_ready".to_string()
        } else {
            "blocked_no_load_provenance_chain".to_string()
        },
        native_load_performed: false,
        selectors_executed: false,
        render_performed: false,
        ofx_route_allowed: false,
        evidence_contains_forbidden_tokens,
        loader_manifest_summary,
        native_stage_plan_summary,
        ofx_readiness_summary,
        fixture_identity_smoke_summary,
        checks,
        blocked_reasons,
        next_action: if ready {
            "Keep the chain as review evidence only; open a separate reviewed loader slice before any native AEX execution work.".to_string()
        } else {
            "Fix the blocked no-load provenance evidence before using this chain as loader or OFX review input.".to_string()
        },
        notes: vec![
            "Provenance audit reads JSON metadata only.".to_string(),
            "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.".to_string(),
            "A ready audit is not loader approval and does not permit OFX routing.".to_string(),
            "OFX may become a facade only after the same AEX worker loader gate is opened and reviewed separately.".to_string(),
            "Optional fixture identity smoke evidence is summarized without PNG path fields and is not AEX render correctness evidence.".to_string(),
        ],
    }
}

fn loader_manifest_summary(manifest: &Value) -> LoaderManifestSummary {
    let readiness = &manifest["readiness_summary"];
    let fixture_refresh = &manifest["preflight_summary"]["fixture_refresh_audit_summary"];
    LoaderManifestSummary {
        status: safe_string(&manifest["status"]),
        native_load_performed: bool_or_true(&manifest["native_load_performed"]),
        broker_may_load_plugin: bool_or_true(&manifest["broker_may_load_plugin"]),
        loader_may_load_plugin: bool_or_true(&manifest["loader_may_load_plugin"]),
        ofx_may_route_to_loader: bool_or_true(&manifest["ofx_may_route_to_loader"]),
        ready_for_separate_loader_slice_review: bool_or_false(
            &manifest["implementation_gate"]["ready_for_separate_loader_slice_review"],
        ),
        native_loader_calls_allowed: bool_or_true(
            &manifest["implementation_gate"]["native_loader_calls_allowed"],
        ),
        broker_may_load_aex: bool_or_true(&manifest["implementation_gate"]["broker_may_load_aex"]),
        ofx_facade_may_route_to_loader: bool_or_true(
            &manifest["implementation_gate"]["ofx_facade_may_route_to_loader"],
        ),
        readiness_provided: bool_or_false(&readiness["provided"]),
        readiness_matched_entry_count: json_array_or_u64_len(&readiness["matched_entry_count"]),
        readiness_status: safe_string(&readiness["status"]),
        readiness_entry_status: safe_string(&readiness["entry_status"]),
        readiness_pipl_content_scan_status: safe_string(&readiness["pipl_content_scan_status"]),
        readiness_pipl_content_scan_ready: readiness["pipl_content_scan_ready"].as_bool(),
        readiness_allows_describe: array_contains_string(
            &readiness["allowed_operations"],
            "describe",
        ),
        fixture_refresh_audit_provided: bool_or_false(&fixture_refresh["provided"]),
        fixture_refresh_audit_schema_version: fixture_refresh["schema_version"]
            .as_u64()
            .and_then(|version| u32::try_from(version).ok()),
        fixture_refresh_audit_publication_status: safe_string(
            &fixture_refresh["publication_status"],
        ),
        fixture_refresh_audit_status: safe_string(&fixture_refresh["status"]),
        fixture_refresh_audit_native_load_performed: fixture_refresh["native_load_performed"]
            .as_bool(),
        fixture_refresh_audit_render_performed: fixture_refresh["render_performed"].as_bool(),
        fixture_refresh_audit_fixture_selected: fixture_refresh["fixture_selected"].as_bool(),
        fixture_refresh_audit_loader_enabled: fixture_refresh["loader_enabled"].as_bool(),
        fixture_refresh_audit_fixture_gate_candidate_count: fixture_refresh
            ["fixture_gate_candidate_count"]
            .as_u64()
            .unwrap_or(0) as usize,
        fixture_refresh_audit_wiztree_total_aex_count: fixture_refresh["wiztree_total_aex_count"]
            .as_u64()
            .unwrap_or(0),
        fixture_refresh_audit_wiztree_canonical_non_generated_count: fixture_refresh
            ["wiztree_canonical_non_generated_count"]
            .as_u64()
            .unwrap_or(0),
        fixture_refresh_audit_wiztree_generated_target_artifact_count: fixture_refresh
            ["wiztree_generated_target_artifact_count"]
            .as_u64()
            .unwrap_or(0),
        fixture_refresh_audit_generated_target_artifacts_excluded: bool_or_false(
            &fixture_refresh["generated_target_artifacts_excluded"],
        ),
        fixture_refresh_audit_candidates_present_in_refresh: bool_or_false(
            &fixture_refresh["candidates_present_in_refresh"],
        ),
        fixture_refresh_audit_input_contains_forbidden_tokens: bool_or_true(
            &fixture_refresh["input_contains_forbidden_tokens"],
        ),
        fixture_refresh_audit_blocked_reason_count: fixture_refresh["blocked_reason_count"]
            .as_u64()
            .unwrap_or(1) as usize,
        blocked_reason_count: json_array_len(&manifest["blocked_reasons"]),
    }
}

fn native_stage_plan_summary(plan: &Value) -> NativeStagePlanAuditSummary {
    let promotion_gate = &plan["promotion_gate"];
    let native_stage_count = json_array_len(&plan["native_stage_order"]);
    let planned_not_run_count = plan["native_stage_order"]
        .as_array()
        .map(|stages| {
            stages
                .iter()
                .filter(|stage| stage["status"].as_str() == Some("planned_not_run"))
                .count()
        })
        .unwrap_or_default();
    let cleanroom_boundary = &plan["cleanroom_boundary_summary"];
    let fixture_refresh = &plan["manifest_fixture_refresh_audit_summary"];
    NativeStagePlanAuditSummary {
        status: safe_string(&plan["status"]),
        native_load_performed: bool_or_true(&plan["native_load_performed"]),
        selectors_executed: bool_or_true(&plan["selectors_executed"]),
        render_performed: bool_or_true(&plan["render_performed"]),
        broker_may_load_plugin: bool_or_true(&plan["broker_may_load_plugin"]),
        worker_may_load_plugin: bool_or_true(&plan["worker_may_load_plugin"]),
        ofx_may_route_to_loader: bool_or_true(&plan["ofx_may_route_to_loader"]),
        native_loader_calls_allowed: bool_or_true(&promotion_gate["native_loader_calls_allowed"]),
        worker_selector_calls_allowed: bool_or_true(
            &promotion_gate["worker_selector_calls_allowed"],
        ),
        worker_pixel_buffers_allowed: bool_or_true(&promotion_gate["worker_pixel_buffers_allowed"]),
        ofx_facade_may_route_to_loader: bool_or_true(
            &promotion_gate["ofx_facade_may_route_to_loader"],
        ),
        native_stage_count,
        planned_not_run_count,
        worker_runtime_evidence_ready: worker_runtime_evidence_ready(
            &plan["ticket_runtime_evidence_summary"],
        ),
        cleanroom_boundary_provided: cleanroom_boundary.is_object(),
        cleanroom_boundary_no_loader_or_sdk: cleanroom_boundary_no_loader_or_sdk(
            cleanroom_boundary,
        ),
        fixture_refresh_audit_provided: bool_or_false(&fixture_refresh["provided"]),
        fixture_refresh_audit_schema_version: fixture_refresh["schema_version"]
            .as_u64()
            .and_then(|version| u32::try_from(version).ok()),
        fixture_refresh_audit_publication_status: safe_string(
            &fixture_refresh["publication_status"],
        ),
        fixture_refresh_audit_status: safe_string(&fixture_refresh["status"]),
        fixture_refresh_audit_native_load_performed: fixture_refresh["native_load_performed"]
            .as_bool(),
        fixture_refresh_audit_render_performed: fixture_refresh["render_performed"].as_bool(),
        fixture_refresh_audit_fixture_selected: fixture_refresh["fixture_selected"].as_bool(),
        fixture_refresh_audit_loader_enabled: fixture_refresh["loader_enabled"].as_bool(),
        fixture_refresh_audit_fixture_gate_candidate_count: fixture_refresh
            ["fixture_gate_candidate_count"]
            .as_u64()
            .unwrap_or(0) as usize,
        fixture_refresh_audit_wiztree_total_aex_count: fixture_refresh["wiztree_total_aex_count"]
            .as_u64()
            .unwrap_or(0),
        fixture_refresh_audit_wiztree_canonical_non_generated_count: fixture_refresh
            ["wiztree_canonical_non_generated_count"]
            .as_u64()
            .unwrap_or(0),
        fixture_refresh_audit_wiztree_generated_target_artifact_count: fixture_refresh
            ["wiztree_generated_target_artifact_count"]
            .as_u64()
            .unwrap_or(0),
        fixture_refresh_audit_generated_target_artifacts_excluded: bool_or_false(
            &fixture_refresh["generated_target_artifacts_excluded"],
        ),
        fixture_refresh_audit_candidates_present_in_refresh: bool_or_false(
            &fixture_refresh["candidates_present_in_refresh"],
        ),
        fixture_refresh_audit_input_contains_forbidden_tokens: bool_or_true(
            &fixture_refresh["input_contains_forbidden_tokens"],
        ),
        fixture_refresh_audit_blocked_reason_count: fixture_refresh["blocked_reason_count"]
            .as_u64()
            .unwrap_or(1) as usize,
        blocked_reason_count: json_array_len(&plan["blocked_reasons"]),
    }
}

fn ofx_readiness_summary(readiness: &Value) -> OfxReadinessAuditSummary {
    let review_gate = &readiness["ofx_facade_review_gate"];
    let native_summary = &readiness["native_stage_plan_summary"];
    OfxReadinessAuditSummary {
        status: safe_string(&readiness["status"]),
        contract_status: safe_string(&readiness["contract_status"]),
        ofx_host_may_load_aex: bool_or_true(&readiness["ofx_host_may_load_aex"]),
        ofx_adapter_may_load_aex: bool_or_true(&readiness["ofx_adapter_may_load_aex"]),
        broker_may_load_aex: bool_or_true(&readiness["broker_may_load_aex"]),
        aviutlas_may_route_through_ofx_to_reach_aex: bool_or_true(
            &readiness["aviutlas_may_route_through_ofx_to_reach_aex"],
        ),
        current_supported_operation_count: json_array_len(
            &readiness["current_supported_operations"],
        ),
        ofx_facade_review_gate_approved: bool_or_true(&review_gate["approved"]),
        ofx_facade_may_point_to_broker: bool_or_true(&review_gate["may_point_to_broker"]),
        ofx_facade_may_issue_describe: bool_or_true(&review_gate["may_issue_describe"]),
        ofx_facade_may_issue_render_png: bool_or_true(&review_gate["may_issue_render_png"]),
        native_stage_plan_summary_provided: bool_or_false(&native_summary["provided"]),
        native_stage_plan_no_load_ready: bool_or_false(&native_summary["no_load_stage_plan_ready"]),
        native_stage_runtime_evidence_ready: bool_or_false(
            &native_summary["worker_runtime_evidence_ready"],
        ),
        native_stage_cleanroom_boundary_no_loader_or_sdk: bool_or_false(
            &native_summary["cleanroom_boundary_no_loader_or_sdk"],
        ),
        native_stage_selector_execution_blocked: bool_or_false(
            &native_summary["selector_execution_blocked"],
        ),
        native_stage_render_blocked: bool_or_false(&native_summary["render_blocked"]),
        native_stage_ofx_route_blocked: bool_or_false(&native_summary["ofx_route_blocked"]),
        native_stage_input_contains_forbidden_tokens: bool_or_true(
            &native_summary["input_contains_forbidden_tokens"],
        ),
        blocked_reason_count: json_array_len(&readiness["blocked_reasons"]),
    }
}

fn loader_manifest_ready_no_load(summary: &LoaderManifestSummary) -> bool {
    summary.status.as_deref() == Some("ready_for_separate_loader_implementation_review_no_load")
        && !summary.native_load_performed
        && !summary.broker_may_load_plugin
        && !summary.loader_may_load_plugin
        && !summary.ofx_may_route_to_loader
        && summary.ready_for_separate_loader_slice_review
        && !summary.native_loader_calls_allowed
        && !summary.broker_may_load_aex
        && !summary.ofx_facade_may_route_to_loader
        && summary.readiness_provided
        && summary.readiness_matched_entry_count == 1
        && summary.readiness_status.as_deref() == Some("probe_readiness_planned")
        && summary.readiness_entry_status.as_deref() == Some("draft_allowlisted")
        && summary.readiness_pipl_content_scan_status.as_deref() == Some("semantic_matches")
        && summary.readiness_pipl_content_scan_ready == Some(true)
        && summary.readiness_allows_describe
        && summary.blocked_reason_count == 0
}

fn native_stage_plan_ready_no_load(summary: &NativeStagePlanAuditSummary) -> bool {
    summary.status.as_deref() == Some("planned_native_stage_contract_no_load")
        && !summary.native_load_performed
        && !summary.selectors_executed
        && !summary.render_performed
        && !summary.broker_may_load_plugin
        && !summary.worker_may_load_plugin
        && !summary.ofx_may_route_to_loader
        && !summary.native_loader_calls_allowed
        && !summary.worker_selector_calls_allowed
        && !summary.worker_pixel_buffers_allowed
        && !summary.ofx_facade_may_route_to_loader
        && summary.native_stage_count > 0
        && summary.native_stage_count == summary.planned_not_run_count
        && summary.blocked_reason_count == 0
}

fn native_stage_runtime_and_cleanroom_ready(summary: &NativeStagePlanAuditSummary) -> bool {
    summary.worker_runtime_evidence_ready
        && summary.cleanroom_boundary_provided
        && summary.cleanroom_boundary_no_loader_or_sdk
}

fn fixture_refresh_audit_preserved_no_load(
    loader: &LoaderManifestSummary,
    native: &NativeStagePlanAuditSummary,
) -> bool {
    if !loader.fixture_refresh_audit_provided && !native.fixture_refresh_audit_provided {
        return true;
    }
    loader_fixture_refresh_ready(loader)
        && native_fixture_refresh_ready(native)
        && loader.fixture_refresh_audit_status == native.fixture_refresh_audit_status
        && loader.fixture_refresh_audit_wiztree_canonical_non_generated_count
            == native.fixture_refresh_audit_wiztree_canonical_non_generated_count
        && loader.fixture_refresh_audit_wiztree_generated_target_artifact_count
            == native.fixture_refresh_audit_wiztree_generated_target_artifact_count
}

fn loader_fixture_refresh_ready(summary: &LoaderManifestSummary) -> bool {
    summary.fixture_refresh_audit_provided
        && summary.fixture_refresh_audit_schema_version == Some(1)
        && summary.fixture_refresh_audit_publication_status.as_deref() == Some("local-only")
        && summary.fixture_refresh_audit_status.as_deref()
            == Some("fixture_gate_refresh_ready_no_load")
        && summary.fixture_refresh_audit_native_load_performed == Some(false)
        && summary.fixture_refresh_audit_render_performed == Some(false)
        && summary.fixture_refresh_audit_fixture_selected == Some(false)
        && summary.fixture_refresh_audit_loader_enabled == Some(false)
        && summary.fixture_refresh_audit_fixture_gate_candidate_count == 2
        && summary.fixture_refresh_audit_wiztree_total_aex_count == 119
        && summary.fixture_refresh_audit_wiztree_canonical_non_generated_count == 40
        && summary.fixture_refresh_audit_wiztree_generated_target_artifact_count == 79
        && summary.fixture_refresh_audit_generated_target_artifacts_excluded
        && summary.fixture_refresh_audit_candidates_present_in_refresh
        && !summary.fixture_refresh_audit_input_contains_forbidden_tokens
        && summary.fixture_refresh_audit_blocked_reason_count == 0
}

fn native_fixture_refresh_ready(summary: &NativeStagePlanAuditSummary) -> bool {
    summary.fixture_refresh_audit_provided
        && summary.fixture_refresh_audit_schema_version == Some(1)
        && summary.fixture_refresh_audit_publication_status.as_deref() == Some("local-only")
        && summary.fixture_refresh_audit_status.as_deref()
            == Some("fixture_gate_refresh_ready_no_load")
        && summary.fixture_refresh_audit_native_load_performed == Some(false)
        && summary.fixture_refresh_audit_render_performed == Some(false)
        && summary.fixture_refresh_audit_fixture_selected == Some(false)
        && summary.fixture_refresh_audit_loader_enabled == Some(false)
        && summary.fixture_refresh_audit_fixture_gate_candidate_count == 2
        && summary.fixture_refresh_audit_wiztree_total_aex_count == 119
        && summary.fixture_refresh_audit_wiztree_canonical_non_generated_count == 40
        && summary.fixture_refresh_audit_wiztree_generated_target_artifact_count == 79
        && summary.fixture_refresh_audit_generated_target_artifacts_excluded
        && summary.fixture_refresh_audit_candidates_present_in_refresh
        && !summary.fixture_refresh_audit_input_contains_forbidden_tokens
        && summary.fixture_refresh_audit_blocked_reason_count == 0
}

fn ofx_readiness_deferred_no_bypass(summary: &OfxReadinessAuditSummary) -> bool {
    summary.status.as_deref() == Some("deferred_contract_only")
        && summary.contract_status.as_deref() == Some("deferred-contract-only")
        && !summary.ofx_host_may_load_aex
        && !summary.ofx_adapter_may_load_aex
        && !summary.broker_may_load_aex
        && !summary.aviutlas_may_route_through_ofx_to_reach_aex
        && summary.current_supported_operation_count == 0
        && !summary.ofx_facade_review_gate_approved
        && !summary.ofx_facade_may_point_to_broker
        && !summary.ofx_facade_may_issue_describe
        && !summary.ofx_facade_may_issue_render_png
        && summary.blocked_reason_count == 0
}

fn ofx_readiness_consumed_native_stage_plan(summary: &OfxReadinessAuditSummary) -> bool {
    summary.native_stage_plan_summary_provided
        && summary.native_stage_plan_no_load_ready
        && summary.native_stage_runtime_evidence_ready
        && summary.native_stage_cleanroom_boundary_no_loader_or_sdk
        && summary.native_stage_selector_execution_blocked
        && summary.native_stage_render_blocked
        && summary.native_stage_ofx_route_blocked
        && !summary.native_stage_input_contains_forbidden_tokens
}

fn fixture_identity_smoke_summary(smoke: Option<&Value>) -> FixtureIdentitySmokeAuditSummary {
    let Some(smoke) = smoke else {
        return FixtureIdentitySmokeAuditSummary {
            provided: false,
            schema_version: None,
            publication_status: None,
            status: None,
            fixture_manifest_status: None,
            transport_operation: None,
            pixel_format: None,
            image_count: 0,
            transport_count: 0,
            identity_pixels_checked_count: 0,
            native_load_performed: None,
            render_performed: None,
            aex_loaded: None,
            worker_started: None,
            broker_invoked: None,
            ofx_route_invoked: None,
            ae_invoked: None,
            private_payload_copied: None,
            aex_render_correctness_evidence: None,
            entry_count: 0,
            expected_synthetic_image_set: false,
            all_entries_identity_transport_ok: false,
            all_entries_identity_pixels_match: false,
            all_entries_no_worker_or_aex_or_render: false,
            required_check_pass_count: 0,
            required_checks_passed: false,
            all_checks_passed: false,
            synthetic_fixture_pixels_checked: false,
            blocked_reason_count: 0,
            input_contains_forbidden_tokens: false,
            sanitized_summary_contains_forbidden_tokens: false,
        };
    };

    let publication_status = safe_string(&smoke["publication_status"]);
    let status = safe_string(&smoke["status"]);
    let fixture_manifest_status = safe_string(&smoke["fixture_manifest_status"]);
    let transport_operation = safe_string(&smoke["transport_operation"]);
    let pixel_format = safe_string(&smoke["pixel_format"]);
    let entries = smoke["entries"].as_array();
    let entry_count = entries.map_or(0, Vec::len);
    let required_check_pass_count = fixture_identity_smoke_required_checks()
        .iter()
        .filter(|name| report_check_passed(smoke, name))
        .count();
    let required_checks_passed =
        required_check_pass_count == fixture_identity_smoke_required_checks().len();
    let all_checks_passed = smoke["checks"]
        .as_array()
        .map(|checks| {
            !checks.is_empty()
                && checks
                    .iter()
                    .all(|check| check["status"].as_str() == Some("passed"))
        })
        .unwrap_or(false);
    let synthetic_fixture_pixels_checked =
        report_check_passed(smoke, "synthetic_fixture_pixels_match");
    let sanitized_summary_contains_forbidden_tokens = [
        publication_status.as_deref(),
        status.as_deref(),
        fixture_manifest_status.as_deref(),
        transport_operation.as_deref(),
        pixel_format.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(string_contains_forbidden_tokens);

    FixtureIdentitySmokeAuditSummary {
        provided: true,
        schema_version: smoke["schema_version"]
            .as_u64()
            .and_then(|version| u32::try_from(version).ok()),
        publication_status,
        status,
        fixture_manifest_status,
        transport_operation,
        pixel_format,
        image_count: smoke["image_count"].as_u64().unwrap_or(0) as usize,
        transport_count: smoke["transport_count"].as_u64().unwrap_or(0) as usize,
        identity_pixels_checked_count: smoke["identity_pixels_checked_count"].as_u64().unwrap_or(0)
            as usize,
        native_load_performed: smoke["native_load_performed"].as_bool(),
        render_performed: smoke["render_performed"].as_bool(),
        aex_loaded: smoke["aex_loaded"].as_bool(),
        worker_started: smoke["worker_started"].as_bool(),
        broker_invoked: smoke["broker_invoked"].as_bool(),
        ofx_route_invoked: smoke["ofx_route_invoked"].as_bool(),
        ae_invoked: smoke["ae_invoked"].as_bool(),
        private_payload_copied: smoke["private_payload_copied"].as_bool(),
        aex_render_correctness_evidence: smoke["aex_render_correctness_evidence"].as_bool(),
        entry_count,
        expected_synthetic_image_set: fixture_identity_smoke_entries_match_expected_set(smoke),
        all_entries_identity_transport_ok: entries
            .map(|entries| {
                !entries.is_empty()
                    && entries.iter().all(|entry| {
                        entry["pixel_format"].as_str() == Some("rgba8")
                            && entry["transport_status"].as_str() == Some("ok")
                            && entry["plugin_class"].as_str() == Some("identity-transport")
                    })
            })
            .unwrap_or(false),
        all_entries_identity_pixels_match: entries
            .map(|entries| {
                !entries.is_empty()
                    && entries
                        .iter()
                        .all(|entry| entry["identity_pixels_match"].as_bool() == Some(true))
            })
            .unwrap_or(false),
        all_entries_no_worker_or_aex_or_render: entries
            .map(|entries| {
                !entries.is_empty()
                    && entries.iter().all(|entry| {
                        entry["worker_started"].as_bool() == Some(false)
                            && entry["aex_loaded"].as_bool() == Some(false)
                            && entry["render_performed"].as_bool() == Some(false)
                    })
            })
            .unwrap_or(false),
        required_check_pass_count,
        required_checks_passed,
        all_checks_passed,
        synthetic_fixture_pixels_checked,
        blocked_reason_count: json_array_len(&smoke["blocked_reasons"]),
        input_contains_forbidden_tokens: fixture_identity_smoke_contains_forbidden_evidence(smoke),
        sanitized_summary_contains_forbidden_tokens,
    }
}

fn fixture_identity_smoke_ready_no_load(summary: &FixtureIdentitySmokeAuditSummary) -> bool {
    summary.provided
        && summary.schema_version == Some(1)
        && summary.publication_status.as_deref() == Some("local-only")
        && summary.status.as_deref() == Some("fixture_identity_smoke_ready_no_load")
        && summary.fixture_manifest_status.as_deref()
            == Some("synthetic_fixture_images_ready_no_load")
        && summary.transport_operation.as_deref() == Some("identity_transport")
        && summary.pixel_format.as_deref() == Some("rgba8")
        && summary.image_count == 3
        && summary.transport_count == 3
        && summary.identity_pixels_checked_count == 3
        && summary.native_load_performed == Some(false)
        && summary.render_performed == Some(false)
        && summary.aex_loaded == Some(false)
        && summary.worker_started == Some(false)
        && summary.broker_invoked == Some(true)
        && summary.ofx_route_invoked == Some(false)
        && summary.ae_invoked == Some(false)
        && summary.private_payload_copied == Some(false)
        && summary.aex_render_correctness_evidence == Some(false)
        && summary.entry_count == 3
        && summary.expected_synthetic_image_set
        && summary.all_entries_identity_transport_ok
        && summary.all_entries_identity_pixels_match
        && summary.all_entries_no_worker_or_aex_or_render
        && summary.required_checks_passed
        && summary.all_checks_passed
        && summary.synthetic_fixture_pixels_checked
        && summary.blocked_reason_count == 0
        && !summary.input_contains_forbidden_tokens
        && !summary.sanitized_summary_contains_forbidden_tokens
}

fn fixture_identity_smoke_entries_match_expected_set(smoke: &Value) -> bool {
    let Some(entries) = smoke["entries"].as_array() else {
        return false;
    };
    let mut ids: Vec<&str> = entries
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect();
    ids.sort_unstable();
    ids == ["checker", "gradient", "solid_alpha"]
}

fn fixture_identity_smoke_contains_forbidden_evidence(smoke: &Value) -> bool {
    contains_forbidden_fixture_identity_smoke_evidence(smoke, None)
}

fn contains_forbidden_fixture_identity_smoke_evidence(
    value: &Value,
    parent_field: Option<&str>,
) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(field, child)| {
            let allowed_path_field =
                parent_field == Some("entries") && fixture_identity_smoke_allowed_path_field(field);
            (!allowed_path_field
                && (forbidden_field_names().contains(&field.as_str())
                    || fixture_identity_smoke_field_name_is_forbidden(field)))
                || contains_forbidden_fixture_identity_smoke_evidence(child, Some(field))
        }),
        Value::Array(array) => array
            .iter()
            .any(|child| contains_forbidden_fixture_identity_smoke_evidence(child, parent_field)),
        Value::String(text) => {
            string_contains_forbidden_tokens(text)
                || fixture_identity_smoke_extra_forbidden_tokens()
                    .iter()
                    .any(|token| text.to_ascii_lowercase().contains(token))
        }
        _ => false,
    }
}

fn fixture_identity_smoke_allowed_path_field(field: &str) -> bool {
    matches!(field, "input_png" | "output_png")
}

fn fixture_identity_smoke_field_name_is_forbidden(field: &str) -> bool {
    let field = field.to_ascii_lowercase();
    fixture_identity_smoke_extra_forbidden_tokens()
        .iter()
        .any(|token| field.contains(token))
}

fn fixture_identity_smoke_extra_forbidden_tokens() -> Vec<&'static str> {
    vec![
        "payload_bytes",
        "copied_asset",
        "native_load_result",
        "worker_loaded",
        "plugin_loaded",
    ]
}

fn report_check_passed(report: &Value, name: &str) -> bool {
    report["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|check| {
            check["name"].as_str() == Some(name) && check["status"].as_str() == Some("passed")
        })
}

fn fixture_identity_smoke_required_checks() -> Vec<&'static str> {
    vec![
        "fixture_manifest_validated",
        "identity_transport_ok",
        "rgba_identity_pixels_match",
        "synthetic_fixture_pixels_match",
        "no_aex_input",
        "no_worker_or_host_invocation",
        "not_render_correctness_evidence",
    ]
}

fn worker_runtime_evidence_ready(evidence: &Value) -> bool {
    evidence["worker_identity_revalidation_required"].as_str() == Some("passed")
        && evidence["worker_attestation_required"].as_str() == Some("passed")
        && evidence["sandbox_preflight_required"].as_str() == Some("passed")
        && evidence["job_object_required"].as_str() == Some("assigned-with-kill-on-close")
        && evidence["handle_inheritance_required"].as_str()
            == Some("sentinel_not_inherited-with-explicit-handle-list")
}

fn cleanroom_boundary_no_loader_or_sdk(boundary: &Value) -> bool {
    boundary["native_loader_calls_allowed"].as_bool() == Some(false)
        && boundary["adobe_sdk_headers_allowed"].as_bool() == Some(false)
        && boundary["abi_generator_allowed"].as_bool() == Some(false)
        && boundary["third_party_effect_host_crate_allowed"].as_bool() == Some(false)
        && boundary["third_party_pipl_crate_allowed"].as_bool() == Some(false)
        && boundary["reuse_existing_aviutl_dynamic_loader_for_aex_allowed"].as_bool() == Some(false)
        && boundary["pf_names_are_planning_labels_only"].as_bool() == Some(true)
        && boundary["metadata_labels_do_not_define_abi"].as_bool() == Some(true)
}

fn input_contains_forbidden_evidence(
    loader_manifest: &Value,
    loader_manifest_text: &str,
    native_stage_plan: &Value,
    native_stage_plan_text: &str,
    ofx_readiness: &Value,
    ofx_readiness_text: &str,
) -> bool {
    string_contains_forbidden_tokens(loader_manifest_text)
        || string_contains_forbidden_tokens(native_stage_plan_text)
        || string_contains_forbidden_tokens(ofx_readiness_text)
        || contains_forbidden_field_names(loader_manifest)
        || contains_forbidden_field_names(native_stage_plan)
        || contains_forbidden_field_names(ofx_readiness)
}

fn contains_forbidden_field_names(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(field, child)| {
            forbidden_field_names().contains(&field.as_str())
                || contains_forbidden_field_names(child)
        }),
        Value::Array(array) => array.iter().any(contains_forbidden_field_names),
        _ => false,
    }
}

fn safe_string(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    if text.is_empty() || string_contains_forbidden_tokens(text) {
        return None;
    }
    Some(text.to_string())
}

fn string_contains_forbidden_tokens(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    forbidden_serialized_tokens()
        .iter()
        .any(|token| text.contains(token))
}

fn forbidden_serialized_tokens() -> Vec<String> {
    vec![
        "sha256".to_string(),
        "base64".to_string(),
        ["load", "library"].concat(),
        ["lib", "loading"].concat(),
        ["effect", "main"].concat(),
        "output_png".to_string(),
        "input_png".to_string(),
        "rendered_pixels".to_string(),
        "host process started".to_string(),
    ]
}

fn forbidden_field_names() -> Vec<&'static str> {
    vec![
        "worker_exe",
        "input_png",
        "output_png",
        "last_probe",
        "hash",
        "sha256",
        "binary_payload",
        "base64_payload",
        "rendered_pixels",
    ]
}

fn bool_or_true(value: &Value) -> bool {
    value.as_bool().unwrap_or(true)
}

fn bool_or_false(value: &Value) -> bool {
    value.as_bool().unwrap_or(false)
}

fn json_array_len(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn json_array_or_u64_len(value: &Value) -> usize {
    value.as_u64().unwrap_or(0) as usize
}

fn array_contains_string(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item.as_str() == Some(expected))
}

fn display_optional(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("missing_or_redacted")
}

fn display_optional_bool(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "missing".to_string())
}

fn push_check(
    checks: &mut Vec<AuditCheck>,
    blocked_reasons: &mut Vec<String>,
    name: &str,
    passed: bool,
    evidence: String,
    blocked_reason: &str,
) {
    checks.push(AuditCheck {
        name: name.to_string(),
        status: if passed { "passed" } else { "blocked" }.to_string(),
        evidence,
    });
    if !passed {
        blocked_reasons.push(blocked_reason.to_string());
    }
}

fn parse_args() -> Result<(PathBuf, PathBuf, PathBuf, Option<PathBuf>, PathBuf), String> {
    let mut loader_manifest = None;
    let mut native_stage_plan = None;
    let mut ofx_readiness = None;
    let mut fixture_identity_smoke = None;
    let mut out = PathBuf::from("target")
        .join("aex-no-load-provenance-audit")
        .join("provenance-audit.local.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--loader-manifest" => {
                loader_manifest = args.next().map(PathBuf::from);
                if loader_manifest.is_none() {
                    return Err("--loader-manifest requires a JSON path".to_string());
                }
            }
            "--native-stage-plan" => {
                native_stage_plan = args.next().map(PathBuf::from);
                if native_stage_plan.is_none() {
                    return Err("--native-stage-plan requires a JSON path".to_string());
                }
            }
            "--ofx-readiness" => {
                ofx_readiness = args.next().map(PathBuf::from);
                if ofx_readiness.is_none() {
                    return Err("--ofx-readiness requires a JSON path".to_string());
                }
            }
            "--fixture-identity-smoke" => {
                fixture_identity_smoke = args.next().map(PathBuf::from);
                if fixture_identity_smoke.is_none() {
                    return Err("--fixture-identity-smoke requires a JSON path".to_string());
                }
            }
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a JSON path".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: aex_no_load_provenance_audit --loader-manifest target/aex-loader-implementation/loader-implementation.local.json --native-stage-plan target/aex-native-stage-plan/native-stage-plan.local.json --ofx-readiness target/aex-ofx-facade-readiness/ofx-facade.local.json [--fixture-identity-smoke target/aex-image-probe/fixture-identity-smoke/smoke.local.json] [--out target/aex-no-load-provenance-audit/provenance-audit.local.json]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok((
        loader_manifest.ok_or_else(|| "--loader-manifest is required".to_string())?,
        native_stage_plan.ok_or_else(|| "--native-stage-plan is required".to_string())?,
        ofx_readiness.ok_or_else(|| "--ofx-readiness is required".to_string())?,
        fixture_identity_smoke,
        out,
    ))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (
        loader_manifest_path,
        native_stage_plan_path,
        ofx_readiness_path,
        fixture_identity_smoke_path,
        out_path,
    ) = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let loader_manifest = std::fs::read_to_string(&loader_manifest_path)?;
    let native_stage_plan = std::fs::read_to_string(&native_stage_plan_path)?;
    let ofx_readiness = std::fs::read_to_string(&ofx_readiness_path)?;
    let fixture_identity_smoke = fixture_identity_smoke_path
        .as_ref()
        .map(std::fs::read_to_string)
        .transpose()?;
    let report = audit_no_load_provenance_json_with_fixture_identity_smoke(
        &loader_manifest,
        &native_stage_plan,
        &ofx_readiness,
        fixture_identity_smoke.as_deref(),
    )?;
    write_no_load_provenance_audit_report_create_new(&out_path, &report)?;
    println!("{}", out_path.display());
    Ok(())
}

pub fn validate_no_load_provenance_audit_report_output_path(path: &Path) -> Result<(), String> {
    if path_has_traversal(path) {
        return Err(
            "AEX no-load provenance audit report path must not contain traversal components"
                .to_string(),
        );
    }
    if !path_has_extension(path, "json") {
        return Err(
            "AEX no-load provenance audit report path must have .json extension".to_string(),
        );
    }
    if !output_is_under_target_root(path) {
        return Err("AEX no-load provenance audit report path must be under target/aex-no-load-provenance-audit".to_string());
    }
    Ok(())
}

pub fn write_no_load_provenance_audit_report_create_new(
    path: &Path,
    text: &str,
) -> Result<(), Box<dyn Error>> {
    validate_no_load_provenance_audit_report_output_path(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !output_parent_canonical_is_under_target_root(path) {
        return Err(
            "AEX no-load provenance audit report parent must resolve under target/aex-no-load-provenance-audit"
                .into(),
        );
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| {
            if err.kind() == ErrorKind::AlreadyExists {
                std::io::Error::new(
                    ErrorKind::AlreadyExists,
                    "AEX no-load provenance audit report already exists",
                )
            } else {
                err
            }
        })?;
    file.write_all(text.as_bytes())?;
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
        .join("aex-no-load-provenance-audit")
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
