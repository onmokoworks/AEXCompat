#[allow(dead_code)]
#[path = "../examples/aepx_patch_probe.rs"]
mod aepx_patch_probe;

use std::path::{Path, PathBuf};

use aepx_patch_probe::run_aepx_patch_request_text;

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-patch-probe")
        .join(name)
}

fn abs_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn request(mode: &str, output_aepx: Option<&Path>) -> String {
    let output_field = output_aepx
        .map(|path| format!(r#","output_aepx":"{}""#, abs_string(path)))
        .unwrap_or_default();
    format!(
        r#"{{
  "schema_version": 1,
  "operation": "{mode}",
  "input_aepx": "D:/AviUtlas/local/source.aepx"{output_field},
  "patch_id": "contract-test",
  "publication_status": "local-only",
  "operations": [
    {{
      "id": "op_001",
      "kind": "rename_comp",
      "target": {{
        "kind": "comp",
        "selector": {{
          "name": "Main"
        }}
      }},
      "expected_old_value": "Main",
      "new_value": "Main Patched",
      "user_supplied": true
    }}
  ],
  "options": {{
    "overwrite": false,
    "allow_ambiguous_selector": false,
    "preserve_unknown_xml": "required",
    "normalization_mode": "none",
    "max_file_bytes": 10485760,
    "max_operations": 10,
    "report_private_payloads": false
  }}
}}"#
    )
}

fn request_value(mode: &str, output_aepx: Option<&Path>) -> serde_json::Value {
    serde_json::from_str(&request(mode, output_aepx)).unwrap()
}

fn request_text_from_value(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap()
}

fn preservation_fields(report: &aepx_patch_probe::AepxPatchReport) -> [&str; 8] {
    [
        report.preservation.unknown_nodes.as_str(),
        report.preservation.unknown_attributes.as_str(),
        report.preservation.xml_declaration.as_str(),
        report.preservation.encoding.as_str(),
        report.preservation.comments.as_str(),
        report.preservation.cdata.as_str(),
        report.preservation.namespace_prefixes.as_str(),
        report.preservation.whitespace.as_str(),
    ]
}

fn assert_no_write_gate(report: &aepx_patch_probe::AepxPatchReport) {
    assert_eq!(
        report.aepx_patch_report_binding.algorithm,
        "fnv1a64-v1-noncryptographic"
    );
    assert!(report
        .aepx_patch_report_binding
        .checksum_hex
        .chars()
        .all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(report.aepx_patch_report_binding.checksum_hex.len(), 16);
    assert!(!report.aepx_patch_report_binding.payloads_embedded);
    assert!(
        !report
            .aepx_patch_report_binding
            .covers_xml_or_private_patch_payloads
    );
    assert_eq!(
        report
            .aepx_patch_report_binding
            .cryptographic_digest
            .algorithm,
        "sha256-v1"
    );
    assert_eq!(
        report
            .aepx_patch_report_binding
            .cryptographic_digest
            .digest_hex
            .len(),
        64
    );
    assert!(report
        .aepx_patch_report_binding
        .cryptographic_digest
        .digest_hex
        .chars()
        .all(|ch| ch.is_ascii_hexdigit()));
    assert!(
        !report
            .aepx_patch_report_binding
            .cryptographic_digest
            .payloads_embedded
    );
    assert!(
        !report
            .aepx_patch_report_binding
            .cryptographic_digest
            .covers_xml_or_private_patch_payloads
    );
    assert_eq!(report.write_gate.patch_application_status, "not_applied");
    assert_eq!(report.write_gate.xml_writer_status, "not_implemented");
    assert_eq!(
        report.write_gate.user_approval_status,
        "required_before_write"
    );
    assert!(!report.write_gate.output_write_performed);
    assert!(!report.write_gate.source_overwrite_performed);
    assert!(!report.io_gate.xml_body_read_performed);
    assert!(!report.io_gate.xml_body_write_performed);
    assert!(!report.io_gate.xml_body_embedded_in_report);
    assert!(!report.io_gate.private_patch_payloads_embedded_in_report);
    assert!(!report.io_gate.external_process_invoked);
    assert!(!report.io_gate.after_effects_invoked);
}

fn preservation_sentinel_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("aepx_preservation_sentinels.aepx")
}

