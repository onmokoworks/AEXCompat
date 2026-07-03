use std::io::ErrorKind;
use std::path::Path;

use serde_json::Value;

const REQUEST_SCHEMA: &str = "analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json";
const SYNTHETIC_IMAGE_FIXTURES_SCHEMA: &str =
    "analysis/AEX_PROBE_SYNTHETIC_IMAGE_FIXTURES_SCHEMA_2026-06-01.json";
const FIXTURE_IDENTITY_SMOKE_SCHEMA: &str =
    "analysis/AEX_PROBE_FIXTURE_IDENTITY_SMOKE_SCHEMA_2026-06-01.json";
const WORKER_REPORT_SCHEMA: &str = "analysis/AEX_WORKER_CAPABILITY_REPORT_SCHEMA_2026-05-31.json";
const EXTERNAL_CAPABILITY_SCHEMA: &str =
    "analysis/EXTERNAL_EFFECT_CAPABILITY_SCHEMA_2026-05-31.json";
const READINESS_CAPABILITY_DRAFT_SCHEMA: &str =
    "analysis/AEX_READINESS_CAPABILITY_DRAFT_SCHEMA_2026-06-01.json";
const LOADER_IMPLEMENTATION_MANIFEST_SCHEMA: &str =
    "analysis/AEX_LOADER_IMPLEMENTATION_MANIFEST_SCHEMA_2026-06-01.json";
const LOADER_PREFLIGHT_REPORT_SCHEMA: &str =
    "analysis/AEX_LOADER_PREFLIGHT_REPORT_SCHEMA_2026-06-01.json";
const WORKER_LOADER_TICKET_SCHEMA: &str =
    "analysis/AEX_WORKER_LOADER_TICKET_SCHEMA_2026-06-01.json";
const NATIVE_STAGE_PLAN_SCHEMA: &str = "analysis/AEX_NATIVE_STAGE_PLAN_SCHEMA_2026-06-01.json";
const HOST_VOCABULARY_BOUNDARY_SCHEMA: &str =
    "analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json";
const ALLOWLIST_EXAMPLE: &str = "analysis/AEX_IMAGE_PROBE_ALLOWLIST.example.json";
const FIXTURE_REVIEW_GATE: &str = "analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json";
const WORKER_START_POLICY: &str = "analysis/AEX_WORKER_START_POLICY_2026-05-31.md";
const OFX_STRATEGY: &str = "analysis/OFX_AEX_BRIDGE_STRATEGY_2026-05-31.md";
const OFX_FACADE_CONTRACT: &str = "analysis/OFX_AEX_FACADE_CONTRACT_2026-05-31.json";
const OFX_FACADE_READINESS_SCHEMA: &str =
    "analysis/OFX_AEX_FACADE_READINESS_REPORT_SCHEMA_2026-06-01.json";
const AEX_NO_LOAD_PROVENANCE_PIPELINE_RUNBOOK: &str =
    "analysis/AEX_NO_LOAD_PROVENANCE_PIPELINE_RUNBOOK_2026-06-01.md";
const AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA: &str =
    "analysis/AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA_2026-06-01.json";
const AEX_LOADER_SLICE_REVIEW_SCHEMA: &str =
    "analysis/AEX_LOADER_SLICE_REVIEW_SCHEMA_2026-06-01.json";
const AEX_LOADER_APPROVAL_RECEIPT_SCHEMA: &str =
    "analysis/AEX_LOADER_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json";
const AEX_WIZTREE_AEX_REFRESH_SCHEMA: &str =
    "analysis/AEX_WIZTREE_AEX_REFRESH_SCHEMA_2026-06-01.json";
const AEX_FIXTURE_GATE_REFRESH_AUDIT_SCHEMA: &str =
    "analysis/AEX_FIXTURE_GATE_REFRESH_AUDIT_SCHEMA_2026-06-01.json";

fn load_analysis_json(relative_path: &str) -> Option<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .join(relative_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!(
                "skipping AEX probe contract guard; artifact is absent: {}",
                path.display()
            );
            return None;
        }
        Err(err) => panic!("failed to read analysis artifact {}: {err}", path.display()),
    };
    Some(serde_json::from_str(&text).expect("analysis artifact should be JSON"))
}

fn load_analysis_text(relative_path: &str) -> Option<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .join(relative_path);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!(
                "skipping AEX probe contract guard; artifact is absent: {}",
                path.display()
            );
            None
        }
        Err(err) => panic!("failed to read analysis artifact {}: {err}", path.display()),
    }
}

fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("expected string field {field} in {value:?}"))
}

fn string_array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item == expected)
}

fn string_array_item_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item.as_str().unwrap_or_default().contains(expected))
}

fn pipe_options(value: &Value) -> Vec<&str> {
    value
        .as_str()
        .unwrap_or_default()
        .split('|')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

#[test]
fn aex_image_probe_request_schema_preserves_broker_worker_boundary() {
    let Some(schema) = load_analysis_json(REQUEST_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "publication_status"),
        "local-only design artifact"
    );
    let request = &schema["request"];
    assert_eq!(
        pipe_options(&request["operation"]),
        vec!["catalog", "describe", "render_png", "identity_transport"],
        "request schema should document the three v0 operations"
    );
    assert_eq!(request["pixel_format"], "rgba8");
    for field in ["plugin_path", "allowlist"] {
        let text = request[field].as_str().unwrap_or_default();
        assert!(
            text.contains("required for describe/render_png"),
            "{field} should be required for describe/render_png"
        );
    }
    for field in ["input_png", "output_png"] {
        let text = request[field].as_str().unwrap_or_default();
        assert!(
            text.contains("required for render_png and identity_transport"),
            "{field} should be required for render_png and identity_transport"
        );
    }
    assert!(
        request["loader_preflight"]
            .as_str()
            .unwrap_or_default()
            .contains("required when loader_intent.request_real_aex_load is true"),
        "loader_preflight evidence should be required before real-load intent"
    );
    let worker_exe = request["worker_exe"].as_str().unwrap_or_default();
    assert!(
        worker_exe.contains("absolute path")
            && worker_exe.contains("reviewed cargo example stub")
            && worker_exe.contains("dynamic test stub"),
        "worker_exe should require an explicit reviewed worker path"
    );

    let response_statuses = schema["response"]["status"]
        .as_array()
        .expect("response statuses should be an array");
    for status in [
        "ok",
        "catalog_ok",
        "allowlist_denied",
        "unsupported_plugin_class",
        "timeout",
        "worker_crash",
        "worker_protocol_error",
    ] {
        assert!(
            response_statuses.iter().any(|item| item == status),
            "missing response status {status}"
        );
    }
    let notes = &schema["notes"];
    assert!(string_array_contains(
        notes,
        "Broker process must not load .aex."
    ));
    assert!(string_array_item_contains(
        notes,
        "Broker may spawn only an explicit reviewed worker_exe path"
    ));
    assert!(string_array_contains(
        notes,
        "Worker process must re-check allowlist before any future native module load."
    ));
    assert!(string_array_contains(
        notes,
        "Current loader_approval reports must keep approved=false, loader_enabled=false, and real_aex_load_enabled=false."
    ));
    assert!(string_array_contains(
        notes,
        "Worker identity revalidation may pass while approved=false, loader_enabled=false, real_aex_load_enabled=false, and output_png is null."
    ));
    assert!(schema["response"]["worker_loader_ticket"]["status"]
        .as_str()
        .unwrap_or_default()
        .contains("accepted_no_load"));
    assert!(
        schema["response"]["worker_loader_ticket"]["worker_may_load_plugin"]
            .as_str()
            .unwrap_or_default()
            .contains("must remain false")
    );
    assert!(string_array_contains(
        notes,
        "When a real-load intent reaches the worker boundary, the broker writes a generated-root worker-loader-ticket JSON and the worker validates it as accepted_no_load; this ticket is still not loader approval and must keep native_load_performed=false and worker_may_load_plugin=false."
    ));
    assert!(string_array_contains(
        notes,
        "Sandbox preflight is measured runtime evidence only. It does not approve native loading by itself."
    ));
    assert!(string_array_item_contains(
        notes,
        "creates a worker transport manifest containing schema_version, transport_protocol_version, pixel_format, width, height, row_stride_bytes, decoded_bytes, raw_rgba_path, and generated_root"
    ));
    assert!(string_array_contains(
        notes,
        "For identity_transport, broker decodes a synthetic PNG, validates dimensions/decoded byte limits, and writes a create-new RGBA8 PNG under target/aex-image-probe without requiring plugin_path, allowlist, worker_exe, loader_preflight, or loader_intent."
    ));
    assert!(string_array_contains(
        notes,
        "identity_transport status ok is a broker pixel-transport check only; it is not .aex rendering, parameter description, worker execution, loader approval, or output correctness evidence."
    ));
    assert!(string_array_contains(
        notes,
        "The worker stub validates the transport manifest and raw file only; the manifest must not contain .aex paths."
    ));
    assert!(string_array_contains(
        notes,
        "When loader_intent.request_real_aex_load is true, the broker requires a separate loader_preflight report before worker identity revalidation can run; missing, blocked, mismatched, or payload-bearing preflight evidence fails closed before worker launch."
    ));
    assert!(string_array_contains(
        notes,
        "A passed sandbox_preflight must not change loader_enabled or real_aex_load_enabled while the current no-load contract remains active."
    ));
    assert!(schema["response"]["sandbox_preflight"]["status"]
        .as_str()
        .unwrap_or_default()
        .contains("passed | failed | not_run"));
    assert!(schema["response"]["sandbox_preflight"]["job_object_status"]
        .as_str()
        .unwrap_or_default()
        .contains("assigned"));
    assert!(
        schema["response"]["sandbox_preflight"]["worker_attestation"]["status"]
            .as_str()
            .unwrap_or_default()
            .contains("passed | failed")
    );
    assert!(string_array_contains(
        notes,
        "No binary payloads, hashes, or private assets are embedded in reports."
    ));
}

#[test]
fn aex_probe_synthetic_image_fixture_schema_keeps_inputs_no_load() {
    let Some(schema) = load_analysis_json(SYNTHETIC_IMAGE_FIXTURES_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "schema_name"),
        "AEX probe synthetic image fixtures"
    );
    assert_eq!(
        string_field(&schema, "compatibility_classification"),
        "Synthetic RGBA8 fixture inputs only"
    );
    for field in [
        "schema_version",
        "generated_by",
        "publication_status",
        "status",
        "output_root",
        "pixel_format",
        "width",
        "height",
        "image_count",
        "native_load_performed",
        "render_performed",
        "aex_loaded",
        "worker_started",
        "broker_invoked",
        "ofx_route_invoked",
        "ae_invoked",
        "private_payload_copied",
        "images",
        "checks",
        "notes",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "synthetic image fixture schema missing field {field}"
        );
    }
    let required = &schema["required_values"];
    assert_eq!(required["generated_by"], "aex_probe_fixture_images");
    assert_eq!(required["status"], "synthetic_fixture_images_ready_no_load");
    assert_eq!(required["pixel_format"], "rgba8");
    assert_eq!(required["image_count"], 3);
    for field in [
        "native_load_performed",
        "render_performed",
        "aex_loaded",
        "worker_started",
        "broker_invoked",
        "ofx_route_invoked",
        "ae_invoked",
        "private_payload_copied",
    ] {
        assert_eq!(required[field], false);
    }
    for file_name in [
        "gradient_rgba8.png",
        "checker_rgba8.png",
        "solid_alpha_rgba8.png",
    ] {
        assert!(
            schema["required_images"]
                .as_array()
                .expect("required_images should be an array")
                .iter()
                .any(|image| image["file_name"] == file_name),
            "synthetic image fixture schema missing {file_name}"
        );
    }
    for check in [
        "output_root_confined",
        "synthetic_rgba8_only",
        "no_aex_input",
        "no_worker_broker_or_host_invocation",
        "no_private_payload_copy",
        "create_new_outputs",
        "all_outputs_preflighted",
        "rollback_on_late_write_failure",
        "no_symlink_or_reparse_output_ancestors",
    ] {
        assert!(string_array_contains(
            &schema["required_check_names"],
            check
        ));
    }
    for token in [
        "sha256",
        "base64",
        "binary_payload",
        "loadlibrary",
        "libloading",
        "effectmain",
        "input_png",
        "output_png",
    ] {
        assert!(string_array_contains(
            &schema["forbidden_manifest_tokens"],
            token
        ));
    }
    for note in [
        "Fixture images are synthetic developer inputs only.",
        "This manifest is not loader approval and not pixel correctness evidence.",
        "No AEX file is opened, copied, loaded, described, or rendered.",
    ] {
        assert!(string_array_contains(&schema["required_notes"], note));
    }
}

