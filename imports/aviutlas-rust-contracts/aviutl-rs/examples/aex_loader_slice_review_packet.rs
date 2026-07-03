//! No-load handoff packet for a separate AEX loader implementation slice.
//!
//! This joins the loader implementation manifest and final no-load provenance
//! audit into a sanitized review packet. It does not open, hash, copy, load,
//! execute, describe, or render `.aex` binaries.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct LoaderSliceReviewPacket {
    schema_version: u32,
    publication_status: String,
    status: String,
    native_load_performed: bool,
    loader_slice_approved: bool,
    loader_enabled: bool,
    real_aex_load_enabled: bool,
    native_loader_calls_allowed: bool,
    broker_may_load_aex: bool,
    worker_may_load_plugin: bool,
    render_performed: bool,
    ofx_route_allowed: bool,
    input_contains_forbidden_tokens: bool,
    fixture_gate_summary: FixtureGateReviewSummary,
    manifest_summary: LoaderManifestReviewSummary,
    provenance_summary: ProvenanceReviewSummary,
    review_requirements: LoaderSliceReviewRequirements,
    checks: Vec<ReviewCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FixtureGateReviewSummary {
    schema_version: Option<u64>,
    status: Option<String>,
    publication_status: Option<String>,
    selected_fixture_present: bool,
    recommended_first_review_present: bool,
    recommendation_status: Option<String>,
    approval_approved: bool,
    approval_loader_enabled: bool,
    approval_real_aex_load_enabled: bool,
    approval_render_png_enabled: bool,
    approval_describe_enabled_for_real_aex: bool,
    candidate_count: usize,
    local_build_candidate_count: usize,
    generated_target_candidate_count: usize,
    selected_candidate_present: bool,
    selected_candidate_review_status: Option<String>,
    selected_candidate_fixture_status: Option<String>,
    selected_candidate_plugin_class: Option<String>,
    selected_candidate_source_license_evidence_present: bool,
    selected_candidate_blocked_reason_count: usize,
    rejected_first_loader_class_count: usize,
    single_fixture_policy_ready: bool,
    runtime_evidence_ready: bool,
    input_contains_forbidden_tokens: bool,
}

#[derive(Debug, Serialize)]
struct LoaderManifestReviewSummary {
    status: Option<String>,
    native_load_performed: bool,
    broker_may_load_plugin: bool,
    loader_may_load_plugin: bool,
    ofx_may_route_to_loader: bool,
    selected_fixture_present: bool,
    selected_effect_id_present: bool,
    selected_plugin_path_redacted: bool,
    ready_for_separate_loader_slice_review: bool,
    native_loader_calls_allowed: bool,
    broker_may_load_aex: bool,
    ofx_facade_may_route_to_loader: bool,
    requires_explicit_user_approval: bool,
    requires_code_review: bool,
    requires_local_fixture_only: bool,
    readiness_provided: bool,
    readiness_status: Option<String>,
    fixture_refresh_audit_provided: bool,
    fixture_refresh_audit_status: Option<String>,
    blocked_reason_count: usize,
    input_contains_forbidden_tokens: bool,
}

#[derive(Debug, Serialize)]
struct ProvenanceReviewSummary {
    status: Option<String>,
    native_load_performed: bool,
    selectors_executed: bool,
    render_performed: bool,
    ofx_route_allowed: bool,
    evidence_contains_forbidden_tokens: bool,
    loader_manifest_ready: bool,
    native_stage_plan_ready: bool,
    ofx_readiness_ready: bool,
    runtime_and_cleanroom_ready: bool,
    fixture_identity_smoke_provided: bool,
    fixture_identity_smoke_ready: bool,
    fixture_identity_smoke_broker_invoked: Option<bool>,
    fixture_identity_smoke_aex_render_correctness_evidence: Option<bool>,
    fixture_identity_smoke_input_contains_forbidden_tokens: Option<bool>,
    blocked_reason_count: usize,
    input_contains_forbidden_tokens: bool,
}

#[derive(Debug, Serialize)]
struct LoaderSliceReviewRequirements {
    separate_loader_slice_required: bool,
    explicit_user_approval_required: bool,
    code_review_required: bool,
    local_build_classic_effect_fixture_required: bool,
    cleanroom_boundary_required: bool,
    license_review_required: bool,
    worker_isolation_evidence_required: bool,
    ofx_facade_review_deferred: bool,
    generated_target_fixtures_forbidden: bool,
}

