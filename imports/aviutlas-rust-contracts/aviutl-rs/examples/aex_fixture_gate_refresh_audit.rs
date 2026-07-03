//! Metadata-only audit that joins the AEX fixture review gate with the latest
//! read-only WizTree AEX refresh.
//!
//! This checker does not open, copy, load, execute, describe, or render `.aex`
//! files. It only confirms the first-loader review queue still points at
//! present local-build candidates and that generated target artifacts remain
//! excluded.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct FixtureGateRefreshAudit {
    schema_version: u32,
    publication_status: String,
    status: String,
    native_load_performed: bool,
    render_performed: bool,
    fixture_selected: bool,
    loader_enabled: bool,
    fixture_gate_summary: FixtureGateSummary,
    wiztree_refresh_summary: WizTreeRefreshSummary,
    candidate_crosscheck: Vec<CandidateCrosscheck>,
    checks: Vec<AuditCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FixtureGateSummary {
    status: Option<String>,
    selected_fixture: Option<String>,
    recommended_first_review: Option<String>,
    recommendation_status: Option<String>,
    approval_all_false: bool,
    candidate_count: usize,
    local_build_candidate_count: usize,
    not_approved_candidate_count: usize,
    max_selected_fixtures: Option<u64>,
    manual_user_approval_required: bool,
    no_parallel_first_loader_fixtures: bool,
}

#[derive(Debug, Serialize)]
struct WizTreeRefreshSummary {
    scan_mode: Option<String>,
    total_aex_count: u64,
    canonical_non_generated_count: u64,
    generated_target_artifact_count: u64,
    generated_target_artifacts_excluded: bool,
    fixture_gate_candidate_count: usize,
    fixture_gate_candidates_queued_not_approved: usize,
    additional_later_review_count: usize,
    keep_current_two_candidate_gate: bool,
    do_not_expand_from_generated_targets: bool,
    do_not_select_without_manual_approval: bool,
}

#[derive(Debug, Serialize)]
struct CandidateCrosscheck {
    id: String,
    path: String,
    gate_observed_size_bytes: Option<u64>,
    refresh_bytes: Option<u64>,
    path_matches_refresh: bool,
    size_matches_refresh: bool,
    gate_review_status: Option<String>,
    refresh_review_status: Option<String>,
    generated_target_artifact: bool,
    blocked_reason_count: usize,
}

#[derive(Debug, Serialize)]
struct AuditCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn audit_fixture_gate_refresh_json(
    fixture_gate_json: &str,
    wiztree_refresh_json: &str,
) -> Result<String, Box<dyn Error>> {
    let fixture_gate: Value = serde_json::from_str(fixture_gate_json)?;
    let wiztree_refresh: Value = serde_json::from_str(wiztree_refresh_json)?;
    let audit = audit_fixture_gate_refresh(
        &fixture_gate,
        fixture_gate_json,
        &wiztree_refresh,
        wiztree_refresh_json,
    );
    Ok(serde_json::to_string_pretty(&audit)?)
}