#[test]
fn readiness_capability_draft_schema_keeps_loader_promotion_closed() {
    let Some(schema) = load_analysis_json(READINESS_CAPABILITY_DRAFT_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "publication_status"),
        "local-only design artifact"
    );
    assert_eq!(
        string_field(&schema, "scope"),
        "aex_probe_readiness capabilities/*.capability.json"
    );
    assert_eq!(
        string_field(&schema, "evidence_mode"),
        "static-classifier-metadata-only"
    );

    for field in [
        "schema_version",
        "effect_id",
        "display_name",
        "plugin_path",
        "publication_status",
        "evidence_mode",
        "load_status",
        "broker_may_load_plugin",
        "current_supported_operations",
        "params_status",
        "params",
        "selectors",
        "aex_worker",
        "ofx_facade",
        "unsupported_or_deferred_surfaces",
        "notes",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "readiness draft schema missing required field {field}"
        );
    }

    let required = &schema["required_values"];
    assert_eq!(required["schema_version"], 1);
    assert_eq!(required["publication_status"], "local-only");
    assert_eq!(required["evidence_mode"], "static-classifier-metadata-only");
    assert_eq!(required["load_status"], "not_loaded");
    assert_eq!(required["broker_may_load_plugin"], false);
    assert!(required["current_supported_operations"]
        .as_array()
        .expect("current_supported_operations should be an array")
        .is_empty());
    assert_eq!(required["params_status"], "unknown");
    assert!(required["params"]
        .as_array()
        .expect("params should be an array")
        .is_empty());

    assert_eq!(
        schema["selector_names"]
            .as_array()
            .expect("selector_names should be an array")
            .len(),
        7
    );
    for selector in [
        "load",
        "global_setup",
        "params_setup",
        "sequence_setup",
        "render",
        "sequence_teardown",
        "global_teardown",
    ] {
        assert!(string_array_contains(&schema["selector_names"], selector));
    }
    assert_eq!(schema["selector_status"], "not_run");
    assert_eq!(schema["aex_worker"]["supported"], false);
    assert_eq!(
        schema["aex_worker"]["status"],
        "deferred_loader_gate_closed"
    );
    assert_eq!(schema["ofx_facade"]["supported"], false);
    assert_eq!(
        schema["ofx_facade"]["status"],
        "deferred_same_aex_worker_gate"
    );

    for surface in [
        "SmartFX",
        "GPU",
        "AEGP suites",
        "AEIO",
        "audio",
        "layer checkout",
        "custom UI",
        "arbitrary file or network APIs",
    ] {
        assert!(string_array_contains(
            &schema["required_unsupported_or_deferred_surfaces"],
            surface
        ));
    }
    for field in [
        "worker_exe",
        "input_png",
        "output_png",
        "render_png",
        "last_probe",
        "hash",
        "sha256",
        "binary_payload",
        "base64_payload",
    ] {
        assert!(
            string_array_contains(&schema["forbidden_fields"], field),
            "readiness draft schema should forbid {field}"
        );
    }
    assert!(string_array_contains(
        &schema["forbidden_serialized_tokens"],
        "sha256"
    ));
    assert!(string_array_contains(
        &schema["forbidden_serialized_tokens"],
        "base64"
    ));

    let promotion = &schema["promotion_gate"];
    assert_eq!(
        promotion["worker_describe_requires_separate_loader_slice"],
        true
    );
    assert_eq!(promotion["render_png_requires_separate_loader_slice"], true);
    assert_eq!(promotion["loader_gate_must_remain_closed"], true);
    assert!(string_array_contains(
        &schema["notes"],
        "Selectors must be not_run; no parameter descriptors, worker execution, or render pixels are claimed."
    ));
}

#[test]
fn loader_preflight_report_schema_carries_fixture_refresh_queue_hygiene() {
    let Some(schema) = load_analysis_json(LOADER_PREFLIGHT_REPORT_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(
        string_field(&schema, "purpose").contains("fixture-refresh queue hygiene evidence"),
        "loader preflight schema should mention fixture-refresh queue evidence"
    );
    for field in [
        "schema_version",
        "publication_status",
        "status",
        "preflight_passed",
        "native_load_performed",
        "broker_may_load_plugin",
        "selected_fixture",
        "selected_candidate",
        "selected_loader_entry",
        "fixture_gate",
        "loader_gate",
        "fixture_refresh_audit_summary",
        "checks",
        "blocked_reasons",
        "next_action",
        "notes",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "loader preflight schema missing required field {field}"
        );
    }
    assert_eq!(schema["required_values"]["native_load_performed"], false);
    assert_eq!(schema["required_values"]["broker_may_load_plugin"], false);
    assert!(string_array_contains(
        &schema["conditional_check_names"],
        "fixture_gate_refresh_audit_ready_no_load"
    ));

    for field in [
        "provided",
        "schema_version",
        "publication_status",
        "status",
        "native_load_performed",
        "render_performed",
        "fixture_selected",
        "loader_enabled",
        "fixture_gate_candidate_count",
        "wiztree_total_aex_count",
        "wiztree_canonical_non_generated_count",
        "wiztree_generated_target_artifact_count",
        "generated_target_artifacts_excluded",
        "candidates_present_in_refresh",
        "input_contains_forbidden_tokens",
        "blocked_reason_count",
    ] {
        assert!(
            string_array_contains(
                &schema["fixture_refresh_audit_summary_required_fields"],
                field
            ),
            "fixture refresh audit summary missing field {field}"
        );
    }
    let ready = &schema["fixture_refresh_audit_summary_required_values_when_provided"];
    assert_eq!(ready["status"], "fixture_gate_refresh_ready_no_load");
    assert_eq!(ready["native_load_performed"], false);
    assert_eq!(ready["render_performed"], false);
    assert_eq!(ready["fixture_selected"], false);
    assert_eq!(ready["loader_enabled"], false);
    assert_eq!(ready["fixture_gate_candidate_count"], 2);
    assert_eq!(ready["wiztree_total_aex_count"], 119);
    assert_eq!(ready["wiztree_canonical_non_generated_count"], 40);
    assert_eq!(ready["wiztree_generated_target_artifact_count"], 79);
    assert_eq!(ready["generated_target_artifacts_excluded"], true);
    assert_eq!(ready["candidates_present_in_refresh"], true);
    assert_eq!(ready["input_contains_forbidden_tokens"], false);
    assert_eq!(ready["blocked_reason_count"], 0);
    for token in [
        "sha256",
        "base64",
        "loadlibrary",
        "libloading",
        "effectmain",
        "input_png",
        "output_png",
        "rendered_pixels",
    ] {
        assert!(
            string_array_contains(&schema["forbidden_serialized_tokens"], token),
            "loader preflight schema should forbid {token}"
        );
    }
}

#[test]
fn loader_implementation_manifest_schema_keeps_review_packet_no_load() {
    let Some(schema) = load_analysis_json(LOADER_IMPLEMENTATION_MANIFEST_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(
        string_field(&schema, "purpose")
            .contains("before any separate native AEX loader implementation slice"),
        "implementation manifest schema should keep the no-load review packet purpose"
    );
    assert!(
        string_field(&schema, "purpose").contains("fixture-refresh queue hygiene evidence"),
        "implementation manifest schema should mention propagated fixture refresh evidence"
    );
    for field in [
        "schema_version",
        "publication_status",
        "status",
        "native_load_performed",
        "broker_may_load_plugin",
        "loader_may_load_plugin",
        "ofx_may_route_to_loader",
        "preflight_summary",
        "capability_summary",
        "readiness_summary",
        "implementation_gate",
        "checks",
        "blocked_reasons",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "implementation manifest schema missing required field {field}"
        );
    }
    let required = &schema["required_values"];
    assert_eq!(required["publication_status"], "local-only");
    assert_eq!(required["native_load_performed"], false);
    assert_eq!(required["broker_may_load_plugin"], false);
    assert_eq!(required["loader_may_load_plugin"], false);
    assert_eq!(required["ofx_may_route_to_loader"], false);
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "ready_for_separate_loader_implementation_review_no_load"
    ));
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "blocked_fixture_refresh_evidence"
    ));
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "blocked_readiness_evidence"
    ));
    assert!(string_array_contains(
        &schema["preflight_summary_required_fields"],
        "fixture_refresh_audit_summary"
    ));
    for field in [
        "provided",
        "schema_version",
        "publication_status",
        "status",
        "native_load_performed",
        "render_performed",
        "fixture_selected",
        "loader_enabled",
        "fixture_gate_candidate_count",
        "wiztree_total_aex_count",
        "wiztree_canonical_non_generated_count",
        "wiztree_generated_target_artifact_count",
        "generated_target_artifacts_excluded",
        "candidates_present_in_refresh",
        "input_contains_forbidden_tokens",
        "blocked_reason_count",
    ] {
        assert!(
            string_array_contains(
                &schema["fixture_refresh_audit_summary_required_fields"],
                field
            ),
            "implementation manifest fixture refresh summary missing field {field}"
        );
    }
    let refresh_ready = &schema["fixture_refresh_audit_summary_ready_values_when_provided"];
    assert_eq!(
        refresh_ready["status"],
        "fixture_gate_refresh_ready_no_load"
    );
    assert_eq!(refresh_ready["native_load_performed"], false);
    assert_eq!(refresh_ready["render_performed"], false);
    assert_eq!(refresh_ready["fixture_selected"], false);
    assert_eq!(refresh_ready["loader_enabled"], false);
    assert_eq!(refresh_ready["fixture_gate_candidate_count"], 2);
    assert_eq!(refresh_ready["wiztree_total_aex_count"], 119);
    assert_eq!(refresh_ready["wiztree_canonical_non_generated_count"], 40);
    assert_eq!(refresh_ready["wiztree_generated_target_artifact_count"], 79);
    assert_eq!(refresh_ready["generated_target_artifacts_excluded"], true);
    assert_eq!(refresh_ready["candidates_present_in_refresh"], true);
    assert_eq!(refresh_ready["input_contains_forbidden_tokens"], false);
    assert_eq!(refresh_ready["blocked_reason_count"], 0);
    assert!(string_array_contains(
        &schema["conditional_check_names"],
        "fixture_gate_refresh_audit_ready_no_load"
    ));
    assert!(string_array_contains(
        &schema["readiness_summary_required_fields"],
        "pipl_content_scan_status"
    ));
    assert!(string_array_contains(
        &schema["readiness_summary_required_fields"],
        "pipl_content_scan_ready"
    ));
    assert!(string_array_contains(
        &schema["conditional_check_names"],
        "readiness_pipl_semantic_gate"
    ));
    assert_eq!(
        schema["required_ready_readiness_values_when_provided"]["pipl_content_scan_status"],
        "semantic_matches"
    );
    assert_eq!(
        schema["required_ready_readiness_values_when_provided"]["pipl_content_scan_ready"],
        true
    );
    let gate = &schema["implementation_gate_required_values"];
    assert_eq!(gate["native_loader_calls_allowed"], false);
    assert_eq!(gate["broker_may_load_aex"], false);
    assert_eq!(gate["ofx_facade_may_route_to_loader"], false);
    assert_eq!(gate["requires_explicit_user_approval"], true);
    assert!(string_array_contains(
        &schema["required_check_names"],
        "evidence_anti_contamination"
    ));
    for token in [
        "sha256",
        "base64",
        "loadlibrary",
        "libloading",
        "effectmain",
    ] {
        assert!(
            string_array_contains(&schema["forbidden_serialized_tokens"], token),
            "implementation manifest should forbid {token}"
        );
    }
    assert!(string_array_contains(
        &schema["required_notes"],
        "A ready manifest is permission to review a separate loader implementation slice, not permission to load a plugin."
    ));
}

#[test]
fn worker_loader_ticket_schema_is_worker_visible_but_no_load() {
    let Some(schema) = load_analysis_json(WORKER_LOADER_TICKET_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(string_field(&schema, "purpose").contains("worker-visible no-load ticket"));
    for field in [
        "schema_version",
        "ticket_protocol_version",
        "generated_by",
        "generated_unix_ms",
        "max_ticket_age_ms",
        "publication_status",
        "status",
        "native_load_performed",
        "worker_may_load_plugin",
        "broker_may_load_plugin",
        "allowlist_id",
        "operation",
        "selected_loader_entry",
        "required_runtime_evidence",
        "planned_stages",
        "denied_surfaces",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "worker loader ticket schema missing required field {field}"
        );
    }
    let required = &schema["required_values"];
    assert_eq!(required["status"], "accepted_no_load");
    assert_eq!(required["native_load_performed"], false);
    assert_eq!(required["worker_may_load_plugin"], false);
    assert_eq!(required["broker_may_load_plugin"], false);
    assert_eq!(required["operation"], "render_png");
    assert_eq!(
        schema["selected_loader_entry_required_values"]["path_match_status"],
        "matched_normalized_path"
    );
    assert_eq!(
        schema["selected_loader_entry_required_values"]["entry_ready"],
        true
    );
    for field in [
        "worker_identity_revalidation_required",
        "worker_attestation_required",
        "sandbox_preflight_required",
        "job_object_required",
        "handle_inheritance_required",
    ] {
        assert!(
            string_array_contains(&schema["required_runtime_evidence_fields"], field),
            "worker loader ticket runtime evidence missing field {field}"
        );
    }
    assert_eq!(
        schema["required_runtime_evidence_values"]["worker_identity_revalidation_required"],
        "passed"
    );
    assert_eq!(
        schema["required_runtime_evidence_values"]["worker_attestation_required"],
        "passed"
    );
    assert_eq!(
        schema["required_runtime_evidence_values"]["sandbox_preflight_required"],
        "passed"
    );
    assert_eq!(
        schema["required_runtime_evidence_values"]["job_object_required"],
        "assigned-with-kill-on-close"
    );
    assert_eq!(
        schema["required_runtime_evidence_values"]["handle_inheritance_required"],
        "sentinel_not_inherited-with-explicit-handle-list"
    );
    assert!(string_array_contains(
        &schema["planned_stage_required_fields"],
        "stage"
    ));
    assert!(string_array_contains(
        &schema["planned_stage_required_fields"],
        "status"
    ));
    for stage in [
        "load",
        "global_setup",
        "params_setup",
        "sequence_setup",
        "render",
        "sequence_teardown",
        "global_teardown",
    ] {
        assert!(string_array_contains(&schema["planned_stage_names"], stage));
    }
    assert_eq!(schema["planned_stage_status"], "planned_not_run");
    assert_eq!(schema["promotion_gate"]["worker_may_load_plugin"], false);
    assert_eq!(schema["promotion_gate"]["native_load_performed"], false);
    assert_eq!(
        schema["promotion_gate"]["requires_separate_loader_slice"],
        true
    );
    for token in [
        "sha256",
        "base64",
        "loadlibrary",
        "libloading",
        "effectmain",
    ] {
        assert!(string_array_contains(
            &schema["forbidden_serialized_tokens"],
            token
        ));
    }
}

