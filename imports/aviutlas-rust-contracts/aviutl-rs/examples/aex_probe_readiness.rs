//! Draft AEX probe-readiness files from a static classifier catalog.
//!
//! This example does not open, hash, load, or execute `.aex` files. It only
//! transforms local static metadata into draft allowlist/readiness artifacts for
//! later reviewed worker-probe use.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};

const DEFAULT_MAX_PLUGIN_BYTES: u64 = 268_435_456;

#[derive(Debug)]
pub struct ReadinessOutputText {
    pub allowlist_json: String,
    pub readiness_json: String,
    pub loader_gate_json: String,
    pub requests: Vec<ReadinessRequestText>,
    pub capabilities: Vec<ReadinessCapabilityText>,
    pub fixture_review: Option<ReadinessFixtureReviewText>,
    pub loader_preflight: Option<ReadinessPreflightText>,
}

#[derive(Debug)]
pub struct ReadinessRequestText {
    pub file_name: String,
    pub json: String,
}

#[derive(Debug)]
pub struct ReadinessCapabilityText {
    pub file_name: String,
    pub json: String,
}

#[derive(Debug)]
pub struct ReadinessFixtureReviewText {
    pub file_name: String,
    pub json: String,
}

#[derive(Debug)]
pub struct ReadinessPreflightText {
    pub file_name: String,
    pub json: String,
}

#[derive(Debug, Deserialize)]
struct FlexibleInput {
    #[serde(default)]
    effects: Vec<CatalogEffect>,
    #[serde(default)]
    aex_candidates: Vec<InventoryCandidate>,
}

#[derive(Debug, Deserialize)]
struct CatalogEffect {
    effect_id: Option<String>,
    path: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    fixture_status: Option<String>,
    #[serde(default)]
    plugin_class: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    pipl_name: Option<String>,
    #[serde(default)]
    pipl_match_name: Option<String>,
    #[serde(default)]
    pipl_content_scan: Option<CatalogPiplContentScan>,
    #[serde(default)]
    source: Option<CatalogSource>,
    #[serde(default)]
    identity: Option<CatalogIdentity>,
    #[serde(default)]
    classification: Option<CatalogClassification>,
}

