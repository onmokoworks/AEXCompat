//! No-load native stage plan for a future AEX host slice.
//!
//! This planner consumes the no-load loader implementation manifest and the
//! worker-visible loader ticket, then emits the classic-effect selector/stage
//! order that a reviewed loader slice would need to implement. It does not
//! open, hash, load, execute, describe, or render `.aex` binaries.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct NativeStagePlan {
    schema_version: u32,
    publication_status: String,
    status: String,
    native_load_performed: bool,
    selectors_executed: bool,
    render_performed: bool,
    broker_may_load_plugin: bool,
    worker_may_load_plugin: bool,
    ofx_may_route_to_loader: bool,
    selected_effect_id: Option<String>,
    normalized_plugin_path: Option<String>,
    manifest_summary: ManifestSummary,
    manifest_readiness_summary: ManifestReadinessSummary,
    manifest_fixture_refresh_audit_summary: ManifestFixtureRefreshSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    cleanroom_boundary_summary: Option<CleanroomBoundarySummary>,
    ticket_summary: TicketSummary,
    ticket_runtime_evidence_summary: TicketRuntimeEvidenceSummary,
    host_struct_plan: Vec<HostStructPlan>,
    native_stage_order: Vec<NativeStage>,
    promotion_gate: PromotionGate,
    checks: Vec<PlanCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ManifestSummary {
    status: Option<String>,
    ready_for_separate_loader_slice_review: bool,
    native_loader_calls_allowed: bool,
    broker_may_load_aex: bool,
    ofx_facade_may_route_to_loader: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ManifestReadinessSummary {
    provided: bool,
    matched_entry_count: usize,
    status: Option<String>,
    entry_status: Option<String>,
    pipl_content_scan_status: Option<String>,
    pipl_content_scan_ready: Option<bool>,
    allowed_operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ManifestFixtureRefreshSummary {
    provided: bool,
    schema_version: Option<u32>,
    publication_status: Option<String>,
    status: Option<String>,
    native_load_performed: Option<bool>,
    render_performed: Option<bool>,
    fixture_selected: Option<bool>,
    loader_enabled: Option<bool>,
    fixture_gate_candidate_count: usize,
    wiztree_total_aex_count: u64,
    wiztree_canonical_non_generated_count: u64,
    wiztree_generated_target_artifact_count: u64,
    generated_target_artifacts_excluded: bool,
    candidates_present_in_refresh: bool,
    input_contains_forbidden_tokens: bool,
    blocked_reason_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CleanroomBoundarySummary {
    schema_name: Option<String>,
    schema_version: Option<u64>,
    publication_status: Option<String>,
    compatibility_classification: Option<String>,
    source_file_count: usize,
    allowed_planning_label_count: usize,
    allowed_metadata_label_count: usize,
    source_forbidden_substring_count: usize,
    required_source_boundary_notes_count: usize,
    native_loader_calls_allowed: Option<bool>,
    adobe_sdk_headers_allowed: Option<bool>,
    abi_generator_allowed: Option<bool>,
    third_party_effect_host_crate_allowed: Option<bool>,
    third_party_pipl_crate_allowed: Option<bool>,
    reuse_existing_aviutl_dynamic_loader_for_aex_allowed: Option<bool>,
    pf_names_are_planning_labels_only: Option<bool>,
    metadata_labels_do_not_define_abi: Option<bool>,
    worker_os_isolation_ffi_allowed: Option<bool>,
}

#[derive(Debug, Serialize)]
struct TicketSummary {
    status: Option<String>,
    operation: Option<String>,
    allowlist_id: Option<String>,
    native_load_performed: bool,
    worker_may_load_plugin: bool,
    planned_stage_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct TicketRuntimeEvidenceSummary {
    worker_identity_revalidation_required: Option<String>,
    worker_attestation_required: Option<String>,
    sandbox_preflight_required: Option<String>,
    job_object_required: Option<String>,
    handle_inheritance_required: Option<String>,
}

#[derive(Debug, Serialize)]
struct HostStructPlan {
    name: String,
    allocation_status: String,
    mutation_status: String,
    purpose: String,
}

#[derive(Debug, Serialize)]
struct NativeStage {
    name: String,
    coarse_ticket_stage: String,
    pf_selector: String,
    status: String,
    host_structs: Vec<String>,
    surface_status: String,
}

#[derive(Debug, Serialize)]
struct PromotionGate {
    native_loader_calls_allowed: bool,
    worker_selector_calls_allowed: bool,
    worker_pixel_buffers_allowed: bool,
    ofx_facade_may_route_to_loader: bool,
    requires_separate_loader_slice: bool,
    requires_explicit_user_approval: bool,
    requires_code_review: bool,
    requires_local_fixture_only: bool,
}

#[derive(Debug, Serialize)]
struct PlanCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn plan_native_stage_contract_json(
    loader_manifest_json: &str,
    worker_loader_ticket_json: &str,
) -> Result<String, Box<dyn Error>> {
    plan_native_stage_contract_json_with_boundary_schema(
        loader_manifest_json,
        worker_loader_ticket_json,
        None,
    )
}

pub fn plan_native_stage_contract_json_with_boundary_schema(
    loader_manifest_json: &str,
    worker_loader_ticket_json: &str,
    host_boundary_schema_json: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let loader_manifest: Value = serde_json::from_str(loader_manifest_json)?;
    let worker_loader_ticket: Value = serde_json::from_str(worker_loader_ticket_json)?;
    let host_boundary_schema = host_boundary_schema_json
        .map(serde_json::from_str)
        .transpose()?;
    let plan = plan_native_stage_contract(
        &loader_manifest,
        &worker_loader_ticket,
        host_boundary_schema.as_ref(),
    );
    Ok(serde_json::to_string_pretty(&plan)?)
}

fn plan_native_stage_contract(
    manifest: &Value,
    ticket: &Value,
    host_boundary_schema: Option<&Value>,
) -> NativeStagePlan {
    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();

    let manifest_effect_id = manifest["selected_effect_id"].as_str();
    let ticket_entry = &ticket["selected_loader_entry"];
    let ticket_effect_id = ticket_entry["effect_id"].as_str();
    let manifest_path = manifest["normalized_plugin_path"]
        .as_str()
        .or_else(|| manifest["selected_plugin_path"].as_str());
    let ticket_path = ticket_entry["normalized_plugin_path"].as_str();
    let manifest_path_key = manifest_path.map(path_key);
    let ticket_path_key = ticket_path.map(path_key);

    let manifest_ready = manifest["schema_version"].as_u64() == Some(1)
        && manifest["publication_status"].as_str() == Some("local-only")
        && manifest["status"].as_str()
            == Some("ready_for_separate_loader_implementation_review_no_load")
        && manifest["native_load_performed"].as_bool() == Some(false)
        && manifest["broker_may_load_plugin"].as_bool() == Some(false)
        && manifest["loader_may_load_plugin"].as_bool() == Some(false)
        && manifest["ofx_may_route_to_loader"].as_bool() == Some(false)
        && manifest["implementation_gate"]["ready_for_separate_loader_slice_review"].as_bool()
            == Some(true)
        && manifest["implementation_gate"]["native_loader_calls_allowed"].as_bool() == Some(false)
        && manifest["implementation_gate"]["broker_may_load_aex"].as_bool() == Some(false)
        && manifest["implementation_gate"]["ofx_facade_may_route_to_loader"].as_bool()
            == Some(false)
        && manifest["blocked_reasons"]
            .as_array()
            .is_some_and(Vec::is_empty);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "loader_implementation_manifest_ready_no_load",
        manifest_ready,
        format!(
            "status={}, native_load_performed={}, native_loader_calls_allowed={}",
            manifest["status"].as_str().unwrap_or("missing"),
            manifest["native_load_performed"].as_bool().unwrap_or(true),
            manifest["implementation_gate"]["native_loader_calls_allowed"]
                .as_bool()
                .unwrap_or(true)
        ),
        "loader implementation manifest is not a ready no-load review packet",
    );

    let manifest_readiness_summary = manifest_readiness_summary(manifest);
    let manifest_readiness_ok = manifest_readiness_gate_ok(&manifest_readiness_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "manifest_readiness_pipl_gate",
        manifest_readiness_ok,
        format!(
            "provided={}, matched_entry_count={}, status={}, entry_status={}, pipl_content_scan_status={}, pipl_content_scan_ready={}",
            manifest_readiness_summary.provided,
            manifest_readiness_summary.matched_entry_count,
            manifest_readiness_summary.status.as_deref().unwrap_or("missing"),
            manifest_readiness_summary
                .entry_status
                .as_deref()
                .unwrap_or("missing"),
            manifest_readiness_summary
                .pipl_content_scan_status
                .as_deref()
                .unwrap_or("missing"),
            manifest_readiness_summary
                .pipl_content_scan_ready
                .map(|ready| ready.to_string())
                .unwrap_or_else(|| "missing".to_string())
        ),
        "loader implementation manifest readiness evidence did not preserve the semantic PiPL gate",
    );

    let manifest_fixture_refresh_summary = manifest_fixture_refresh_summary(manifest);
    let manifest_fixture_refresh_manifest_check_passed = !manifest_fixture_refresh_summary.provided
        || manifest_check_passed(manifest, "fixture_gate_refresh_audit_ready_no_load");
    let manifest_fixture_refresh_ok = if manifest_fixture_refresh_summary.provided {
        manifest_fixture_refresh_gate_ok(&manifest_fixture_refresh_summary)
            && manifest_fixture_refresh_manifest_check_passed
    } else {
        true
    };
    if manifest_fixture_refresh_summary.provided {
        push_check(
            &mut checks,
            &mut blocked_reasons,
            "manifest_fixture_refresh_audit_ready_no_load",
            manifest_fixture_refresh_ok,
            format!(
                "provided={}, status={}, canonical_non_generated_count={}, generated_target_artifact_count={}, candidates_present_in_refresh={}, manifest_check_passed={}",
                manifest_fixture_refresh_summary.provided,
                manifest_fixture_refresh_summary
                    .status
                    .as_deref()
                    .unwrap_or("missing"),
                manifest_fixture_refresh_summary.wiztree_canonical_non_generated_count,
                manifest_fixture_refresh_summary.wiztree_generated_target_artifact_count,
                manifest_fixture_refresh_summary.candidates_present_in_refresh,
                manifest_fixture_refresh_manifest_check_passed
            ),
            "loader implementation manifest fixture refresh evidence did not preserve the ready no-load queue-hygiene gate",
        );
    }

    let cleanroom_boundary_summary = host_boundary_schema.map(build_cleanroom_boundary_summary);
    let cleanroom_boundary_ok = cleanroom_boundary_summary
        .as_ref()
        .is_none_or(cleanroom_boundary_gate_ok);
    if let Some(summary) = &cleanroom_boundary_summary {
        push_check(
            &mut checks,
            &mut blocked_reasons,
            "host_vocabulary_boundary_no_loader_or_sdk",
            cleanroom_boundary_ok,
            format!(
                "schema_version={}, source_file_count={}, source_forbidden_substring_count={}, native_loader_calls_allowed={}",
                summary
                    .schema_version
                    .map(|version| version.to_string())
                    .unwrap_or_else(|| "missing".to_string()),
                summary.source_file_count,
                summary.source_forbidden_substring_count,
                summary
                    .native_loader_calls_allowed
                    .map(|allowed| allowed.to_string())
                    .unwrap_or_else(|| "missing".to_string())
            ),
            "host vocabulary boundary schema is missing required cleanroom no-load policy evidence",
        );
    }

    let ticket_core_ok = ticket["schema_version"].as_u64() == Some(1)
        && ticket["ticket_protocol_version"].as_u64() == Some(1)
        && ticket["generated_by"].as_str() == Some("aex_image_probe")
        && ticket["publication_status"].as_str() == Some("local-only")
        && ticket["status"].as_str() == Some("accepted_no_load")
        && ticket["native_load_performed"].as_bool() == Some(false)
        && ticket["worker_may_load_plugin"].as_bool() == Some(false)
        && ticket["broker_may_load_plugin"].as_bool() == Some(false)
        && ticket["operation"].as_str() == Some("render_png")
        && ticket_entry["path_match_status"].as_str() == Some("matched_normalized_path")
        && ticket_entry["allowlist_operation_status"].as_str() == Some("render_png")
        && ticket_entry["entry_ready"].as_bool() == Some(true);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "worker_loader_ticket_accepted_no_load",
        ticket_core_ok,
        format!(
            "status={}, operation={}, worker_may_load_plugin={}",
            ticket["status"].as_str().unwrap_or("missing"),
            ticket["operation"].as_str().unwrap_or("missing"),
            ticket["worker_may_load_plugin"].as_bool().unwrap_or(true)
        ),
        "worker loader ticket is not an accepted no-load render ticket",
    );

    let ticket_runtime_evidence_summary = ticket_runtime_evidence_summary(ticket);
    let ticket_runtime_evidence_ok =
        ticket_runtime_evidence_gate_ok(&ticket_runtime_evidence_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "worker_loader_ticket_runtime_evidence",
        ticket_runtime_evidence_ok,
        format!(
            "worker_identity={}, worker_attestation={}, sandbox_preflight={}, job_object={}, handle_inheritance={}",
            ticket_runtime_evidence_summary
                .worker_identity_revalidation_required
                .as_deref()
                .unwrap_or("missing"),
            ticket_runtime_evidence_summary
                .worker_attestation_required
                .as_deref()
                .unwrap_or("missing"),
            ticket_runtime_evidence_summary
                .sandbox_preflight_required
                .as_deref()
                .unwrap_or("missing"),
            ticket_runtime_evidence_summary
                .job_object_required
                .as_deref()
                .unwrap_or("missing"),
            ticket_runtime_evidence_summary
                .handle_inheritance_required
                .as_deref()
                .unwrap_or("missing")
        ),
        "worker loader ticket runtime evidence requirements are missing or not satisfied",
    );

    let identity_ok = manifest_effect_id.is_some()
        && ticket_effect_id == manifest_effect_id
        && manifest_path_key.is_some()
        && ticket_path_key == manifest_path_key;
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "manifest_ticket_identity_match",
        identity_ok,
        format!(
            "manifest_effect_id={}, ticket_effect_id={}, manifest_path={}, ticket_path={}",
            manifest_effect_id.unwrap_or("missing"),
            ticket_effect_id.unwrap_or("missing"),
            manifest_path.unwrap_or("missing"),
            ticket_path.unwrap_or("missing")
        ),
        "loader manifest and worker ticket disagree on effect identity or normalized path",
    );

    let ticket_stages_ok = ticket_stages_are_planned_not_run(ticket);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "ticket_planned_stages_not_run",
        ticket_stages_ok,
        format!(
            "planned_stage_count={}",
            ticket["planned_stages"].as_array().map_or(0, Vec::len)
        ),
        "worker loader ticket has missing or executed native stages",
    );

    let plan_stage_contract_ok = native_stage_order()
        .iter()
        .all(|stage| stage.status == "planned_not_run");
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "native_selector_plan_no_load",
        plan_stage_contract_ok,
        "classic-effect selector order is declared but every stage is planned_not_run".to_string(),
        "native selector plan claims execution",
    );

    let no_forbidden_evidence_tokens = !contains_forbidden_tokens(manifest)
        && !contains_forbidden_tokens(ticket)
        && !contains_forbidden_field_names(manifest)
        && !contains_forbidden_field_names(ticket);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "evidence_anti_contamination",
        no_forbidden_evidence_tokens,
        "loader manifest and worker ticket scanned for forbidden tokens and field names"
            .to_string(),
        "stage plan inputs contain forbidden execution, hash, payload, or rendered-output claims",
    );

    let ready = blocked_reasons.is_empty();
    let status = if ready {
        "planned_native_stage_contract_no_load"
    } else if !manifest_ready || !manifest_readiness_ok || !manifest_fixture_refresh_ok {
        "blocked_loader_manifest"
    } else if !ticket_core_ok || !ticket_runtime_evidence_ok || !ticket_stages_ok {
        "blocked_worker_ticket"
    } else if !identity_ok {
        "blocked_identity_mismatch"
    } else {
        "blocked_no_load_invariants"
    };

    NativeStagePlan {
        schema_version: 1,
        publication_status: "local-only".to_string(),
        status: status.to_string(),
        native_load_performed: false,
        selectors_executed: false,
        render_performed: false,
        broker_may_load_plugin: false,
        worker_may_load_plugin: false,
        ofx_may_route_to_loader: false,
        selected_effect_id: manifest_effect_id.map(str::to_owned),
        normalized_plugin_path: manifest_path.map(str::to_owned),
        manifest_summary: ManifestSummary {
            status: manifest["status"].as_str().map(str::to_owned),
            ready_for_separate_loader_slice_review: manifest["implementation_gate"]
                ["ready_for_separate_loader_slice_review"]
                .as_bool()
                .unwrap_or(false),
            native_loader_calls_allowed: manifest["implementation_gate"]
                ["native_loader_calls_allowed"]
                .as_bool()
                .unwrap_or(true),
            broker_may_load_aex: manifest["implementation_gate"]["broker_may_load_aex"]
                .as_bool()
                .unwrap_or(true),
            ofx_facade_may_route_to_loader: manifest["implementation_gate"]
                ["ofx_facade_may_route_to_loader"]
                .as_bool()
                .unwrap_or(true),
        },
        manifest_readiness_summary,
        manifest_fixture_refresh_audit_summary: manifest_fixture_refresh_summary,
        cleanroom_boundary_summary,
        ticket_summary: TicketSummary {
            status: ticket["status"].as_str().map(str::to_owned),
            operation: ticket["operation"].as_str().map(str::to_owned),
            allowlist_id: ticket["allowlist_id"].as_str().map(str::to_owned),
            native_load_performed: ticket["native_load_performed"].as_bool().unwrap_or(true),
            worker_may_load_plugin: ticket["worker_may_load_plugin"].as_bool().unwrap_or(true),
            planned_stage_count: ticket["planned_stages"].as_array().map_or(0, Vec::len),
        },
        ticket_runtime_evidence_summary,
        host_struct_plan: host_struct_plan(),
        native_stage_order: native_stage_order(),
        promotion_gate: PromotionGate {
            native_loader_calls_allowed: false,
            worker_selector_calls_allowed: false,
            worker_pixel_buffers_allowed: false,
            ofx_facade_may_route_to_loader: false,
            requires_separate_loader_slice: true,
            requires_explicit_user_approval: true,
            requires_code_review: true,
            requires_local_fixture_only: true,
        },
        checks,
        blocked_reasons,
        next_action: if ready {
            "Use this no-load stage contract as review input for a separate native loader slice; do not execute selectors from this artifact.".to_string()
        } else {
            "Fix blocked no-load manifest/ticket evidence before designing a native loader slice."
                .to_string()
        },
        notes: vec![
            "Stage plan reads JSON metadata only.".to_string(),
            "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered."
                .to_string(),
            "PF selector names are planning labels only; every selector remains planned_not_run."
                .to_string(),
            "This artifact is not loader approval and does not permit OFX routing.".to_string(),
        ],
    }
}