#[test]
fn native_stage_plan_schema_maps_pf_selectors_without_execution() {
    let Some(schema) = load_analysis_json(NATIVE_STAGE_PLAN_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(string_field(&schema, "purpose").contains("no-load PF selector"));
    for field in [
        "schema_version",
        "publication_status",
        "status",
        "native_load_performed",
        "selectors_executed",
        "render_performed",
        "broker_may_load_plugin",
        "worker_may_load_plugin",
        "ofx_may_route_to_loader",
        "manifest_summary",
        "manifest_readiness_summary",
        "manifest_fixture_refresh_audit_summary",
        "ticket_summary",
        "ticket_runtime_evidence_summary",
        "host_struct_plan",
        "native_stage_order",
        "promotion_gate",
        "checks",
        "blocked_reasons",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "native stage plan schema missing required field {field}"
        );
    }
    let required = &schema["required_values"];
    assert_eq!(required["native_load_performed"], false);
    assert_eq!(required["selectors_executed"], false);
    assert_eq!(required["render_performed"], false);
    assert_eq!(required["broker_may_load_plugin"], false);
    assert_eq!(required["worker_may_load_plugin"], false);
    assert_eq!(required["ofx_may_route_to_loader"], false);
    assert_eq!(
        schema["ready_status"],
        "planned_native_stage_contract_no_load"
    );
    for selector in [
        "PF_Cmd_GLOBAL_SETUP",
        "PF_Cmd_PARAMS_SETUP",
        "PF_Cmd_SEQUENCE_SETUP",
        "PF_Cmd_FRAME_SETUP",
        "PF_Cmd_RENDER",
        "PF_Cmd_FRAME_SETDOWN",
        "PF_Cmd_SEQUENCE_SETDOWN",
        "PF_Cmd_GLOBAL_SETDOWN",
    ] {
        assert!(
            string_array_contains(&schema["pf_selector_names"], selector),
            "native stage plan schema missing selector {selector}"
        );
    }
    assert_eq!(schema["native_stage_status"], "planned_not_run");
    for host_struct in [
        "PF_InData",
        "PF_OutData",
        "PF_ParamDef[]",
        "PF_LayerDef source",
        "PF_LayerDef destination",
    ] {
        assert!(
            string_array_contains(&schema["host_struct_names"], host_struct),
            "native stage plan schema missing host struct {host_struct}"
        );
    }
    let gate = &schema["promotion_gate_required_values"];
    assert_eq!(gate["native_loader_calls_allowed"], false);
    assert_eq!(gate["worker_selector_calls_allowed"], false);
    assert_eq!(gate["worker_pixel_buffers_allowed"], false);
    assert_eq!(gate["ofx_facade_may_route_to_loader"], false);
    assert_eq!(gate["requires_separate_loader_slice"], true);
    assert!(string_array_contains(
        &schema["required_check_names"],
        "manifest_ticket_identity_match"
    ));
    assert!(string_array_contains(
        &schema["required_check_names"],
        "manifest_readiness_pipl_gate"
    ));
    assert!(string_array_contains(
        &schema["conditional_check_names"],
        "manifest_fixture_refresh_audit_ready_no_load"
    ));
    assert!(string_array_contains(
        &schema["required_check_names"],
        "worker_loader_ticket_runtime_evidence"
    ));
    assert!(string_array_contains(
        &schema["manifest_readiness_summary_required_fields"],
        "pipl_content_scan_status"
    ));
    assert!(string_array_contains(
        &schema["manifest_readiness_summary_required_fields"],
        "pipl_content_scan_ready"
    ));
    assert_eq!(
        schema["required_ready_manifest_readiness_values_when_provided"]
            ["pipl_content_scan_status"],
        "semantic_matches"
    );
    assert_eq!(
        schema["required_ready_manifest_readiness_values_when_provided"]["pipl_content_scan_ready"],
        true
    );
    for field in [
        "provided",
        "schema_version",
        "publication_status",
        "status",
        "native_load_performed",
        "render_performed",
        "fixture_selected",
        "loader_enabled",
        "fixture_gate_candidate_count",
        "wiztree_total_aex_count",
        "wiztree_canonical_non_generated_count",
        "wiztree_generated_target_artifact_count",
        "generated_target_artifacts_excluded",
        "candidates_present_in_refresh",
        "input_contains_forbidden_tokens",
        "blocked_reason_count",
    ] {
        assert!(
            string_array_contains(
                &schema["manifest_fixture_refresh_audit_summary_required_fields"],
                field
            ),
            "native stage plan fixture refresh summary missing field {field}"
        );
    }
    let refresh_ready =
        &schema["manifest_fixture_refresh_audit_summary_ready_values_when_provided"];
    assert_eq!(
        refresh_ready["status"],
        "fixture_gate_refresh_ready_no_load"
    );
    assert_eq!(refresh_ready["native_load_performed"], false);
    assert_eq!(refresh_ready["render_performed"], false);
    assert_eq!(refresh_ready["fixture_selected"], false);
    assert_eq!(refresh_ready["loader_enabled"], false);
    assert_eq!(refresh_ready["fixture_gate_candidate_count"], 2);
    assert_eq!(refresh_ready["wiztree_total_aex_count"], 119);
    assert_eq!(refresh_ready["wiztree_canonical_non_generated_count"], 40);
    assert_eq!(refresh_ready["wiztree_generated_target_artifact_count"], 79);
    assert_eq!(refresh_ready["generated_target_artifacts_excluded"], true);
    assert_eq!(refresh_ready["candidates_present_in_refresh"], true);
    assert_eq!(refresh_ready["input_contains_forbidden_tokens"], false);
    assert_eq!(refresh_ready["blocked_reason_count"], 0);
    for field in [
        "worker_identity_revalidation_required",
        "worker_attestation_required",
        "sandbox_preflight_required",
        "job_object_required",
        "handle_inheritance_required",
    ] {
        assert!(
            string_array_contains(
                &schema["ticket_runtime_evidence_summary_required_fields"],
                field
            ),
            "native stage plan runtime evidence summary missing field {field}"
        );
    }
    assert_eq!(
        schema["ticket_runtime_evidence_summary_required_values"]
            ["worker_identity_revalidation_required"],
        "passed"
    );
    assert_eq!(
        schema["ticket_runtime_evidence_summary_required_values"]["worker_attestation_required"],
        "passed"
    );
    assert_eq!(
        schema["ticket_runtime_evidence_summary_required_values"]["sandbox_preflight_required"],
        "passed"
    );
    assert_eq!(
        schema["ticket_runtime_evidence_summary_required_values"]["job_object_required"],
        "assigned-with-kill-on-close"
    );
    assert_eq!(
        schema["ticket_runtime_evidence_summary_required_values"]["handle_inheritance_required"],
        "sentinel_not_inherited-with-explicit-handle-list"
    );
    for field in [
        "schema_name",
        "schema_version",
        "publication_status",
        "compatibility_classification",
        "source_file_count",
        "allowed_planning_label_count",
        "allowed_metadata_label_count",
        "source_forbidden_substring_count",
        "required_source_boundary_notes_count",
        "native_loader_calls_allowed",
        "adobe_sdk_headers_allowed",
        "abi_generator_allowed",
        "third_party_effect_host_crate_allowed",
        "third_party_pipl_crate_allowed",
        "reuse_existing_aviutl_dynamic_loader_for_aex_allowed",
        "pf_names_are_planning_labels_only",
        "metadata_labels_do_not_define_abi",
        "worker_os_isolation_ffi_allowed",
    ] {
        assert!(
            string_array_contains(
                &schema["cleanroom_boundary_summary_required_fields_when_present"],
                field
            ),
            "native stage plan cleanroom boundary summary missing field {field}"
        );
    }
    assert_eq!(
        schema["cleanroom_boundary_summary_required_values_when_present"]
            ["native_loader_calls_allowed"],
        false
    );
    assert_eq!(
        schema["cleanroom_boundary_summary_required_values_when_present"]["abi_generator_allowed"],
        false
    );
    assert_eq!(
        schema["cleanroom_boundary_summary_required_values_when_present"]
            ["third_party_effect_host_crate_allowed"],
        false
    );
    assert_eq!(
        schema["cleanroom_boundary_summary_required_values_when_present"]
            ["reuse_existing_aviutl_dynamic_loader_for_aex_allowed"],
        false
    );
    assert!(string_array_contains(
        &schema["conditional_check_names"],
        "host_vocabulary_boundary_no_loader_or_sdk"
    ));
    assert!(string_array_contains(
        &schema["required_notes"],
        "PF selector names are planning labels only; every selector remains planned_not_run."
    ));
}

#[test]
fn host_vocabulary_boundary_schema_keeps_aex_cleanroom_guard_visible() {
    let Some(schema) = load_analysis_json(HOST_VOCABULARY_BOUNDARY_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "schema_name"),
        "AEX host vocabulary cleanroom boundary"
    );
    assert_eq!(
        string_field(&schema, "publication_status"),
        "local-only design artifact"
    );
    assert_eq!(
        string_field(&schema, "compatibility_classification"),
        "Cleanroom planning vocabulary guard"
    );
    assert_eq!(
        schema["allowed_policy"]["native_loader_calls_allowed"],
        false
    );
    assert_eq!(schema["allowed_policy"]["adobe_sdk_headers_allowed"], false);
    assert_eq!(schema["allowed_policy"]["bindgen_allowed"], false);
    assert_eq!(
        schema["allowed_policy"]["third_party_after_effects_crate_allowed"],
        false
    );
    assert_eq!(
        schema["allowed_policy"]["third_party_pipl_crate_allowed"],
        false
    );
    assert_eq!(
        schema["allowed_policy"]["reuse_existing_aviutl_libloading_path_for_aex_allowed"],
        false
    );
    assert_eq!(
        schema["allowed_policy"]["pf_names_are_planning_labels_only"],
        true
    );
    assert_eq!(
        schema["allowed_policy"]["metadata_labels_do_not_define_abi"],
        true
    );
    assert_eq!(
        schema["allowed_policy"]["worker_os_isolation_ffi_allowed"],
        true
    );

    for source in [
        "aviutl-rs/examples/aex_static_classifier.rs",
        "aviutl-rs/examples/aex_probe_readiness.rs",
        "aviutl-rs/examples/aex_loader_preflight.rs",
        "aviutl-rs/examples/aex_loader_implementation_manifest.rs",
        "aviutl-rs/examples/aex_image_probe.rs",
        "aviutl-rs/examples/aex_probe_fixture_images.rs",
        "aviutl-rs/examples/aex_effect_worker_stub.rs",
        "aviutl-rs/examples/aex_native_stage_plan.rs",
        "aviutl-rs/examples/ofx_aex_facade_readiness.rs",
        "aviutl-rs/examples/aex_no_load_provenance_audit.rs",
        "aviutl-rs/examples/aex_fixture_gate_refresh_audit.rs",
        "aviutl-rs/examples/aex_loader_slice_review_packet.rs",
        "aviutl-rs/examples/aex_loader_approval_receipt.rs",
    ] {
        assert!(
            string_array_contains(&schema["source_files"], source),
            "host vocabulary schema should scan {source}"
        );
    }

    for label in [
        "PF_Cmd_GLOBAL_SETUP",
        "PF_Cmd_PARAMS_SETUP",
        "PF_Cmd_SEQUENCE_SETUP",
        "PF_Cmd_FRAME_SETUP",
        "PF_Cmd_RENDER",
        "PF_Cmd_FRAME_SETDOWN",
        "PF_Cmd_SEQUENCE_SETDOWN",
        "PF_Cmd_GLOBAL_SETDOWN",
        "PF_InData",
        "PF_OutData",
        "PF_ParamDef[]",
        "PF_LayerDef source",
        "PF_LayerDef destination",
    ] {
        assert!(
            string_array_contains(&schema["allowed_planning_labels"], label),
            "planning label {label} should be allowlisted as label-only"
        );
    }

    for label in [
        "EffectMain",
        "AEEffect",
        "PIPLType::AEEffect",
        "Property::AE_Effect_Match_Name",
        "Property::CodeWin64X86",
    ] {
        assert!(
            string_array_contains(&schema["allowed_metadata_labels"], label),
            "metadata label {label} should be explicit"
        );
    }

    for token in [
        "libloading",
        "LoadLibrary",
        "LoadLibraryEx",
        "GetProcAddress",
        "FreeLibrary",
        "windows::Win32::System::LibraryLoader",
        "bindgen",
        "after_effects",
        "pipl::",
        "repr(C)",
        "extern \"C\" fn EffectMain",
        "extern \"system\" fn EffectMain",
        "struct PF_InData",
        "struct PF_OutData",
        "struct PF_ParamDef",
        "struct PF_LayerDef",
        "std::mem::transmute",
    ] {
        assert!(
            string_array_contains(&schema["source_forbidden_substrings"], token),
            "source forbidden token {token} should stay guarded"
        );
    }

    for note in [
        "PF_* names may appear only as no-load planning labels, not struct definitions or ABI layouts.",
        "EffectMain and AEEffect may appear only as static metadata labels, not callable symbols.",
        "AEX loader work requires a separate reviewed loader slice with explicit user approval.",
    ] {
        assert!(string_array_contains(
            &schema["required_source_boundary_notes"],
            note
        ));
    }
}

