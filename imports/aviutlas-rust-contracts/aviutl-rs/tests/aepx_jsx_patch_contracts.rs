use std::io::ErrorKind;
use std::path::Path;

use serde_json::Value;

const AEPX_SCHEMA: &str = "analysis/AEPX_PATCH_REQUEST_SCHEMA_2026-05-31.json";
const JSX_SCHEMA: &str = "analysis/JSX_TRANSACTION_REQUEST_SCHEMA_2026-05-31.json";
const JSX_REPORT_SCHEMA: &str = "analysis/AE_JSX_TRANSACTION_REPORT_SCHEMA_2026-06-01.json";
const EXPORT_MANIFEST_SCHEMA: &str = "analysis/AE_JSX_EXPORT_MANIFEST_SCHEMA_2026-06-01.json";
const EXPORT_ARTIFACT_VERIFY_SCHEMA: &str =
    "analysis/AE_JSX_EXPORT_ARTIFACT_VERIFY_REPORT_SCHEMA_2026-06-01.json";
const EXPORT_APPROVAL_SCHEMA: &str =
    "analysis/AE_JSX_EXPORT_APPROVAL_RECEIPT_SCHEMA_2026-06-01.json";
const EXPORT_CLOSEOUT_SCHEMA: &str =
    "analysis/AE_JSX_EXPORT_MANUAL_SMOKE_CLOSEOUT_REPORT_SCHEMA_2026-06-01.json";
const PATCH_SPEC: &str = "analysis/AEPX_JSX_PATCH_TOOL_SPEC_2026-05-31.md";
const PATCH_HANDOFF: &str = "analysis/AEPX_JSX_PATCH_DEVELOPMENT_HANDOFF_2026-05-31.md";
const OPERATOR_RUNBOOK: &str = "analysis/AE_PROJECT_EDIT_OPERATOR_RUNBOOK_2026-06-01.md";
const AEPX_WRITER_SPIKE_DOC: &str = "analysis/AEPX_XML_PRESERVATION_WRITER_SPIKE_2026-06-01.md";
const AEPX_PRODUCTION_LANE_GATE_SCHEMA: &str =
    "analysis/AEPX_PRODUCTION_LANE_GATE_SCHEMA_2026-06-01.json";

fn repository_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .to_path_buf()
}

fn load_analysis_json(relative_path: &str) -> Option<Value> {
    let path = repository_root().join(relative_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!(
                "skipping AEPX/JSX contract guard; artifact is absent: {}",
                path.display()
            );
            return None;
        }
        Err(err) => panic!("failed to read analysis artifact {}: {err}", path.display()),
    };
    Some(serde_json::from_str(&text).expect("analysis artifact should be JSON"))
}

