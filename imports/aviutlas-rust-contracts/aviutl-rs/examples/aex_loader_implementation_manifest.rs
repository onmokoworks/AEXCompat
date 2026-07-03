//! No-load manifest before any AEX loader implementation slice.
//!
//! This checker ties together a passing loader preflight receipt and static
//! readiness capability drafts. It does not open, hash, load, execute, describe,
//! or render `.aex` binaries.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct LoaderImplementationManifest {
    schema_version: u32,
    publication_status: String,
    status: String,
    native_load_performed: bool,
    broker_may_load_plugin: bool,
    loader_may_load_plugin: bool,
    ofx_may_route_to_loader: bool,
    selected_fixture: Option<String>,
    selected_effect_id: Option<String>,
    selected_plugin_path: Option<String>,
    normalized_plugin_path: Option<String>,
    preflight_summary: PreflightSummary,
    capability_summary: CapabilitySummary,
    readiness_summary: ReadinessSummary,
    implementation_gate: ImplementationGate,
    checks: Vec<ManifestCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PreflightSummary {
    status: Option<String>,
    preflight_passed: bool,
    selected_candidate_id: Option<String>,
    selected_loader_entry_effect_id: Option<String>,
    selected_loader_entry_ready: bool,
    fixture_refresh_audit_summary: PreflightFixtureRefreshSummary,
}

#[derive(Debug, Serialize)]
struct PreflightFixtureRefreshSummary {
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
struct CapabilitySummary {
    matched_capability_count: usize,
    effect_id: Option<String>,
    plugin_path: Option<String>,
    evidence_mode: Option<String>,
    load_status: Option<String>,
    broker_may_load_plugin: Option<bool>,
    aex_worker_supported: Option<bool>,
    ofx_facade_supported: Option<bool>,
    selector_statuses: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ReadinessSummary {
    provided: bool,
    matched_entry_count: usize,
    status: Option<String>,
    effect_id: Option<String>,
    plugin_path: Option<String>,
    entry_status: Option<String>,
    pipl_content_scan_status: Option<String>,
    pipl_content_scan_ready: Option<bool>,
    allowed_operations: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ImplementationGate {
    ready_for_separate_loader_slice_review: bool,
    native_loader_calls_allowed: bool,
    broker_may_load_aex: bool,
    ofx_facade_may_route_to_loader: bool,
    requires_explicit_user_approval: bool,
    requires_code_review: bool,
    requires_local_fixture_only: bool,
}

#[derive(Debug, Serialize)]
struct ManifestCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn plan_loader_implementation_manifest_json(
    loader_preflight_json: &str,
    capability_jsons: &[&str],
) -> Result<String, Box<dyn Error>> {
    plan_loader_implementation_manifest_json_with_readiness(
        loader_preflight_json,
        capability_jsons,
        None,
    )
}

pub fn plan_loader_implementation_manifest_json_with_readiness(
    loader_preflight_json: &str,
    capability_jsons: &[&str],
    readiness_json: Option<&str>,
) -> Result<String, Box<dyn Error>> {
    let loader_preflight: Value = serde_json::from_str(loader_preflight_json)?;
    let capabilities = capability_jsons
        .iter()
        .map(|text| serde_json::from_str::<Value>(text))
        .collect::<Result<Vec<_>, _>>()?;
    let readiness = readiness_json
        .map(serde_json::from_str::<Value>)
        .transpose()?;
    let manifest =
        plan_loader_implementation_manifest(&loader_preflight, &capabilities, readiness.as_ref());
    Ok(serde_json::to_string_pretty(&manifest)?)
}

fn plan_loader_implementation_manifest(
    preflight: &Value,
    capabilities: &[Value],
    readiness: Option<&Value>,
) -> LoaderImplementationManifest {
    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();

    let selected_candidate = object_or_none(&preflight["selected_candidate"]);
    let selected_loader_entry = object_or_none(&preflight["selected_loader_entry"]);
    let selected_fixture = preflight["selected_fixture"].as_str().map(str::to_owned);
    let selected_candidate_id =
        selected_candidate.and_then(|value| value["id"].as_str().map(str::to_owned));
    let selected_effect_id =
        selected_loader_entry.and_then(|value| value["effect_id"].as_str().map(str::to_owned));
    let selected_candidate_effect_id =
        selected_candidate.and_then(|value| value["loader_gate_effect_id"].as_str());
    let selected_candidate_path =
        selected_candidate.and_then(|value| value["plugin_path"].as_str());
    let selected_entry_path = selected_loader_entry.and_then(|value| value["plugin_path"].as_str());
    let selected_entry_normalized =
        selected_loader_entry.and_then(|value| value["normalized_plugin_path"].as_str());

    let preflight_core_ok = preflight["schema_version"].as_u64() == Some(1)
        && preflight["publication_status"].as_str() == Some("local-only")
        && preflight["status"].as_str() == Some("preflight_passed_no_load")
        && preflight["preflight_passed"].as_bool() == Some(true)
        && preflight["native_load_performed"].as_bool() == Some(false)
        && preflight["broker_may_load_plugin"].as_bool() == Some(false);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "loader_preflight_core_no_load",
        preflight_core_ok,
        format!(
            "status={}, preflight_passed={}, native_load_performed={}, broker_may_load_plugin={}",
            preflight["status"].as_str().unwrap_or("missing"),
            preflight["preflight_passed"].as_bool().unwrap_or(false),
            preflight["native_load_performed"].as_bool().unwrap_or(true),
            preflight["broker_may_load_plugin"]
                .as_bool()
                .unwrap_or(true)
        ),
        "loader preflight is not a passing no-load receipt",
    );

    let fixture_refresh_summary = preflight_fixture_refresh_summary(preflight);
    let fixture_refresh_preflight_check_passed = !fixture_refresh_summary.provided
        || preflight_check_passed(preflight, "fixture_gate_refresh_audit_ready_no_load");
    let fixture_refresh_gate_ok = if fixture_refresh_summary.provided {
        preflight_fixture_refresh_summary_ready(&fixture_refresh_summary)
            && fixture_refresh_preflight_check_passed
    } else {
        true
    };
    if fixture_refresh_summary.provided {
        push_check(
            &mut checks,
            &mut blocked_reasons,
            "fixture_gate_refresh_audit_ready_no_load",
            fixture_refresh_gate_ok,
            format!(
                "provided={}, status={}, canonical_non_generated_count={}, generated_target_artifact_count={}, candidates_present_in_refresh={}, preflight_check_passed={}",
                fixture_refresh_summary.provided,
                fixture_refresh_summary
                    .status
                    .as_deref()
                    .unwrap_or("missing"),
                fixture_refresh_summary.wiztree_canonical_non_generated_count,
                fixture_refresh_summary.wiztree_generated_target_artifact_count,
                fixture_refresh_summary.candidates_present_in_refresh,
                fixture_refresh_preflight_check_passed
            ),
            "preflight fixture refresh summary is not a ready no-load queue-hygiene receipt",
        );
    }

    let fixture_identity_ok = selected_fixture.as_deref().is_some()
        && selected_fixture.as_deref() == selected_candidate_id.as_deref();
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "selected_fixture_identity",
        fixture_identity_ok,
        format!(
            "selected_fixture={}, selected_candidate_id={}",
            selected_fixture.as_deref().unwrap_or("missing"),
            selected_candidate_id.as_deref().unwrap_or("missing")
        ),
        "selected fixture does not match selected candidate id",
    );