#[test]
fn aex_worker_report_schema_keeps_unsupported_features_explicit() {
    let Some(schema) = load_analysis_json(WORKER_REPORT_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "publication_status"),
        "local-only design artifact"
    );
    let capability = &schema["capability"];
    assert!(capability["plugin_class"]
        .as_str()
        .unwrap_or_default()
        .contains("classic-effect | aegp | aeio | smartfx | unknown"));
    assert!(capability["load_status"]
        .as_str()
        .unwrap_or_default()
        .contains("not_loaded | loaded | failed | timeout | crash"));
    assert_eq!(capability["pixel_formats"][0], "rgba8");

    let unsupported = capability["unsupported"]
        .as_array()
        .expect("unsupported list should be present");
    for feature in ["SmartFX", "GPU", "AEGP suites", "AEIO", "custom UI"] {
        assert!(
            unsupported.iter().any(|item| item == feature),
            "unsupported feature should stay explicit: {feature}"
        );
    }
    assert!(schema["notes"]
        .as_array()
        .expect("notes should be an array")
        .iter()
        .any(|note| note
            .as_str()
            .unwrap_or_default()
            .contains("Unsupported features must be reported instead of faked.")));
}

#[test]
fn external_effect_capability_schema_defaults_to_worker_isolation() {
    let Some(schema) = load_analysis_json(EXTERNAL_CAPABILITY_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "publication_status"),
        "local-only design artifact"
    );
    let effect = &schema["effect"];
    assert_eq!(effect["frame"]["preferred_pixel_format"], "rgba8");
    assert_eq!(effect["frame"]["transport"][0], "png-v0");
    assert_eq!(effect["execution"]["worker_required"], true);
    assert_eq!(effect["execution"]["broker_may_load_plugin"], false);
    assert_eq!(
        effect["execution"]["allowlist_status"]
            .as_str()
            .unwrap_or_default(),
        "required | allowed | denied | not-required"
    );
    assert_eq!(effect["host_surfaces"]["aex_worker"]["supported"], false);
    assert_eq!(effect["host_surfaces"]["ofx_facade"]["supported"], false);
    assert_eq!(
        effect["risk"]["publication"],
        "do-not-publish-binary | public-candidate | unknown"
    );
}

#[test]
fn ofx_strategy_cannot_bypass_aex_worker_gates() {
    let Some(strategy) = load_analysis_text(OFX_STRATEGY) else {
        return;
    };

    for expected in [
        "OFX remains deferred",
        "same broker/worker/sandbox contract",
        "same allowlist, loader approval, worker identity revalidation, and sandbox preflight gates",
        "must not bypass those gates",
        "must not load `.aex` inside an OFX host process",
        "must not become AviUtlas's route to AEX",
    ] {
        assert!(strategy.contains(expected), "OFX strategy missing {expected:?}");
    }
}

#[test]
fn ofx_facade_contract_is_deferred_and_cannot_bypass_aex_gates() {
    let Some(contract) = load_analysis_json(OFX_FACADE_CONTRACT) else {
        return;
    };

    assert_eq!(contract["schema_version"], 1);
    assert_eq!(string_field(&contract, "status"), "deferred-contract-only");
    assert_eq!(
        contract["route"]["aviutlas_to_aex_route"],
        "direct-aex-broker-not-ofx"
    );
    assert_eq!(contract["route"]["ofx_host_may_load_aex"], false);
    assert_eq!(contract["route"]["ofx_adapter_may_load_aex"], false);
    assert_eq!(contract["route"]["broker_may_load_aex"], false);
    assert_eq!(contract["route"]["worker_required"], true);
    assert_eq!(
        contract["route"]["first_loader_owner"],
        "aex-worker-loader-slice-not-ofx"
    );

    let gates = &contract["required_shared_gates"];
    assert_eq!(gates["allowlist"], "same-aex-allowlist-required");
    assert_eq!(gates["loader_approval"], "same-loader-approval-required");
    assert_eq!(gates["worker_identity_revalidation"], "passed-required");
    assert_eq!(gates["sandbox_preflight"], "passed-required");
    assert_eq!(gates["worker_attestation"], "passed-required");
    assert_eq!(gates["job_object"], "assigned-with-kill-on-close-required");
    assert_eq!(
        gates["handle_inheritance"],
        "sentinel_not_inherited-with-explicit-handle-list-required"
    );
    assert_eq!(
        gates["loader_gate"],
        "separate-loader-slice-must-open-before-render"
    );
    let ofx_review = &contract["ofx_facade_review_gate"];
    assert_eq!(ofx_review["status"], "not_reviewed");
    assert_eq!(ofx_review["approved"], false);
    assert_eq!(ofx_review["may_point_to_broker"], false);
    assert_eq!(ofx_review["may_issue_describe"], false);
    assert_eq!(ofx_review["may_issue_render_png"], false);
    assert_eq!(
        ofx_review["requires_separate_review_after_aex_loader_gate"],
        true
    );
    assert_eq!(ofx_review["requires_same_shared_gates"], true);

    for forbidden in [
        "OFX host process loads .aex",
        "OFX adapter process loads .aex",
        "AviUtlas routes through OFX to reach AEX",
        "OFX adapter bypasses AEX allowlist",
        "OFX SDK or header code is vendored before license audit",
    ] {
        assert!(
            string_array_contains(&contract["forbidden_paths"], forbidden),
            "OFX facade contract should forbid {forbidden}"
        );
    }

    assert!(contract["current_supported_operations"]
        .as_array()
        .expect("current operation list should be present")
        .is_empty());
    assert_eq!(contract["future_operations"][0]["name"], "describe");
    assert_eq!(
        contract["future_operations"][1]["requires_loader_enabled"],
        true
    );
}

#[test]
fn ofx_facade_readiness_schema_keeps_native_stage_plan_summary_no_bypass() {
    let Some(schema) = load_analysis_json(OFX_FACADE_READINESS_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(
        string_field(&schema, "purpose").contains("no-load OFX facade readiness report"),
        "OFX readiness schema should stay no-load"
    );
    for field in [
        "schema_version",
        "publication_status",
        "status",
        "contract_status",
        "ofx_host_may_load_aex",
        "ofx_adapter_may_load_aex",
        "aviutlas_may_route_through_ofx_to_reach_aex",
        "broker_may_load_aex",
        "fixture_gate",
        "loader_gate",
        "native_stage_plan_summary",
        "loader_slice_review_summary",
        "loader_approval_summary",
        "aex_metadata_gate_summary",
        "ofx_facade_review_gate",
        "describe_exposure",
        "render_png_exposure",
        "entries",
        "forbidden_paths",
        "blocked_reasons",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "OFX readiness schema missing field {field}"
        );
    }

    let required = &schema["required_values"];
    assert_eq!(required["publication_status"], "local-only");
    assert_eq!(required["contract_status"], "deferred-contract-only");
    assert_eq!(required["ofx_host_may_load_aex"], false);
    assert_eq!(required["ofx_adapter_may_load_aex"], false);
    assert_eq!(
        required["aviutlas_may_route_through_ofx_to_reach_aex"],
        false
    );
    assert_eq!(required["broker_may_load_aex"], false);
    assert_eq!(required["worker_required"], true);
    assert_eq!(
        required["first_loader_owner"],
        "aex-worker-loader-slice-not-ofx"
    );
    assert!(required["current_supported_operations"]
        .as_array()
        .expect("current_supported_operations should be an array")
        .is_empty());

    for field in [
        "provided",
        "status",
        "no_load_stage_plan_ready",
        "worker_runtime_evidence_ready",
        "cleanroom_boundary_provided",
        "cleanroom_boundary_no_loader_or_sdk",
        "selector_execution_blocked",
        "render_blocked",
        "ofx_route_blocked",
        "native_stage_count",
        "planned_not_run_count",
        "input_contains_forbidden_tokens",
    ] {
        assert!(
            string_array_contains(&schema["native_stage_plan_summary_required_fields"], field),
            "OFX native-stage summary missing field {field}"
        );
    }
    let native_summary = &schema["native_stage_plan_summary_required_values_when_provided"];
    assert_eq!(
        native_summary["status"],
        "planned_native_stage_contract_no_load"
    );
    assert_eq!(native_summary["no_load_stage_plan_ready"], true);
    assert_eq!(native_summary["worker_runtime_evidence_ready"], true);
    assert_eq!(native_summary["selector_execution_blocked"], true);
    assert_eq!(native_summary["render_blocked"], true);
    assert_eq!(native_summary["ofx_route_blocked"], true);
    assert_eq!(native_summary["input_contains_forbidden_tokens"], false);

    for field in [
        "provided",
        "status",
        "report_kind",
        "publication_status",
        "ready_for_ofx_review_no_load",
        "native_load_performed",
        "render_performed",
        "aex_loaded",
        "worker_started",
        "broker_load_or_render_allowed",
        "ofx_route_allowed",
        "ae_invoked",
        "private_payload_copied",
        "fixture_gate_status",
        "loader_gate_status",
        "identity_smoke_aex_render_correctness_evidence",
        "ofx_contract_status",
        "blocked_reason_count",
        "input_contains_forbidden_tokens",
    ] {
        assert!(
            string_array_contains(&schema["aex_metadata_gate_summary_required_fields"], field),
            "OFX AEX metadata gate summary missing field {field}"
        );
    }
    let metadata_summary = &schema["aex_metadata_gate_summary_required_values_when_provided"];
    assert_eq!(
        metadata_summary["status"],
        "aex_metadata_gate_ready_no_load"
    );
    assert_eq!(metadata_summary["ready_for_ofx_review_no_load"], true);
    assert_eq!(metadata_summary["native_load_performed"], false);
    assert_eq!(metadata_summary["render_performed"], false);
    assert_eq!(metadata_summary["aex_loaded"], false);
    assert_eq!(metadata_summary["ofx_route_allowed"], false);
    assert_eq!(
        metadata_summary["identity_smoke_aex_render_correctness_evidence"],
        false
    );
    assert_eq!(
        metadata_summary["ofx_contract_status"],
        "deferred-contract-only"
    );

    let review_gate = &schema["ofx_facade_review_gate_required_values"];
    assert_eq!(review_gate["approved"], false);
    assert_eq!(review_gate["may_point_to_broker"], false);
    assert_eq!(review_gate["may_issue_describe"], false);
    assert_eq!(review_gate["may_issue_render_png"], false);
    assert_eq!(
        review_gate["requires_separate_review_after_aex_loader_gate"],
        true
    );
    assert_eq!(review_gate["requires_same_shared_gates"], true);

    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "deferred_contract_only"
    ));
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "blocked_contract_mismatch"
    ));
    assert!(string_array_contains(
        &schema["allowed_render_png_exposures"],
        "blocked_loader_gate_closed"
    ));
    assert!(string_array_contains(
        &schema["allowed_render_png_exposures"],
        "blocked_pending_separate_ofx_review"
    ));
    let output_boundary = &schema["report_output_boundary"];
    assert_eq!(
        output_boundary["required_output_root"],
        "target/aex-ofx-facade-readiness"
    );
    assert_eq!(output_boundary["required_extension"], ".json");
    assert_eq!(output_boundary["create_new_only"], true);
    assert_eq!(output_boundary["path_traversal_allowed"], false);
    assert_eq!(
        output_boundary["canonical_parent_must_resolve_under_output_root"],
        true
    );
    for token in [
        "sha256",
        "base64",
        "loadlibrary",
        "libloading",
        "effectmain",
        "output_png",
        "input_png",
        "rendered_pixels",
    ] {
        assert!(
            string_array_contains(&schema["forbidden_serialized_tokens"], token),
            "OFX readiness schema should forbid {token}"
        );
    }
    for note in [
        "OFX facade readiness reads JSON metadata only.",
        "No OFX SDK, OFX host process, .aex load, worker describe, or render is used.",
        "AviUtlas must keep the direct AEX broker path as the first loader route.",
    ] {
        assert!(string_array_contains(&schema["required_notes"], note));
    }
}