#[derive(Debug, Serialize)]
struct ReviewCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn plan_loader_slice_review_json(
    loader_manifest_json: &str,
    provenance_audit_json: &str,
    fixture_gate_json: &str,
) -> Result<String, Box<dyn Error>> {
    let manifest: Value = serde_json::from_str(loader_manifest_json)?;
    let provenance: Value = serde_json::from_str(provenance_audit_json)?;
    let fixture_gate: Value = serde_json::from_str(fixture_gate_json)?;
    let packet = plan_loader_slice_review(
        &manifest,
        loader_manifest_json,
        &provenance,
        provenance_audit_json,
        &fixture_gate,
        fixture_gate_json,
    );
    Ok(serde_json::to_string_pretty(&packet)?)
}

fn plan_loader_slice_review(
    manifest: &Value,
    manifest_text: &str,
    provenance: &Value,
    provenance_text: &str,
    fixture_gate: &Value,
    fixture_gate_text: &str,
) -> LoaderSliceReviewPacket {
    let fixture_gate_summary = fixture_gate_summary(fixture_gate, fixture_gate_text);
    let manifest_summary = manifest_summary(manifest, manifest_text);
    let provenance_summary = provenance_summary(provenance, provenance_text);
    let review_requirements = review_requirements();
    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();

    let fixture_gate_ready = fixture_gate_manual_approval_ready(&fixture_gate_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "fixture_gate_manual_approval_ready_no_load",
        fixture_gate_ready,
        format!(
            "status={}, selected_fixture_present={}, approval_approved={}, approval_real_aex_load_enabled={}, selected_candidate_review_status={}",
            display_optional(&fixture_gate_summary.status),
            fixture_gate_summary.selected_fixture_present,
            fixture_gate_summary.approval_approved,
            fixture_gate_summary.approval_real_aex_load_enabled,
            display_optional(&fixture_gate_summary.selected_candidate_review_status)
        ),
        "fixture review gate has not selected and approved exactly one local-build classic effect candidate",
    );

    let manifest_ready = loader_manifest_ready_no_load(&manifest_summary) && fixture_gate_ready;
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "loader_manifest_ready_no_load",
        manifest_ready,
        format!(
            "status={}, ready_for_review={}, native_loader_calls_allowed={}, broker_may_load_aex={}",
            display_optional(&manifest_summary.status),
            manifest_summary.ready_for_separate_loader_slice_review,
            manifest_summary.native_loader_calls_allowed,
            manifest_summary.broker_may_load_aex
        ),
        "loader implementation manifest is not a ready no-load review packet",
    );

    let provenance_ready = provenance_ready_no_load(&provenance_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "provenance_chain_ready_no_load",
        provenance_ready,
        format!(
            "status={}, native_load_performed={}, render_performed={}, ofx_route_allowed={}",
            display_optional(&provenance_summary.status),
            provenance_summary.native_load_performed,
            provenance_summary.render_performed,
            provenance_summary.ofx_route_allowed
        ),
        "no-load provenance chain is not ready or preserved",
    );

    if provenance_summary.fixture_identity_smoke_provided {
        let fixture_identity_ready = provenance_fixture_identity_smoke_ready(&provenance_summary);
        push_check(
            &mut checks,
            &mut blocked_reasons,
            "fixture_identity_smoke_preserved_no_load",
            fixture_identity_ready,
            format!(
                "provided={}, ready={}, broker_invoked={}, aex_render_correctness_evidence={}",
                provenance_summary.fixture_identity_smoke_provided,
                provenance_summary.fixture_identity_smoke_ready,
                display_optional_bool(provenance_summary.fixture_identity_smoke_broker_invoked),
                display_optional_bool(
                    provenance_summary.fixture_identity_smoke_aex_render_correctness_evidence,
                )
            ),
            "fixture identity smoke provenance is not preserved as broker-only no-load evidence",
        );
    }

    let review_requirements_closed = review_requirements_closed(&review_requirements);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "manual_review_requirements_closed",
        review_requirements_closed,
        "explicit approval, code review, local fixture, cleanroom, license, worker isolation, and OFX deferral are required".to_string(),
        "manual loader-slice review requirements are not all closed",
    );

    let no_execution_permissions = !manifest_summary.native_loader_calls_allowed
        && !manifest_summary.broker_may_load_aex
        && !manifest_summary.ofx_facade_may_route_to_loader
        && !provenance_summary.native_load_performed
        && !provenance_summary.render_performed
        && !provenance_summary.ofx_route_allowed;
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "no_execution_or_route_permission",
        no_execution_permissions,
        format!(
            "native_loader_calls_allowed={}, broker_may_load_aex={}, ofx_route_allowed={}",
            manifest_summary.native_loader_calls_allowed,
            manifest_summary.broker_may_load_aex,
            provenance_summary.ofx_route_allowed
        ),
        "loader review packet must not grant execution or OFX route permission",
    );

    let input_contains_forbidden_tokens = manifest_summary.input_contains_forbidden_tokens
        || provenance_summary.input_contains_forbidden_tokens
        || fixture_gate_summary.input_contains_forbidden_tokens
        || provenance_summary.evidence_contains_forbidden_tokens;
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "evidence_anti_contamination",
        !input_contains_forbidden_tokens,
        "fixture gate, manifest, and provenance audit inputs scanned without echoing private paths or payload fields".to_string(),
        "fixture gate, manifest, or provenance input contains forbidden payload, hash, rendered-output, or loader evidence",
    );

    let ready = blocked_reasons.is_empty();
    LoaderSliceReviewPacket {
        schema_version: 1,
        publication_status: "local-only".to_string(),
        status: if ready {
            "ready_for_manual_loader_slice_review_no_load".to_string()
        } else {
            "blocked_loader_slice_review_packet".to_string()
        },
        native_load_performed: false,
        loader_slice_approved: false,
        loader_enabled: false,
        real_aex_load_enabled: false,
        native_loader_calls_allowed: false,
        broker_may_load_aex: false,
        worker_may_load_plugin: false,
        render_performed: false,
        ofx_route_allowed: false,
        input_contains_forbidden_tokens,
        fixture_gate_summary,
        manifest_summary,
        provenance_summary,
        review_requirements,
        checks,
        blocked_reasons,
        next_action: if ready {
            "Use this as handoff evidence for a separate loader implementation review; do not enable native loading in this packet.".to_string()
        } else {
            "Fix the blocked no-load handoff evidence before starting a loader implementation review slice.".to_string()
        },
        notes: vec![
            "Loader slice review packet reads JSON metadata only.".to_string(),
            "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.".to_string(),
            "This packet is not loader approval and does not permit worker, broker, or OFX execution.".to_string(),
            "Private plugin paths from upstream manifests are not serialized in this packet.".to_string(),
        ],
    }
}