#[test]
fn checked_in_dry_run_fixture_matches_current_guard_contract() {
    let fixture = include_str!("fixtures/aepx_patch_request.dry_run.json");
    let report = run_aepx_patch_request_text(fixture, None).unwrap();

    assert_eq!(report.status, "dry_run_ok");
    assert_eq!(report.operations[0].status, "planned");
    assert!(!report.metadata.xml_body_read);
    assert!(preservation_fields(&report)
        .iter()
        .all(|status| *status == "not_written"));
    assert_no_write_gate(&report);
}

#[test]
fn dry_run_validates_without_reading_or_writing_aepx() {
    let output = target_path("dry-run-output.aepx");
    let _ = std::fs::remove_file(&output);
    let report = run_aepx_patch_request_text(&request("dry_run", Some(&output)), None).unwrap();

    assert_eq!(report.status, "dry_run_ok");
    assert_eq!(
        report.output_aepx.as_deref(),
        Some(abs_string(&output).as_str())
    );
    assert_eq!(report.operations[0].status, "planned");
    assert_eq!(report.preservation.unknown_nodes, "not_written");
    assert_no_write_gate(&report);
    assert!(!output.exists());
}

#[test]
fn synthetic_preservation_fixture_is_fail_closed_without_echo_or_write() {
    let input = preservation_sentinel_fixture();
    let output = target_path("preservation-sentinel-dry-run-output.aepx");
    let _ = std::fs::remove_file(&output);
    let input_before = std::fs::read_to_string(&input).unwrap();
    let mut value = request_value("dry_run", Some(&output));
    value["input_aepx"] = serde_json::json!(abs_string(&input));

    let report = run_aepx_patch_request_text(&request_text_from_value(&value), None).unwrap();

    assert_eq!(report.status, "dry_run_ok");
    assert!(preservation_fields(&report)
        .iter()
        .all(|status| *status == "not_written"));
    assert!(!output.exists());
    assert_eq!(std::fs::read_to_string(&input).unwrap(), input_before);
    assert_no_write_gate(&report);

    let report_text = serde_json::to_string(&report).unwrap();
    for sentinel in [
        "UNKNOWN_NODE_SENTINEL",
        "UNKNOWN_ATTR_SENTINEL",
        "COMMENT_SENTINEL",
        "CDATA_SENTINEL",
        "PREFIX_SENTINEL",
    ] {
        assert!(
            !report_text.contains(sentinel),
            "report should not echo fixture body sentinel {sentinel}"
        );
    }
}

#[test]
fn synthetic_preservation_fixture_apply_still_writes_nothing() {
    let input = preservation_sentinel_fixture();
    let output = target_path("preservation-sentinel-apply-output.aepx");
    let _ = std::fs::remove_file(&output);
    let input_before = std::fs::read_to_string(&input).unwrap();
    let mut value = request_value("apply", Some(&output));
    value["input_aepx"] = serde_json::json!(abs_string(&input));

    let report = run_aepx_patch_request_text(&request_text_from_value(&value), None).unwrap();

    assert_eq!(report.status, "unsupported_operation");
    assert!(preservation_fields(&report)
        .iter()
        .all(|status| *status == "not_written"));
    assert!(report
        .unsupported
        .iter()
        .any(|item| item.contains("not implemented")));
    assert_no_write_gate(&report);
    assert!(!output.exists());
    assert_eq!(std::fs::read_to_string(&input).unwrap(), input_before);
}

#[test]
fn noop_validate_does_not_require_output_path() {
    let report = run_aepx_patch_request_text(&request("noop_validate", None), None).unwrap();

    assert_eq!(report.status, "dry_run_ok");
    assert!(report.output_aepx.is_none());
    assert_no_write_gate(&report);
}

#[test]
fn inspect_metadata_is_metadata_only_without_payloads() {
    let input = preservation_sentinel_fixture();
    let mut value = request_value("inspect_metadata", None);
    value["input_aepx"] = serde_json::json!(abs_string(&input));

    let report = run_aepx_patch_request_text(&request_text_from_value(&value), None).unwrap();

    assert_eq!(report.status, "ok");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("XML body not read")));
    assert!(preservation_fields(&report)
        .iter()
        .all(|status| *status == "not_written"));
    assert_no_write_gate(&report);

    let report_text = serde_json::to_string(&report).unwrap();
    assert!(!report_text.contains("UNKNOWN_NODE_SENTINEL"));
}