#[test]
fn aex_no_load_provenance_audit_schema_pins_pipeline_no_load() {
    let Some(schema) = load_analysis_json(AEX_NO_LOAD_PROVENANCE_AUDIT_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(
        string_field(&schema, "purpose")
            .contains("loader manifest -> native stage plan -> OFX readiness"),
        "provenance audit schema should describe the full no-load chain"
    );
    for field in [
        "schema_version",
        "publication_status",
        "status",
        "native_load_performed",
        "selectors_executed",
        "render_performed",
        "ofx_route_allowed",
        "evidence_contains_forbidden_tokens",
        "loader_manifest_summary",
        "native_stage_plan_summary",
        "ofx_readiness_summary",
        "fixture_identity_smoke_summary",
        "checks",
        "blocked_reasons",
        "notes",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "provenance audit schema missing field {field}"
        );
    }

    let required = &schema["required_values"];
    assert_eq!(required["publication_status"], "local-only");
    assert_eq!(required["native_load_performed"], false);
    assert_eq!(required["selectors_executed"], false);
    assert_eq!(required["render_performed"], false);
    assert_eq!(required["ofx_route_allowed"], false);
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "no_load_provenance_chain_ready"
    ));
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "blocked_no_load_provenance_chain"
    ));

    for field in [
        "status",
        "native_load_performed",
        "loader_may_load_plugin",
        "ofx_may_route_to_loader",
        "readiness_provided",
        "readiness_pipl_content_scan_status",
        "readiness_pipl_content_scan_ready",
        "readiness_allows_describe",
        "fixture_refresh_audit_provided",
        "fixture_refresh_audit_status",
        "fixture_refresh_audit_native_load_performed",
        "fixture_refresh_audit_render_performed",
        "fixture_refresh_audit_fixture_selected",
        "fixture_refresh_audit_loader_enabled",
        "fixture_refresh_audit_generated_target_artifacts_excluded",
        "fixture_refresh_audit_candidates_present_in_refresh",
        "fixture_refresh_audit_input_contains_forbidden_tokens",
        "fixture_refresh_audit_blocked_reason_count",
    ] {
        assert!(
            string_array_contains(&schema["loader_manifest_summary_required_fields"], field),
            "loader manifest summary missing field {field}"
        );
    }
    let loader_ready = &schema["loader_manifest_summary_required_ready_values"];
    assert_eq!(
        loader_ready["status"],
        "ready_for_separate_loader_implementation_review_no_load"
    );
    assert_eq!(loader_ready["native_load_performed"], false);
    assert_eq!(loader_ready["loader_may_load_plugin"], false);
    assert_eq!(loader_ready["ofx_may_route_to_loader"], false);
    assert_eq!(loader_ready["readiness_provided"], true);
    assert_eq!(
        loader_ready["readiness_pipl_content_scan_status"],
        "semantic_matches"
    );
    assert_eq!(loader_ready["readiness_pipl_content_scan_ready"], true);

    for field in [
        "status",
        "native_load_performed",
        "selectors_executed",
        "render_performed",
        "worker_may_load_plugin",
        "ofx_may_route_to_loader",
        "worker_runtime_evidence_ready",
        "cleanroom_boundary_provided",
        "cleanroom_boundary_no_loader_or_sdk",
        "fixture_refresh_audit_provided",
        "fixture_refresh_audit_status",
        "fixture_refresh_audit_native_load_performed",
        "fixture_refresh_audit_render_performed",
        "fixture_refresh_audit_fixture_selected",
        "fixture_refresh_audit_loader_enabled",
        "fixture_refresh_audit_generated_target_artifacts_excluded",
        "fixture_refresh_audit_candidates_present_in_refresh",
        "fixture_refresh_audit_input_contains_forbidden_tokens",
        "fixture_refresh_audit_blocked_reason_count",
    ] {
        assert!(
            string_array_contains(&schema["native_stage_plan_summary_required_fields"], field),
            "native stage summary missing field {field}"
        );
    }
    let native_ready = &schema["native_stage_plan_summary_required_ready_values"];
    assert_eq!(
        native_ready["status"],
        "planned_native_stage_contract_no_load"
    );
    assert_eq!(native_ready["native_load_performed"], false);
    assert_eq!(native_ready["selectors_executed"], false);
    assert_eq!(native_ready["render_performed"], false);
    assert_eq!(native_ready["worker_runtime_evidence_ready"], true);
    assert_eq!(native_ready["cleanroom_boundary_provided"], true);
    assert_eq!(native_ready["cleanroom_boundary_no_loader_or_sdk"], true);
    let refresh_ready = &schema["fixture_refresh_audit_summary_ready_values_when_provided"];
    assert_eq!(
        refresh_ready["fixture_refresh_audit_status"],
        "fixture_gate_refresh_ready_no_load"
    );
    assert_eq!(
        refresh_ready["fixture_refresh_audit_native_load_performed"],
        false
    );
    assert_eq!(
        refresh_ready["fixture_refresh_audit_render_performed"],
        false
    );
    assert_eq!(
        refresh_ready["fixture_refresh_audit_fixture_selected"],
        false
    );
    assert_eq!(refresh_ready["fixture_refresh_audit_loader_enabled"], false);
    assert_eq!(
        refresh_ready["fixture_refresh_audit_wiztree_canonical_non_generated_count"],
        40
    );
    assert_eq!(
        refresh_ready["fixture_refresh_audit_wiztree_generated_target_artifact_count"],
        79
    );

    for field in [
        "status",
        "contract_status",
        "ofx_host_may_load_aex",
        "ofx_adapter_may_load_aex",
        "broker_may_load_aex",
        "aviutlas_may_route_through_ofx_to_reach_aex",
        "ofx_facade_review_gate_approved",
        "native_stage_plan_summary_provided",
        "native_stage_plan_no_load_ready",
        "native_stage_runtime_evidence_ready",
        "native_stage_ofx_route_blocked",
        "native_stage_input_contains_forbidden_tokens",
    ] {
        assert!(
            string_array_contains(&schema["ofx_readiness_summary_required_fields"], field),
            "OFX readiness summary missing field {field}"
        );
    }
    let ofx_ready = &schema["ofx_readiness_summary_required_ready_values"];
    assert_eq!(ofx_ready["status"], "deferred_contract_only");
    assert_eq!(ofx_ready["contract_status"], "deferred-contract-only");
    assert_eq!(ofx_ready["ofx_host_may_load_aex"], false);
    assert_eq!(ofx_ready["ofx_adapter_may_load_aex"], false);
    assert_eq!(ofx_ready["broker_may_load_aex"], false);
    assert_eq!(
        ofx_ready["aviutlas_may_route_through_ofx_to_reach_aex"],
        false
    );
    assert_eq!(ofx_ready["native_stage_plan_summary_provided"], true);
    assert_eq!(ofx_ready["native_stage_plan_no_load_ready"], true);
    assert_eq!(ofx_ready["native_stage_ofx_route_blocked"], true);

    for field in [
        "provided",
        "schema_version",
        "publication_status",
        "status",
        "fixture_manifest_status",
        "transport_operation",
        "pixel_format",
        "image_count",
        "transport_count",
        "identity_pixels_checked_count",
        "native_load_performed",
        "render_performed",
        "aex_loaded",
        "worker_started",
        "broker_invoked",
        "ofx_route_invoked",
        "ae_invoked",
        "private_payload_copied",
        "aex_render_correctness_evidence",
        "entry_count",
        "expected_synthetic_image_set",
        "all_entries_identity_transport_ok",
        "all_entries_identity_pixels_match",
        "all_entries_no_worker_or_aex_or_render",
        "required_check_pass_count",
        "required_checks_passed",
        "all_checks_passed",
        "synthetic_fixture_pixels_checked",
        "blocked_reason_count",
        "input_contains_forbidden_tokens",
        "sanitized_summary_contains_forbidden_tokens",
    ] {
        assert!(
            string_array_contains(
                &schema["fixture_identity_smoke_summary_required_fields"],
                field
            ),
            "fixture identity smoke summary missing field {field}"
        );
    }
    let smoke_ready = &schema["fixture_identity_smoke_summary_ready_values_when_provided"];
    assert_eq!(
        smoke_ready["status"],
        "fixture_identity_smoke_ready_no_load"
    );
    assert_eq!(
        smoke_ready["fixture_manifest_status"],
        "synthetic_fixture_images_ready_no_load"
    );
    assert_eq!(smoke_ready["transport_operation"], "identity_transport");
    assert_eq!(smoke_ready["image_count"], 3);
    assert_eq!(smoke_ready["broker_invoked"], true);
    assert_eq!(smoke_ready["aex_render_correctness_evidence"], false);
    assert_eq!(smoke_ready["expected_synthetic_image_set"], true);
    assert_eq!(smoke_ready["all_entries_identity_pixels_match"], true);
    assert_eq!(smoke_ready["required_check_pass_count"], 7);
    assert_eq!(smoke_ready["input_contains_forbidden_tokens"], false);
    assert!(
        string_field(&schema, "fixture_identity_smoke_input_path_field_exception")
            .contains("never serializes those path fields downstream"),
        "schema should document the optional smoke input path-field exception"
    );

    for check in [
        "loader_manifest_ready_no_load",
        "native_stage_plan_ready_no_load",
        "native_stage_runtime_and_cleanroom_ready",
        "ofx_readiness_deferred_no_bypass",
        "ofx_readiness_consumed_native_stage_plan",
        "evidence_anti_contamination",
    ] {
        assert!(
            string_array_contains(&schema["required_check_names"], check),
            "provenance audit schema missing check {check}"
        );
    }
    assert!(string_array_contains(
        &schema["conditional_check_names"],
        "fixture_refresh_audit_preserved_no_load"
    ));
    assert!(string_array_contains(
        &schema["conditional_check_names"],
        "fixture_identity_smoke_ready_no_load"
    ));
    for token in [
        "sha256",
        "base64",
        "loadlibrary",
        "libloading",
        "effectmain",
        "output_png",
        "input_png",
        "rendered_pixels",
    ] {
        assert!(
            string_array_contains(&schema["forbidden_serialized_tokens"], token),
            "provenance audit schema should forbid {token}"
        );
    }
    let output_boundary = &schema["report_output_boundary"];
    assert_eq!(
        output_boundary["required_output_root"],
        "target/aex-no-load-provenance-audit"
    );
    assert_eq!(output_boundary["required_extension"], ".json");
    assert_eq!(output_boundary["create_new_only"], true);
    assert_eq!(output_boundary["path_traversal_allowed"], false);
    assert_eq!(
        output_boundary["canonical_parent_must_resolve_under_output_root"],
        true
    );
    for note in [
        "Provenance audit reads JSON metadata only.",
        "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.",
        "A ready audit is not loader approval and does not permit OFX routing.",
        "Optional fixture identity smoke evidence is summarized without PNG path fields and is not AEX render correctness evidence.",
    ] {
        assert!(string_array_contains(&schema["required_notes"], note));
    }
}