fn audit_fixture_gate_refresh(
    fixture_gate: &Value,
    fixture_gate_text: &str,
    wiztree_refresh: &Value,
    wiztree_refresh_text: &str,
) -> FixtureGateRefreshAudit {
    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();
    let fixture_gate_summary = fixture_gate_summary(fixture_gate);
    let wiztree_refresh_summary = wiztree_refresh_summary(wiztree_refresh);
    let candidate_crosscheck = candidate_crosscheck(fixture_gate, wiztree_refresh);
    let input_contaminated = input_contains_forbidden_evidence(
        fixture_gate,
        fixture_gate_text,
        wiztree_refresh,
        wiztree_refresh_text,
    );

    let gate_closed = fixture_gate_closed_no_selection(&fixture_gate_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "fixture_gate_closed_no_selection",
        gate_closed,
        format!(
            "status={}, selected_fixture={}, approval_all_false={}",
            display_optional(&fixture_gate_summary.status),
            display_optional(&fixture_gate_summary.selected_fixture),
            fixture_gate_summary.approval_all_false
        ),
        "fixture review gate is not a closed unselected review queue",
    );

    let gate_policy_ok = fixture_gate_policy_ok(&fixture_gate_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "fixture_gate_candidate_policy",
        gate_policy_ok,
        format!(
            "candidate_count={}, local_build_candidate_count={}, not_approved_candidate_count={}, max_selected_fixtures={}",
            fixture_gate_summary.candidate_count,
            fixture_gate_summary.local_build_candidate_count,
            fixture_gate_summary.not_approved_candidate_count,
            fixture_gate_summary
                .max_selected_fixtures
                .map(|count| count.to_string())
                .unwrap_or_else(|| "missing".to_string())
        ),
        "fixture review gate candidate policy is not narrow, local-build, and unapproved",
    );

    let refresh_inventory_ok = wiztree_refresh_inventory_ok(&wiztree_refresh_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "wiztree_refresh_non_generated_inventory_match",
        refresh_inventory_ok,
        format!(
            "total_aex_count={}, canonical_non_generated_count={}, generated_target_artifact_count={}",
            wiztree_refresh_summary.total_aex_count,
            wiztree_refresh_summary.canonical_non_generated_count,
            wiztree_refresh_summary.generated_target_artifact_count
        ),
        "WizTree refresh no longer matches the canonical non-generated AEX inventory split",
    );

    let generated_targets_excluded = wiztree_generated_targets_excluded(&wiztree_refresh_summary);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "wiztree_refresh_generated_targets_excluded",
        generated_targets_excluded,
        format!(
            "generated_target_artifacts_excluded={}, do_not_expand_from_generated_targets={}",
            wiztree_refresh_summary.generated_target_artifacts_excluded,
            wiztree_refresh_summary.do_not_expand_from_generated_targets
        ),
        "WizTree refresh does not explicitly exclude generated target artifacts from fixture review",
    );

    let candidates_present = candidates_present_in_refresh(&candidate_crosscheck);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "fixture_gate_candidates_present_in_refresh",
        candidates_present,
        format!(
            "candidate_count={}, matching_candidate_count={}",
            candidate_crosscheck.len(),
            candidate_crosscheck
                .iter()
                .filter(|candidate| candidate.path_matches_refresh && candidate.size_matches_refresh)
                .count()
        ),
        "fixture review gate candidates are missing or size-mismatched in the WizTree refresh",
    );

    let no_generated_candidate = candidate_crosscheck
        .iter()
        .all(|candidate| !candidate.generated_target_artifact);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "fixture_gate_excludes_generated_target_artifacts",
        no_generated_candidate,
        format!(
            "generated_candidate_count={}",
            candidate_crosscheck
                .iter()
                .filter(|candidate| candidate.generated_target_artifact)
                .count()
        ),
        "fixture review gate includes generated target artifacts",
    );

    push_check(
        &mut checks,
        &mut blocked_reasons,
        "evidence_anti_contamination",
        !input_contaminated,
        "fixture gate and WizTree refresh scanned for payload, copied-asset, native-load, and rendered-output evidence".to_string(),
        "fixture gate refresh inputs contain forbidden evidence fields or tokens",
    );

    let ready = blocked_reasons.is_empty();
    FixtureGateRefreshAudit {
        schema_version: 1,
        publication_status: "local-only".to_string(),
        status: if ready {
            "fixture_gate_refresh_ready_no_load".to_string()
        } else {
            "blocked_fixture_gate_refresh".to_string()
        },
        native_load_performed: false,
        render_performed: false,
        fixture_selected: false,
        loader_enabled: false,
        fixture_gate_summary,
        wiztree_refresh_summary,
        candidate_crosscheck,
        checks,
        blocked_reasons,
        next_action: if ready {
            "Keep the current two-candidate review queue closed until explicit manual approval and a separate loader slice exist.".to_string()
        } else {
            "Fix fixture gate or refresh evidence before using the queue for any loader preflight.".to_string()
        },
        notes: vec![
            "Fixture gate refresh audit reads JSON metadata only.".to_string(),
            "No .aex file is opened, copied, loaded, executed, described, or rendered.".to_string(),
            "A ready audit confirms queue hygiene only; it is not fixture selection or loader approval.".to_string(),
            "Generated target artifacts must stay excluded from first-loader fixture review.".to_string(),
        ],
    }
}