fn fixture_gate_summary(fixture_gate: &Value, fixture_gate_text: &str) -> FixtureGateReviewSummary {
    let selected_fixture = fixture_gate["selected_fixture"].as_str();
    let selected_candidate = selected_fixture.and_then(|selected| {
        fixture_gate["candidates"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|candidate| candidate["id"].as_str() == Some(selected))
    });
    let candidates = fixture_gate["candidates"].as_array();
    FixtureGateReviewSummary {
        schema_version: fixture_gate["schema_version"].as_u64(),
        status: safe_string(&fixture_gate["status"]),
        publication_status: safe_string(&fixture_gate["publication_status"]),
        selected_fixture_present: selected_fixture.is_some(),
        recommended_first_review_present: fixture_gate["recommended_first_review"]
            .as_str()
            .is_some(),
        recommendation_status: safe_string(&fixture_gate["recommendation_status"]),
        approval_approved: bool_or_false(&fixture_gate["approval"]["approved"]),
        approval_loader_enabled: bool_or_false(&fixture_gate["approval"]["loader_enabled"]),
        approval_real_aex_load_enabled: bool_or_false(
            &fixture_gate["approval"]["real_aex_load_enabled"],
        ),
        approval_render_png_enabled: bool_or_false(&fixture_gate["approval"]["render_png_enabled"]),
        approval_describe_enabled_for_real_aex: bool_or_false(
            &fixture_gate["approval"]["describe_enabled_for_real_aex"],
        ),
        candidate_count: json_array_len(&fixture_gate["candidates"]),
        local_build_candidate_count: candidates
            .into_iter()
            .flatten()
            .filter(|candidate| {
                candidate["fixture_status"].as_str() == Some("local-build-candidate")
                    && candidate["plugin_class"].as_str() == Some("classic-effect-candidate")
            })
            .count(),
        generated_target_candidate_count: candidates
            .into_iter()
            .flatten()
            .filter(|candidate| candidate["generated_target_artifact"].as_bool() == Some(true))
            .count(),
        selected_candidate_present: selected_candidate.is_some(),
        selected_candidate_review_status: selected_candidate
            .and_then(|candidate| safe_string(&candidate["review_status"])),
        selected_candidate_fixture_status: selected_candidate
            .and_then(|candidate| safe_string(&candidate["fixture_status"])),
        selected_candidate_plugin_class: selected_candidate
            .and_then(|candidate| safe_string(&candidate["plugin_class"])),
        selected_candidate_source_license_evidence_present: selected_candidate
            .and_then(|candidate| candidate["source_license_evidence"].as_str())
            .is_some_and(|text| !text.trim().is_empty()),
        selected_candidate_blocked_reason_count: selected_candidate
            .map(|candidate| json_array_len(&candidate["blocked_reasons"]))
            .unwrap_or(0),
        rejected_first_loader_class_count: json_array_len(
            &fixture_gate["rejected_first_loader_classes"],
        ),
        single_fixture_policy_ready: single_fixture_policy_ready(
            &fixture_gate["single_fixture_policy"],
        ),
        runtime_evidence_ready: runtime_evidence_ready(
            &fixture_gate["required_runtime_evidence_before_loader"],
        ),
        input_contains_forbidden_tokens: fixture_gate_contains_forbidden_evidence(
            fixture_gate,
            fixture_gate_text,
        ),
    }
}