#[test]
fn aex_loader_slice_review_schema_pins_manual_handoff_no_load() {
    let Some(schema) = load_analysis_json(AEX_LOADER_SLICE_REVIEW_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(
        string_field(&schema, "purpose").contains("separate AEX loader implementation review"),
        "loader slice review schema should describe the handoff boundary"
    );
    assert!(
        string_field(&schema, "purpose").contains("fixture gate approval evidence"),
        "loader slice review schema should require fixture gate approval evidence"
    );
    for field in [
        "schema_version",
        "publication_status",
        "status",
        "native_load_performed",
        "loader_slice_approved",
        "loader_enabled",
        "real_aex_load_enabled",
        "native_loader_calls_allowed",
        "broker_may_load_aex",
        "worker_may_load_plugin",
        "render_performed",
        "ofx_route_allowed",
        "input_contains_forbidden_tokens",
        "fixture_gate_summary",
        "manifest_summary",
        "provenance_summary",
        "review_requirements",
        "checks",
        "blocked_reasons",
        "notes",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "loader slice review schema missing field {field}"
        );
    }

    let required = &schema["required_values"];
    assert_eq!(required["publication_status"], "local-only");
    assert_eq!(required["native_load_performed"], false);
    assert_eq!(required["loader_slice_approved"], false);
    assert_eq!(required["loader_enabled"], false);
    assert_eq!(required["real_aex_load_enabled"], false);
    assert_eq!(required["native_loader_calls_allowed"], false);
    assert_eq!(required["broker_may_load_aex"], false);
    assert_eq!(required["worker_may_load_plugin"], false);
    assert_eq!(required["render_performed"], false);
    assert_eq!(required["ofx_route_allowed"], false);
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "ready_for_manual_loader_slice_review_no_load"
    ));
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "blocked_loader_slice_review_packet"
    ));

    for field in [
        "schema_version",
        "status",
        "publication_status",
        "selected_fixture_present",
        "recommended_first_review_present",
        "approval_approved",
        "approval_loader_enabled",
        "approval_real_aex_load_enabled",
        "approval_render_png_enabled",
        "approval_describe_enabled_for_real_aex",
        "candidate_count",
        "local_build_candidate_count",
        "generated_target_candidate_count",
        "selected_candidate_present",
        "selected_candidate_review_status",
        "selected_candidate_fixture_status",
        "selected_candidate_plugin_class",
        "selected_candidate_source_license_evidence_present",
        "selected_candidate_blocked_reason_count",
        "single_fixture_policy_ready",
        "runtime_evidence_ready",
        "input_contains_forbidden_tokens",
    ] {
        assert!(
            string_array_contains(&schema["fixture_gate_summary_required_fields"], field),
            "loader slice fixture gate summary missing field {field}"
        );
    }
    let fixture_gate_ready = &schema["fixture_gate_summary_ready_values"];
    assert_eq!(
        fixture_gate_ready["status"],
        "review_queue_approved_local_only"
    );
    assert_eq!(fixture_gate_ready["selected_fixture_present"], true);
    assert_eq!(fixture_gate_ready["approval_approved"], true);
    assert_eq!(fixture_gate_ready["approval_loader_enabled"], true);
    assert_eq!(fixture_gate_ready["approval_real_aex_load_enabled"], true);
    assert_eq!(
        fixture_gate_ready["selected_candidate_review_status"],
        "approved-local-only"
    );
    assert_eq!(
        fixture_gate_ready["selected_candidate_fixture_status"],
        "local-build-candidate"
    );
    assert_eq!(
        fixture_gate_ready["selected_candidate_plugin_class"],
        "classic-effect-candidate"
    );
    assert_eq!(fixture_gate_ready["generated_target_candidate_count"], 0);
    assert_eq!(fixture_gate_ready["runtime_evidence_ready"], true);
    assert_eq!(fixture_gate_ready["input_contains_forbidden_tokens"], false);

    for field in [
        "status",
        "native_load_performed",
        "broker_may_load_plugin",
        "loader_may_load_plugin",
        "ofx_may_route_to_loader",
        "selected_fixture_present",
        "selected_effect_id_present",
        "selected_plugin_path_redacted",
        "ready_for_separate_loader_slice_review",
        "native_loader_calls_allowed",
        "broker_may_load_aex",
        "ofx_facade_may_route_to_loader",
        "requires_explicit_user_approval",
        "requires_code_review",
        "requires_local_fixture_only",
        "readiness_provided",
        "readiness_status",
        "blocked_reason_count",
        "input_contains_forbidden_tokens",
    ] {
        assert!(
            string_array_contains(&schema["manifest_summary_required_fields"], field),
            "loader slice manifest summary missing field {field}"
        );
    }
    let manifest_ready = &schema["manifest_summary_ready_values"];
    assert_eq!(
        manifest_ready["status"],
        "ready_for_separate_loader_implementation_review_no_load"
    );
    assert_eq!(manifest_ready["native_load_performed"], false);
    assert_eq!(manifest_ready["broker_may_load_aex"], false);
    assert_eq!(manifest_ready["native_loader_calls_allowed"], false);
    assert_eq!(manifest_ready["requires_explicit_user_approval"], true);
    assert_eq!(
        manifest_ready["readiness_status"],
        "probe_readiness_planned"
    );

    for field in [
        "status",
        "native_load_performed",
        "selectors_executed",
        "render_performed",
        "ofx_route_allowed",
        "evidence_contains_forbidden_tokens",
        "loader_manifest_ready",
        "native_stage_plan_ready",
        "ofx_readiness_ready",
        "runtime_and_cleanroom_ready",
        "fixture_identity_smoke_provided",
        "fixture_identity_smoke_ready",
        "fixture_identity_smoke_broker_invoked",
        "fixture_identity_smoke_aex_render_correctness_evidence",
        "fixture_identity_smoke_input_contains_forbidden_tokens",
        "blocked_reason_count",
        "input_contains_forbidden_tokens",
    ] {
        assert!(
            string_array_contains(&schema["provenance_summary_required_fields"], field),
            "loader slice provenance summary missing field {field}"
        );
    }
    let provenance_ready = &schema["provenance_summary_ready_values"];
    assert_eq!(provenance_ready["status"], "no_load_provenance_chain_ready");
    assert_eq!(provenance_ready["native_load_performed"], false);
    assert_eq!(provenance_ready["selectors_executed"], false);
    assert_eq!(provenance_ready["render_performed"], false);
    assert_eq!(provenance_ready["ofx_route_allowed"], false);
    assert_eq!(provenance_ready["ofx_readiness_ready"], true);
    assert_eq!(provenance_ready["runtime_and_cleanroom_ready"], true);

    for (field, expected) in schema["review_requirements_required_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &schema["review_requirements_required_values"][field], expected,
            "review requirement {field} should be pinned"
        );
    }
    assert_eq!(
        schema["fixture_identity_smoke_ready_values_when_provided"]
            ["fixture_identity_smoke_aex_render_correctness_evidence"],
        false
    );
    for check in [
        "fixture_gate_manual_approval_ready_no_load",
        "loader_manifest_ready_no_load",
        "provenance_chain_ready_no_load",
        "manual_review_requirements_closed",
        "no_execution_or_route_permission",
        "evidence_anti_contamination",
    ] {
        assert!(
            string_array_contains(&schema["required_check_names"], check),
            "loader slice review schema missing check {check}"
        );
    }
    assert!(string_array_contains(
        &schema["conditional_check_names"],
        "fixture_identity_smoke_preserved_no_load"
    ));
    for token in [
        "sha256",
        "base64",
        "loadlibrary",
        "libloading",
        "effectmain",
        "aeeffect",
        "input_png",
        "output_png",
        "rendered_pixels",
        "worker_exe",
        "binary_payload",
    ] {
        assert!(
            string_array_contains(&schema["forbidden_serialized_tokens"], token),
            "loader slice review schema should forbid {token}"
        );
    }
    assert!(
        string_field(&schema, "fixture_gate_input_metadata_exception")
            .contains("EffectMain/AEEffect static metadata"),
        "loader slice schema should document fixture gate metadata sanitization"
    );
    for note in [
        "Loader slice review packet reads JSON metadata only.",
        "No .aex file is opened, hashed, copied, loaded, executed, described, or rendered.",
        "This packet is not loader approval and does not permit worker, broker, or OFX execution.",
        "Private plugin paths from upstream manifests are not serialized in this packet.",
    ] {
        assert!(string_array_contains(&schema["required_notes"], note));
    }
}

#[test]
fn aex_loader_approval_receipt_schema_requires_explicit_review_only_receipt() {
    let Some(schema) = load_analysis_json(AEX_LOADER_APPROVAL_RECEIPT_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(
        string_field(&schema, "purpose").contains("explicit human approval receipt"),
        "approval receipt schema should require explicit human receipt"
    );
    assert!(
        string_field(&schema, "purpose").contains("without enabling native AEX loading"),
        "approval receipt schema should keep native loading disabled"
    );

    for field in [
        "schema_version",
        "receipt_kind",
        "approval_receipt_id",
        "approval_status",
        "approved_by",
        "approved_at_utc",
        "approval_scope",
        "prerequisite_reports",
        "approval_effect",
        "safety_acknowledgements",
    ] {
        assert!(
            string_array_contains(&schema["receipt"]["required_fields"], field),
            "approval receipt schema missing receipt field {field}"
        );
    }
    assert_eq!(
        schema["receipt"]["receipt_kind"]["const"],
        "aex_loader_manual_approval_receipt"
    );
    assert_eq!(
        schema["receipt"]["approval_status"]["required_for_acceptance"],
        "approved"
    );

    for field in [
        "loader_slice_review_packet_status",
        "loader_slice_review_packet_checksum_algorithm",
        "loader_slice_review_packet_checksum_hex",
        "fixture_gate_status_required",
        "selected_candidate_review_status_required",
        "selected_candidate_fixture_status_required",
        "selected_candidate_plugin_class_required",
        "operation_scope",
        "ofx_route_scope",
    ] {
        assert!(
            string_array_contains(&schema["receipt"]["approval_scope_required_fields"], field),
            "approval scope missing {field}"
        );
    }
    let scope = &schema["receipt"]["approval_scope_required_values"];
    assert_eq!(
        scope["loader_slice_review_packet_status"],
        "ready_for_manual_loader_slice_review_no_load"
    );
    assert_eq!(
        scope["fixture_gate_status_required"],
        "review_queue_approved_local_only"
    );
    assert_eq!(
        scope["selected_candidate_review_status_required"],
        "approved-local-only"
    );
    assert_eq!(scope["ofx_route_scope"], "deferred_not_approved");

    for field in [
        "loader_slice_review_packet_reviewed",
        "fixture_gate_reviewed",
        "worker_identity_revalidation_reviewed",
        "sandbox_preflight_reviewed",
        "worker_attestation_reviewed",
        "job_object_reviewed",
        "handle_inheritance_reviewed",
        "license_reviewed",
        "cleanroom_reviewed",
        "generated_target_exclusion_reviewed",
    ] {
        assert!(
            string_array_contains(
                &schema["receipt"]["prerequisite_reports_required_true"],
                field
            ),
            "approval prerequisites missing {field}"
        );
    }

    let effect = &schema["receipt"]["approval_effect_required_values"];
    assert_eq!(effect["allow_separate_loader_implementation_review"], true);
    for field in [
        "allow_native_aex_load",
        "allow_worker_plugin_load",
        "allow_broker_may_load_aex",
        "allow_render_png",
        "allow_ofx_route",
        "allow_aex_sdk_or_abi_import",
        "allow_private_path_publication",
    ] {
        assert_eq!(
            effect[field], false,
            "approval effect {field} should stay false"
        );
    }

    let packet_requirements = &schema["loader_slice_review_packet_requirements"];
    assert_eq!(
        packet_requirements["status"],
        "ready_for_manual_loader_slice_review_no_load"
    );
    for check in [
        "fixture_gate_manual_approval_ready_no_load",
        "loader_manifest_ready_no_load",
        "provenance_chain_ready_no_load",
        "manual_review_requirements_closed",
        "no_execution_or_route_permission",
        "evidence_anti_contamination",
    ] {
        assert!(
            string_array_contains(&packet_requirements["required_passed_checks"], check),
            "approval schema should require packet check {check}"
        );
    }

    let report = &schema["validator_report"];
    for field in [
        "approval_accepted",
        "loader_review_approved",
        "native_load_performed",
        "loader_enabled",
        "real_aex_load_enabled",
        "worker_may_load_plugin",
        "broker_may_load_aex",
        "render_performed",
        "ofx_route_allowed",
        "loader_slice_review_packet_checksum_hex",
    ] {
        assert!(
            string_array_contains(&report["required_fields"], field),
            "validator report missing field {field}"
        );
    }
    let no_load = &report["required_no_load_values"];
    assert_eq!(no_load["native_load_performed"], false);
    assert_eq!(no_load["loader_enabled"], false);
    assert_eq!(no_load["real_aex_load_enabled"], false);
    assert_eq!(no_load["worker_may_load_plugin"], false);
    assert_eq!(no_load["render_performed"], false);
    assert_eq!(no_load["ofx_route_allowed"], false);
    for token in [
        "sha256",
        "base64",
        "loadlibrary",
        "libloading",
        "effectmain",
        "aeeffect",
        "worker_exe",
        "plugin_path",
        "normalized_plugin_path",
        ".aex",
    ] {
        assert!(
            string_array_contains(&report["forbidden_serialized_tokens"], token),
            "approval validator schema should forbid {token}"
        );
    }
    for note in [
        "AEX loader approval receipt validation reads JSON metadata only.",
        "An accepted receipt approves only a separate loader implementation review slice.",
        "This validator does not enable native AEX loading, worker plug-in loading, render, or OFX routing.",
    ] {
        assert!(string_array_contains(&report["required_notes"], note));
    }

    let template = &schema["tool_generated_template"];
    assert_eq!(template["may_be_emitted_by"], "aex_loader_approval_receipt");
    assert!(
        string_field(template, "command").contains("--draft-template"),
        "template command should expose --draft-template"
    );
    assert_eq!(template["template_status"], "draft_unapproved_template");
    assert_eq!(template["approval_status"], "draft_unapproved");
    assert!(template["approval_receipt_id"].is_null());
    assert_eq!(template["requires_ready_sanitized_packet"], true);
    assert_eq!(template["rejects_blocked_packet"], true);
    assert_eq!(template["rejects_contaminated_packet"], true);
    assert_eq!(
        template["approval_effect_defaults"]["allow_separate_loader_implementation_review"],
        false
    );
    assert_eq!(
        template["approval_effect_defaults"]["allow_native_aex_load"],
        false
    );
    assert_eq!(
        template["approval_effect_defaults"]["allow_worker_plugin_load"],
        false
    );
    assert_eq!(
        template["approval_effect_defaults"]["allow_render_png"],
        false
    );
    assert_eq!(
        template["approval_effect_defaults"]["allow_ofx_route"],
        false
    );
    assert_eq!(
        template["runtime_effect_defaults"]["native_load_performed"],
        false
    );
    assert_eq!(
        template["runtime_effect_defaults"]["worker_may_load_plugin"],
        false
    );
    assert_eq!(
        template["runtime_effect_defaults"]["render_performed"],
        false
    );
    assert_eq!(
        template["runtime_effect_defaults"]["ofx_route_allowed"],
        false
    );
    assert_eq!(
        template["validator_result_before_human_fill"],
        "invalid_receipt"
    );
}

#[test]
fn aex_probe_fixture_identity_smoke_schema_keeps_broker_transport_no_load() {
    let schema = load_analysis_json(FIXTURE_IDENTITY_SMOKE_SCHEMA)
        .expect("fixture identity smoke schema should exist");
    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "schema_name"),
        "AEX probe fixture identity smoke"
    );
    assert_eq!(
        string_field(&schema, "compatibility_classification"),
        "Synthetic fixture broker identity transport only"
    );
    for field in [
        "schema_version",
        "generated_by",
        "publication_status",
        "status",
        "fixture_manifest",
        "fixture_manifest_status",
        "output_root",
        "transport_operation",
        "pixel_format",
        "image_count",
        "transport_count",
        "identity_pixels_checked_count",
        "native_load_performed",
        "render_performed",
        "aex_loaded",
        "worker_started",
        "broker_invoked",
        "ofx_route_invoked",
        "ae_invoked",
        "private_payload_copied",
        "aex_render_correctness_evidence",
        "entries",
        "checks",
        "notes",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "fixture identity smoke schema missing field {field}"
        );
    }
    let required = &schema["required_values"];
    assert_eq!(required["generated_by"], "aex_probe_fixture_identity_smoke");
    assert_eq!(required["status"], "fixture_identity_smoke_ready_no_load");
    assert_eq!(
        required["fixture_manifest_status"],
        "synthetic_fixture_images_ready_no_load"
    );
    assert_eq!(required["transport_operation"], "identity_transport");
    assert_eq!(required["pixel_format"], "rgba8");
    assert_eq!(required["image_count"], 3);
    assert_eq!(required["transport_count"], 3);
    assert_eq!(required["identity_pixels_checked_count"], 3);
    assert_eq!(required["native_load_performed"], false);
    assert_eq!(required["render_performed"], false);
    assert_eq!(required["aex_loaded"], false);
    assert_eq!(required["worker_started"], false);
    assert_eq!(required["broker_invoked"], true);
    assert_eq!(required["ofx_route_invoked"], false);
    assert_eq!(required["ae_invoked"], false);
    assert_eq!(required["private_payload_copied"], false);
    assert_eq!(required["aex_render_correctness_evidence"], false);
    for check in [
        "fixture_manifest_validated",
        "identity_transport_ok",
        "rgba_identity_pixels_match",
        "synthetic_fixture_pixels_match",
        "no_aex_input",
        "no_worker_or_host_invocation",
        "not_render_correctness_evidence",
    ] {
        assert!(string_array_contains(
            &schema["required_check_names"],
            check
        ));
    }
    for id in ["gradient", "checker", "solid_alpha"] {
        assert!(string_array_contains(&schema["required_entry_ids"], id));
    }
    for token in [
        "sha256",
        "base64",
        "binary_payload",
        "loadlibrary",
        "libloading",
        "effectmain",
        "worker_loaded",
        "plugin_loaded",
    ] {
        assert!(string_array_contains(
            &schema["forbidden_report_tokens"],
            token
        ));
    }
}