fn load_analysis_text(relative_path: &str) -> Option<String> {
    let path = repository_root().join(relative_path);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!(
                "skipping AEPX/JSX contract guard; artifact is absent: {}",
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

fn pipe_options(value: &Value) -> Vec<&str> {
    value
        .as_str()
        .unwrap_or_default()
        .split('|')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

fn array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item == expected)
}

fn notes_contain(schema: &Value, expected: &str) -> bool {
    schema["notes"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item.as_str().unwrap_or_default().contains(expected))
}

#[test]
fn aepx_patch_schema_keeps_modes_and_edit_kinds_disjoint() {
    let Some(schema) = load_analysis_json(AEPX_SCHEMA) else {
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
        vec!["inspect_metadata", "noop_validate", "dry_run", "apply"],
        "top-level operation should be the AEPX tool mode vocabulary"
    );

    let edit_kinds = pipe_options(&request["operations"][0]["kind"]);
    for kind in [
        "rename_comp",
        "rename_layer",
        "set_comment",
        "set_marker",
        "replace_text_source",
        "relink_asset_path",
    ] {
        assert!(
            edit_kinds.contains(&kind),
            "missing AEPX edit operation kind {kind}"
        );
    }
    for mode in ["inspect_metadata", "noop_validate", "dry_run", "apply"] {
        assert!(
            !edit_kinds.contains(&mode),
            "tool mode should not drift into operations[].kind: {mode}"
        );
    }

    assert_eq!(
        request["options"]["overwrite"],
        "boolean, must be false in v0"
    );
    assert_eq!(
        request["options"]["allow_ambiguous_selector"],
        "boolean, must be false in v0"
    );
    assert!(
        string_field(&request["operations"][0], "expected_old_value")
            .contains("required guard for every mutating operation"),
        "AEPX schema should document universal old-value guards"
    );
    assert!(
        string_field(&request["operations"][0], "new_value")
            .contains("string required for rename_comp, rename_layer, set_comment"),
        "AEPX schema should document sensitive payload string guards"
    );
    assert!(
        string_field(&request["operations"][0], "user_supplied")
            .contains("comments, markers, relink paths, and text replacement"),
        "AEPX schema should document user-supplied guards for text-bearing/path operations"
    );
    for expected in [
        "rename_comp",
        "rename_layer",
        "set_comment",
        "replace_text_source",
        "relink_asset_path",
    ] {
        assert!(
            string_field(&request["operations"][0], "expected_old_value").contains(expected),
            "AEPX schema should document string expected_old_value for {expected}"
        );
    }
    assert!(
        string_field(&request["operations"][0], "expected_old_value")
            .contains("object for set_marker"),
        "AEPX schema should document marker old-value object guard"
    );
    assert_eq!(request["options"]["preserve_unknown_xml"], "required");
    assert_eq!(
        request["options"]["report_private_payloads"],
        "boolean, must be false"
    );

    for note in [
        "Top-level operation is the tool mode; operations[].kind is the edit operation.",
        "apply requires output_aepx and output_aepx must be a new path.",
        "inspect_metadata returns source_not_found when input_aepx is missing or not a file",
        "input_aepx and output_aepx must not be equal after path normalization.",
        "When output_aepx is supplied for dry_run, noop_validate, or inspect_metadata",
        "Current v0 reports expose io_gate",
        "Current v0 reports expose write_gate",
        "overwrite is rejected in v0.",
        "All mutating operations must include expected_old_value before dry_run or future apply can pass.",
        "publication_status unknown should fail closed before writing.",
        "report_private_payloads true is invalid.",
        "Reports must not embed full XML payloads or private project content.",
    ] {
        assert!(notes_contain(&schema, note), "missing AEPX note: {note}");
    }
}

#[test]
fn aepx_patch_schema_exposes_failure_and_preservation_statuses() {
    let Some(schema) = load_analysis_json(AEPX_SCHEMA) else {
        return;
    };

    let statuses = &schema["response"]["status"];
    for status in [
        "dry_run_ok",
        "invalid_request",
        "schema_error",
        "output_exists",
        "output_same_as_source",
        "unsupported_operation",
        "ambiguous_target",
        "expected_value_mismatch",
        "parse_error",
        "preservation_failed",
        "write_failed",
    ] {
        assert!(
            array_contains(statuses, status),
            "missing AEPX response status {status}"
        );
    }

    let preservation = &schema["response"]["preservation"];
    for field in [
        "unknown_nodes",
        "unknown_attributes",
        "xml_declaration",
        "encoding",
        "comments",
        "cdata",
        "namespace_prefixes",
        "whitespace",
    ] {
        assert!(
            preservation[field].is_string(),
            "missing preservation field {field}"
        );
    }
    let encoding = string_field(preservation, "encoding");
    assert!(encoding.contains("preserved"));
    assert!(encoding.contains("rewritten"));
    assert!(encoding.contains("normalized"));
    assert!(encoding.contains("not_written"));
    assert_eq!(
        schema["response"]["io_gate"]["xml_body_read_performed"],
        "boolean, false in current v0 reports"
    );
    assert_eq!(
        schema["response"]["io_gate"]["after_effects_invoked"],
        "boolean, false in current v0 reports"
    );
    assert_eq!(
        schema["response"]["write_gate"]["patch_application_status"],
        "not_applied in current v0 reports"
    );
    assert_eq!(
        schema["response"]["write_gate"]["xml_writer_status"],
        "not_implemented in current v0 reports"
    );
    assert_eq!(
        schema["response"]["write_gate"]["output_write_performed"],
        "boolean, false in current v0 reports"
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["algorithm"],
        "fnv1a64-v1-noncryptographic"
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["payloads_embedded"],
        "false"
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["covers_xml_or_private_patch_payloads"],
        "false"
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["cryptographic_digest"]["algorithm"],
        "sha256-v1"
    );
}

#[test]
fn aepx_production_lane_gate_keeps_apply_closed_until_required_evidence() {
    let Some(schema) = load_analysis_json(AEPX_PRODUCTION_LANE_GATE_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "publication_status"),
        "local-only gate artifact"
    );
    for field in [
        "aepx_patch_probe_apply_enabled",
        "real_aepx_input_enabled",
        "production_xml_writer_enabled",
        "binary_aep_writer_enabled",
        "source_overwrite_enabled",
        "after_effects_tool_launch_enabled",
        "external_process_invocation_in_writer_enabled",
    ] {
        assert_eq!(
            schema["current_state"][field], false,
            "production lane gate should keep {field} false"
        );
    }
    assert_eq!(
        schema["allowed_first_candidate"]["operation_kind"],
        "rename_comp"
    );
    assert_eq!(schema["allowed_first_candidate"]["selector_kind"], "xml_id");
    assert_eq!(schema["allowed_first_candidate"]["target_count"], 1);
    assert_eq!(
        schema["allowed_first_candidate"]["source_overwrite_allowed"],
        false
    );
    for evidence in [
        "synthetic_preservation_proof_contract_green",
        "production_writer_path_boundary_contract_green",
        "production_writer_exact_byte_diff_contract_green",
        "production_report_privacy_contract_green",
        "real_aepx_fixture_review_receipt_local_only",
        "parent_approval_receipt_for_apply_slice",
    ] {
        assert!(
            array_contains(&schema["required_green_evidence_before_apply"], evidence),
            "missing production gate evidence {evidence}"
        );
    }
    for forbidden in [
        "binary .aep writing",
        "source overwrite",
        "automated After Effects launch",
        ".aex loading",
        "OFX routing",
    ] {
        assert!(
            array_contains(&schema["hard_forbidden_in_first_apply_slice"], forbidden),
            "missing hard-forbidden production scope {forbidden}"
        );
    }
    assert_eq!(
        schema["transition_rule"]["dry_run_evidence_accepted_as_apply_permission"],
        false
    );
    assert_eq!(
        schema["transition_rule"]["synthetic_proof_accepted_as_ae_compatibility"],
        false
    );
}