fn manifest_summary(manifest: &Value, manifest_text: &str) -> LoaderManifestReviewSummary {
    let implementation_gate = &manifest["implementation_gate"];
    let readiness = &manifest["readiness_summary"];
    let fixture_refresh = &manifest["preflight_summary"]["fixture_refresh_audit_summary"];
    LoaderManifestReviewSummary {
        status: safe_string(&manifest["status"]),
        native_load_performed: bool_or_true(&manifest["native_load_performed"]),
        broker_may_load_plugin: bool_or_true(&manifest["broker_may_load_plugin"]),
        loader_may_load_plugin: bool_or_true(&manifest["loader_may_load_plugin"]),
        ofx_may_route_to_loader: bool_or_true(&manifest["ofx_may_route_to_loader"]),
        selected_fixture_present: manifest["selected_fixture"].as_str().is_some(),
        selected_effect_id_present: manifest["selected_effect_id"].as_str().is_some(),
        selected_plugin_path_redacted: manifest["selected_plugin_path"].as_str().is_some()
            || manifest["normalized_plugin_path"].as_str().is_some(),
        ready_for_separate_loader_slice_review: bool_or_false(
            &implementation_gate["ready_for_separate_loader_slice_review"],
        ),
        native_loader_calls_allowed: bool_or_true(
            &implementation_gate["native_loader_calls_allowed"],
        ),
        broker_may_load_aex: bool_or_true(&implementation_gate["broker_may_load_aex"]),
        ofx_facade_may_route_to_loader: bool_or_true(
            &implementation_gate["ofx_facade_may_route_to_loader"],
        ),
        requires_explicit_user_approval: bool_or_false(
            &implementation_gate["requires_explicit_user_approval"],
        ),
        requires_code_review: bool_or_false(&implementation_gate["requires_code_review"]),
        requires_local_fixture_only: bool_or_false(
            &implementation_gate["requires_local_fixture_only"],
        ),
        readiness_provided: bool_or_false(&readiness["provided"]),
        readiness_status: safe_string(&readiness["status"]),
        fixture_refresh_audit_provided: bool_or_false(&fixture_refresh["provided"]),
        fixture_refresh_audit_status: safe_string(&fixture_refresh["status"]),
        blocked_reason_count: json_array_len(&manifest["blocked_reasons"]),
        input_contains_forbidden_tokens: input_contains_forbidden_evidence(manifest, manifest_text),
    }
}