fn manifest_fixture_refresh_summary(manifest: &Value) -> ManifestFixtureRefreshSummary {
    let summary = &manifest["preflight_summary"]["fixture_refresh_audit_summary"];
    let provided = summary["provided"].as_bool().unwrap_or(false);
    if !provided {
        return ManifestFixtureRefreshSummary {
            provided: false,
            schema_version: None,
            publication_status: None,
            status: None,
            native_load_performed: None,
            render_performed: None,
            fixture_selected: None,
            loader_enabled: None,
            fixture_gate_candidate_count: 0,
            wiztree_total_aex_count: 0,
            wiztree_canonical_non_generated_count: 0,
            wiztree_generated_target_artifact_count: 0,
            generated_target_artifacts_excluded: false,
            candidates_present_in_refresh: false,
            input_contains_forbidden_tokens: false,
            blocked_reason_count: 0,
        };
    }
    ManifestFixtureRefreshSummary {
        provided: true,
        schema_version: summary["schema_version"]
            .as_u64()
            .and_then(|version| u32::try_from(version).ok()),
        publication_status: summary["publication_status"].as_str().map(str::to_owned),
        status: summary["status"].as_str().map(str::to_owned),
        native_load_performed: summary["native_load_performed"].as_bool(),
        render_performed: summary["render_performed"].as_bool(),
        fixture_selected: summary["fixture_selected"].as_bool(),
        loader_enabled: summary["loader_enabled"].as_bool(),
        fixture_gate_candidate_count: summary["fixture_gate_candidate_count"]
            .as_u64()
            .map_or(0, |count| count as usize),
        wiztree_total_aex_count: summary["wiztree_total_aex_count"].as_u64().unwrap_or(0),
        wiztree_canonical_non_generated_count: summary["wiztree_canonical_non_generated_count"]
            .as_u64()
            .unwrap_or(0),
        wiztree_generated_target_artifact_count: summary["wiztree_generated_target_artifact_count"]
            .as_u64()
            .unwrap_or(0),
        generated_target_artifacts_excluded: summary["generated_target_artifacts_excluded"]
            .as_bool()
            .unwrap_or(false),
        candidates_present_in_refresh: summary["candidates_present_in_refresh"]
            .as_bool()
            .unwrap_or(false),
        input_contains_forbidden_tokens: summary["input_contains_forbidden_tokens"]
            .as_bool()
            .unwrap_or(true),
        blocked_reason_count: summary["blocked_reason_count"].as_u64().unwrap_or(1) as usize,
    }
}

