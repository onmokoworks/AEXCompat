//! Final metadata-only readiness gate for the AEX loader lane.
//!
//! This consumes existing no-load preflight and provenance reports and emits a
//! sanitized final gate report. It does not open, hash, load, execute,
//! describe, or render `.aex` binaries.

use serde::Serialize;
use serde_json::Value;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct LoaderReadinessGateReport {
    schema_version: u32,
    publication_status: String,
    status: String,
    evidence_complete: bool,
    final_gate_closed: bool,
    native_load_performed: bool,
    selectors_executed: bool,
    render_performed: bool,
    may_load_aex: bool,
    preflight_summary: PreflightSummary,
    provenance_summary: ProvenanceSummary,
    checks: Vec<GateCheck>,
    blocked_reasons: Vec<String>,
    next_action: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PreflightSummary {
    status: Option<String>,
    preflight_passed: bool,
    native_load_performed: bool,
    broker_may_load_plugin: bool,
    selected_fixture_present: bool,
    selected_loader_entry_present: bool,
}

#[derive(Debug, Serialize)]
struct ProvenanceSummary {
    status: Option<String>,
    no_load_chain_ready: bool,
    native_load_performed: bool,
    selectors_executed: bool,
    render_performed: bool,
    ofx_route_allowed: bool,
    evidence_contains_forbidden_tokens: bool,
    fixture_identity_smoke_provided: bool,
}

#[derive(Debug, Serialize)]
struct GateCheck {
    name: String,
    status: String,
    evidence: String,
}

pub fn evaluate_loader_readiness_gate_json(
    preflight_json: &str,
    provenance_json: &str,
) -> Result<String, Box<dyn Error>> {
    let preflight: Value = serde_json::from_str(preflight_json)?;
    let provenance: Value = serde_json::from_str(provenance_json)?;
    let report = evaluate_loader_readiness_gate(&preflight, &provenance);
    Ok(serde_json::to_string_pretty(&report)?)
}

fn evaluate_loader_readiness_gate(
    preflight: &Value,
    provenance: &Value,
) -> LoaderReadinessGateReport {
    let preflight_summary = PreflightSummary {
        status: string_opt(&preflight["status"]),
        preflight_passed: bool_field(preflight, "preflight_passed"),
        native_load_performed: bool_field(preflight, "native_load_performed"),
        broker_may_load_plugin: bool_field(preflight, "broker_may_load_plugin"),
        selected_fixture_present: !preflight["selected_fixture"].is_null(),
        selected_loader_entry_present: !preflight["selected_loader_entry"].is_null(),
    };

    let provenance_summary = ProvenanceSummary {
        status: string_opt(&provenance["status"]),
        no_load_chain_ready: provenance["status"] == "no_load_provenance_chain_ready",
        native_load_performed: bool_field(provenance, "native_load_performed"),
        selectors_executed: bool_field(provenance, "selectors_executed"),
        render_performed: bool_field(provenance, "render_performed"),
        ofx_route_allowed: bool_field(provenance, "ofx_route_allowed"),
        evidence_contains_forbidden_tokens: bool_field(
            provenance,
            "evidence_contains_forbidden_tokens",
        ),
        fixture_identity_smoke_provided: bool_field(
            &provenance["fixture_identity_smoke_summary"],
            "provided",
        ),
    };

    let mut checks = Vec::new();
    let mut blocked_reasons = Vec::new();
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "preflight_passed_no_load",
        preflight_summary.preflight_passed
            && !preflight_summary.native_load_performed
            && !preflight_summary.broker_may_load_plugin,
        format!(
            "status={:?}, native_load_performed={}, broker_may_load_plugin={}",
            preflight_summary.status,
            preflight_summary.native_load_performed,
            preflight_summary.broker_may_load_plugin
        ),
        "preflight is not a passed no-load report",
    );
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "preflight_selected_fixture_and_entry_present",
        preflight_summary.selected_fixture_present
            && preflight_summary.selected_loader_entry_present,
        format!(
            "selected_fixture_present={}, selected_loader_entry_present={}",
            preflight_summary.selected_fixture_present,
            preflight_summary.selected_loader_entry_present
        ),
        "preflight is missing selected fixture or loader entry evidence",
    );
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "provenance_chain_ready_no_load",
        provenance_summary.no_load_chain_ready
            && !provenance_summary.native_load_performed
            && !provenance_summary.selectors_executed
            && !provenance_summary.render_performed
            && !provenance_summary.ofx_route_allowed,
        format!(
            "status={:?}, native_load_performed={}, selectors_executed={}, render_performed={}, ofx_route_allowed={}",
            provenance_summary.status,
            provenance_summary.native_load_performed,
            provenance_summary.selectors_executed,
            provenance_summary.render_performed,
            provenance_summary.ofx_route_allowed
        ),
        "provenance chain is not ready with no-load invariants intact",
    );
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "provenance_forbidden_token_scan_clean",
        !provenance_summary.evidence_contains_forbidden_tokens,
        format!(
            "evidence_contains_forbidden_tokens={}",
            provenance_summary.evidence_contains_forbidden_tokens
        ),
        "provenance evidence contains forbidden native-loader tokens",
    );
    push_check(
        &mut checks,
        &mut blocked_reasons,
        "fixture_identity_smoke_evidence_present",
        provenance_summary.fixture_identity_smoke_provided,
        format!(
            "fixture_identity_smoke_provided={}",
            provenance_summary.fixture_identity_smoke_provided
        ),
        "fixture identity smoke evidence is missing",
    );

    let evidence_complete = blocked_reasons.is_empty();
    LoaderReadinessGateReport {
        schema_version: 1,
        publication_status: "local-only".to_owned(),
        status: if evidence_complete {
            "loader_readiness_evidence_complete_gate_closed".to_owned()
        } else {
            "blocked_loader_readiness_gate".to_owned()
        },
        evidence_complete,
        final_gate_closed: true,
        native_load_performed: false,
        selectors_executed: false,
        render_performed: false,
        may_load_aex: false,
        preflight_summary,
        provenance_summary,
        checks,
        blocked_reasons,
        next_action: if evidence_complete {
            "Request explicit operator approval before any separate native AEX loader slice."
                .to_owned()
        } else {
            "Repair blocked no-load evidence before considering a native AEX loader slice."
                .to_owned()
        },
        notes: vec![
            "This gate is metadata-only and intentionally keeps native loading closed.".to_owned(),
            "It is not AEX selector, render, OFX route, or pixel parity evidence.".to_owned(),
        ],
    }
}