#[test]
fn inspect_metadata_reports_missing_source_without_payloads() {
    let report = run_aepx_patch_request_text(&request("inspect_metadata", None), None).unwrap();

    assert_eq!(report.status, "source_not_found");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("not found")));
    assert!(preservation_fields(&report)
        .iter()
        .all(|status| *status == "not_written"));
    assert_no_write_gate(&report);
}

#[test]
fn apply_requires_output_path() {
    let report = run_aepx_patch_request_text(&request("apply", None), None).unwrap();

    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("output_aepx")));
    assert_no_write_gate(&report);
}

#[test]
fn apply_same_path_reports_output_same_as_source() {
    let request = request("apply", Some(Path::new("d:/aviutlas/local/./SOURCE.aepx")));
    let report = run_aepx_patch_request_text(&request, None).unwrap();

    assert_eq!(report.status, "output_same_as_source");
}

#[test]
fn dry_run_output_path_collisions_fail_closed_before_planning() {
    for mode in ["dry_run", "noop_validate", "inspect_metadata"] {
        let same_path = request(mode, Some(Path::new("d:/aviutlas/local/./SOURCE.aepx")));
        let report = run_aepx_patch_request_text(&same_path, None).unwrap();

        assert_eq!(report.status, "output_same_as_source", "{mode}");
        assert!(report.operations.is_empty(), "{mode}");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("distinct")),
            "{mode}"
        );
    }

    let output = target_path("dry-run-existing-output.aepx");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&output, "existing dry-run output").unwrap();

    for mode in ["dry_run", "noop_validate", "inspect_metadata"] {
        let report = run_aepx_patch_request_text(&request(mode, Some(&output)), None).unwrap();

        assert_eq!(report.status, "output_exists", "{mode}");
        assert!(report.operations.is_empty(), "{mode}");
        assert_eq!(
            std::fs::read_to_string(&output).unwrap(),
            "existing dry-run output",
            "{mode}"
        );
    }
    let _ = std::fs::remove_file(&output);
}

#[test]
fn apply_existing_output_reports_output_exists_and_does_not_write() {
    let output = target_path("existing-output.aepx");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&output, "existing").unwrap();

    let report = run_aepx_patch_request_text(&request("apply", Some(&output)), None).unwrap();

    assert_eq!(report.status, "output_exists");
    assert_no_write_gate(&report);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "existing");
    let _ = std::fs::remove_file(&output);
}

#[test]
fn valid_apply_stops_before_xml_write() {
    let output = target_path("valid-apply-output.aepx");
    let _ = std::fs::remove_file(&output);

    let report = run_aepx_patch_request_text(&request("apply", Some(&output)), None).unwrap();

    assert_eq!(report.status, "unsupported_operation");
    assert!(report
        .unsupported
        .iter()
        .any(|item| item.contains("not implemented")));
    assert_no_write_gate(&report);
    assert!(!output.exists());
}

#[test]
fn unsafe_options_and_unknown_fields_are_rejected() {
    for (request_text, expected) in [
        (
            request("dry_run", None).replace("\"overwrite\": false", "\"overwrite\": true"),
            "overwrite",
        ),
        (
            request("dry_run", None).replace(
                "\"allow_ambiguous_selector\": false",
                "\"allow_ambiguous_selector\": true",
            ),
            "ambiguous",
        ),
        (
            request("dry_run", None).replace(
                "\"preserve_unknown_xml\": \"required\"",
                "\"preserve_unknown_xml\": \"best_effort\"",
            ),
            "preserve_unknown_xml",
        ),
        (
            request("dry_run", None).replace(
                "\"report_private_payloads\": false",
                "\"report_private_payloads\": true",
            ),
            "report_private_payloads",
        ),
        (
            request("dry_run", None).replace(
                "\"patch_id\": \"contract-test\"",
                "\"patch_id\": \"contract-test\", \"xml_payload\": \"private\"",
            ),
            "parse",
        ),
    ] {
        let report = run_aepx_patch_request_text(&request_text, None).unwrap();
        assert_eq!(report.status, "invalid_request");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains(expected)),
            "expected warning containing {expected:?}, got {:?}",
            report.warnings
        );
    }
}