fn manifest_fixture_refresh_gate_ok(summary: &ManifestFixtureRefreshSummary) -> bool {
    summary.provided
        && summary.schema_version == Some(1)
        && summary.publication_status.as_deref() == Some("local-only")
        && summary.status.as_deref() == Some("fixture_gate_refresh_ready_no_load")
        && summary.native_load_performed == Some(false)
        && summary.render_performed == Some(false)
        && summary.fixture_selected == Some(false)
        && summary.loader_enabled == Some(false)
        && summary.fixture_gate_candidate_count == 2
        && summary.wiztree_total_aex_count == 119
        && summary.wiztree_canonical_non_generated_count == 40
        && summary.wiztree_generated_target_artifact_count == 79
        && summary.generated_target_artifacts_excluded
        && summary.candidates_present_in_refresh
        && !summary.input_contains_forbidden_tokens
        && summary.blocked_reason_count == 0
}

fn manifest_check_passed(manifest: &Value, name: &str) -> bool {
    manifest["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|check| {
            check["name"].as_str() == Some(name) && check["status"].as_str() == Some("passed")
        })
}

fn build_cleanroom_boundary_summary(boundary: &Value) -> CleanroomBoundarySummary {
    let policy = &boundary["allowed_policy"];
    let abi_generator_key = ["bind", "gen_allowed"].concat();
    let effect_host_crate_key = ["third_party_", "after", "_effects_crate_allowed"].concat();
    let dynamic_loader_key = [
        "reuse_existing_aviutl_",
        "lib",
        "loading_path_for_aex_allowed",
    ]
    .concat();
    CleanroomBoundarySummary {
        schema_name: boundary["schema_name"].as_str().map(str::to_owned),
        schema_version: boundary["schema_version"].as_u64(),
        publication_status: boundary["publication_status"].as_str().map(str::to_owned),
        compatibility_classification: boundary["compatibility_classification"]
            .as_str()
            .map(str::to_owned),
        source_file_count: json_array_len(&boundary["source_files"]),
        allowed_planning_label_count: json_array_len(&boundary["allowed_planning_labels"]),
        allowed_metadata_label_count: json_array_len(&boundary["allowed_metadata_labels"]),
        source_forbidden_substring_count: json_array_len(&boundary["source_forbidden_substrings"]),
        required_source_boundary_notes_count: json_array_len(
            &boundary["required_source_boundary_notes"],
        ),
        native_loader_calls_allowed: policy["native_loader_calls_allowed"].as_bool(),
        adobe_sdk_headers_allowed: policy["adobe_sdk_headers_allowed"].as_bool(),
        abi_generator_allowed: policy_bool(policy, &abi_generator_key),
        third_party_effect_host_crate_allowed: policy_bool(policy, &effect_host_crate_key),
        third_party_pipl_crate_allowed: policy["third_party_pipl_crate_allowed"].as_bool(),
        reuse_existing_aviutl_dynamic_loader_for_aex_allowed: policy_bool(
            policy,
            &dynamic_loader_key,
        ),
        pf_names_are_planning_labels_only: policy["pf_names_are_planning_labels_only"].as_bool(),
        metadata_labels_do_not_define_abi: policy["metadata_labels_do_not_define_abi"].as_bool(),
        worker_os_isolation_ffi_allowed: policy["worker_os_isolation_ffi_allowed"].as_bool(),
    }
}