#[test]
fn jsx_transaction_schema_is_fail_closed_and_no_runtime_by_default() {
    let Some(schema) = load_analysis_json(JSX_SCHEMA) else {
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
        vec!["validate", "generate_jsx_transaction", "manual_smoke_plan"],
        "top-level operation should be the JSX transaction mode vocabulary"
    );
    assert_eq!(
        request["runtime"]["run_after_effects"],
        "boolean, must be false in automated v0 tests"
    );

    let policy = &request["policy"];
    for field in [
        "allow_shell",
        "allow_system",
        "allow_dialogs",
        "allow_bridge_talk",
        "allow_execute_command",
        "allow_eval",
    ] {
        assert!(
            string_field(policy, field).contains("must be false in v0"),
            "{field} should be false by default"
        );
    }
    assert!(string_field(policy, "allow_file_mutation").contains("must be false in v0"));
    assert_eq!(policy["report_private_payloads"], "boolean, must be false");

    let selector = &request["operations"][0]["target"]["selector"];
    for field in [
        "ae_item_id",
        "name",
        "comp_name",
        "comp_ae_item_id",
        "layer_name",
        "layer_index",
        "asset_path",
        "marker_index",
    ] {
        assert!(
            selector.get(field).is_some(),
            "JSX selector vocabulary missing {field}"
        );
    }
    assert!(
        string_field(&request["operations"][0], "user_supplied")
            .contains("comments, markers, text replacement, and relink paths"),
        "JSX schema should document every user-supplied operation guard"
    );
    assert!(
        string_field(&request["operations"][0], "expected_old_value")
            .contains("required guard for mutating operations"),
        "JSX schema should document universal old-value guards"
    );
    for expected in [
        "rename_comp",
        "rename_layer",
        "set_comment",
        "set_marker",
        "replace_text_source",
        "relink_asset_path",
    ] {
        assert!(
            string_field(&request["operations"][0], "expected_old_value").contains(expected),
            "JSX schema should document expected_old_value for {expected}"
        );
    }

    for note in [
        "Generated JSX is a thin AE-side interpreter",
        "V0 automated tests generate JSX but do not launch After Effects.",
        "Generated JSX must not call shell/system/dialog/BridgeTalk/executeCommand APIs by default.",
        "Generated JSX must not call eval, Function, $.evalFile, File.execute, ExternalObject, Socket, arbitrary File mutation APIs, alert, or bare app.project.save().",
        "source_project.path, output_project.path, and generated_jsx.path must be pairwise distinct after path normalization.",
        "generated_jsx.path must be explicit, non-existing in v0, and separate from source/output project paths.",
        "validate and manual_smoke_plan do not write generated JSX",
        "run_after_effects true is fail-closed in v0 automated flows",
        "artifact_gate distinguishes the allowed generated JSX create-new write",
        "No-AE v0 reports must expose execution_gate",
        "No-AE v0 reports record source_metadata",
        "No-AE v0 reports expose jsx_transaction_report_binding",
        "All mutating operations must include expected_old_value before validation or JSX generation can pass.",
        "publication_status unknown should fail closed before writing.",
        "report_private_payloads true is invalid.",
        "The only allowed save is to the explicit output_project path.",
    ] {
        assert!(notes_contain(&schema, note), "missing JSX note: {note}");
    }
}