#[test]
fn aex_wiztree_refresh_schema_keeps_generated_targets_out_of_fixture_gate() {
    let Some(schema) = load_analysis_json(AEX_WIZTREE_AEX_REFRESH_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(
        string_field(&schema, "purpose").contains(
            "generated target artifacts are not mistaken for first-loader fixture candidates"
        ),
        "WizTree refresh schema should explain fixture-gate safety purpose"
    );
    for field in [
        "schema_version",
        "generated_at",
        "publication_status",
        "scan_mode",
        "target_root",
        "wiztree_csv_snapshot",
        "safety_notes",
        "total_aex_scan",
        "canonical_non_generated_aex",
        "generated_target_test_artifacts",
        "fixture_gate_candidates_verified_present",
        "recommended_fixture_gate_delta",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "WizTree refresh schema missing field {field}"
        );
    }

    assert_eq!(
        schema["required_values"]["scan_mode"],
        "wiztree-read-only-filtered-aex"
    );
    assert_eq!(schema["total_aex_scan_required_values"]["count"], 119);
    assert_eq!(
        schema["canonical_non_generated_aex_required_values"]["count"],
        40
    );
    assert_eq!(
        schema["generated_target_test_artifacts_required_values"]["count"],
        79
    );
    assert_eq!(
        schema["required_generated_name_groups"]["ClassicTest.aex"],
        65
    );
    assert_eq!(schema["required_generated_name_groups"]["OtherTest.aex"], 9);
    assert!(string_array_contains(
        &schema["fixture_gate_candidate_required_ids"],
        "adaptive-filter-local"
    ));
    assert!(string_array_contains(
        &schema["fixture_gate_candidate_required_ids"],
        "median-pro-local"
    ));
    assert_eq!(
        schema["recommended_fixture_gate_delta_required_values"]["keep_current_two_candidate_gate"],
        true
    );
    assert_eq!(
        schema["recommended_fixture_gate_delta_required_values"]
            ["do_not_expand_first_loader_queue_from_generated_target_artifacts"],
        true
    );
    assert_eq!(
        schema["recommended_fixture_gate_delta_required_values"]
            ["do_not_select_fixture_without_manual_approval"],
        true
    );
    for token in [
        "sha256",
        "base64",
        "binary_payload",
        "payload_bytes",
        "copied_asset",
        "native_load_result",
        "loadlibrary",
        "rendered_pixels",
    ] {
        assert!(
            string_array_contains(&schema["forbidden_serialized_tokens"], token),
            "WizTree refresh schema should forbid {token}"
        );
    }
}

#[test]
fn aex_fixture_gate_refresh_audit_schema_pins_no_load_queue_hygiene() {
    let Some(schema) = load_analysis_json(AEX_FIXTURE_GATE_REFRESH_AUDIT_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert!(
        string_field(&schema, "purpose")
            .contains("fixture review gate still matches the read-only WizTree AEX refresh"),
        "fixture gate refresh audit schema should explain the gate/refresh join"
    );
    for field in [
        "schema_version",
        "publication_status",
        "status",
        "native_load_performed",
        "render_performed",
        "fixture_selected",
        "loader_enabled",
        "fixture_gate_summary",
        "wiztree_refresh_summary",
        "candidate_crosscheck",
        "checks",
        "blocked_reasons",
        "notes",
    ] {
        assert!(
            string_array_contains(&schema["required_fields"], field),
            "fixture gate refresh audit schema missing field {field}"
        );
    }
    let required = &schema["required_values"];
    assert_eq!(required["publication_status"], "local-only");
    assert_eq!(required["native_load_performed"], false);
    assert_eq!(required["render_performed"], false);
    assert_eq!(required["fixture_selected"], false);
    assert_eq!(required["loader_enabled"], false);
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "fixture_gate_refresh_ready_no_load"
    ));
    assert!(string_array_contains(
        &schema["allowed_statuses"],
        "blocked_fixture_gate_refresh"
    ));

    let gate_ready = &schema["fixture_gate_summary_required_ready_values"];
    assert_eq!(gate_ready["status"], "review_queue_not_approved");
    assert_eq!(gate_ready["selected_fixture"], serde_json::Value::Null);
    assert_eq!(gate_ready["approval_all_false"], true);
    assert_eq!(gate_ready["candidate_count"], 2);
    assert_eq!(gate_ready["local_build_candidate_count"], 2);
    assert_eq!(gate_ready["not_approved_candidate_count"], 2);
    assert_eq!(gate_ready["manual_user_approval_required"], true);

    let refresh_ready = &schema["wiztree_refresh_summary_required_ready_values"];
    assert_eq!(refresh_ready["scan_mode"], "wiztree-read-only-filtered-aex");
    assert_eq!(refresh_ready["total_aex_count"], 119);
    assert_eq!(refresh_ready["canonical_non_generated_count"], 40);
    assert_eq!(refresh_ready["generated_target_artifact_count"], 79);
    assert_eq!(refresh_ready["generated_target_artifacts_excluded"], true);
    assert_eq!(refresh_ready["fixture_gate_candidate_count"], 2);
    assert_eq!(
        refresh_ready["fixture_gate_candidates_queued_not_approved"],
        2
    );
    assert_eq!(refresh_ready["do_not_expand_from_generated_targets"], true);
    assert_eq!(refresh_ready["do_not_select_without_manual_approval"], true);

    for id in ["adaptive-filter-local", "median-pro-local"] {
        assert!(
            string_array_contains(&schema["candidate_crosscheck_required_ids"], id),
            "fixture gate refresh audit schema missing candidate {id}"
        );
    }
    let candidate_ready = &schema["candidate_crosscheck_required_ready_values"];
    assert_eq!(candidate_ready["path_matches_refresh"], true);
    assert_eq!(candidate_ready["size_matches_refresh"], true);
    assert_eq!(candidate_ready["gate_review_status"], "not-approved");
    assert_eq!(
        candidate_ready["refresh_review_status"],
        "queued-not-approved"
    );
    assert_eq!(candidate_ready["generated_target_artifact"], false);

    for check in [
        "fixture_gate_closed_no_selection",
        "fixture_gate_candidate_policy",
        "wiztree_refresh_non_generated_inventory_match",
        "wiztree_refresh_generated_targets_excluded",
        "fixture_gate_candidates_present_in_refresh",
        "fixture_gate_excludes_generated_target_artifacts",
        "evidence_anti_contamination",
    ] {
        assert!(
            string_array_contains(&schema["required_check_names"], check),
            "fixture gate refresh audit schema missing check {check}"
        );
    }
    for token in [
        "sha256",
        "base64",
        "binary_payload",
        "payload_bytes",
        "copied_asset",
        "native_load_result",
        "loadlibrary",
        "rendered_pixels",
        "\"approved\":true",
        "\"loader_enabled\":true",
    ] {
        assert!(
            string_array_contains(&schema["forbidden_serialized_tokens"], token),
            "fixture gate refresh audit schema should forbid {token}"
        );
    }
    for note in [
        "Fixture gate refresh audit reads JSON metadata only.",
        "No .aex file is opened, copied, loaded, executed, described, or rendered.",
        "A ready audit confirms queue hygiene only; it is not fixture selection or loader approval.",
        "Generated target artifacts must stay excluded from first-loader fixture review.",
    ] {
        assert!(string_array_contains(&schema["required_notes"], note));
    }
}