fn provenance_summary(provenance: &Value, provenance_text: &str) -> ProvenanceReviewSummary {
    let checks = &provenance["checks"];
    let fixture_identity = &provenance["fixture_identity_smoke_summary"];
    let fixture_identity_provided = bool_or_false(&fixture_identity["provided"]);
    ProvenanceReviewSummary {
        status: safe_string(&provenance["status"]),
        native_load_performed: bool_or_true(&provenance["native_load_performed"]),
        selectors_executed: bool_or_true(&provenance["selectors_executed"]),
        render_performed: bool_or_true(&provenance["render_performed"]),
        ofx_route_allowed: bool_or_true(&provenance["ofx_route_allowed"]),
        evidence_contains_forbidden_tokens: bool_or_true(
            &provenance["evidence_contains_forbidden_tokens"],
        ),
        loader_manifest_ready: report_check_passed(checks, "loader_manifest_ready_no_load"),
        native_stage_plan_ready: report_check_passed(checks, "native_stage_plan_ready_no_load"),
        ofx_readiness_ready: report_check_passed(checks, "ofx_readiness_deferred_no_bypass")
            && report_check_passed(checks, "ofx_readiness_consumed_native_stage_plan"),
        runtime_and_cleanroom_ready: report_check_passed(
            checks,
            "native_stage_runtime_and_cleanroom_ready",
        ),
        fixture_identity_smoke_provided: fixture_identity_provided,
        fixture_identity_smoke_ready: if fixture_identity_provided {
            report_check_passed(checks, "fixture_identity_smoke_ready_no_load")
        } else {
            false
        },
        fixture_identity_smoke_broker_invoked: fixture_identity["broker_invoked"].as_bool(),
        fixture_identity_smoke_aex_render_correctness_evidence: fixture_identity
            ["aex_render_correctness_evidence"]
            .as_bool(),
        fixture_identity_smoke_input_contains_forbidden_tokens: fixture_identity
            ["input_contains_forbidden_tokens"]
            .as_bool(),
        blocked_reason_count: json_array_len(&provenance["blocked_reasons"]),
        input_contains_forbidden_tokens: input_contains_forbidden_evidence(
            provenance,
            provenance_text,
        ),
    }
}

fn review_requirements() -> LoaderSliceReviewRequirements {
    LoaderSliceReviewRequirements {
        separate_loader_slice_required: true,
        explicit_user_approval_required: true,
        code_review_required: true,
        local_build_classic_effect_fixture_required: true,
        cleanroom_boundary_required: true,
        license_review_required: true,
        worker_isolation_evidence_required: true,
        ofx_facade_review_deferred: true,
        generated_target_fixtures_forbidden: true,
    }
}

fn fixture_gate_manual_approval_ready(summary: &FixtureGateReviewSummary) -> bool {
    summary.schema_version == Some(1)
        && summary.status.as_deref() == Some("review_queue_approved_local_only")
        && summary.publication_status.as_deref() == Some("local-only design artifact")
        && summary.selected_fixture_present
        && summary.recommended_first_review_present
        && summary.approval_approved
        && summary.approval_loader_enabled
        && summary.approval_real_aex_load_enabled
        && summary.approval_render_png_enabled
        && summary.approval_describe_enabled_for_real_aex
        && summary.candidate_count > 0
        && summary.local_build_candidate_count == summary.candidate_count
        && summary.generated_target_candidate_count == 0
        && summary.selected_candidate_present
        && summary.selected_candidate_review_status.as_deref() == Some("approved-local-only")
        && summary.selected_candidate_fixture_status.as_deref() == Some("local-build-candidate")
        && summary.selected_candidate_plugin_class.as_deref() == Some("classic-effect-candidate")
        && summary.selected_candidate_source_license_evidence_present
        && summary.selected_candidate_blocked_reason_count == 0
        && summary.rejected_first_loader_class_count > 0
        && summary.single_fixture_policy_ready
        && summary.runtime_evidence_ready
        && !summary.input_contains_forbidden_tokens
}