#[test]
fn unknown_fields_are_rejected_at_nested_layers() {
    let mut cases = Vec::new();

    let mut top = request_value("dry_run", None);
    top["extra_top_level"] = serde_json::json!(true);
    cases.push(top);

    let mut options = request_value("dry_run", None);
    options["options"]["extra_option"] = serde_json::json!(true);
    cases.push(options);

    let mut operation = request_value("dry_run", None);
    operation["operations"][0]["extra_operation"] = serde_json::json!(true);
    cases.push(operation);

    let mut target = request_value("dry_run", None);
    target["operations"][0]["target"]["extra_target"] = serde_json::json!(true);
    cases.push(target);

    let mut selector = request_value("dry_run", None);
    selector["operations"][0]["target"]["selector"]["private_extra"] = serde_json::json!("secret");
    cases.push(selector);

    for case in cases {
        let report = run_aepx_patch_request_text(&request_text_from_value(&case), None).unwrap();
        assert_eq!(report.status, "invalid_request");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("parse")),
            "expected parse warning, got {:?}",
            report.warnings
        );
    }
}

#[test]
fn schema_mode_and_target_contracts_fail_closed() {
    let mut bad_schema = request_value("dry_run", None);
    bad_schema["schema_version"] = serde_json::json!(2);
    let report = run_aepx_patch_request_text(&request_text_from_value(&bad_schema), None).unwrap();
    assert_eq!(report.status, "invalid_request");

    let report = run_aepx_patch_request_text(&request("mutate_in_place", None), None).unwrap();
    assert_eq!(report.status, "invalid_request");

    let mut bad_target = request_value("dry_run", None);
    bad_target["operations"][0]["target"]["kind"] = serde_json::json!("camera");
    let report = run_aepx_patch_request_text(&request_text_from_value(&bad_target), None).unwrap();
    assert_eq!(report.status, "invalid_request");
}

#[test]
fn default_max_operations_and_max_file_bytes_are_enforced() {
    let mut too_many = request_value("dry_run", None);
    let operation = too_many["operations"][0].clone();
    too_many["options"]
        .as_object_mut()
        .unwrap()
        .remove("max_operations");
    too_many["operations"] = serde_json::Value::Array(vec![operation; 101]);
    let report = run_aepx_patch_request_text(&request_text_from_value(&too_many), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("max_operations")));

    let input = target_path("oversize-input.aepx");
    if let Some(parent) = input.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&input, "larger-than-limit").unwrap();
    let mut oversized = request_value("dry_run", None);
    oversized["input_aepx"] = serde_json::json!(abs_string(&input));
    oversized["options"]["max_file_bytes"] = serde_json::json!(3);
    let report = run_aepx_patch_request_text(&request_text_from_value(&oversized), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("max_file_bytes")));
    let _ = std::fs::remove_file(&input);
}

#[test]
fn user_supplied_sensitive_operations_and_ambiguous_selectors_are_rejected() {
    for kind in ["replace_text_source", "relink_asset_path"] {
        let mut value = request_value("dry_run", None);
        value["operations"][0]["kind"] = serde_json::json!(kind);
        value["operations"][0]["target"]["kind"] = serde_json::json!("text_source");
        value["operations"][0]["target"]["selector"] = serde_json::json!({"xml_id": "txt-1"});
        value["operations"][0]["user_supplied"] = serde_json::json!(false);
        if kind == "relink_asset_path" {
            value["operations"][0]["target"]["kind"] = serde_json::json!("asset");
            value["operations"][0]["target"]["selector"] =
                serde_json::json!({"xml_id": "asset-1", "asset_path": "D:/media/source.png"});
        }

        let report = run_aepx_patch_request_text(&request_text_from_value(&value), None).unwrap();
        assert_eq!(report.status, "invalid_request");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("user_supplied")));
    }

    let mut comment = request_value("dry_run", None);
    comment["operations"][0]["kind"] = serde_json::json!("set_comment");
    comment["operations"][0]["target"]["kind"] = serde_json::json!("comp");
    comment["operations"][0]["target"]["selector"] = serde_json::json!({"xml_id": "comp-1"});
    comment["operations"][0]["expected_old_value"] = serde_json::json!("");
    comment["operations"][0]["new_value"] = serde_json::json!("Reviewed comment");
    comment["operations"][0]["user_supplied"] = serde_json::json!(false);
    let report = run_aepx_patch_request_text(&request_text_from_value(&comment), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("set_comment requires user_supplied true")));

    let mut marker = request_value("dry_run", None);
    marker["operations"][0]["kind"] = serde_json::json!("set_marker");
    marker["operations"][0]["target"]["kind"] = serde_json::json!("marker");
    marker["operations"][0]["target"]["selector"] =
        serde_json::json!({"comp_name": "Main", "marker_index": 0});
    marker["operations"][0]["expected_old_value"] =
        serde_json::json!({"time_seconds": 0.0, "comment": "old marker"});
    marker["operations"][0]["new_value"] =
        serde_json::json!({"time_seconds": 1.5, "comment": "Reviewed marker"});
    marker["operations"][0]["user_supplied"] = serde_json::json!(false);
    let report = run_aepx_patch_request_text(&request_text_from_value(&marker), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("set_marker requires user_supplied true")));

    let mut ambiguous = request_value("dry_run", None);
    ambiguous["operations"][0]["kind"] = serde_json::json!("rename_layer");
    ambiguous["operations"][0]["target"]["kind"] = serde_json::json!("layer");
    ambiguous["operations"][0]["target"]["selector"] = serde_json::json!({"name": "Layer 1"});
    let report = run_aepx_patch_request_text(&request_text_from_value(&ambiguous), None).unwrap();
    assert_eq!(report.status, "ambiguous_target");
}