#[test]
fn no_load_provenance_pipeline_runbook_links_aex_to_ofx_without_loading() {
    let Some(runbook) = load_analysis_text(AEX_NO_LOAD_PROVENANCE_PIPELINE_RUNBOOK) else {
        return;
    };

    for command in [
        "cargo run --example aex_fixture_gate_refresh_audit",
        "cargo run --example aex_loader_preflight",
        "cargo run --example aex_loader_implementation_manifest",
        "cargo run --example aex_native_stage_plan",
        "cargo run --example ofx_aex_facade_readiness",
        "cargo run --example aex_no_load_provenance_audit",
        "cargo run --example aex_loader_slice_review_packet",
        "cargo run --example aex_loader_approval_receipt",
    ] {
        assert!(
            runbook.contains(command),
            "provenance runbook missing command {command:?}"
        );
    }
    for argument in [
        "--preflight",
        "--readiness",
        "--fixture-gate",
        "--wiztree-refresh",
        "--fixture-refresh-audit",
        "--manifest",
        "--ticket",
        "--host-boundary",
        "--native-stage-plan",
        "--loader-manifest",
        "--ofx-readiness",
        "--provenance-audit",
        "--draft-template",
        "--packet",
        "--receipt",
        "--fixture-identity-smoke",
        "--capability-dir",
    ] {
        assert!(
            runbook.contains(argument),
            "provenance runbook missing argument {argument:?}"
        );
    }
    for expected in [
        "ready_for_separate_loader_implementation_review_no_load",
        "planned_native_stage_contract_no_load",
        "deferred_contract_only",
        "no_load_provenance_chain_ready",
        "fixture_gate_refresh_ready_no_load",
        "native_load_performed=false",
        "selectors_executed=false",
        "render_performed=false",
        "fixture_selected=false",
        "loader_enabled=false",
        "ofx_route_allowed=false",
        "evidence_contains_forbidden_tokens=false",
        "ofx_may_route_to_loader=false",
        "ofx_host_may_load_aex=false",
        "ofx_adapter_may_load_aex=false",
        "broker_may_load_aex=false",
        "aviutlas_may_route_through_ofx_to_reach_aex=false",
        "loader_manifest_summary.readiness_provided=true",
        "native_stage_plan_summary.cleanroom_boundary_no_loader_or_sdk=true",
        "native_stage_plan_summary.no_load_stage_plan_ready=true",
        "native_stage_plan_summary.worker_runtime_evidence_ready=true",
        "native_stage_plan_summary.ofx_route_blocked=true",
        "ofx_readiness_summary.native_stage_plan_summary_provided=true",
        "ofx_readiness_summary.native_stage_ofx_route_blocked=true",
        "fixture_gate_summary.approval_all_false=true",
        "wiztree_refresh_summary.canonical_non_generated_count=40",
        "wiztree_refresh_summary.generated_target_artifact_count=79",
        "wiztree_refresh_summary.do_not_expand_from_generated_targets=true",
        "fixture_refresh_audit_summary.provided=true",
        "fixture_refresh_audit_summary.status=fixture_gate_refresh_ready_no_load",
        "fixture_refresh_audit_summary.wiztree_canonical_non_generated_count=40",
        "fixture_refresh_audit_summary.wiztree_generated_target_artifact_count=79",
        "fixture_refresh_audit_summary.candidates_present_in_refresh=true",
        "fixture_gate_refresh_audit_ready_no_load",
        "manifest_fixture_refresh_audit_summary.provided=true",
        "manifest_fixture_refresh_audit_summary.status=fixture_gate_refresh_ready_no_load",
        "manifest_fixture_refresh_audit_summary.wiztree_canonical_non_generated_count=40",
        "manifest_fixture_refresh_audit_summary.wiztree_generated_target_artifact_count=79",
        "manifest_fixture_refresh_audit_ready_no_load",
        "loader_manifest_summary.fixture_refresh_audit_provided=true",
        "native_stage_plan_summary.fixture_refresh_audit_provided=true",
        "loader_manifest_summary.fixture_refresh_audit_status=fixture_gate_refresh_ready_no_load",
        "native_stage_plan_summary.fixture_refresh_audit_status=fixture_gate_refresh_ready_no_load",
        "fixture_refresh_audit_preserved_no_load",
        "fixture_identity_smoke_ready_no_load",
        "fixture_identity_smoke_summary.provided=true",
        "fixture_identity_smoke_summary.status=fixture_identity_smoke_ready_no_load",
        "fixture_identity_smoke_summary.transport_operation=identity_transport",
        "fixture_identity_smoke_summary.broker_invoked=true",
        "fixture_identity_smoke_summary.aex_render_correctness_evidence=false",
        "fixture_identity_smoke_summary.expected_synthetic_image_set=true",
        "fixture_identity_smoke_summary.all_entries_identity_pixels_match=true",
        "fixture_identity_smoke_summary.input_contains_forbidden_tokens=false",
        "fixture_identity_smoke_summary.sanitized_summary_contains_forbidden_tokens=false",
        "ready_for_manual_loader_slice_review_no_load",
        "loader_slice_approved=false",
        "loader_enabled=false",
        "real_aex_load_enabled=false",
        "worker_may_load_plugin=false",
        "manifest_summary.selected_plugin_path_redacted=true",
        "manifest_summary.native_loader_calls_allowed=false",
        "manifest_summary.broker_may_load_aex=false",
        "provenance_summary.status=no_load_provenance_chain_ready",
        "provenance_summary.ofx_readiness_ready=true",
        "provenance_summary.runtime_and_cleanroom_ready=true",
        "review_queue_not_approved",
        "blocked_loader_slice_review_packet",
        "fixture_gate_summary.selected_fixture_present=true",
        "fixture_gate_summary.approval_approved=true",
        "fixture_gate_summary.approval_loader_enabled=true",
        "fixture_gate_summary.approval_real_aex_load_enabled=true",
        "fixture_gate_summary.selected_candidate_review_status=approved-local-only",
        "fixture_gate_summary.generated_target_candidate_count=0",
        "fixture_gate_summary.input_contains_forbidden_tokens=false",
        "fixture_gate_manual_approval_ready_no_load",
        "provenance_summary.fixture_identity_smoke_ready=true",
        "fixture_identity_smoke_preserved_no_load",
        "review_requirements.explicit_user_approval_required=true",
        "review_requirements.code_review_required=true",
        "review_requirements.local_build_classic_effect_fixture_required=true",
        "review_requirements.cleanroom_boundary_required=true",
        "review_requirements.license_review_required=true",
        "review_requirements.worker_isolation_evidence_required=true",
        "review_requirements.ofx_facade_review_deferred=true",
        "review_requirements.generated_target_fixtures_forbidden=true",
        "approved_for_loader_implementation_review_no_load",
        "draft_unapproved_template",
        "approval_status=draft_unapproved",
        "approval_accepted=true",
        "loader_review_approved=true",
        "receipt_allows_separate_loader_implementation_review=true",
        "receipt_allows_native_aex_load=false",
        "receipt_allows_worker_plugin_load=false",
        "receipt_allows_render_png=false",
        "receipt_allows_ofx_route=false",
        "sentinel_not_inherited-with-explicit-handle-list",
        "assigned-with-kill-on-close",
    ] {
        assert!(
            runbook.contains(expected),
            "provenance runbook missing no-load invariant {expected:?}"
        );
    }
    for forbidden_boundary in [
        "It is not loader approval.",
        "It does not open, hash, copy, load, execute, describe, or render `.aex` binaries.",
        "must not become the first AEX loader",
        "must not route AviUtlas through OFX",
        "must not issue describe or render requests before a separate OFX review gate is explicitly approved",
        "This audit is a join check only.",
        "It is not fixture selection and is not loader approval.",
        "The optional `--fixture-identity-smoke` input is the narrow exception",
        "must never serialize those PNG path fields downstream",
        "This packet is sanitized handoff evidence only.",
        "This validation report is not the loader implementation and is not a runtime",
        "does not approve a loader",
        "enable worker, broker, render, or OFX execution",
    ] {
        assert!(
            runbook.contains(forbidden_boundary),
            "provenance runbook missing boundary wording {forbidden_boundary:?}"
        );
    }
}

#[test]
fn aex_image_probe_allowlist_example_is_example_only_and_bounded() {
    let Some(allowlist) = load_analysis_json(ALLOWLIST_EXAMPLE) else {
        return;
    };

    assert_eq!(allowlist["schema_version"], 1);
    assert_eq!(
        string_field(&allowlist, "publication_status"),
        "example-only"
    );
    assert!(
        string_field(&allowlist, "description").contains("Do not add private absolute paths"),
        "allowlist example should warn against publishing private paths"
    );
    for blocked in ["aegp", "aeio", "smartfx-only", "gpu-only", "unknown"] {
        assert!(
            string_array_contains(&allowlist["blocked_classes"], blocked),
            "allowlist should block {blocked}"
        );
    }

    let entries = allowlist["entries"]
        .as_array()
        .expect("allowlist entries should be an array");
    assert!(
        !entries.is_empty(),
        "allowlist should include example entries"
    );
    for entry in entries {
        assert!(string_field(entry, "plugin_path").ends_with(".aex"));
        assert_eq!(string_field(entry, "expected_class"), "classic-effect");
        assert_eq!(
            string_field(entry, "fixture_status"),
            "local-build-candidate"
        );
        let max_width = entry["max_width"].as_u64().unwrap_or_default();
        let max_height = entry["max_height"].as_u64().unwrap_or_default();
        let timeout_ms = entry["timeout_ms"].as_u64().unwrap_or_default();
        assert!((1..=4096).contains(&max_width));
        assert!((1..=4096).contains(&max_height));
        assert!((1..=30_000).contains(&timeout_ms));
        assert!(string_array_contains(
            &entry["allowed_operations"],
            "describe"
        ));
        assert!(string_array_contains(
            &entry["allowed_operations"],
            "render_png"
        ));
        assert!(
            !string_array_contains(&entry["allowed_operations"], "catalog"),
            "catalog is broker-facing and should not be per-plugin allowlisted"
        );
    }
}

#[test]
fn aex_fixture_review_gate_selects_no_fixture_and_keeps_loader_closed() {
    let Some(gate) = load_analysis_json(FIXTURE_REVIEW_GATE) else {
        return;
    };

    assert_eq!(gate["schema_version"], 1);
    assert_eq!(string_field(&gate, "status"), "review_queue_not_approved");
    assert_eq!(
        string_field(&gate, "metadata_mode"),
        "path-and-size-only-no-hash-no-binary-payload"
    );
    assert!(gate["selected_fixture"].is_null());
    assert_eq!(
        string_field(&gate, "recommended_first_review"),
        "adaptive-filter-local"
    );
    assert_eq!(
        string_field(&gate, "recommendation_status"),
        "queue-order-only-not-approval"
    );
    assert_eq!(gate["approval"]["approved"], false);
    assert_eq!(gate["approval"]["loader_enabled"], false);
    assert_eq!(gate["approval"]["real_aex_load_enabled"], false);
    assert_eq!(gate["approval"]["render_png_enabled"], false);

    let policy = &gate["single_fixture_policy"];
    assert_eq!(policy["max_selected_fixtures"], 1);
    assert_eq!(policy["selection_requires_manual_user_approval"], true);
    assert_eq!(
        policy["selection_requires_loader_gate_opened_by_separate_slice"],
        true
    );
    assert_eq!(policy["no_parallel_first_loader_fixtures"], true);

    let runtime = &gate["required_runtime_evidence_before_loader"];
    assert_eq!(
        runtime["allowlist_loader_approval_status"],
        "approved-local-only"
    );
    assert_eq!(runtime["sandbox_preflight"], "passed");
    assert_eq!(
        runtime["handle_inheritance"],
        "sentinel_not_inherited-with-explicit-handle-list"
    );
    assert_eq!(runtime["ofx_facade"], "not-a-loader-and-not-a-bypass");

    let candidates = gate["candidates"]
        .as_array()
        .expect("candidate queue should be an array");
    assert_eq!(candidates.len(), 2);
    assert_eq!(string_field(&candidates[0], "id"), "adaptive-filter-local");
    assert_eq!(string_field(&candidates[1], "id"), "median-pro-local");
    for candidate in candidates {
        assert_eq!(string_field(candidate, "review_status"), "not-approved");
        assert_eq!(
            string_field(candidate, "fixture_status"),
            "local-build-candidate"
        );
        assert_eq!(
            string_field(candidate, "plugin_class"),
            "classic-effect-candidate"
        );
        let evidence = &candidate["optional_static_evidence"];
        assert_eq!(
            string_field(evidence, "evidence_status"),
            "observed-read-only-no-load"
        );
        assert_eq!(string_field(evidence, "pe_machine"), "x86_64");
        assert!(string_field(evidence, "pe_resource_summary").contains("types=PIPL"));
        assert_eq!(string_field(evidence, "pe_export_entrypoint"), "EffectMain");
        assert_eq!(string_field(evidence, "adjacent_pipl_kind"), "AEEffect");
        assert_eq!(string_field(evidence, "adjacent_pipl_category"), "Filter");
        assert_eq!(string_field(evidence, "adjacent_entrypoint"), "EffectMain");
        assert_eq!(
            string_field(evidence, "smart_render_status"),
            "declared-but-deferred"
        );
        assert!(
            candidate["observed_size_bytes"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "candidate should record size metadata without embedding payloads"
        );
        assert!(string_array_contains(
            &candidate["blocked_reasons"],
            "real loader slice is not opened"
        ));
    }

    let serialized = serde_json::to_string(&gate).unwrap();
    assert!(!serialized.to_ascii_lowercase().contains("sha256"));
    assert!(!serialized.to_ascii_lowercase().contains("base64"));
}

#[test]
fn aex_worker_start_policy_freezes_spawn_without_load_boundary() {
    let Some(policy) = load_analysis_text(WORKER_START_POLICY) else {
        return;
    };

    for expected in [
        "The broker may eventually spawn a worker process. The broker must never load",
        "`std::process::Command` or a Windows process API is allowed only in a",
        "no shell (`cmd.exe`, PowerShell, `wscript`, file association, or PATH search)",
        "worker protocol version handshake before any `.aex` load",
        "Prefer Windows Job Object with kill-on-job-close before real loading.",
        "Disable handle inheritance by default.",
        "Do not rely on ambient PATH.",
        "No content hashes are taken by default.",
        "Use the schema/broker status names as canonical v0",
        "Do not introduce parallel `load_failed`, `setup_failed`, `render_ok`, or",
        "worker should not read arbitrary input paths directly in v0",
        "failed worker reports must keep `output_png` null",
        "SmartFX-capable plug-ins may only run if static/describe evidence shows a",
        "The next implementation slice should be a worker-launch stub, not real",
        "keep `.aex` loading disabled until sandbox and allowlist identity gates are",
    ] {
        assert!(
            policy.contains(expected),
            "worker start policy missing {expected:?}"
        );
    }

    for status in [
        "`allowlist_denied`",
        "`unsupported_plugin_class`",
        "`unsupported_selector`",
        "`unsupported_suite`",
        "`timeout`",
        "`plugin_exception`",
        "`worker_crash`",
        "`worker_protocol_error`",
    ] {
        assert!(
            policy.contains(status),
            "worker start policy should keep canonical status {status}"
        );
    }
}