fn loader_manifest_ready_no_load(summary: &LoaderManifestReviewSummary) -> bool {
    summary.status.as_deref() == Some("ready_for_separate_loader_implementation_review_no_load")
        && !summary.native_load_performed
        && !summary.broker_may_load_plugin
        && !summary.loader_may_load_plugin
        && !summary.ofx_may_route_to_loader
        && summary.selected_fixture_present
        && summary.selected_effect_id_present
        && summary.selected_plugin_path_redacted
        && summary.ready_for_separate_loader_slice_review
        && !summary.native_loader_calls_allowed
        && !summary.broker_may_load_aex
        && !summary.ofx_facade_may_route_to_loader
        && summary.requires_explicit_user_approval
        && summary.requires_code_review
        && summary.requires_local_fixture_only
        && summary.readiness_provided
        && summary.readiness_status.as_deref() == Some("probe_readiness_planned")
        && summary.blocked_reason_count == 0
        && !summary.input_contains_forbidden_tokens
}

fn provenance_ready_no_load(summary: &ProvenanceReviewSummary) -> bool {
    summary.status.as_deref() == Some("no_load_provenance_chain_ready")
        && !summary.native_load_performed
        && !summary.selectors_executed
        && !summary.render_performed
        && !summary.ofx_route_allowed
        && !summary.evidence_contains_forbidden_tokens
        && summary.loader_manifest_ready
        && summary.native_stage_plan_ready
        && summary.ofx_readiness_ready
        && summary.runtime_and_cleanroom_ready
        && summary.blocked_reason_count == 0
        && !summary.input_contains_forbidden_tokens
}

fn provenance_fixture_identity_smoke_ready(summary: &ProvenanceReviewSummary) -> bool {
    summary.fixture_identity_smoke_provided
        && summary.fixture_identity_smoke_ready
        && summary.fixture_identity_smoke_broker_invoked == Some(true)
        && summary.fixture_identity_smoke_aex_render_correctness_evidence == Some(false)
        && summary.fixture_identity_smoke_input_contains_forbidden_tokens == Some(false)
}

fn review_requirements_closed(requirements: &LoaderSliceReviewRequirements) -> bool {
    requirements.separate_loader_slice_required
        && requirements.explicit_user_approval_required
        && requirements.code_review_required
        && requirements.local_build_classic_effect_fixture_required
        && requirements.cleanroom_boundary_required
        && requirements.license_review_required
        && requirements.worker_isolation_evidence_required
        && requirements.ofx_facade_review_deferred
        && requirements.generated_target_fixtures_forbidden
}

fn input_contains_forbidden_evidence(value: &Value, text: &str) -> bool {
    string_contains_forbidden_tokens(text) || contains_forbidden_field_names(value)
}

fn fixture_gate_contains_forbidden_evidence(value: &Value, text: &str) -> bool {
    fixture_gate_text_contains_forbidden_tokens(text)
        || contains_forbidden_fixture_gate_field_names(value)
}

fn fixture_gate_text_contains_forbidden_tokens(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    fixture_gate_forbidden_serialized_tokens()
        .iter()
        .any(|token| text.contains(token))
}

fn fixture_gate_forbidden_serialized_tokens() -> Vec<String> {
    vec![
        "sha256".to_string(),
        "base64".to_string(),
        "binary_payload".to_string(),
        "payload_bytes".to_string(),
        "copied_asset".to_string(),
        "native_load_result".to_string(),
        "rendered_pixels".to_string(),
        "worker_exe".to_string(),
        "input_png".to_string(),
        "output_png".to_string(),
        ["load", "library"].concat(),
        ["lib", "loading"].concat(),
    ]
}

fn contains_forbidden_fixture_gate_field_names(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(field, child)| {
            forbidden_fixture_gate_field_names().contains(&field.as_str())
                || contains_forbidden_fixture_gate_field_names(child)
        }),
        Value::Array(array) => array
            .iter()
            .any(contains_forbidden_fixture_gate_field_names),
        _ => false,
    }
}