    let loader_entry_identity_ok = selected_candidate_effect_id.is_some()
        && selected_effect_id.as_deref() == selected_candidate_effect_id;
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "selected_loader_entry_identity",
        loader_entry_identity_ok,
        format!(
            "selected_candidate.loader_gate_effect_id={}, selected_loader_entry.effect_id={}",
            selected_candidate_effect_id.unwrap_or("missing"),
            selected_effect_id.as_deref().unwrap_or("missing")
        ),
        "selected loader entry effect id does not match selected candidate loader gate id",
    );

    let selected_path_key = selected_candidate_path.map(path_key);
    let entry_path_key = selected_entry_path.map(path_key);
    let path_identity_ok = selected_path_key.is_some()
        && entry_path_key.is_some()
        && selected_path_key == entry_path_key
        && selected_entry_normalized == entry_path_key.as_deref()
        && selected_loader_entry.and_then(|value| value["path_match_status"].as_str())
            == Some("matched_normalized_path");
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "selected_loader_entry_path_identity",
        path_identity_ok,
        format!(
            "selected_candidate_path={}, selected_loader_entry_path={}",
            selected_candidate_path.unwrap_or("missing"),
            selected_entry_path.unwrap_or("missing")
        ),
        "selected loader entry path evidence is inconsistent",
    );

    let selected_loader_entry_ready =
        selected_loader_entry.and_then(|value| value["entry_ready"].as_bool()) == Some(true);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "selected_loader_entry_ready",
        selected_loader_entry_ready,
        format!("entry_ready={selected_loader_entry_ready}"),
        "selected loader entry is not marked ready in the preflight receipt",
    );

    let matched_capabilities = matching_capabilities(capabilities, selected_effect_id.as_deref());
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "single_matching_capability_draft",
        matched_capabilities.len() == 1,
        format!("matched_capability_count={}", matched_capabilities.len()),
        "exactly one matching no-load capability draft is required",
    );

    let matched_capability = matched_capabilities.first().copied();
    let capability_path_ok = matched_capability
        .and_then(|capability| capability["plugin_path"].as_str())
        .map(path_key)
        == entry_path_key;
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "capability_path_identity",
        matched_capability.is_some() && capability_path_ok,
        matched_capability
            .and_then(|capability| capability["plugin_path"].as_str())
            .map(|path| format!("capability_plugin_path={path}"))
            .unwrap_or_else(|| "capability_plugin_path=missing".to_string()),
        "matching capability draft path does not match the selected loader entry",
    );

    let capability_no_load_ok = matched_capability
        .map(capability_is_no_load_static_draft)
        .unwrap_or(false);
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "capability_no_load_static_draft",
        capability_no_load_ok,
        matched_capability
            .map(|capability| {
                format!(
                    "evidence_mode={}, load_status={}, broker_may_load_plugin={}",
                    capability["evidence_mode"].as_str().unwrap_or("missing"),
                    capability["load_status"].as_str().unwrap_or("missing"),
                    capability["broker_may_load_plugin"]
                        .as_bool()
                        .unwrap_or(true)
                )
            })
            .unwrap_or_else(|| "capability=missing".to_string()),
        "matching capability draft claims execution or loader support",
    );

    let readiness_summary = readiness_summary(
        readiness,
        selected_effect_id.as_deref(),
        entry_path_key.as_deref(),
    );
    let readiness_gate_ok = readiness
        .map(|report| {
            readiness_core_ok(report)
                && readiness_summary.matched_entry_count == 1
                && readiness_summary.entry_status.as_deref() == Some("draft_allowlisted")
                && readiness_summary.pipl_content_scan_status.as_deref() == Some("semantic_matches")
                && readiness_summary.pipl_content_scan_ready == Some(true)
                && readiness_summary
                    .allowed_operations
                    .iter()
                    .any(|operation| operation == "describe")
                && !contains_forbidden_tokens(report)
                && !contains_forbidden_field_names(report)
        })
        .unwrap_or(true);
    if readiness.is_some() {
        push_check(
            &mut checks,
            &mut blocked_reasons,
            "readiness_pipl_semantic_gate",
            readiness_gate_ok,
            format!(
                "provided={}, matched_entry_count={}, entry_status={}, pipl_content_scan_status={}, pipl_content_scan_ready={}",
                readiness_summary.provided,
                readiness_summary.matched_entry_count,
                readiness_summary.entry_status.as_deref().unwrap_or("missing"),
                readiness_summary
                    .pipl_content_scan_status
                    .as_deref()
                    .unwrap_or("missing"),
                readiness_summary
                    .pipl_content_scan_ready
                    .map(|ready| ready.to_string())
                    .unwrap_or_else(|| "missing".to_string())
            ),
            "readiness report does not prove the selected effect passed the semantic PiPL readiness gate",
        );
    }

    let no_forbidden_evidence_tokens = !contains_forbidden_tokens(preflight)
        && capabilities
            .iter()
            .all(|cap| !contains_forbidden_tokens(cap) && !contains_forbidden_field_names(cap));
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "evidence_anti_contamination",
        no_forbidden_evidence_tokens,
        "metadata-only reports scanned for forbidden tokens and field names".to_string(),
        "metadata evidence contains forbidden execution, hash, payload, or rendered-output claims",
    );

    let review_ready = blocked_reasons.is_empty();
    let status = if review_ready {
        "ready_for_separate_loader_implementation_review_no_load"
    } else if !preflight_core_ok {
        "blocked_preflight_receipt"
    } else if !fixture_refresh_gate_ok {
        "blocked_fixture_refresh_evidence"
    } else if !loader_entry_identity_ok || !path_identity_ok {
        "blocked_identity_mismatch"
    } else if matched_capabilities.len() != 1 || !capability_path_ok {
        "blocked_capability_draft"
    } else if !readiness_gate_ok {
        "blocked_readiness_evidence"
    } else {
        "blocked_no_load_invariants"
    };

    LoaderImplementationManifest {
        schema_version: 1,
        publication_status: "local-only".to_string(),
        status: status.to_string(),
        native_load_performed: false,
        broker_may_load_plugin: false,
        loader_may_load_plugin: false,
        ofx_may_route_to_loader: false,
        selected_fixture,
        selected_effect_id: selected_effect_id.clone(),
        selected_plugin_path: selected_entry_path.map(str::to_owned),
        normalized_plugin_path: selected_entry_normalized.map(str::to_owned),
        preflight_summary: PreflightSummary {
            status: preflight["status"].as_str().map(str::to_owned),
            preflight_passed: preflight["preflight_passed"].as_bool().unwrap_or(false),
            selected_candidate_id,
            selected_loader_entry_effect_id: selected_effect_id,
            selected_loader_entry_ready,
            fixture_refresh_audit_summary: fixture_refresh_summary,
        },
        capability_summary: capability_summary(matched_capability, matched_capabilities.len()),
        readiness_summary,
        implementation_gate: ImplementationGate {
            ready_for_separate_loader_slice_review: review_ready,
            native_loader_calls_allowed: false,
            broker_may_load_aex: false,
            ofx_facade_may_route_to_loader: false,
            requires_explicit_user_approval: true,
            requires_code_review: true,
            requires_local_fixture_only: true,
        },
        checks,
        blocked_reasons,
        next_action: if review_ready {
            "Open a separate reviewed loader implementation slice; this manifest still permits no native loading.".to_string()
        } else {
            "Fix blocked no-load evidence before opening a loader implementation review slice.".to_string()
        },
        notes: vec![
            "Manifest reads JSON metadata only.".to_string(),
            "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.".to_string(),
            "A ready manifest is permission to review a separate loader implementation slice, not permission to load a plugin.".to_string(),
        ],
    }
}