#[test]
fn mutating_operations_require_expected_old_value_guards() {
    for (kind, target_kind, selector, new_value) in [
        (
            "rename_comp",
            "comp",
            serde_json::json!({"name": "Main"}),
            serde_json::json!("Main Patched"),
        ),
        (
            "rename_layer",
            "layer",
            serde_json::json!({"xml_id": "layer-1"}),
            serde_json::json!("Layer Patched"),
        ),
        (
            "set_comment",
            "comp",
            serde_json::json!({"xml_id": "comp-1"}),
            serde_json::json!("Reviewed comment"),
        ),
        (
            "set_marker",
            "marker",
            serde_json::json!({"comp_name": "Main", "marker_index": 0}),
            serde_json::json!({"comment": "Reviewed marker"}),
        ),
    ] {
        let mut value = request_value("dry_run", None);
        value["operations"][0]["kind"] = serde_json::json!(kind);
        value["operations"][0]["target"]["kind"] = serde_json::json!(target_kind);
        value["operations"][0]["target"]["selector"] = selector;
        value["operations"][0]["new_value"] = new_value;
        value["operations"][0]["expected_old_value"] = serde_json::Value::Null;

        let report = run_aepx_patch_request_text(&request_text_from_value(&value), None).unwrap();
        assert_eq!(report.status, "invalid_request", "{kind}");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("expected_old_value guard")),
            "expected guard warning for {kind}, got {:?}",
            report.warnings
        );
    }
}