fn bool_field(value: &Value, field: &str) -> bool {
    value[field].as_bool().unwrap_or(false)
}

fn string_opt(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn push_check(
    checks: &mut Vec<GateCheck>,
    blocked_reasons: &mut Vec<String>,
    name: &str,
    passed: bool,
    evidence: String,
    blocked_reason: &str,
) {
    checks.push(GateCheck {
        name: name.to_owned(),
        status: if passed { "passed" } else { "blocked" }.to_owned(),
        evidence,
    });
    if !passed {
        blocked_reasons.push(blocked_reason.to_owned());
    }
}

pub fn validate_loader_readiness_gate_report_output_path(path: &Path) -> Result<(), String> {
    if path_has_traversal(path) {
        return Err(
            "loader readiness report path must not contain traversal components".to_string(),
        );
    }
    if !path_has_extension(path, "json") {
        return Err("loader readiness report path must have .json extension".to_string());
    }
    if !output_is_under_target_root(path) {
        return Err(
            "loader readiness report path must be under target/aex-loader-readiness-gate"
                .to_string(),
        );
    }
    Ok(())
}

pub fn write_loader_readiness_gate_report_create_new(
    path: &Path,
    body: &str,
) -> Result<(), Box<dyn Error>> {
    validate_loader_readiness_gate_report_output_path(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !output_parent_canonical_is_under_target_root(path) {
        return Err(
            "loader readiness report parent must resolve under target/aex-loader-readiness-gate"
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
                    "loader readiness report already exists",
                )
            } else {
                err
            }
        })?;
    file.write_all(body.as_bytes())?;
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
        .join("aex-loader-readiness-gate")
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

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        return Err(
            "usage: aex_loader_readiness_gate <preflight.json> <provenance.json> <output.json>"
                .into(),
        );
    }
    let preflight_json = std::fs::read_to_string(&args[1])?;
    let provenance_json = std::fs::read_to_string(&args[2])?;
    let report = evaluate_loader_readiness_gate_json(&preflight_json, &provenance_json)?;
    write_loader_readiness_gate_report_create_new(&PathBuf::from(&args[3]), &report)?;
    Ok(())
}
