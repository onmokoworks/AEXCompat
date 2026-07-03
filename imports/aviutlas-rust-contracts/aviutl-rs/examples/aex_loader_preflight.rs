//! Fail-closed preflight before any real AEX loader slice.
//!
//! This reads only local JSON artifacts from the fixture review gate and
//! readiness loader gate. It does not open, hash, load, execute, describe, or
//! render `.aex` binaries.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct FixtureReviewGate {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    status: String,
    #[serde(default)]
    selected_fixture: Option<String>,
    #[serde(default)]
    recommended_first_review: Option<String>,
    #[serde(default)]
    recommendation_status: Option<String>,
    #[serde(default)]
    approval: FixtureApproval,
    #[serde(default)]
    candidates: Vec<FixtureCandidate>,
}

#[derive(Debug, Default, Deserialize)]
struct FixtureApproval {
    #[serde(default)]
    approved: bool,
    #[serde(default)]
    loader_enabled: bool,
    #[serde(default)]
    real_aex_load_enabled: bool,
    #[serde(default)]
    render_png_enabled: bool,
    #[serde(default)]
    describe_enabled_for_real_aex: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct FixtureCandidate {
    id: String,
    path: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    review_status: String,
    #[serde(default)]
    fixture_status: String,
    #[serde(default)]
    plugin_class: String,
}

#[derive(Debug, Deserialize)]
struct ReadinessLoaderGate {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    status: String,
    #[serde(default)]
    approved: bool,
    #[serde(default)]
    loader_enabled: bool,
    #[serde(default)]
    real_aex_load_enabled: bool,
    #[serde(default)]
    open_candidate_count: u32,
    #[serde(default)]
    entries: Vec<LoaderGateEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct LoaderGateEntry {
    effect_id: String,
    plugin_path: String,
    #[serde(default)]
    pre_loader_status: String,
    #[serde(default)]
    loader_approval_status: String,
    #[serde(default)]
    allowlist_operation_status: String,
    #[serde(default)]
    handle_inheritance_required: String,
    #[serde(default)]
    worker_identity_revalidation_required: String,
    #[serde(default)]
    worker_attestation_required: String,
    #[serde(default)]
    sandbox_preflight_required: String,
    #[serde(default)]
    job_object_required: String,
}

#[derive(Debug, Serialize)]
struct LoaderPreflightReport {
    schema_version: u32,
    publication_status: String,
    status: String,
    preflight_passed: bool,
    native_load_performed: bool,
    broker_may_load_plugin: bool,
    selected_fixture: Option<String>,
    selected_candidate: Option<SelectedCandidateReport>,
    selected_loader_entry: Option<SelectedLoaderEntryReport>,
    fixture_gate: FixtureGateReport,
    loader_gate: LoaderGateReport,
    fixture_refresh_audit_summary: FixtureRefreshAuditSummary,
    checks: Vec<PreflightCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SelectedCandidateReport {
    id: String,
    display_name: Option<String>,
    plugin_path: String,
    normalized_plugin_path: String,
    loader_gate_plugin_path: Option<String>,
    loader_gate_effect_id: Option<String>,
    path_match_status: String,
}

#[derive(Debug, Serialize)]
struct SelectedLoaderEntryReport {
    effect_id: String,
    plugin_path: String,
    normalized_plugin_path: String,
    path_match_status: String,
    pre_loader_status: String,
    loader_approval_status: String,
    allowlist_operation_status: String,
    handle_inheritance_required: String,
    worker_identity_revalidation_required: String,
    worker_attestation_required: String,
    sandbox_preflight_required: String,
    job_object_required: String,
    entry_ready: bool,
}

#[derive(Debug, Serialize)]
struct FixtureGateReport {
    status: String,
    selected_fixture: Option<String>,
    recommended_first_review: Option<String>,
    recommendation_status: Option<String>,
    approval_approved: bool,
    approval_loader_enabled: bool,
    approval_real_aex_load_enabled: bool,
    approval_render_png_enabled: bool,
    approval_describe_enabled_for_real_aex: bool,
    candidate_count: usize,
}

#[derive(Debug, Serialize)]
struct LoaderGateReport {
    status: String,
    approved: bool,
    loader_enabled: bool,
    real_aex_load_enabled: bool,
    open_candidate_count: u32,
    entry_count: usize,
}

#[derive(Debug)]
struct FixtureRefreshAuditInput {
    value: Value,
    input_contains_forbidden_tokens: bool,
}

#[derive(Debug, Serialize)]
struct FixtureRefreshAuditSummary {
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

#[derive(Debug, Serialize)]
struct PreflightCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn plan_loader_preflight_json(
    fixture_gate_json: &str,
    loader_gate_json: &str,
) -> Result<String, Box<dyn Error>> {
    plan_loader_preflight_json_with_fixture_refresh_audit(fixture_gate_json, loader_gate_json, None)
}

pub fn plan_loader_preflight_json_with_fixture_refresh_audit(
    fixture_gate_json: &str,
    loader_gate_json: &str,
    fixture_refresh_audit_json: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let fixture_gate: FixtureReviewGate = serde_json::from_str(fixture_gate_json)?;
    let loader_gate: ReadinessLoaderGate = serde_json::from_str(loader_gate_json)?;
    let fixture_refresh_audit = fixture_refresh_audit_json
        .map(|text| {
            let value: Value = serde_json::from_str(text)?;
            Ok::<FixtureRefreshAuditInput, serde_json::Error>(FixtureRefreshAuditInput {
                value,
                input_contains_forbidden_tokens: input_contains_forbidden_evidence(text),
            })
        })
        .transpose()?;
    let report = plan_loader_preflight(fixture_gate, loader_gate, fixture_refresh_audit.as_ref());
    Ok(serde_json::to_string_pretty(&report)?)
}

fn plan_loader_preflight(
    fixture_gate: FixtureReviewGate,
    loader_gate: ReadinessLoaderGate,
    fixture_refresh_audit: Option<&FixtureRefreshAuditInput>,
) -> LoaderPreflightReport {
    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();

    push_check(
        &mut checks,
        "fixture_gate_schema_version",
        fixture_gate.schema_version == 1,
        format!("schema_version={}", fixture_gate.schema_version),
    );
    if fixture_gate.schema_version != 1 {
        blocked_reasons.push("fixture review gate schema_version must be 1".to_string());
    }

    push_check(
        &mut checks,
        "loader_gate_schema_version",
        loader_gate.schema_version == 1,
        format!("schema_version={}", loader_gate.schema_version),
    );
    if loader_gate.schema_version != 1 {
        blocked_reasons.push("readiness loader gate schema_version must be 1".to_string());
    }

    let fixture_candidates_unique = candidates_are_unique(&fixture_gate.candidates);
    push_check(
        &mut checks,
        "fixture_gate_unique_candidates",
        fixture_candidates_unique,
        format!("candidate_count={}", fixture_gate.candidates.len()),
    );
    if !fixture_candidates_unique {
        blocked_reasons
            .push("fixture review gate has duplicate candidate ids or paths".to_string());
    }

    let loader_entries_unique = loader_entries_are_unique(&loader_gate.entries);
    push_check(
        &mut checks,
        "loader_gate_unique_entries",
        loader_entries_unique,
        format!("entry_count={}", loader_gate.entries.len()),
    );
    if !loader_entries_unique {
        blocked_reasons.push("readiness loader gate has duplicate effect ids or paths".to_string());
    }

    let fixture_refresh_audit_summary = fixture_refresh_audit_summary(fixture_refresh_audit);
    if fixture_refresh_audit.is_some() {
        let refresh_audit_ready =
            fixture_refresh_audit_ready_no_load(&fixture_refresh_audit_summary);
        push_check(
            &mut checks,
            "fixture_gate_refresh_audit_ready_no_load",
            refresh_audit_ready,
            format!(
                "provided={}, status={}, canonical_non_generated_count={}, generated_target_artifact_count={}, candidates_present_in_refresh={}",
                fixture_refresh_audit_summary.provided,
                display_optional(&fixture_refresh_audit_summary.status),
                fixture_refresh_audit_summary.wiztree_canonical_non_generated_count,
                fixture_refresh_audit_summary.wiztree_generated_target_artifact_count,
                fixture_refresh_audit_summary.candidates_present_in_refresh
            ),
        );
        if !refresh_audit_ready {
            blocked_reasons.push(
                "fixture gate refresh audit is not a ready no-load queue-hygiene receipt"
                    .to_string(),
            );
        }
    }

    let selected_candidate = fixture_gate
        .selected_fixture
        .as_deref()
        .and_then(|selected| {
            fixture_gate
                .candidates
                .iter()
                .find(|candidate| candidate.id == selected)
        })
        .cloned();
    push_check(
        &mut checks,
        "selected_fixture",
        selected_candidate.is_some(),
        match &fixture_gate.selected_fixture {
            Some(selected) => format!("selected_fixture={selected}"),
            None => "selected_fixture is null".to_string(),
        },
    );
    if fixture_gate.selected_fixture.is_none() {
        blocked_reasons.push("fixture_review_gate.selected_fixture is null".to_string());
    } else if selected_candidate.is_none() {
        blocked_reasons
            .push("fixture_review_gate.selected_fixture is not present in candidates".to_string());
    }

    if let Some(candidate) = selected_candidate.as_ref() {
        let metadata_ready = candidate.review_status == "approved-local-only"
            && candidate.fixture_status == "local-build-candidate"
            && candidate.plugin_class == "classic-effect-candidate";
        push_check(
            &mut checks,
            "selected_fixture_metadata",
            metadata_ready,
            format!(
                "review_status={}, fixture_status={}, plugin_class={}",
                candidate.review_status, candidate.fixture_status, candidate.plugin_class
            ),
        );
        if !metadata_ready {
            blocked_reasons.push(
                "selected fixture metadata is not approved-local-only classic local-build evidence"
                    .to_string(),
            );
        }
    }

    push_check(
        &mut checks,
        "fixture_gate_approval",
        fixture_approval_is_open(&fixture_gate.approval),
        format!(
            "approved={}, loader_enabled={}, real_aex_load_enabled={}, render_png_enabled={}, describe_enabled_for_real_aex={}",
            fixture_gate.approval.approved,
            fixture_gate.approval.loader_enabled,
            fixture_gate.approval.real_aex_load_enabled,
            fixture_gate.approval.render_png_enabled,
            fixture_gate.approval.describe_enabled_for_real_aex
        ),
    );
    if !fixture_approval_is_open(&fixture_gate.approval) {
        blocked_reasons.push("fixture review gate approval flags are not all enabled".to_string());
    }

    let matched_entry = selected_candidate
        .as_ref()
        .and_then(|candidate| find_loader_entry_by_path(&loader_gate.entries, &candidate.path));
    let ready_entries: Vec<_> = loader_gate
        .entries
        .iter()
        .filter(|entry| loader_entry_is_ready(entry))
        .collect();
    let selected_matches_unique_ready_entry = match (matched_entry, ready_entries.as_slice()) {
        (Some(selected), [ready]) => same_loader_entry_path(selected, ready),
        _ => false,
    };
    push_check(
        &mut checks,
        "loader_gate_single_ready_entry",
        ready_entries.len() == 1 && selected_matches_unique_ready_entry,
        format!(
            "ready_entry_count={}, selected_matches_unique_ready_entry={}",
            ready_entries.len(),
            selected_matches_unique_ready_entry
        ),
    );
    if ready_entries.len() != 1 {
        blocked_reasons.push(
            "readiness loader gate must contain exactly one entry-level approved render_png candidate"
                .to_string(),
        );
    } else if matched_entry.is_some() && !selected_matches_unique_ready_entry {
        blocked_reasons.push(
            "selected fixture does not match the unique entry-level approved loader candidate"
                .to_string(),
        );
    }
    push_check(
        &mut checks,
        "selected_fixture_in_loader_gate",
        matched_entry.is_some(),
        selected_candidate
            .as_ref()
            .map(|candidate| format!("plugin_path={}", candidate.path))
            .unwrap_or_else(|| "no selected fixture path to match".to_string()),
    );
    if selected_candidate.is_some() && matched_entry.is_none() {
        blocked_reasons.push("selected fixture is not present in loader gate entries".to_string());
    }

    push_check(
        &mut checks,
        "loader_gate_open",
        loader_gate.approved
            && loader_gate.loader_enabled
            && loader_gate.real_aex_load_enabled
            && loader_gate.open_candidate_count == 1,
        format!(
            "approved={}, loader_enabled={}, real_aex_load_enabled={}, open_candidate_count={}",
            loader_gate.approved,
            loader_gate.loader_enabled,
            loader_gate.real_aex_load_enabled,
            loader_gate.open_candidate_count
        ),
    );
    if !(loader_gate.approved
        && loader_gate.loader_enabled
        && loader_gate.real_aex_load_enabled
        && loader_gate.open_candidate_count == 1)
    {
        blocked_reasons
            .push("readiness loader gate is not open for exactly one candidate".to_string());
    }

    if let Some(entry) = matched_entry {
        let entry_ready = loader_entry_is_ready(entry);
        push_check(
            &mut checks,
            "selected_loader_entry",
            entry_ready,
            format!(
                "effect_id={}, pre_loader_status={}, loader_approval_status={}, operation={}",
                entry.effect_id,
                entry.pre_loader_status,
                entry.loader_approval_status,
                entry.allowlist_operation_status
            ),
        );
        if !entry_ready {
            blocked_reasons.push(
                "selected loader gate entry is not approved for render_png with required sandbox evidence"
                    .to_string(),
            );
        }
    }

    let preflight_passed = blocked_reasons.is_empty();
    let status = if preflight_passed {
        "preflight_passed_no_load"
    } else if fixture_gate.selected_fixture.is_none() {
        "blocked_no_selected_fixture"
    } else if selected_candidate.is_none() {
        "blocked_selected_fixture_not_in_gate"
    } else if !fixture_approval_is_open(&fixture_gate.approval) {
        "blocked_fixture_not_approved"
    } else {
        "blocked_loader_gate_closed"
    };

    let selected_candidate_report = selected_candidate.map(|candidate| {
        let normalized_plugin_path = normalize_path_key(&candidate.path);
        let loader_gate_plugin_path = matched_entry.map(|entry| entry.plugin_path.clone());
        let loader_gate_effect_id = matched_entry.map(|entry| entry.effect_id.clone());
        let path_match_status = if matched_entry.is_some() {
            "matched_normalized_path"
        } else {
            "missing_loader_gate_entry"
        };

        SelectedCandidateReport {
            id: candidate.id,
            display_name: candidate.display_name,
            plugin_path: candidate.path,
            normalized_plugin_path,
            loader_gate_plugin_path,
            loader_gate_effect_id,
            path_match_status: path_match_status.to_string(),
        }
    });
    let selected_loader_entry_report = matched_entry.map(|entry| SelectedLoaderEntryReport {
        effect_id: entry.effect_id.clone(),
        plugin_path: entry.plugin_path.clone(),
        normalized_plugin_path: normalize_path_key(&entry.plugin_path),
        path_match_status: "matched_normalized_path".to_string(),
        pre_loader_status: entry.pre_loader_status.clone(),
        loader_approval_status: entry.loader_approval_status.clone(),
        allowlist_operation_status: entry.allowlist_operation_status.clone(),
        handle_inheritance_required: entry.handle_inheritance_required.clone(),
        worker_identity_revalidation_required: entry.worker_identity_revalidation_required.clone(),
        worker_attestation_required: entry.worker_attestation_required.clone(),
        sandbox_preflight_required: entry.sandbox_preflight_required.clone(),
        job_object_required: entry.job_object_required.clone(),
        entry_ready: loader_entry_is_ready(entry),
    });

    LoaderPreflightReport {
        schema_version: 1,
        publication_status: "local-only".to_string(),
        status: status.to_string(),
        preflight_passed,
        native_load_performed: false,
        broker_may_load_plugin: false,
        selected_fixture: fixture_gate.selected_fixture.clone(),
        selected_candidate: selected_candidate_report,
        selected_loader_entry: selected_loader_entry_report,
        fixture_gate: FixtureGateReport {
            status: fixture_gate.status,
            selected_fixture: fixture_gate.selected_fixture,
            recommended_first_review: fixture_gate.recommended_first_review,
            recommendation_status: fixture_gate.recommendation_status,
            approval_approved: fixture_gate.approval.approved,
            approval_loader_enabled: fixture_gate.approval.loader_enabled,
            approval_real_aex_load_enabled: fixture_gate.approval.real_aex_load_enabled,
            approval_render_png_enabled: fixture_gate.approval.render_png_enabled,
            approval_describe_enabled_for_real_aex: fixture_gate
                .approval
                .describe_enabled_for_real_aex,
            candidate_count: fixture_gate.candidates.len(),
        },
        loader_gate: LoaderGateReport {
            status: loader_gate.status,
            approved: loader_gate.approved,
            loader_enabled: loader_gate.loader_enabled,
            real_aex_load_enabled: loader_gate.real_aex_load_enabled,
            open_candidate_count: loader_gate.open_candidate_count,
            entry_count: loader_gate.entries.len(),
        },
        fixture_refresh_audit_summary,
        checks,
        blocked_reasons,
        next_action: if preflight_passed {
            "Open a separate explicit loader implementation slice; this preflight still performed no native loading."
                .to_string()
        } else {
            "Select exactly one reviewed fixture and open a separate loader gate before attempting native loading."
                .to_string()
        },
        notes: vec![
            "Preflight reads JSON metadata only.".to_string(),
            "No .aex file is opened, hashed, loaded, executed, described, or rendered.".to_string(),
            "A passing preflight is permission to start a separate loader slice, not proof that native loading is implemented.".to_string(),
        ],
    }
}

fn fixture_refresh_audit_summary(
    input: Option<&FixtureRefreshAuditInput>,
) -> FixtureRefreshAuditSummary {
    let Some(input) = input else {
        return FixtureRefreshAuditSummary {
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
    };
    let value = &input.value;
    FixtureRefreshAuditSummary {
        provided: true,
        schema_version: value["schema_version"]
            .as_u64()
            .and_then(|version| u32::try_from(version).ok()),
        publication_status: safe_string(&value["publication_status"]),
        status: safe_string(&value["status"]),
        native_load_performed: value["native_load_performed"].as_bool(),
        render_performed: value["render_performed"].as_bool(),
        fixture_selected: value["fixture_selected"].as_bool(),
        loader_enabled: value["loader_enabled"].as_bool(),
        fixture_gate_candidate_count: value["fixture_gate_summary"]["candidate_count"]
            .as_u64()
            .map_or(0, |count| count as usize),
        wiztree_total_aex_count: value["wiztree_refresh_summary"]["total_aex_count"]
            .as_u64()
            .unwrap_or(0),
        wiztree_canonical_non_generated_count: value["wiztree_refresh_summary"]
            ["canonical_non_generated_count"]
            .as_u64()
            .unwrap_or(0),
        wiztree_generated_target_artifact_count: value["wiztree_refresh_summary"]
            ["generated_target_artifact_count"]
            .as_u64()
            .unwrap_or(0),
        generated_target_artifacts_excluded: value["wiztree_refresh_summary"]
            ["generated_target_artifacts_excluded"]
            .as_bool()
            .unwrap_or(false),
        candidates_present_in_refresh: audit_candidates_ready(value),
        input_contains_forbidden_tokens: input.input_contains_forbidden_tokens
            || contains_forbidden_field_names(value),
        blocked_reason_count: json_array_len(&value["blocked_reasons"]),
    }
}

fn fixture_refresh_audit_ready_no_load(summary: &FixtureRefreshAuditSummary) -> bool {
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

fn audit_candidates_ready(value: &Value) -> bool {
    let Some(candidates) = value["candidate_crosscheck"].as_array() else {
        return false;
    };
    if candidates.len() != 2 {
        return false;
    }
    let required_ids: HashSet<&str> = ["adaptive-filter-local", "median-pro-local"]
        .into_iter()
        .collect();
    let candidate_ids: HashSet<&str> = candidates
        .iter()
        .filter_map(|candidate| candidate["id"].as_str())
        .collect();
    candidate_ids == required_ids
        && candidates.iter().all(|candidate| {
            candidate["path_matches_refresh"].as_bool() == Some(true)
                && candidate["size_matches_refresh"].as_bool() == Some(true)
                && candidate["generated_target_artifact"].as_bool() == Some(false)
                && candidate["gate_review_status"].as_str() == Some("not-approved")
                && candidate["refresh_review_status"].as_str() == Some("queued-not-approved")
        })
}

fn input_contains_forbidden_evidence(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    forbidden_input_tokens()
        .iter()
        .any(|token| text.contains(token))
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
    if text.is_empty() || input_contains_forbidden_evidence(text) {
        return None;
    }
    Some(text.to_string())
}

fn forbidden_input_tokens() -> Vec<String> {
    vec![
        "sha256".to_string(),
        "base64".to_string(),
        "binary_payload".to_string(),
        "payload_bytes".to_string(),
        "copied_asset".to_string(),
        "native_load_result".to_string(),
        ["load", "library"].concat(),
        ["lib", "loading"].concat(),
        "effectmain".to_string(),
        "input_png".to_string(),
        "output_png".to_string(),
        "rendered_pixels".to_string(),
        "pixel_hash".to_string(),
        "\"approved\":true".to_string(),
        "\"loader_enabled\":true".to_string(),
        "\"real_aex_load_enabled\":true".to_string(),
        "\"render_png_enabled\":true".to_string(),
    ]
}

fn forbidden_field_names() -> Vec<&'static str> {
    vec![
        "sha256",
        "binary_payload",
        "base64_payload",
        "payload_bytes",
        "copied_asset",
        "native_load_result",
        "input_png",
        "output_png",
        "rendered_pixels",
        "pixel_hash",
    ]
}

fn display_optional(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("none")
}

fn json_array_len(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn fixture_approval_is_open(approval: &FixtureApproval) -> bool {
    approval.approved
        && approval.loader_enabled
        && approval.real_aex_load_enabled
        && approval.render_png_enabled
        && approval.describe_enabled_for_real_aex
}

fn find_loader_entry_by_path<'a>(
    entries: &'a [LoaderGateEntry],
    plugin_path: &str,
) -> Option<&'a LoaderGateEntry> {
    let key = normalize_path_key(plugin_path);
    entries
        .iter()
        .find(|entry| normalize_path_key(&entry.plugin_path) == key)
}

fn same_loader_entry_path(left: &LoaderGateEntry, right: &LoaderGateEntry) -> bool {
    normalize_path_key(&left.plugin_path) == normalize_path_key(&right.plugin_path)
}

fn loader_entry_is_ready(entry: &LoaderGateEntry) -> bool {
    entry.pre_loader_status == "approved-local-only"
        && entry.loader_approval_status == "approved-local-only"
        && entry.allowlist_operation_status == "render_png"
        && entry.handle_inheritance_required == "sentinel_not_inherited-with-explicit-handle-list"
        && entry.worker_identity_revalidation_required == "passed"
        && entry.worker_attestation_required == "passed"
        && entry.sandbox_preflight_required == "passed"
        && entry.job_object_required == "assigned-with-kill-on-close"
}

fn candidates_are_unique(candidates: &[FixtureCandidate]) -> bool {
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    candidates.iter().all(|candidate| {
        ids.insert(candidate.id.as_str()) && paths.insert(normalize_path_key(&candidate.path))
    })
}

fn loader_entries_are_unique(entries: &[LoaderGateEntry]) -> bool {
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    entries.iter().all(|entry| {
        ids.insert(entry.effect_id.as_str()) && paths.insert(normalize_path_key(&entry.plugin_path))
    })
}

fn push_check(checks: &mut Vec<PreflightCheck>, name: &str, passed: bool, evidence: String) {
    checks.push(PreflightCheck {
        name: name.to_string(),
        status: if passed { "passed" } else { "blocked" }.to_string(),
        evidence,
    });
}

fn normalize_path_key(path: &str) -> String {
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

fn parse_args() -> Result<(PathBuf, PathBuf, Option<PathBuf>, PathBuf), String> {
    let mut fixture_gate = None;
    let mut loader_gate = None;
    let mut fixture_refresh_audit = None;
    let mut out = PathBuf::from("target")
        .join("aex-loader-preflight")
        .join("preflight.local.json");
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture-gate" => {
                fixture_gate = args.next().map(PathBuf::from);
                if fixture_gate.is_none() {
                    return Err("--fixture-gate requires a JSON path".to_string());
                }
            }
            "--loader-gate" => {
                loader_gate = args.next().map(PathBuf::from);
                if loader_gate.is_none() {
                    return Err("--loader-gate requires a JSON path".to_string());
                }
            }
            "--fixture-refresh-audit" => {
                fixture_refresh_audit = args.next().map(PathBuf::from);
                if fixture_refresh_audit.is_none() {
                    return Err("--fixture-refresh-audit requires a JSON path".to_string());
                }
            }
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a JSON path".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: aex_loader_preflight --fixture-gate analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json --loader-gate target/aex-probe-readiness/loader-gate.local.json [--fixture-refresh-audit target/aex-fixture-gate-refresh-audit/fixture-gate-refresh.local.json] [--out target/aex-loader-preflight/preflight.local.json]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok((
        fixture_gate.ok_or_else(|| "--fixture-gate is required".to_string())?,
        loader_gate.ok_or_else(|| "--loader-gate is required".to_string())?,
        fixture_refresh_audit,
        out,
    ))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (fixture_gate_path, loader_gate_path, fixture_refresh_audit_path, out_path) =
        match parse_args() {
            Ok(args) => args,
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(2);
            }
        };
    let fixture_gate = std::fs::read_to_string(&fixture_gate_path)?;
    let loader_gate = std::fs::read_to_string(&loader_gate_path)?;
    let fixture_refresh_audit = fixture_refresh_audit_path
        .as_ref()
        .map(std::fs::read_to_string)
        .transpose()?;
    let report = plan_loader_preflight_json_with_fixture_refresh_audit(
        &fixture_gate,
        &loader_gate,
        fixture_refresh_audit.as_deref(),
    )?;
    write_text(&out_path, &report)?;
    println!("{}", out_path.display());
    Ok(())
}

fn write_text(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}