fn preflight_fixture_refresh_summary(preflight: &Value) -> PreflightFixtureRefreshSummary {
    let summary = &preflight["fixture_refresh_audit_summary"];
    let provided = summary["provided"].as_bool().unwrap_or(false);
    if !provided {
        return PreflightFixtureRefreshSummary {
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

    PreflightFixtureRefreshSummary {
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

fn preflight_fixture_refresh_summary_ready(summary: &PreflightFixtureRefreshSummary) -> bool {
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

fn preflight_check_passed(preflight: &Value, name: &str) -> bool {
    preflight["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|check| {
            check["name"].as_str() == Some(name) && check["status"].as_str() == Some("passed")
        })
}

fn object_or_none(value: &Value) -> Option<&Value> {
    if value.is_object() {
        Some(value)
    } else {
        None
    }
}

fn matching_capabilities<'a>(
    capabilities: &'a [Value],
    selected_effect_id: Option<&str>,
) -> Vec<&'a Value> {
    capabilities
        .iter()
        .filter(|capability| capability["effect_id"].as_str() == selected_effect_id)
        .collect()
}

fn capability_summary(capability: Option<&Value>, matched_count: usize) -> CapabilitySummary {
    CapabilitySummary {
        matched_capability_count: matched_count,
        effect_id: capability.and_then(|value| value["effect_id"].as_str().map(str::to_owned)),
        plugin_path: capability.and_then(|value| value["plugin_path"].as_str().map(str::to_owned)),
        evidence_mode: capability
            .and_then(|value| value["evidence_mode"].as_str().map(str::to_owned)),
        load_status: capability.and_then(|value| value["load_status"].as_str().map(str::to_owned)),
        broker_may_load_plugin: capability
            .and_then(|value| value["broker_may_load_plugin"].as_bool()),
        aex_worker_supported: capability
            .and_then(|value| value["aex_worker"]["supported"].as_bool()),
        ofx_facade_supported: capability
            .and_then(|value| value["ofx_facade"]["supported"].as_bool()),
        selector_statuses: capability
            .and_then(|value| value["selectors"].as_array())
            .map(|selectors| {
                selectors
                    .iter()
                    .filter_map(|selector| selector["status"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn readiness_summary(
    readiness: Option<&Value>,
    selected_effect_id: Option<&str>,
    selected_path_key: Option<&str>,
) -> ReadinessSummary {
    let matched_entries = readiness
        .and_then(|report| report["entries"].as_array())
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| {
                    entry["effect_id"].as_str() == selected_effect_id
                        && entry["plugin_path"].as_str().map(path_key).as_deref()
                            == selected_path_key
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let entry = matched_entries.first().copied();
    ReadinessSummary {
        provided: readiness.is_some(),
        matched_entry_count: matched_entries.len(),
        status: readiness.and_then(|report| report["status"].as_str().map(str::to_owned)),
        effect_id: entry.and_then(|value| value["effect_id"].as_str().map(str::to_owned)),
        plugin_path: entry.and_then(|value| value["plugin_path"].as_str().map(str::to_owned)),
        entry_status: entry.and_then(|value| value["status"].as_str().map(str::to_owned)),
        pipl_content_scan_status: entry.and_then(|value| {
            value["pipl_content_scan_status"]
                .as_str()
                .map(str::to_owned)
        }),
        pipl_content_scan_ready: entry.and_then(|value| value["pipl_content_scan_ready"].as_bool()),
        allowed_operations: entry
            .and_then(|value| value["allowed_operations"].as_array())
            .map(|operations| {
                operations
                    .iter()
                    .filter_map(|operation| operation.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn readiness_core_ok(readiness: &Value) -> bool {
    readiness["schema_version"].as_u64() == Some(1)
        && readiness["publication_status"].as_str() == Some("local-only")
        && readiness["status"].as_str() == Some("probe_readiness_planned")
}

fn capability_is_no_load_static_draft(capability: &Value) -> bool {
    capability["schema_version"].as_u64() == Some(1)
        && capability["publication_status"].as_str() == Some("local-only")
        && capability["evidence_mode"].as_str() == Some("static-classifier-metadata-only")
        && capability["load_status"].as_str() == Some("not_loaded")
        && capability["broker_may_load_plugin"].as_bool() == Some(false)
        && capability["current_supported_operations"]
            .as_array()
            .is_some_and(Vec::is_empty)
        && capability["params_status"].as_str() == Some("unknown")
        && capability["params"].as_array().is_some_and(Vec::is_empty)
        && capability["aex_worker"]["supported"].as_bool() == Some(false)
        && capability["ofx_facade"]["supported"].as_bool() == Some(false)
        && capability["selectors"].as_array().is_some_and(|selectors| {
            !selectors.is_empty()
                && selectors
                    .iter()
                    .all(|selector| selector["status"].as_str() == Some("not_run"))
        })
}

fn contains_forbidden_tokens(value: &Value) -> bool {
    let serialized = serde_json::to_string(value)
        .unwrap_or_default()
        .to_ascii_lowercase();
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
    checks: &mut Vec<ManifestCheck>,
    blocked_reasons: &mut Vec<String>,
    name: &str,
    passed: bool,
    evidence: String,
    blocked_reason: &str,
) {
    checks.push(ManifestCheck {
        name: name.to_string(),
        status: if passed { "passed" } else { "blocked" }.to_string(),
        evidence,
    });
    if !passed {
        blocked_reasons.push(blocked_reason.to_string());
    }
}

fn parse_args() -> Result<(PathBuf, Vec<PathBuf>, Option<PathBuf>, PathBuf), String> {
    let mut preflight = None;
    let mut capabilities = Vec::new();
    let mut readiness = None;
    let mut out = PathBuf::from("target")
        .join("aex-loader-implementation")
        .join("loader-implementation.local.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--preflight" => {
                preflight = args.next().map(PathBuf::from);
                if preflight.is_none() {
                    return Err("--preflight requires a path".to_string());
                }
            }
            "--capability" => {
                let Some(path) = args.next().map(PathBuf::from) else {
                    return Err("--capability requires a path".to_string());
                };
                capabilities.push(path);
            }
            "--readiness" => {
                readiness = args.next().map(PathBuf::from);
                if readiness.is_none() {
                    return Err("--readiness requires a path".to_string());
                }
            }
            "--out" => {
                out = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a path".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: aex_loader_implementation_manifest --preflight target/aex-loader-preflight/preflight.local.json --capability target/aex-probe-readiness/capabilities/Example.capability.json [--readiness target/aex-probe-readiness/readiness.local.json] [--out target/aex-loader-implementation/loader-implementation.local.json]".to_string());
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let preflight = preflight.ok_or_else(|| "--preflight is required".to_string())?;
    if capabilities.is_empty() {
        return Err("at least one --capability is required".to_string());
    }
    Ok((preflight, capabilities, readiness, out))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (preflight_path, capability_paths, readiness_path, out_path) = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let preflight = std::fs::read_to_string(&preflight_path)?;
    let capability_texts = capability_paths
        .iter()
        .map(std::fs::read_to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let capability_refs = capability_texts
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let readiness = readiness_path
        .as_ref()
        .map(std::fs::read_to_string)
        .transpose()?;
    let report = plan_loader_implementation_manifest_json_with_readiness(
        &preflight,
        &capability_refs,
        readiness.as_deref(),
    )?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, report)?;
    println!("{}", out_path.display());
    Ok(())
}