#[derive(Debug, Deserialize)]
struct CatalogPiplContentScan {
    status: Option<String>,
    contents_emitted: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CatalogSource {
    path: Option<String>,
    origin: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CatalogIdentity {
    display_name: Option<String>,
    match_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CatalogClassification {
    plugin_class: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InventoryCandidate {
    path: String,
    #[serde(default)]
    bytes: Option<u64>,
    #[serde(default)]
    inferred_class: String,
    #[serde(default)]
    fixture_status: String,
}

#[derive(Debug, Deserialize)]
struct FixtureReviewGateInput {
    #[serde(default)]
    status: String,
    #[serde(default)]
    selected_fixture: Option<String>,
    #[serde(default)]
    recommended_first_review: Option<String>,
    #[serde(default)]
    recommendation_status: Option<String>,
    #[serde(default)]
    candidates: Vec<FixtureReviewGateCandidate>,
}

#[derive(Debug, Deserialize)]
struct FixtureReviewGateCandidate {
    id: String,
    path: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    source_tree: Option<String>,
    #[serde(default)]
    observed_size_bytes: Option<u64>,
    #[serde(default)]
    fixture_status: Option<String>,
    #[serde(default)]
    classifier_status: Option<String>,
    #[serde(default)]
    plugin_class: Option<String>,
    #[serde(default)]
    review_priority: Option<u32>,
    #[serde(default)]
    review_status: Option<String>,
    #[serde(default)]
    blocked_reasons: Vec<String>,
}

#[derive(Debug, Clone)]
struct FixtureReviewGate {
    status: String,
    selected_fixture: Option<String>,
    recommended_first_review: Option<String>,
    recommendation_status: Option<String>,
    candidates: Vec<FixtureReviewCandidateMetadata>,
    candidate_ids: Vec<String>,
    candidate_paths: HashSet<String>,
}

#[derive(Debug, Clone)]
struct FixtureReviewCandidateMetadata {
    id: String,
    path: String,
    display_name: Option<String>,
    source_tree: Option<String>,
    observed_size_bytes: Option<u64>,
    fixture_status: Option<String>,
    classifier_status: Option<String>,
    plugin_class: Option<String>,
    review_priority: Option<u32>,
    review_status: Option<String>,
    blocked_reasons: Vec<String>,
}

#[derive(Debug, Clone)]
struct Candidate {
    effect_id: String,
    path: String,
    artifact_stem: String,
    size_bytes: Option<u64>,
    fixture_status: String,
    plugin_class: String,
    classifier_status: String,
    display_name: String,
    pipl_content_scan_status: Option<String>,
    pipl_content_scan_contents_emitted: Option<bool>,
    pipl_scan_ready_for_readiness: bool,
}

#[derive(Debug, Serialize)]
struct ProbeAllowlistDraft {
    schema_version: u32,
    publication_status: String,
    allowlist_publication_status: String,
    draft_status: String,
    default_max_plugin_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    fixture_review_gate: Option<FixtureReviewGateSummary>,
    entries: Vec<AllowlistEntryDraft>,
    blocked_classes: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AllowlistEntryDraft {
    id: String,
    plugin_path: String,
    expected_class: String,
    allowed_operations: Vec<String>,
    max_width: u32,
    max_height: u32,
    timeout_ms: u32,
    fixture_status: String,
    publication_status: String,
    license_status: String,
    classifier_status: String,
    classifier_inferred_class: String,
    max_plugin_bytes: u64,
}

#[derive(Debug, Serialize)]
struct ReadinessReport {
    schema_version: u32,
    status: String,
    publication_status: String,
    candidate_count: usize,
    allowlist_entry_count: usize,
    blocked_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    fixture_review_gate: Option<FixtureReviewGateSummary>,
    generated_files: Vec<String>,
    entries: Vec<ReadinessEntry>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LoaderGateReport {
    schema_version: u32,
    status: String,
    publication_status: String,
    approved: bool,
    loader_enabled: bool,
    real_aex_load_enabled: bool,
    candidate_count: usize,
    open_candidate_count: usize,
    blocked_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    fixture_review_gate: Option<FixtureReviewGateSummary>,
    entries: Vec<LoaderGateEntry>,
    requirements_before_open: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LoaderGateEntry {
    effect_id: String,
    display_name: String,
    plugin_path: String,
    declared_size_bytes: Option<u64>,
    max_plugin_bytes: u64,
    pre_loader_status: String,
    loader_approval_status: String,
    allowlist_operation_status: String,
    sandbox_preflight_required: String,
    job_object_required: String,
    handle_inheritance_required: String,
    worker_identity_revalidation_required: String,
    worker_attestation_required: String,
    ofx_facade_status: String,
    blocked_reasons: Vec<String>,
    required_before_loader: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct FixtureReviewGateSummary {
    status: String,
    selected_fixture: Option<String>,
    recommended_first_review: Option<String>,
    recommendation_status: Option<String>,
    allowed_candidate_count: usize,
    candidate_ids: Vec<String>,
    effect: String,
}

#[derive(Debug, Serialize)]
struct ReadinessEntry {
    effect_id: String,
    display_name: String,
    plugin_path: String,
    status: String,
    classifier_status: String,
    classifier_inferred_class: String,
    pipl_content_scan_status: String,
    pipl_content_scan_ready: bool,
    allowed_operations: Vec<String>,
    blocked_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct CapabilityDraft {
    schema_version: u32,
    effect_id: String,
    display_name: String,
    plugin_path: String,
    publication_status: String,
    evidence_mode: String,
    load_status: String,
    broker_may_load_plugin: bool,
    current_supported_operations: Vec<String>,
    params_status: String,
    params: Vec<serde_json::Value>,
    selectors: Vec<CapabilitySelectorStatus>,
    aex_worker: CapabilitySupportStatus,
    ofx_facade: CapabilitySupportStatus,
    unsupported_or_deferred_surfaces: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CapabilitySelectorStatus {
    name: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct CapabilitySupportStatus {
    supported: bool,
    status: String,
}

#[derive(Debug, Serialize)]
struct ReadinessLoaderPreflightDraft {
    schema_version: u32,
    status: String,
    preflight_passed: bool,
    native_load_performed: bool,
    broker_may_load_plugin: bool,
    selected_fixture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fixture_review_gate: Option<FixtureReviewGateSummary>,
    loader_gate_status: String,
    loader_gate_approved: bool,
    loader_enabled: bool,
    real_aex_load_enabled: bool,
    open_candidate_count: usize,
    blocked_reasons: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FixtureReviewPacket {
    schema_version: u32,
    publication_status: String,
    status: String,
    metadata_mode: String,
    selected_fixture: Option<String>,
    recommended_first_review: Option<String>,
    recommendation_status: Option<String>,
    candidate_count: usize,
    selected_candidate_count: usize,
    auto_approval_granted: bool,
    queue: Vec<FixtureReviewPacketCandidate>,
    required_manual_decisions: Vec<String>,
    forbidden_actions: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FixtureReviewPacketCandidate {
    id: String,
    display_name: Option<String>,
    plugin_path: String,
    source_tree: Option<String>,
    observed_size_bytes: Option<u64>,
    fixture_status: Option<String>,
    classifier_status: Option<String>,
    plugin_class: Option<String>,
    review_priority: Option<u32>,
    review_status: Option<String>,
    recommendation: String,
    selection_status: String,
    manual_selection_required: bool,
    blocked_reasons: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DescribeRequest {
    schema_version: u32,
    operation: String,
    plugin_path: String,
    allowlist: String,
    params: serde_json::Value,
    pixel_format: String,
    timeouts_ms: ProbeTimeouts,
}

#[derive(Debug, Serialize)]
struct ProbeTimeouts {
    launch: u32,
    setup: u32,
    render: u32,
    teardown: u32,
}

pub fn plan_probe_readiness_json(input: &str) -> Result<ReadinessOutputText, Box<dyn Error>> {
    plan_probe_readiness_json_with_fixture_gate(input, None)
}

pub fn plan_probe_readiness_json_with_fixture_gate(
    input: &str,
    fixture_gate: Option<&str>,
) -> Result<ReadinessOutputText, Box<dyn Error>> {
    let parsed: FlexibleInput = serde_json::from_str(input)?;
    let fixture_gate = fixture_gate.map(parse_fixture_review_gate).transpose()?;
    let fixture_gate_summary = fixture_gate.as_ref().map(fixture_review_gate_summary);
    let candidates = collect_candidates(parsed);
    let artifact_stem_counts = artifact_stem_counts(&candidates);
    let ready = candidates
        .iter()
        .filter(|candidate| {
            is_probe_candidate(candidate, fixture_gate.as_ref(), &artifact_stem_counts)
        })
        .cloned()
        .collect::<Vec<_>>();

    let allowlist = ProbeAllowlistDraft {
        schema_version: 1,
        publication_status: "local-only draft".to_string(),
        allowlist_publication_status: "local-only".to_string(),
        draft_status: "not-approved".to_string(),
        default_max_plugin_bytes: DEFAULT_MAX_PLUGIN_BYTES,
        fixture_review_gate: fixture_gate_summary.clone(),
        entries: ready
            .iter()
            .map(|candidate| AllowlistEntryDraft {
                id: candidate.effect_id.clone(),
                plugin_path: candidate.path.clone(),
                expected_class: "classic-effect".to_string(),
                allowed_operations: vec!["describe".to_string()],
                max_width: 4096,
                max_height: 4096,
                timeout_ms: 1000,
                fixture_status: candidate.fixture_status.clone(),
                publication_status: "local-only".to_string(),
                license_status: "local-only-unpublished".to_string(),
                classifier_status: candidate.classifier_status.clone(),
                classifier_inferred_class: candidate.plugin_class.clone(),
                max_plugin_bytes: DEFAULT_MAX_PLUGIN_BYTES,
            })
            .collect(),
        blocked_classes: vec![
            "aegp".to_string(),
            "aeio".to_string(),
            "smartfx-only".to_string(),
            "gpu-only".to_string(),
            "unknown".to_string(),
        ],
        notes: vec![
            "Draft only: not reviewed for worker launch or render_png.".to_string(),
            "Generated from static metadata without opening, hashing, loading, or executing .aex files.".to_string(),
            "Only describe is allowlisted; render_png needs explicit approval.".to_string(),
            if fixture_gate.is_some() {
                "Fixture review gate applied: draft entries are limited to gate candidates and still not approved.".to_string()
            } else {
                "No fixture review gate was applied to this draft allowlist.".to_string()
            },
        ],
    };

    let mut generated_files = vec![
        "allowlist.local.draft.json".to_string(),
        "readiness.local.json".to_string(),
    ];
    generated_files.extend(
        ready
            .iter()
            .map(|candidate| format!("requests/{}.describe.json", candidate.artifact_stem)),
    );
    generated_files.extend(
        ready
            .iter()
            .map(|candidate| format!("capabilities/{}.capability.json", candidate.artifact_stem)),
    );
    generated_files.push("loader-gate.local.json".to_string());
    if fixture_gate.is_some() {
        generated_files.push("fixture-review.local.json".to_string());
        generated_files.push("loader-preflight.local.json".to_string());
    }

    let readiness = ReadinessReport {
        schema_version: 1,
        status: "probe_readiness_planned".to_string(),
        publication_status: "local-only".to_string(),
        candidate_count: candidates.len(),
        allowlist_entry_count: ready.len(),
        blocked_count: candidates.len().saturating_sub(ready.len()),
        fixture_review_gate: fixture_gate_summary.clone(),
        generated_files,
        entries: candidates
            .iter()
            .map(|candidate| {
                let ready =
                    is_probe_candidate(candidate, fixture_gate.as_ref(), &artifact_stem_counts);
                ReadinessEntry {
                    effect_id: candidate.effect_id.clone(),
                    display_name: candidate.display_name.clone(),
                    plugin_path: candidate.path.clone(),
                    status: if ready {
                        "draft_allowlisted"
                    } else {
                        "blocked_or_deferred"
                    }
                    .to_string(),
                    classifier_status: candidate.classifier_status.clone(),
                    classifier_inferred_class: candidate.plugin_class.clone(),
                    pipl_content_scan_status: candidate
                        .pipl_content_scan_status
                        .clone()
                        .unwrap_or_else(|| "missing".to_string()),
                    pipl_content_scan_ready: candidate.pipl_scan_ready_for_readiness,
                    allowed_operations: if ready {
                        vec!["describe".to_string()]
                    } else {
                        Vec::new()
                    },
                    blocked_reason: if ready {
                        None
                    } else {
                        Some(blocked_reason(
                            candidate,
                            fixture_gate.as_ref(),
                            &artifact_stem_counts,
                        ))
                    },
                }
            })
            .collect(),
        notes: vec![
            "Draft readiness does not prove plugin loadability or render support.".to_string(),
            "Worker execution still requires explicit reviewed worker_exe and allowlist review."
                .to_string(),
            "Static-classifier catalog candidates require pipl_content_scan.status=semantic_matches with contents_emitted=false before readiness planning.".to_string(),
        ],
    };

    let loader_gate = LoaderGateReport {
        schema_version: 1,
        status: "loader_gate_not_opened".to_string(),
        publication_status: "local-only".to_string(),
        approved: false,
        loader_enabled: false,
        real_aex_load_enabled: false,
        candidate_count: ready.len(),
        open_candidate_count: 0,
        blocked_count: ready.len(),
        fixture_review_gate: fixture_gate_summary.clone(),
        entries: ready
            .iter()
            .map(|candidate| LoaderGateEntry {
                effect_id: candidate.effect_id.clone(),
                display_name: candidate.display_name.clone(),
                plugin_path: candidate.path.clone(),
                declared_size_bytes: candidate.size_bytes,
                max_plugin_bytes: DEFAULT_MAX_PLUGIN_BYTES,
                pre_loader_status: "blocked_pending_review".to_string(),
                loader_approval_status: "not-approved".to_string(),
                allowlist_operation_status: "describe-only".to_string(),
                sandbox_preflight_required: "passed".to_string(),
                job_object_required: "assigned-with-kill-on-close".to_string(),
                handle_inheritance_required:
                    "sentinel_not_inherited-with-explicit-handle-list".to_string(),
                worker_identity_revalidation_required: "passed".to_string(),
                worker_attestation_required: "passed".to_string(),
                ofx_facade_status: "deferred-same-broker-worker-contract".to_string(),
                blocked_reasons: loader_gate_blockers(candidate),
                required_before_loader: loader_gate_requirements(),
            })
            .collect(),
        requirements_before_open: loader_gate_requirements(),
        notes: vec![
            "This gate is evidence for a future loader slice; it is not loader approval.".to_string(),
            "OFX must remain a facade over the same broker/worker/sandbox contract and cannot bypass AEX allowlist gates.".to_string(),
            "No .aex file is opened, hashed, loaded, executed, copied, or rendered by this planner.".to_string(),
            if fixture_gate.is_some() {
                "Fixture review gate was applied; selected_fixture remains external review state and does not approve loading.".to_string()
            } else {
                "No fixture review gate was applied; first-loader review must still be narrowed separately.".to_string()
            },
        ],
    };

    let requests = ready
        .iter()
        .map(|candidate| {
            let request = DescribeRequest {
                schema_version: 1,
                operation: "describe".to_string(),
                plugin_path: candidate.path.clone(),
                allowlist: "../allowlist.local.draft.json".to_string(),
                params: serde_json::json!({}),
                pixel_format: "rgba8".to_string(),
                timeouts_ms: ProbeTimeouts {
                    launch: 3000,
                    setup: 3000,
                    render: 5000,
                    teardown: 1000,
                },
            };
            Ok(ReadinessRequestText {
                file_name: format!("{}.describe.json", candidate.artifact_stem),
                json: serde_json::to_string_pretty(&request)?,
            })
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;

    let capabilities = ready
        .iter()
        .map(|candidate| {
            let capability = CapabilityDraft {
                schema_version: 1,
                effect_id: candidate.effect_id.clone(),
                display_name: candidate.display_name.clone(),
                plugin_path: candidate.path.clone(),
                publication_status: "local-only".to_string(),
                evidence_mode: "static-classifier-metadata-only".to_string(),
                load_status: "not_loaded".to_string(),
                broker_may_load_plugin: false,
                current_supported_operations: Vec::new(),
                params_status: "unknown".to_string(),
                params: Vec::new(),
                selectors: selector_not_run_statuses(),
                aex_worker: CapabilitySupportStatus {
                    supported: false,
                    status: "deferred_loader_gate_closed".to_string(),
                },
                ofx_facade: CapabilitySupportStatus {
                    supported: false,
                    status: "deferred_same_aex_worker_gate".to_string(),
                },
                unsupported_or_deferred_surfaces: vec![
                    "SmartFX".to_string(),
                    "GPU".to_string(),
                    "AEGP suites".to_string(),
                    "AEIO".to_string(),
                    "audio".to_string(),
                    "layer checkout".to_string(),
                    "custom UI".to_string(),
                    "arbitrary file or network APIs".to_string(),
                ],
                notes: vec![
                    "Capability draft only: generated from static metadata without loading .aex."
                        .to_string(),
                    "Selectors are not_run; no parameter descriptors or render pixels are claimed."
                        .to_string(),
                    "Real loader, AEX worker support, and OFX facade support remain disabled."
                        .to_string(),
                ],
            };
            Ok(ReadinessCapabilityText {
                file_name: format!("{}.capability.json", candidate.artifact_stem),
                json: serde_json::to_string_pretty(&capability)?,
            })
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;

    let fixture_review = fixture_gate
        .as_ref()
        .map(|gate| {
            let review = fixture_review_packet(gate);
            serde_json::to_string_pretty(&review).map(|json| ReadinessFixtureReviewText {
                file_name: "fixture-review.local.json".to_string(),
                json,
            })
        })
        .transpose()?;

    let loader_preflight = fixture_gate
        .as_ref()
        .map(|gate| {
            let preflight =
                loader_preflight_draft(gate, &loader_gate, fixture_gate_summary.clone());
            serde_json::to_string_pretty(&preflight).map(|json| ReadinessPreflightText {
                file_name: "loader-preflight.local.json".to_string(),
                json,
            })
        })
        .transpose()?;

    Ok(ReadinessOutputText {
        allowlist_json: serde_json::to_string_pretty(&allowlist)?,
        readiness_json: serde_json::to_string_pretty(&readiness)?,
        loader_gate_json: serde_json::to_string_pretty(&loader_gate)?,
        requests,
        capabilities,
        fixture_review,
        loader_preflight,
    })
}

fn fixture_review_packet(fixture_gate: &FixtureReviewGate) -> FixtureReviewPacket {
    let selected_candidate_count = fixture_gate
        .selected_fixture
        .as_deref()
        .map(|selected| {
            fixture_gate
                .candidates
                .iter()
                .filter(|candidate| candidate.id == selected)
                .count()
        })
        .unwrap_or(0);
    let status = match (&fixture_gate.selected_fixture, selected_candidate_count) {
        (None, _) => "manual_selection_required",
        (Some(_), 1) => "selected_pending_separate_loader_gate",
        (Some(_), _) => "blocked_selected_fixture_not_in_candidates",
    };
    FixtureReviewPacket {
        schema_version: 1,
        publication_status: "local-only".to_string(),
        status: status.to_string(),
        metadata_mode: "path-size-static-evidence-only-no-hash-no-binary-payload".to_string(),
        selected_fixture: fixture_gate.selected_fixture.clone(),
        recommended_first_review: fixture_gate.recommended_first_review.clone(),
        recommendation_status: fixture_gate.recommendation_status.clone(),
        candidate_count: fixture_gate.candidates.len(),
        selected_candidate_count,
        auto_approval_granted: false,
        queue: fixture_gate
            .candidates
            .iter()
            .map(|candidate| {
                let selected =
                    fixture_gate.selected_fixture.as_deref() == Some(candidate.id.as_str());
                FixtureReviewPacketCandidate {
                    id: candidate.id.clone(),
                    display_name: candidate.display_name.clone(),
                    plugin_path: candidate.path.clone(),
                    source_tree: candidate.source_tree.clone(),
                    observed_size_bytes: candidate.observed_size_bytes,
                    fixture_status: candidate.fixture_status.clone(),
                    classifier_status: candidate.classifier_status.clone(),
                    plugin_class: candidate.plugin_class.clone(),
                    review_priority: candidate.review_priority,
                    review_status: candidate.review_status.clone(),
                    recommendation: if fixture_gate.recommended_first_review.as_deref()
                        == Some(candidate.id.as_str())
                    {
                        "recommended_first_review"
                    } else {
                        "queued"
                    }
                    .to_string(),
                    selection_status: if selected {
                        "selected_but_not_approved"
                    } else {
                        "not_selected"
                    }
                    .to_string(),
                    manual_selection_required: !selected,
                    blocked_reasons: candidate.blocked_reasons.clone(),
                }
            })
            .collect(),
        required_manual_decisions: vec![
            "choose zero or one local-only fixture for the first native loader slice".to_string(),
            "record source/license review before enabling loader approval".to_string(),
            "open loader and render_png approval only in a separate explicit slice".to_string(),
        ],
        forbidden_actions: vec![
            "open .aex binary".to_string(),
            "hash .aex binary".to_string(),
            "copy .aex binary into fixtures or public artifacts".to_string(),
            "load .aex in broker or OFX host".to_string(),
            "call native EffectMain or render selectors".to_string(),
        ],
        notes: vec![
            "This review packet is metadata-only and cannot approve loading by itself.".to_string(),
            "A recommendation is queue order only; selected_fixture must remain an explicit review decision.".to_string(),
            "A passing loader preflight is still no-load and only permits a later loader implementation slice.".to_string(),
        ],
    }
}

fn loader_preflight_draft(
    fixture_gate: &FixtureReviewGate,
    loader_gate: &LoaderGateReport,
    fixture_gate_summary: Option<FixtureReviewGateSummary>,
) -> ReadinessLoaderPreflightDraft {
    let mut blocked_reasons = Vec::new();
    if fixture_gate.selected_fixture.is_none() {
        blocked_reasons.push("fixture_review_gate.selected_fixture is null".to_string());
    }
    blocked_reasons.push("fixture review gate approval flags are not all enabled".to_string());
    if !(loader_gate.approved
        && loader_gate.loader_enabled
        && loader_gate.real_aex_load_enabled
        && loader_gate.open_candidate_count == 1)
    {
        blocked_reasons
            .push("readiness loader gate is not open for exactly one candidate".to_string());
    }
    let status = if fixture_gate.selected_fixture.is_none() {
        "blocked_no_selected_fixture"
    } else {
        "blocked_loader_gate_closed"
    };
    ReadinessLoaderPreflightDraft {
        schema_version: 1,
        status: status.to_string(),
        preflight_passed: false,
        native_load_performed: false,
        broker_may_load_plugin: false,
        selected_fixture: fixture_gate.selected_fixture.clone(),
        fixture_review_gate: fixture_gate_summary,
        loader_gate_status: loader_gate.status.clone(),
        loader_gate_approved: loader_gate.approved,
        loader_enabled: loader_gate.loader_enabled,
        real_aex_load_enabled: loader_gate.real_aex_load_enabled,
        open_candidate_count: loader_gate.open_candidate_count,
        blocked_reasons,
        notes: vec![
            "Readiness preflight draft only; generated from metadata without loading .aex."
                .to_string(),
            "Run aex_loader_preflight before any separate native loader implementation slice."
                .to_string(),
        ],
    }
}

fn parse_fixture_review_gate(input: &str) -> Result<FixtureReviewGate, serde_json::Error> {
    let gate: FixtureReviewGateInput = serde_json::from_str(input)?;
    let candidates = gate
        .candidates
        .into_iter()
        .map(|candidate| FixtureReviewCandidateMetadata {
            id: candidate.id,
            path: normalize_plugin_path(&candidate.path),
            display_name: candidate.display_name,
            source_tree: candidate.source_tree,
            observed_size_bytes: candidate.observed_size_bytes,
            fixture_status: candidate.fixture_status,
            classifier_status: candidate.classifier_status,
            plugin_class: candidate.plugin_class,
            review_priority: candidate.review_priority,
            review_status: candidate.review_status,
            blocked_reasons: candidate.blocked_reasons,
        })
        .collect::<Vec<_>>();
    Ok(FixtureReviewGate {
        status: gate.status,
        selected_fixture: gate.selected_fixture,
        recommended_first_review: gate.recommended_first_review,
        recommendation_status: gate.recommendation_status,
        candidate_ids: candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect(),
        candidate_paths: candidates
            .iter()
            .map(|candidate| normalize_path_key(&candidate.path))
            .collect(),
        candidates,
    })
}

fn fixture_review_gate_summary(gate: &FixtureReviewGate) -> FixtureReviewGateSummary {
    FixtureReviewGateSummary {
        status: gate.status.clone(),
        selected_fixture: gate.selected_fixture.clone(),
        recommended_first_review: gate.recommended_first_review.clone(),
        recommendation_status: gate.recommendation_status.clone(),
        allowed_candidate_count: gate.candidate_paths.len(),
        candidate_ids: gate.candidate_ids.clone(),
        effect: "filter-describe-draft-to-review-queue".to_string(),
    }
}

fn collect_candidates(input: FlexibleInput) -> Vec<Candidate> {
    if !input.effects.is_empty() {
        return input
            .effects
            .into_iter()
            .filter_map(candidate_from_catalog_effect)
            .collect();
    }
    input
        .aex_candidates
        .into_iter()
        .map(candidate_from_inventory)
        .collect()
}

fn candidate_from_catalog_effect(effect: CatalogEffect) -> Option<Candidate> {
    let path = normalize_plugin_path(&effect.path.or_else(|| {
        effect
            .source
            .as_ref()
            .and_then(|source| source.path.clone())
    })?);
    let (pipl_content_scan_status, pipl_content_scan_contents_emitted, pipl_scan_ready) =
        pipl_content_scan_readiness(effect.pipl_content_scan.as_ref());
    let display_name = effect
        .identity
        .as_ref()
        .and_then(|identity| identity.display_name.clone())
        .or_else(|| effect.pipl_name.clone())
        .unwrap_or_else(|| file_stem(&path));
    let match_name = effect
        .identity
        .as_ref()
        .and_then(|identity| identity.match_name.clone())
        .or_else(|| effect.pipl_match_name.clone());
    let artifact_stem = normalized_artifact_stem(match_name.as_deref(), &display_name);
    Some(Candidate {
        effect_id: normalized_effect_id(effect.effect_id.as_deref(), &path),
        path,
        artifact_stem,
        size_bytes: effect.size,
        fixture_status: effect
            .fixture_status
            .or_else(|| {
                effect
                    .source
                    .as_ref()
                    .and_then(|source| source.origin.clone())
            })
            .unwrap_or_else(|| "unknown".to_string()),
        plugin_class: effect
            .classification
            .as_ref()
            .and_then(|classification| classification.plugin_class.clone())
            .or(effect.plugin_class)
            .unwrap_or_else(|| "unknown".to_string()),
        classifier_status: effect.status.unwrap_or_else(|| "unknown".to_string()),
        display_name,
        pipl_content_scan_status,
        pipl_content_scan_contents_emitted,
        pipl_scan_ready_for_readiness: pipl_scan_ready,
    })
}

fn candidate_from_inventory(candidate: InventoryCandidate) -> Candidate {
    let path = normalize_plugin_path(&candidate.path);
    let stem = file_stem(&path);
    let status = if matches!(
        stem.to_ascii_lowercase().as_str(),
        "adaptivefilter" | "medianpro"
    ) {
        "candidate_for_contract_probe"
    } else if candidate.inferred_class == "likely-classic-effect" {
        "classified_from_inventory"
    } else {
        "blocked_or_deferred"
    };
    let plugin_class = if candidate.inferred_class == "likely-classic-effect" {
        "classic-effect-candidate"
    } else if candidate.inferred_class == "aegp" {
        "aegp"
    } else if candidate.inferred_class == "heavy-or-specialized-effect" {
        "blocked"
    } else {
        "unknown"
    };
    Candidate {
        effect_id: normalized_effect_id(None, &path),
        path,
        artifact_stem: normalized_artifact_stem(None, &stem),
        size_bytes: candidate.bytes,
        fixture_status: candidate.fixture_status,
        plugin_class: plugin_class.to_string(),
        classifier_status: status.to_string(),
        display_name: stem,
        pipl_content_scan_status: Some("not-applicable-inventory-input".to_string()),
        pipl_content_scan_contents_emitted: Some(false),
        pipl_scan_ready_for_readiness: true,
    }
}

fn pipl_content_scan_readiness(
    scan: Option<&CatalogPiplContentScan>,
) -> (Option<String>, Option<bool>, bool) {
    let status = scan.and_then(|scan| scan.status.clone());
    let contents_emitted = scan.and_then(|scan| scan.contents_emitted);
    let ready = status.as_deref() == Some("semantic_matches") && contents_emitted == Some(false);
    (status, contents_emitted, ready)
}

fn is_probe_candidate(
    candidate: &Candidate,
    fixture_gate: Option<&FixtureReviewGate>,
    artifact_stem_counts: &HashMap<String, usize>,
) -> bool {
    candidate.classifier_status == "candidate_for_contract_probe"
        && candidate.plugin_class == "classic-effect-candidate"
        && candidate.fixture_status == "local-build-candidate"
        && candidate.pipl_scan_ready_for_readiness
        && !has_duplicate_artifact_stem(candidate, artifact_stem_counts)
        && fixture_gate
            .map(|gate| {
                gate.candidate_paths
                    .contains(&normalize_path_key(&candidate.path))
            })
            .unwrap_or(true)
}

fn blocked_reason(
    candidate: &Candidate,
    fixture_gate: Option<&FixtureReviewGate>,
    artifact_stem_counts: &HashMap<String, usize>,
) -> String {
    if has_duplicate_artifact_stem(candidate, artifact_stem_counts) {
        return format!(
            "artifact_stem {} is duplicated; generated manifest/request output is blocked",
            candidate.artifact_stem
        );
    }
    if candidate.fixture_status != "local-build-candidate" {
        return format!("fixture_status is {}", candidate.fixture_status);
    }
    if candidate.classifier_status != "candidate_for_contract_probe" {
        return format!("classifier_status is {}", candidate.classifier_status);
    }
    if candidate.plugin_class != "classic-effect-candidate" {
        return format!("classifier_inferred_class is {}", candidate.plugin_class);
    }
    if !candidate.pipl_scan_ready_for_readiness {
        return pipl_content_scan_blocked_reason(candidate);
    }
    if fixture_gate
        .map(|gate| {
            !gate
                .candidate_paths
                .contains(&normalize_path_key(&candidate.path))
        })
        .unwrap_or(false)
    {
        return "not present in fixture review gate candidate queue".to_string();
    }
    format!("classifier_inferred_class is {}", candidate.plugin_class)
}

fn pipl_content_scan_blocked_reason(candidate: &Candidate) -> String {
    let status = candidate
        .pipl_content_scan_status
        .as_deref()
        .unwrap_or("missing");
    let contents_emitted = candidate
        .pipl_content_scan_contents_emitted
        .map(|contents_emitted| contents_emitted.to_string())
        .unwrap_or_else(|| "missing".to_string());
    format!(
        "pipl_content_scan.status is {status}, contents_emitted is {contents_emitted}; requires status semantic_matches and contents_emitted false to pass contract-probe readiness"
    )
}

fn artifact_stem_counts(candidates: &[Candidate]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for candidate in candidates {
        *counts.entry(candidate.artifact_stem.clone()).or_insert(0) += 1;
    }
    counts
}

fn has_duplicate_artifact_stem(
    candidate: &Candidate,
    artifact_stem_counts: &HashMap<String, usize>,
) -> bool {
    artifact_stem_counts
        .get(&candidate.artifact_stem)
        .copied()
        .unwrap_or(0)
        > 1
}

fn loader_gate_blockers(candidate: &Candidate) -> Vec<String> {
    let mut blockers = vec![
        "manual fixture approval has not been granted".to_string(),
        "loader_approval_status remains not-approved".to_string(),
        "allowlist stays describe-only; render_png is not approved".to_string(),
        "sandbox_preflight.status has not been accepted for native loading".to_string(),
        "sandbox_preflight.job_object_status must be assigned with kill_on_job_close".to_string(),
        "sandbox_preflight.handle_inheritance_status must be sentinel_not_inherited".to_string(),
        "sandbox_preflight.handle_inheritance_disabled must be true".to_string(),
        "worker_identity_revalidation.status has not been accepted for native loading".to_string(),
        "worker_attestation.status has not been accepted for native loading".to_string(),
    ];
    match candidate.size_bytes {
        Some(0) => blockers.push("declared_size_bytes is zero".to_string()),
        Some(size) if size > DEFAULT_MAX_PLUGIN_BYTES => blockers.push(format!(
            "declared_size_bytes {size} exceeds max_plugin_bytes {DEFAULT_MAX_PLUGIN_BYTES}"
        )),
        None => blockers.push("declared_size_bytes is missing".to_string()),
        _ => {}
    }
    blockers
}

fn loader_gate_requirements() -> Vec<String> {
    vec![
        "manual local-only fixture approval for exactly one classic CPU effect".to_string(),
        "license and publication review recorded as local-only-reviewed".to_string(),
        "allowlist loader_approval_status set to approved-local-only by an explicit later slice"
            .to_string(),
        "allowlist operation expanded from describe to render_png by an explicit later slice"
            .to_string(),
        "sandbox_preflight.status=passed and worker_attestation.status=passed accepted as runtime evidence".to_string(),
        "sandbox_preflight.job_object_status=assigned and kill_on_job_close=true accepted as runtime evidence".to_string(),
        "sandbox_preflight.handle_inheritance_status=sentinel_not_inherited and handle_inheritance_disabled=true accepted as runtime evidence".to_string(),
        "Windows worker launcher uses an explicit inherited-handle list; any sentinel_inherited report blocks approval".to_string(),
        "worker_identity_revalidation.status=passed accepted as runtime evidence".to_string(),
        "decision recorded on whether further suspended-start or handle-table enumeration proof is required before native loading".to_string(),
        "OFX facade, if added, uses the same broker/worker/sandbox protocol and does not bypass this gate".to_string(),
    ]
}

fn selector_not_run_statuses() -> Vec<CapabilitySelectorStatus> {
    [
        "load",
        "global_setup",
        "params_setup",
        "sequence_setup",
        "render",
        "sequence_teardown",
        "global_teardown",
    ]
    .into_iter()
    .map(|name| CapabilitySelectorStatus {
        name: name.to_string(),
        status: "not_run".to_string(),
    })
    .collect()
}

fn normalized_artifact_stem(match_name: Option<&str>, display_name: &str) -> String {
    let stem = match_name
        .map(slug_text)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| slug_text(display_name));
    if stem.is_empty() {
        "unknown-local".to_string()
    } else {
        stem
    }
}

fn slug_text(text: &str) -> String {
    let mut safe = String::new();
    let mut previous_dash = false;
    for ch in text.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            safe.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            safe.push('-');
            previous_dash = true;
        }
    }
    safe.trim_matches('-').to_string()
}

fn file_stem(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .split('.')
        .next()
        .unwrap_or(path)
        .to_string()
}

fn normalized_effect_id(effect_id: Option<&str>, path: &str) -> String {
    let raw = effect_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| file_stem(path).to_ascii_lowercase());
    let id = slug_text(&raw.to_ascii_lowercase());
    if id.is_empty() {
        "unknown-local".to_string()
    } else if id.ends_with("-local") {
        id
    } else {
        format!("{id}-local")
    }
}

fn normalize_plugin_path(path: &str) -> String {
    path.trim().replace('/', "\\")
}

fn normalize_path_key(path: &str) -> String {
    normalize_plugin_path(path).to_ascii_lowercase()
}

fn parse_args() -> Result<(PathBuf, PathBuf, Option<PathBuf>), String> {
    let mut input = None;
    let mut out = PathBuf::from("target").join("aex-probe-readiness");
    let mut fixture_gate = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                input = args.next().map(PathBuf::from);
            }
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a directory".to_string())?;
            }
            "--fixture-gate" => {
                fixture_gate = args.next().map(PathBuf::from);
                if fixture_gate.is_none() {
                    return Err("--fixture-gate requires a JSON path".to_string());
                }
            }
            "--help" | "-h" => {
                return Err("usage: aex_probe_readiness --input catalog-or-inventory.json [--out target/aex-probe-readiness] [--fixture-gate analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok((
        input.ok_or_else(|| "--input <catalog-or-inventory.json> is required".to_string())?,
        out,
        fixture_gate,
    ))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (input_path, out_dir, fixture_gate_path) = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let input = std::fs::read_to_string(&input_path)?;
    let fixture_gate = fixture_gate_path
        .as_ref()
        .map(std::fs::read_to_string)
        .transpose()?;
    let output = plan_probe_readiness_json_with_fixture_gate(&input, fixture_gate.as_deref())?;
    write_text(
        &out_dir.join("allowlist.local.draft.json"),
        &output.allowlist_json,
    )?;
    write_text(
        &out_dir.join("readiness.local.json"),
        &output.readiness_json,
    )?;
    write_text(
        &out_dir.join("loader-gate.local.json"),
        &output.loader_gate_json,
    )?;
    let request_dir = out_dir.join("requests");
    for request in output.requests {
        write_text(&request_dir.join(request.file_name), &request.json)?;
    }
    let capability_dir = out_dir.join("capabilities");
    for capability in output.capabilities {
        write_text(&capability_dir.join(capability.file_name), &capability.json)?;
    }
    if let Some(review) = output.fixture_review {
        write_text(&out_dir.join(review.file_name), &review.json)?;
    }
    if let Some(preflight) = output.loader_preflight {
        write_text(&out_dir.join(preflight.file_name), &preflight.json)?;
    }
    println!("{}", out_dir.display());
    Ok(())
}

fn write_text(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}