fn fixture_gate_summary(gate: &Value) -> FixtureGateSummary {
    let candidates = gate["candidates"].as_array().cloned().unwrap_or_default();
    FixtureGateSummary {
        status: safe_string(&gate["status"]),
        selected_fixture: safe_string(&gate["selected_fixture"]),
        recommended_first_review: safe_string(&gate["recommended_first_review"]),
        recommendation_status: safe_string(&gate["recommendation_status"]),
        approval_all_false: approval_all_false(&gate["approval"]),
        candidate_count: candidates.len(),
        local_build_candidate_count: candidates
            .iter()
            .filter(|candidate| {
                candidate["fixture_status"].as_str() == Some("local-build-candidate")
            })
            .count(),
        not_approved_candidate_count: candidates
            .iter()
            .filter(|candidate| candidate["review_status"].as_str() == Some("not-approved"))
            .count(),
        max_selected_fixtures: gate["single_fixture_policy"]["max_selected_fixtures"].as_u64(),
        manual_user_approval_required: gate["single_fixture_policy"]
            ["selection_requires_manual_user_approval"]
            .as_bool()
            .unwrap_or(false),
        no_parallel_first_loader_fixtures: gate["single_fixture_policy"]
            ["no_parallel_first_loader_fixtures"]
            .as_bool()
            .unwrap_or(false),
    }
}

fn wiztree_refresh_summary(refresh: &Value) -> WizTreeRefreshSummary {
    let fixture_candidates = refresh["fixture_gate_candidates_verified_present"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    WizTreeRefreshSummary {
        scan_mode: safe_string(&refresh["scan_mode"]),
        total_aex_count: refresh["total_aex_scan"]["count"].as_u64().unwrap_or(0),
        canonical_non_generated_count: refresh["canonical_non_generated_aex"]["count"]
            .as_u64()
            .unwrap_or(0),
        generated_target_artifact_count: refresh["generated_target_test_artifacts"]["count"]
            .as_u64()
            .unwrap_or(0),
        generated_target_artifacts_excluded: refresh["generated_target_test_artifacts"]["policy"]
            .as_str()
            .is_some_and(|policy| policy.contains("exclude from first-loader fixture review")),
        fixture_gate_candidate_count: fixture_candidates.len(),
        fixture_gate_candidates_queued_not_approved: fixture_candidates
            .iter()
            .filter(|candidate| {
                candidate["current_review_status"].as_str() == Some("queued-not-approved")
            })
            .count(),
        additional_later_review_count: json_array_len(
            &refresh["additional_small_local_builds_for_later_review"],
        ),
        keep_current_two_candidate_gate: refresh["recommended_fixture_gate_delta"]
            ["keep_current_two_candidate_gate"]
            .as_bool()
            .unwrap_or(false),
        do_not_expand_from_generated_targets: refresh["recommended_fixture_gate_delta"]
            ["do_not_expand_first_loader_queue_from_generated_target_artifacts"]
            .as_bool()
            .unwrap_or(false),
        do_not_select_without_manual_approval: refresh["recommended_fixture_gate_delta"]
            ["do_not_select_fixture_without_manual_approval"]
            .as_bool()
            .unwrap_or(false),
    }
}

fn candidate_crosscheck(gate: &Value, refresh: &Value) -> Vec<CandidateCrosscheck> {
    let refresh_candidates = refresh["fixture_gate_candidates_verified_present"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    gate["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|candidate| {
            let id = candidate["id"].as_str().unwrap_or("missing").to_string();
            let path = candidate["path"].as_str().unwrap_or("missing").to_string();
            let gate_size = candidate["observed_size_bytes"].as_u64();
            let refresh_candidate = refresh_candidates.iter().find(|refresh_candidate| {
                refresh_candidate["id"].as_str() == Some(id.as_str())
                    && path_key(refresh_candidate["path"].as_str().unwrap_or_default())
                        == path_key(&path)
            });
            let refresh_bytes = refresh_candidate.and_then(|value| value["bytes"].as_u64());
            CandidateCrosscheck {
                id,
                path: path.clone(),
                gate_observed_size_bytes: gate_size,
                refresh_bytes,
                path_matches_refresh: refresh_candidate.is_some(),
                size_matches_refresh: gate_size.is_some() && gate_size == refresh_bytes,
                gate_review_status: safe_string(&candidate["review_status"]),
                refresh_review_status: refresh_candidate
                    .and_then(|value| safe_string(&value["current_review_status"])),
                generated_target_artifact: is_generated_target_path(&path),
                blocked_reason_count: json_array_len(&candidate["blocked_reasons"]),
            }
        })
        .collect()
}

fn fixture_gate_closed_no_selection(summary: &FixtureGateSummary) -> bool {
    summary.status.as_deref() == Some("review_queue_not_approved")
        && summary.selected_fixture.is_none()
        && summary.approval_all_false
}

fn fixture_gate_policy_ok(summary: &FixtureGateSummary) -> bool {
    summary.candidate_count == 2
        && summary.local_build_candidate_count == summary.candidate_count
        && summary.not_approved_candidate_count == summary.candidate_count
        && summary.max_selected_fixtures == Some(1)
        && summary.manual_user_approval_required
        && summary.no_parallel_first_loader_fixtures
        && summary.recommendation_status.as_deref() == Some("queue-order-only-not-approval")
}

fn wiztree_refresh_inventory_ok(summary: &WizTreeRefreshSummary) -> bool {
    summary.scan_mode.as_deref() == Some("wiztree-read-only-filtered-aex")
        && summary.total_aex_count == 119
        && summary.canonical_non_generated_count == 40
        && summary.generated_target_artifact_count == 79
        && summary.canonical_non_generated_count + summary.generated_target_artifact_count
            == summary.total_aex_count
}

fn wiztree_generated_targets_excluded(summary: &WizTreeRefreshSummary) -> bool {
    summary.generated_target_artifacts_excluded
        && summary.keep_current_two_candidate_gate
        && summary.do_not_expand_from_generated_targets
        && summary.do_not_select_without_manual_approval
}

fn candidates_present_in_refresh(candidates: &[CandidateCrosscheck]) -> bool {
    candidates.len() == 2
        && candidates.iter().all(|candidate| {
            candidate.path_matches_refresh
                && candidate.size_matches_refresh
                && candidate.gate_review_status.as_deref() == Some("not-approved")
                && candidate.refresh_review_status.as_deref() == Some("queued-not-approved")
                && candidate.blocked_reason_count > 0
        })
}

fn approval_all_false(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    !object.is_empty() && object.values().all(|value| value.as_bool() == Some(false))
}

fn input_contains_forbidden_evidence(
    fixture_gate: &Value,
    fixture_gate_text: &str,
    wiztree_refresh: &Value,
    wiztree_refresh_text: &str,
) -> bool {
    string_contains_forbidden_tokens(fixture_gate_text)
        || string_contains_forbidden_tokens(wiztree_refresh_text)
        || contains_forbidden_field_names(fixture_gate)
        || contains_forbidden_field_names(wiztree_refresh)
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
        "binary_payload".to_string(),
        "payload_bytes".to_string(),
        "copied_asset".to_string(),
        "native_load_result".to_string(),
        ["load", "library"].concat(),
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
        "rendered_pixels",
        "pixel_hash",
    ]
}