#[test]
fn sensitive_aepx_operations_require_precise_targets_and_payload_shapes() {
    let mut text = request_value("dry_run", None);
    text["operations"][0]["kind"] = serde_json::json!("replace_text_source");
    text["operations"][0]["target"]["kind"] = serde_json::json!("text_source");
    text["operations"][0]["target"]["selector"] = serde_json::json!({"xml_id": "txt-1"});
    text["operations"][0]["expected_old_value"] = serde_json::json!("old text");
    text["operations"][0]["new_value"] = serde_json::json!("new text");
    text["operations"][0]["user_supplied"] = serde_json::json!(true);
    let report = run_aepx_patch_request_text(&request_text_from_value(&text), None).unwrap();
    assert_eq!(report.status, "dry_run_ok");

    let mut wrong_text_target = text.clone();
    wrong_text_target["operations"][0]["target"]["kind"] = serde_json::json!("layer");
    let report =
        run_aepx_patch_request_text(&request_text_from_value(&wrong_text_target), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("target.kind must be text_source")));

    let mut missing_guard = text.clone();
    missing_guard["operations"][0]["expected_old_value"] = serde_json::Value::Null;
    let report =
        run_aepx_patch_request_text(&request_text_from_value(&missing_guard), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("expected_old_value guard")));

    let mut numeric_text = text.clone();
    numeric_text["operations"][0]["new_value"] = serde_json::json!(42);
    let report =
        run_aepx_patch_request_text(&request_text_from_value(&numeric_text), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("replace_text_source new_value must be a string")));

    let mut marker = request_value("dry_run", None);
    marker["operations"][0]["kind"] = serde_json::json!("set_marker");
    marker["operations"][0]["target"]["kind"] = serde_json::json!("marker");
    marker["operations"][0]["target"]["selector"] =
        serde_json::json!({"comp_name": "Main", "marker_index": 0});
    marker["operations"][0]["expected_old_value"] =
        serde_json::json!({"time_seconds": 0.0, "comment": "old marker"});
    marker["operations"][0]["new_value"] =
        serde_json::json!({"time_seconds": 1.5, "comment": "Reviewed marker"});
    marker["operations"][0]["user_supplied"] = serde_json::json!(true);
    let report = run_aepx_patch_request_text(&request_text_from_value(&marker), None).unwrap();
    assert_eq!(report.status, "dry_run_ok");

    let mut marker_missing_time = marker.clone();
    marker_missing_time["operations"][0]["new_value"] =
        serde_json::json!({"comment": "Reviewed marker"});
    let report =
        run_aepx_patch_request_text(&request_text_from_value(&marker_missing_time), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("new_value.time_seconds")));

    let mut relink = request_value("dry_run", None);
    relink["operations"][0]["kind"] = serde_json::json!("relink_asset_path");
    relink["operations"][0]["target"]["kind"] = serde_json::json!("asset");
    relink["operations"][0]["target"]["selector"] =
        serde_json::json!({"xml_id": "asset-1", "asset_path": "D:/media/source.png"});
    relink["operations"][0]["expected_old_value"] = serde_json::json!("D:/media/source.png");
    relink["operations"][0]["new_value"] = serde_json::json!("D:/media/replacement.png");
    relink["operations"][0]["user_supplied"] = serde_json::json!(true);
    let report = run_aepx_patch_request_text(&request_text_from_value(&relink), None).unwrap();
    assert_eq!(report.status, "dry_run_ok");

    let mut wrong_relink_target = relink.clone();
    wrong_relink_target["operations"][0]["target"]["kind"] = serde_json::json!("text_source");
    let report =
        run_aepx_patch_request_text(&request_text_from_value(&wrong_relink_target), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("target.kind must be asset")));

    let mut numeric_relink = relink.clone();
    numeric_relink["operations"][0]["new_value"] = serde_json::json!(42);
    let report =
        run_aepx_patch_request_text(&request_text_from_value(&numeric_relink), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("relink_asset_path new_value must be a non-empty string")));

    let mut mismatched_relink_guard = relink.clone();
    mismatched_relink_guard["operations"][0]["expected_old_value"] =
        serde_json::json!("D:/media/other.png");
    let report =
        run_aepx_patch_request_text(&request_text_from_value(&mismatched_relink_guard), None)
            .unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("expected_old_value must match selector.asset_path")));

    let mut missing_relink_guard = relink.clone();
    missing_relink_guard["operations"][0]["expected_old_value"] = serde_json::Value::Null;
    let report =
        run_aepx_patch_request_text(&request_text_from_value(&missing_relink_guard), None).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("expected_old_value guard")));
}

#[test]
fn reports_do_not_echo_private_patch_payloads() {
    let mut value = request_value("dry_run", None);
    value["operations"][0]["target"]["selector"] = serde_json::json!({"name": "PRIVATE_SELECTOR"});
    value["operations"][0]["expected_old_value"] = serde_json::json!("PRIVATE_OLD_VALUE");
    value["operations"][0]["new_value"] = serde_json::json!("PRIVATE_NEW_VALUE");

    let report = run_aepx_patch_request_text(&request_text_from_value(&value), None).unwrap();
    assert_eq!(report.status, "dry_run_ok");
    let report_text = serde_json::to_string(&report).unwrap();
    assert!(!report_text.contains("PRIVATE_SELECTOR"));
    assert!(!report_text.contains("PRIVATE_OLD_VALUE"));
    assert!(!report_text.contains("PRIVATE_NEW_VALUE"));
}

#[test]
fn unsupported_kind_and_empty_selector_are_rejected() {
    let unsupported = request("dry_run", None).replace("rename_comp", "run_arbitrary_js");
    let report = run_aepx_patch_request_text(&unsupported, None).unwrap();
    assert_eq!(report.status, "unsupported_operation");

    let empty_selector = request("dry_run", None).replace(
        r#""selector": {
          "name": "Main"
        }"#,
        r#""selector": {}"#,
    );
    let report = run_aepx_patch_request_text(&empty_selector, None).unwrap();
    assert_eq!(report.status, "invalid_request");
}