#[test]
fn jsx_transaction_report_schema_keeps_generated_jsx_operation_vocabulary_synced() {
    let Some(schema) = load_analysis_json(JSX_REPORT_SCHEMA) else {
        return;
    };

    let vocabularies = &schema["operation_vocabularies"];
    for expected in [
        "rename_comp",
        "rename_layer",
        "set_comment",
        "set_marker",
        "replace_text_source",
        "relink_asset_path",
    ] {
        assert!(
            array_contains(&vocabularies["allowed_request_kinds"], expected),
            "report schema allowed_request_kinds missing {expected}"
        );
        assert!(
            array_contains(&vocabularies["generated_jsx_v0_supported_kinds"], expected),
            "report schema generated_jsx_v0_supported_kinds missing {expected}"
        );
    }
    assert!(
        vocabularies["validated_but_not_generated_in_jsx_v0"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "report schema should not leave relink_asset_path in validate-only vocabulary"
    );
}

#[test]
fn jsx_transaction_schema_exposes_manual_smoke_and_forbidden_api_statuses() {
    let Some(schema) = load_analysis_json(JSX_SCHEMA) else {
        return;
    };

    let statuses = &schema["response"]["status"];
    for status in [
        "generated_jsx",
        "manual_smoke_plan_ok",
        "ae_run_ok",
        "invalid_request",
        "schema_error",
        "output_exists",
        "output_same_as_source",
        "unsupported_operation",
        "ambiguous_target",
        "ae_not_run",
        "ae_runtime_failed",
        "forbidden_jsx_api",
    ] {
        assert!(
            array_contains(statuses, status),
            "missing JSX response status {status}"
        );
    }

    assert_eq!(
        schema["response"]["forbidden_api_scan"]["status"],
        "passed | failed | not_run"
    );
    assert_eq!(
        schema["response"]["artifact_gate"]["generated_jsx_write_performed"],
        "boolean, true only when status is generated_jsx"
    );
    assert_eq!(
        schema["response"]["artifact_gate"]["output_project_write_performed_by_rust"],
        "boolean, false in current v0 reports"
    );
    assert_eq!(
        schema["response"]["execution_gate"]["application_status"],
        "not_applied in no-AE v0 reports"
    );
    assert_eq!(
        schema["response"]["execution_gate"]["user_approval_status"],
        "required until a human approves AE execution"
    );
    assert_eq!(
        schema["response"]["execution_gate"]["after_effects_status"],
        "not_launched in automated v0 reports"
    );
    assert_eq!(
        schema["response"]["source_project_path"],
        "flat path alias for CLI reports"
    );
    assert_eq!(
        schema["response"]["generated_jsx_path"],
        "flat path alias for CLI reports or null"
    );
    assert_eq!(
        schema["response"]["source_metadata"]["project_body_read"],
        "false in no-AE v0 reports"
    );
    assert_eq!(
        schema["response"]["jsx_transaction_report_binding"]["algorithm"],
        "fnv1a64-v1-noncryptographic"
    );
    assert_eq!(
        schema["response"]["jsx_transaction_report_binding"]["payloads_embedded"],
        "false"
    );
    assert_eq!(
        schema["response"]["jsx_transaction_report_binding"]
            ["covers_project_or_generated_jsx_or_private_patch_payloads"],
        "false"
    );
    assert_eq!(
        schema["response"]["jsx_transaction_report_binding"]["cryptographic_digest"]["algorithm"],
        "sha256-v1"
    );
}

#[test]
fn jsx_export_manifest_schema_is_metadata_only_and_approval_gated() {
    let Some(schema) = load_analysis_json(EXPORT_MANIFEST_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["manifest"]["manifest_kind"]["current_value"],
        "ae_jsx_export_artifact_manifest"
    );
    assert_eq!(
        schema["manifest"]["publication_status"]["current_value"],
        "local-only"
    );
    assert_eq!(
        schema["manifest"]["export_report_schema"]["current_value"],
        "analysis/AE_JSX_EXPORT_REPORT_SCHEMA_2026-06-01.json"
    );
    assert_eq!(
        schema["manifest"]["payload_privacy"]["snapshot_json_embedded"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["payload_privacy"]["generated_jsx_embedded"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["payload_privacy"]["private_payloads_embedded"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["summary_fields"]["generated_jsx_artifact_digest"]["algorithm"]
            ["current_value"],
        "sha256-v1"
    );
    assert_eq!(
        schema["manifest"]["summary_fields"]["generated_jsx_artifact_digest"]
            ["generated_jsx_embedded"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["execution_expectations"]["after_effects_launch_expected"]
            ["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["execution_expectations"]["jsx_execution_expected"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["execution_expectations"]["project_write_expected"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["execution_expectations"]["source_overwrite_expected"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["human_approval_gate"]["approval_status"]["current_value"],
        "required_pending"
    );
    assert_eq!(
        schema["manifest"]["human_approval_gate"]["approval_receipt_required"]["current_value"],
        true
    );
    assert_eq!(
        schema["manifest"]["human_approval_gate"]["required_before_ae_execution"]["current_value"],
        true
    );
    assert_eq!(
        schema["manifest"]["human_approval_gate"]["required_before_jsx_execution"]["current_value"],
        true
    );
    assert_eq!(
        schema["manifest"]["human_approval_gate"]["required_before_project_write"]["current_value"],
        true
    );
    assert_eq!(
        schema["manifest"]["binding"]["algorithm"]["current_value"],
        "fnv1a64-v1-noncryptographic"
    );
    assert_eq!(
        schema["manifest"]["binding"]["payloads_embedded"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["binding"]["covers_snapshot_or_jsx_bodies"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["binding"]["cryptographic_digest"]["algorithm"]["current_value"],
        "sha256-v1"
    );
    assert_eq!(
        schema["manifest"]["binding"]["cryptographic_digest"]["payloads_embedded"]["current_value"],
        false
    );
    assert_eq!(
        schema["manifest"]["binding"]["cryptographic_digest"]["covers_snapshot_or_jsx_bodies"]
            ["current_value"],
        false
    );
    assert_eq!(
        schema["cli_boundary"]["optional_manifest_flag"],
        "--manifest manifest.json"
    );
    assert_eq!(
        schema["cli_boundary"]["manifest_output_write_mode"],
        "create_new"
    );
    assert_eq!(schema["cli_boundary"]["after_effects_launch"], false);
    assert_eq!(schema["cli_boundary"]["jsx_execution"], false);
    assert_eq!(schema["cli_boundary"]["project_write"], false);
    assert_eq!(schema["cli_boundary"]["source_overwrite"], false);
}

#[test]
fn jsx_export_artifact_verify_schema_is_pre_approval_and_no_runtime() {
    let Some(schema) = load_analysis_json(EXPORT_ARTIFACT_VERIFY_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["scope"]["accepted_manifest_kind"],
        "ae_jsx_export_artifact_manifest"
    );
    assert_eq!(schema["scope"]["not_an_approval_receipt"], true);
    assert_eq!(schema["scope"]["not_a_runtime_oracle"], true);
    assert_eq!(schema["scope"]["does_not_apply_jsx"], true);
    assert_eq!(
        schema["report"]["report_kind"]["current_value"],
        "ae_jsx_export_artifact_verification"
    );
    assert_eq!(
        schema["report"]["verification_status"]["accepted_value"],
        "verified"
    );
    assert_eq!(
        schema["report"]["generated_jsx_digest"]["current_algorithm"],
        "sha256-v1"
    );
    assert_eq!(
        schema["report"]["generated_jsx_digest"]["generated_jsx_embedded"],
        false
    );
    assert_eq!(
        schema["report"]["manifest_binding"]["binding_recalculation_required"],
        true
    );
    assert_eq!(
        schema["report"]["manifest_binding"]["covers_generated_jsx_body_directly"],
        false
    );
    assert_eq!(
        schema["report"]["manifest_binding"]["covers_generated_jsx_body_digest_fields"],
        true
    );
    assert_eq!(
        schema["cli_boundary"]["verification_report_write_mode"],
        "create_new"
    );
    assert_eq!(schema["cli_boundary"]["after_effects_launch"], false);
    assert_eq!(schema["cli_boundary"]["jsx_execution"], false);
    assert_eq!(schema["cli_boundary"]["project_write"], false);
    assert_eq!(schema["cli_boundary"]["source_overwrite"], false);
}

#[test]
fn jsx_export_approval_schema_is_separate_and_operator_only() {
    let Some(schema) = load_analysis_json(EXPORT_APPROVAL_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["scope"]["accepted_manifest_kind"],
        "ae_jsx_export_artifact_manifest"
    );
    assert_eq!(
        schema["scope"]["rejected_manifest_kind"],
        "ae_project_edit_ir_jsx_request_pair"
    );
    assert_eq!(schema["scope"]["not_a_tool_execution_permission"], true);
    assert_eq!(
        schema["receipt"]["receipt_kind"]["current_value"],
        "ae_jsx_export_manual_approval_receipt"
    );
    assert_eq!(
        schema["receipt"]["approval_effect"]["allow_manual_after_effects_launch"],
        true
    );
    assert_eq!(
        schema["receipt"]["approval_effect"]["allow_tool_after_effects_launch"],
        false
    );
    assert_eq!(
        schema["receipt"]["approval_effect"]["allow_tool_jsx_execution"],
        false
    );
    assert_eq!(
        schema["receipt"]["approval_effect"]["allow_tool_project_write"],
        false
    );
    assert_eq!(
        schema["receipt"]["approval_scope"]["binding_recalculation_required"],
        true
    );
    assert_eq!(
        schema["validation_report"]["manifest_acceptance_scope"]["current_value"],
        "ae_jsx_export_artifact_manifest_only"
    );
    assert_eq!(
        schema["validation_report"]["request_pair_manifest_accepted"],
        false
    );
    assert_eq!(
        schema["validation_report"]["export_manifest_binding_verified"]["accepted_value"],
        true
    );
    assert_eq!(schema["cli_boundary"]["after_effects_launch"], false);
    assert_eq!(schema["cli_boundary"]["jsx_execution"], false);
    assert_eq!(schema["cli_boundary"]["project_write"], false);
    assert_eq!(schema["cli_boundary"]["source_overwrite"], false);
}

#[test]
fn jsx_export_closeout_schema_is_json_only_and_not_an_oracle() {
    let Some(schema) = load_analysis_json(EXPORT_CLOSEOUT_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["scope"]["accepted_manifest_kind"],
        "ae_jsx_export_artifact_manifest"
    );
    assert_eq!(
        schema["scope"]["approval_receipt_kind"],
        "ae_jsx_export_manual_approval_receipt"
    );
    assert_eq!(schema["scope"]["validator_reads_only_json_artifacts"], true);
    assert_eq!(schema["scope"]["not_an_ae_oracle"], true);
    assert_eq!(
        schema["closeout"]["metadata_binding_recalculation_required"],
        true
    );
    assert_eq!(
        schema["closeout"]["always_forbidden"]["after_effects_launched_by_tool"],
        false
    );
    assert_eq!(
        schema["closeout"]["always_forbidden"]["project_write_performed_by_tool"],
        false
    );
    assert_eq!(
        schema["validation_report"]["tool_side_effects"]["jsx_executed_by_tool"],
        false
    );
    assert_eq!(schema["cli_boundary"]["after_effects_launch"], false);
    assert_eq!(schema["cli_boundary"]["jsx_execution"], false);
    assert_eq!(schema["cli_boundary"]["project_write"], false);
    assert_eq!(schema["cli_boundary"]["source_overwrite"], false);
}

#[test]
fn patch_spec_and_handoff_document_the_safety_contract() {
    let Some(spec) = load_analysis_text(PATCH_SPEC) else {
        return;
    };
    let Some(handoff) = load_analysis_text(PATCH_HANDOFF) else {
        return;
    };

    for expected in [
        "top-level `operation` means the tool mode",
        "`operations[].kind` means the individual edit operation",
        "Top-level AEPX modes",
        "Top-level JSX modes",
        "Forbidden-token scanning should run on generated code outside the escaped JSON",
        "payload. If a forbidden token appears inside user-supplied patch data",
        "v0 should fail closed for `unknown` before writing",
        "`report_private_payloads=true` is invalid",
        "same-source/output path should report `output_same_as_source`",
        "`export_ae_jsx --manifest manifest.json`",
        "It is review metadata, not approval to run AE.",
        "JSX export artifact verification",
        "`ae_jsx_export_artifact_verify`",
        "not an approval receipt and not tool-side execution permission",
        "Standalone JSX export approval",
        "`ae_jsx_export_approval`",
        "`ae_jsx_export_manual_approval_receipt`",
        "recalculates the manifest's metadata-only FNV checksum",
        "not tool-side execution permission",
        "Standalone JSX export closeout",
        "`ae_jsx_export_smoke_closeout`",
        "not an AE/runtime oracle",
        "`ae_project_edit_ir_jsx_request_pair` manifests as approval scope",
        "reject `ae_jsx_export_artifact_manifest`",
        "`validation_status=review_ready`",
        "`ae_project_edit_review_packet`",
        "`review_ready` is pre-approval",
        "consistency evidence only",
        "`dry_run_ok` report as no-write evidence",
        "permit AEPX XML apply",
    ] {
        assert!(spec.contains(expected), "spec missing {expected:?}");
    }

    for forbidden in [
        "`system.callSystem`",
        "`File.openDialog`",
        "`Folder.selectDialog`",
        "`app.executeCommand`",
        "`BridgeTalk`",
        "`eval`",
        "`Function`",
        "`$.evalFile`",
        "`File.execute`",
        "`ExternalObject`",
        "`Socket`",
        "`File.open`",
        "`File.write`",
        "`File.remove`",
        "`File.rename`",
        "`File.copy`",
        "`afterfx`",
        "`aerender`",
        "`cmd.exe`",
        "`powershell`",
        "`wscript`",
        "`alert(`",
        "bare `app.project.save()`",
    ] {
        assert!(
            spec.contains(forbidden) || handoff.contains(forbidden),
            "forbidden JSX surface should be documented: {forbidden}"
        );
    }

    for expected in [
        "Reject pairwise equality among source project, output project, and generated",
        "`run_after_effects=true` produces a plan-only status and does not spawn AE",
        "`report_private_payloads=true` is rejected",
        "`publication_status=unknown` fails closed before writes",
        "same source/output path reports `output_same_as_source`",
        "Do not launch After Effects in automated tests.",
        "`--manifest manifest.json`",
        "it is not an approval receipt",
        "Added `ae_jsx_export_artifact_verify`",
        "local consistency evidence only",
        "not tool-side execution permission",
        "Added `ae_jsx_export_approval`",
        "`ae_jsx_export_manual_approval_receipt`",
        "recalculates",
        "not tool-side execution permission",
        "Added `ae_jsx_export_smoke_closeout`",
        "consistency evidence only, not an AE/runtime oracle",
        "rejects `ae_jsx_export_artifact_manifest`",
        "`validation_status=review_ready`",
        "a receipt by itself is rejected",
        "standalone export manifests",
        "remain review metadata",
        "`ae_project_edit_review_packet`",
        "`review_ready` is not approval",
        "AEPX `dry_run_ok` report as no-write",
        "AEPX XML apply",
    ] {
        assert!(handoff.contains(expected), "handoff missing {expected:?}");
    }
}

#[test]
fn operator_runbook_preserves_manual_only_evidence_chain() {
    let Some(runbook) = load_analysis_text(OPERATOR_RUNBOOK) else {
        return;
    };
    let Some(spec) = load_analysis_text(PATCH_SPEC) else {
        return;
    };
    let Some(handoff) = load_analysis_text(PATCH_HANDOFF) else {
        return;
    };

    assert!(
        spec.contains(OPERATOR_RUNBOOK),
        "patch spec should reference the operator runbook"
    );
    assert!(
        handoff.contains(OPERATOR_RUNBOOK),
        "handoff should reference the operator runbook"
    );

    for expected in [
        "This runbook is an operating checklist only.",
        "Direct binary `.aep` writes.",
        "Tool-side After Effects launch.",
        "Tool-side JSX execution.",
        "Source project overwrite.",
        "Any validator reports `invalid_*`, `blocked`, or a non-empty",
        "Any approval template still has `approval_status=draft_unapproved`.",
        "Any artifact verification report is missing, not `verified`",
        "Any review packet is missing or not `review_ready`",
        "cargo run --example export_ae_jsx",
        "cargo run --example ae_jsx_export_artifact_verify",
        "cargo run --example ae_jsx_export_approval",
        "cargo run --example ae_jsx_export_smoke_closeout",
        "cargo run --example ae_project_edit_review_packet",
        "cargo run --example ae_manual_smoke_closeout",
        "`verification_status=verified`",
        "`validation_status=approved`",
        "`validation_status=review_ready`",
        "`validation_status=accepted`",
        "`accepted` means internal JSON consistency evidence only",
        "Do not use standalone",
        "A metadata binding proves only",
        "A generated JSX artifact digest proves only",
        "An approval receipt records an operator decision only",
        "A closeout records operator-reported smoke evidence only",
        "Do not add AE launch, JSX execution, direct `.aep` write, AEX/OFX loading, or AviUtl core",
    ] {
        assert!(
            runbook.contains(expected),
            "operator runbook missing {expected:?}"
        );
    }
}

#[test]
fn aepx_writer_spike_doc_keeps_preservation_result_bounded() {
    let Some(spike) = load_analysis_text(AEPX_WRITER_SPIKE_DOC) else {
        return;
    };

    for expected in [
        "Synthetic `.aepx`-like fixtures only.",
        "No binary `.aep` parsing or writing.",
        "No After Effects launch.",
        "No production `aepx_patch_probe` apply integration in this spike.",
        "No new XML crate or third-party dependency was added.",
        "exact-id composition rename",
        "exact old-value guard",
        "create-new output only",
        "source overwrite never performed",
        "unknown elements, unknown attributes, XML declaration, UTF-8 declaration",
        "ambiguous name-only composition selectors",
        "expected-old-value mismatch",
        "existing output paths",
        "This is **not** a production XML writer.",
        "not a production XML writer, not an XML library choice, and not an AE compatibility claim",
        "Future XML library candidates still require exact license-file audit",
    ] {
        assert!(
            spike.contains(expected),
            "writer spike doc missing bounded claim {expected:?}"
        );
    }
}