fn is_generated_target_path(path: &str) -> bool {
    let normalized = path_key(path);
    normalized.contains("\\aviutlas\\aviutl-rs\\target\\")
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

fn display_optional(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("none")
}

fn json_array_len(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
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

fn parse_args() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let mut fixture_gate = None;
    let mut wiztree_refresh = None;
    let mut out = PathBuf::from("target")
        .join("aex-fixture-gate-refresh-audit")
        .join("fixture-gate-refresh.local.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture-gate" => {
                fixture_gate = args.next().map(PathBuf::from);
                if fixture_gate.is_none() {
                    return Err("--fixture-gate requires a JSON path".to_string());
                }
            }
            "--wiztree-refresh" => {
                wiztree_refresh = args.next().map(PathBuf::from);
                if wiztree_refresh.is_none() {
                    return Err("--wiztree-refresh requires a JSON path".to_string());
                }
            }
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a JSON path".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: aex_fixture_gate_refresh_audit --fixture-gate analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json --wiztree-refresh analysis/AEX_WIZTREE_AEX_REFRESH_2026-06-01.json [--out target/aex-fixture-gate-refresh-audit/fixture-gate-refresh.local.json]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok((
        fixture_gate.ok_or_else(|| "--fixture-gate is required".to_string())?,
        wiztree_refresh.ok_or_else(|| "--wiztree-refresh is required".to_string())?,
        out,
    ))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (fixture_gate_path, wiztree_refresh_path, out_path) = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let fixture_gate = std::fs::read_to_string(&fixture_gate_path)?;
    let wiztree_refresh = std::fs::read_to_string(&wiztree_refresh_path)?;
    let report = audit_fixture_gate_refresh_json(&fixture_gate, &wiztree_refresh)?;
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