fn forbidden_fixture_gate_field_names() -> Vec<&'static str> {
    vec![
        "sha256",
        "base64",
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

fn single_fixture_policy_ready(policy: &Value) -> bool {
    policy["max_selected_fixtures"].as_u64() == Some(1)
        && bool_or_false(&policy["selection_requires_manual_user_approval"])
        && bool_or_false(&policy["selection_requires_local_only_license_review"])
        && bool_or_false(&policy["selection_requires_source_tree_review"])
        && bool_or_false(&policy["selection_requires_binary_redistribution_review"])
        && bool_or_false(&policy["selection_requires_loader_gate_opened_by_separate_slice"])
        && bool_or_false(&policy["no_parallel_first_loader_fixtures"])
}

fn runtime_evidence_ready(runtime: &Value) -> bool {
    runtime["allowlist_loader_approval_status"].as_str() == Some("approved-local-only")
        && runtime["request_loader_approval_status"].as_str() == Some("approved-local-only")
        && runtime["allowed_operation"].as_str() == Some("render_png")
        && runtime["worker_identity_revalidation"].as_str() == Some("passed")
        && runtime["sandbox_preflight"].as_str() == Some("passed")
        && runtime["worker_attestation"].as_str() == Some("passed")
        && runtime["job_object"].as_str() == Some("assigned-with-kill-on-close")
        && runtime["handle_inheritance"].as_str()
            == Some("sentinel_not_inherited-with-explicit-handle-list")
        && runtime["ofx_facade"].as_str() == Some("not-a-loader-and-not-a-bypass")
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

fn report_check_passed(checks: &Value, name: &str) -> bool {
    checks.as_array().into_iter().flatten().any(|check| {
        check["name"].as_str() == Some(name) && check["status"].as_str() == Some("passed")
    })
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
        "worker_exe".to_string(),
        "binary_payload".to_string(),
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

fn display_optional(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("missing_or_redacted")
}

fn display_optional_bool(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "missing".to_string())
}

fn push_check(
    checks: &mut Vec<ReviewCheck>,
    blocked_reasons: &mut Vec<String>,
    name: &str,
    passed: bool,
    evidence: String,
    blocked_reason: &str,
) {
    checks.push(ReviewCheck {
        name: name.to_string(),
        status: if passed { "passed" } else { "blocked" }.to_string(),
        evidence,
    });
    if !passed {
        blocked_reasons.push(blocked_reason.to_string());
    }
}

pub fn parse_args_from(
    args: impl IntoIterator<Item = String>,
) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), String> {
    let mut manifest = None;
    let mut provenance_audit = None;
    let mut fixture_gate = None;
    let mut out = PathBuf::from("target")
        .join("aex-loader-slice-review")
        .join("loader-slice-review.local.json");
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => {
                manifest = args.next().map(PathBuf::from);
                if manifest.is_none() {
                    return Err("--manifest requires a JSON path".to_string());
                }
            }
            "--provenance-audit" => {
                provenance_audit = args.next().map(PathBuf::from);
                if provenance_audit.is_none() {
                    return Err("--provenance-audit requires a JSON path".to_string());
                }
            }
            "--fixture-gate" => {
                fixture_gate = args.next().map(PathBuf::from);
                if fixture_gate.is_none() {
                    return Err("--fixture-gate requires a JSON path".to_string());
                }
            }
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a JSON path".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: aex_loader_slice_review_packet --manifest target/aex-loader-implementation/loader-implementation.local.json --provenance-audit target/aex-no-load-provenance-audit/provenance-audit.local.json --fixture-gate ../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json [--out target/aex-loader-slice-review/loader-slice-review.local.json]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok((
        manifest.ok_or_else(|| "--manifest is required".to_string())?,
        provenance_audit.ok_or_else(|| "--provenance-audit is required".to_string())?,
        fixture_gate.ok_or_else(|| "--fixture-gate is required".to_string())?,
        out,
    ))
}

fn parse_args() -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), String> {
    parse_args_from(std::env::args().skip(1))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (manifest_path, provenance_audit_path, fixture_gate_path, out_path) = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let manifest = std::fs::read_to_string(&manifest_path)?;
    let provenance = std::fs::read_to_string(&provenance_audit_path)?;
    let fixture_gate = std::fs::read_to_string(&fixture_gate_path)?;
    let packet = plan_loader_slice_review_json(&manifest, &provenance, &fixture_gate)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, packet)?;
    println!("{}", out_path.display());
    Ok(())
}