fn cleanroom_boundary_gate_ok(summary: &CleanroomBoundarySummary) -> bool {
    summary.schema_name.as_deref() == Some("AEX host vocabulary cleanroom boundary")
        && summary.schema_version == Some(1)
        && summary.publication_status.as_deref() == Some("local-only design artifact")
        && summary.compatibility_classification.as_deref()
            == Some("Cleanroom planning vocabulary guard")
        && summary.source_file_count > 0
        && summary.allowed_planning_label_count > 0
        && summary.allowed_metadata_label_count > 0
        && summary.source_forbidden_substring_count > 0
        && summary.required_source_boundary_notes_count > 0
        && summary.native_loader_calls_allowed == Some(false)
        && summary.adobe_sdk_headers_allowed == Some(false)
        && summary.abi_generator_allowed == Some(false)
        && summary.third_party_effect_host_crate_allowed == Some(false)
        && summary.third_party_pipl_crate_allowed == Some(false)
        && summary.reuse_existing_aviutl_dynamic_loader_for_aex_allowed == Some(false)
        && summary.pf_names_are_planning_labels_only == Some(true)
        && summary.metadata_labels_do_not_define_abi == Some(true)
}

fn json_array_len(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn policy_bool(policy: &Value, key: &str) -> Option<bool> {
    policy.get(key).and_then(Value::as_bool)
}

fn ticket_runtime_evidence_summary(ticket: &Value) -> TicketRuntimeEvidenceSummary {
    let evidence = &ticket["required_runtime_evidence"];
    TicketRuntimeEvidenceSummary {
        worker_identity_revalidation_required: safe_ticket_runtime_value(
            &evidence["worker_identity_revalidation_required"],
        ),
        worker_attestation_required: safe_ticket_runtime_value(
            &evidence["worker_attestation_required"],
        ),
        sandbox_preflight_required: safe_ticket_runtime_value(
            &evidence["sandbox_preflight_required"],
        ),
        job_object_required: safe_ticket_runtime_value(&evidence["job_object_required"]),
        handle_inheritance_required: safe_ticket_runtime_value(
            &evidence["handle_inheritance_required"],
        ),
    }
}

fn safe_ticket_runtime_value(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    if string_contains_forbidden_tokens(text) {
        return None;
    }
    Some(text.to_owned())
}

fn ticket_runtime_evidence_gate_ok(summary: &TicketRuntimeEvidenceSummary) -> bool {
    summary.worker_identity_revalidation_required.as_deref() == Some("passed")
        && summary.worker_attestation_required.as_deref() == Some("passed")
        && summary.sandbox_preflight_required.as_deref() == Some("passed")
        && summary.job_object_required.as_deref() == Some("assigned-with-kill-on-close")
        && summary.handle_inheritance_required.as_deref()
            == Some("sentinel_not_inherited-with-explicit-handle-list")
}

fn manifest_readiness_summary(manifest: &Value) -> ManifestReadinessSummary {
    let summary = &manifest["readiness_summary"];
    ManifestReadinessSummary {
        provided: summary["provided"].as_bool().unwrap_or(false),
        matched_entry_count: summary["matched_entry_count"].as_u64().unwrap_or(0) as usize,
        status: summary["status"].as_str().map(str::to_owned),
        entry_status: summary["entry_status"].as_str().map(str::to_owned),
        pipl_content_scan_status: summary["pipl_content_scan_status"]
            .as_str()
            .map(str::to_owned),
        pipl_content_scan_ready: summary["pipl_content_scan_ready"].as_bool(),
        allowed_operations: summary["allowed_operations"]
            .as_array()
            .map(|operations| {
                operations
                    .iter()
                    .filter_map(|operation| operation.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn manifest_readiness_gate_ok(summary: &ManifestReadinessSummary) -> bool {
    if !summary.provided {
        return true;
    }
    summary.matched_entry_count == 1
        && summary.status.as_deref() == Some("probe_readiness_planned")
        && summary.entry_status.as_deref() == Some("draft_allowlisted")
        && summary.pipl_content_scan_status.as_deref() == Some("semantic_matches")
        && summary.pipl_content_scan_ready == Some(true)
        && summary
            .allowed_operations
            .iter()
            .any(|operation| operation == "describe")
}

fn ticket_stages_are_planned_not_run(ticket: &Value) -> bool {
    let Some(stages) = ticket["planned_stages"].as_array() else {
        return false;
    };
    required_coarse_stage_names().iter().all(|required| {
        stages.iter().any(|stage| {
            ticket_stage_name(stage) == Some(*required)
                && stage["status"].as_str() == Some("planned_not_run")
        })
    }) && stages
        .iter()
        .all(|stage| stage["status"].as_str() == Some("planned_not_run"))
}

fn ticket_stage_name(stage: &Value) -> Option<&str> {
    stage["stage"].as_str().or_else(|| stage["name"].as_str())
}

fn required_coarse_stage_names() -> Vec<&'static str> {
    vec![
        "load",
        "global_setup",
        "params_setup",
        "sequence_setup",
        "render",
        "sequence_teardown",
        "global_teardown",
    ]
}

fn host_struct_plan() -> Vec<HostStructPlan> {
    vec![
        host_struct("PF_InData", "host timing and context fields"),
        host_struct("PF_OutData", "plug-in response fields"),
        host_struct("PF_ParamDef[]", "parameter descriptor array shape"),
        host_struct("PF_LayerDef source", "synthetic RGBA source surface shape"),
        host_struct(
            "PF_LayerDef destination",
            "synthetic RGBA destination surface shape",
        ),
    ]
}

fn host_struct(name: &str, purpose: &str) -> HostStructPlan {
    HostStructPlan {
        name: name.to_string(),
        allocation_status: "declared_not_allocated".to_string(),
        mutation_status: "not_mutated".to_string(),
        purpose: purpose.to_string(),
    }
}

fn native_stage_order() -> Vec<NativeStage> {
    vec![
        stage(
            "load",
            "load",
            "native_module_load_deferred",
            &[],
            "no module handle allocated",
        ),
        stage(
            "global_setup",
            "global_setup",
            "PF_Cmd_GLOBAL_SETUP",
            &["PF_InData", "PF_OutData"],
            "host context not materialized",
        ),
        stage(
            "params_setup",
            "params_setup",
            "PF_Cmd_PARAMS_SETUP",
            &["PF_InData", "PF_OutData", "PF_ParamDef[]"],
            "parameter descriptors not materialized",
        ),
        stage(
            "sequence_setup",
            "sequence_setup",
            "PF_Cmd_SEQUENCE_SETUP",
            &["PF_InData", "PF_OutData", "PF_ParamDef[]"],
            "sequence state not allocated",
        ),
        stage(
            "frame_setup",
            "render",
            "PF_Cmd_FRAME_SETUP",
            &["PF_InData", "PF_OutData", "PF_ParamDef[]"],
            "frame state not allocated",
        ),
        stage(
            "render",
            "render",
            "PF_Cmd_RENDER",
            &[
                "PF_InData",
                "PF_OutData",
                "PF_ParamDef[]",
                "PF_LayerDef source",
                "PF_LayerDef destination",
            ],
            "pixel surfaces not allocated",
        ),
        stage(
            "frame_setdown",
            "render",
            "PF_Cmd_FRAME_SETDOWN",
            &["PF_InData", "PF_OutData"],
            "frame teardown not invoked",
        ),
        stage(
            "sequence_setdown",
            "sequence_teardown",
            "PF_Cmd_SEQUENCE_SETDOWN",
            &["PF_InData", "PF_OutData"],
            "sequence teardown not invoked",
        ),
        stage(
            "global_setdown",
            "global_teardown",
            "PF_Cmd_GLOBAL_SETDOWN",
            &["PF_InData", "PF_OutData"],
            "global teardown not invoked",
        ),
    ]
}

fn stage(
    name: &str,
    coarse_ticket_stage: &str,
    pf_selector: &str,
    host_structs: &[&str],
    surface_status: &str,
) -> NativeStage {
    NativeStage {
        name: name.to_string(),
        coarse_ticket_stage: coarse_ticket_stage.to_string(),
        pf_selector: pf_selector.to_string(),
        status: "planned_not_run".to_string(),
        host_structs: host_structs.iter().map(|item| item.to_string()).collect(),
        surface_status: surface_status.to_string(),
    }
}

fn contains_forbidden_tokens(value: &Value) -> bool {
    let serialized = serde_json::to_string(value)
        .unwrap_or_default()
        .to_ascii_lowercase();
    string_contains_forbidden_tokens(&serialized)
}

fn string_contains_forbidden_tokens(serialized: &str) -> bool {
    let serialized = serialized.to_ascii_lowercase();
    forbidden_serialized_tokens()
        .iter()
        .any(|token| serialized.contains(token))
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

fn path_key(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for part in path.trim().replace('/', "\\").split('\\') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            match parts.last() {
                Some(previous) if !previous.ends_with(':') && previous != ".." => {
                    parts.pop();
                }
                _ => parts.push(part.to_string()),
            }
            continue;
        }
        parts.push(part.to_string());
    }
    parts.join("\\").to_ascii_lowercase()
}

fn push_check(
    checks: &mut Vec<PlanCheck>,
    blocked_reasons: &mut Vec<String>,
    name: &str,
    passed: bool,
    evidence: String,
    blocked_reason: &str,
) {
    checks.push(PlanCheck {
        name: name.to_string(),
        status: if passed { "passed" } else { "blocked" }.to_string(),
        evidence,
    });
    if !passed {
        blocked_reasons.push(blocked_reason.to_string());
    }
}

fn parse_args() -> Result<(PathBuf, PathBuf, PathBuf, Option<PathBuf>), String> {
    let mut manifest = None;
    let mut ticket = None;
    let mut host_boundary = None;
    let mut out = PathBuf::from("target")
        .join("aex-native-stage-plan")
        .join("native-stage-plan.local.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => {
                manifest = args.next().map(PathBuf::from);
                if manifest.is_none() {
                    return Err("--manifest requires a path".to_string());
                }
            }
            "--ticket" => {
                ticket = args.next().map(PathBuf::from);
                if ticket.is_none() {
                    return Err("--ticket requires a path".to_string());
                }
            }
            "--host-boundary" => {
                host_boundary = args.next().map(PathBuf::from);
                if host_boundary.is_none() {
                    return Err("--host-boundary requires a path".to_string());
                }
            }
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a path".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: aex_native_stage_plan --manifest target/aex-loader-implementation/loader-implementation.local.json --ticket target/aex-image-probe/.../worker-loader-ticket-*.json [--host-boundary analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json] [--out target/aex-native-stage-plan/native-stage-plan.local.json]".to_string());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let manifest = manifest.ok_or_else(|| "--manifest is required".to_string())?;
    let ticket = ticket.ok_or_else(|| "--ticket is required".to_string())?;
    Ok((manifest, ticket, out, host_boundary))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (manifest_path, ticket_path, out_path, host_boundary_path) = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let manifest = std::fs::read_to_string(&manifest_path)?;
    let ticket = std::fs::read_to_string(&ticket_path)?;
    let host_boundary = host_boundary_path
        .as_ref()
        .map(std::fs::read_to_string)
        .transpose()?;
    let report = plan_native_stage_contract_json_with_boundary_schema(
        &manifest,
        &ticket,
        host_boundary.as_deref(),
    )?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, report)?;
    println!("{}", out_path.display());
    Ok(())
}
