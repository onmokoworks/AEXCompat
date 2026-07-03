#[allow(dead_code)]
#[path = "../examples/aepx_synthetic_preservation_proof.rs"]
mod aepx_synthetic_preservation_proof;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const PROOF_SCHEMA: &str =
    include_str!("../../analysis/AEPX_SYNTHETIC_PRESERVATION_PROOF_SCHEMA_2026-06-01.json");

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-synthetic-preservation-proof")
        .join(format!("{}-{name}", std::process::id()))
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn abs_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn request(output: &Path) -> Value {
    json!({
        "schema_version": 1,
        "proof_kind": "aepx_synthetic_preservation_proof",
        "input_fixture_name": "aepx_writer_spike_preservation.aepx",
        "output_aepx": abs_string(output),
        "operation": {
            "kind": "rename_comp",
            "selector_kind": "xml_id",
            "selector_value": "comp-main",
            "expected_old_value": "Main",
            "new_value": "Main Reviewed"
        },
        "options": {
            "synthetic_fixture_only": true,
            "create_new_only": true,
            "preserve_unknown_xml": "required",
            "report_private_payloads": false
        }
    })
}

fn run(value: &Value) -> Value {
    let report = aepx_synthetic_preservation_proof::run_synthetic_preservation_proof_request_json(
        &serde_json::to_string(value).unwrap(),
    )
    .expect("synthetic preservation proof should return JSON");
    serde_json::from_str(&report).expect("proof report should parse")
}

fn preservation_fields(report: &Value) -> [&Value; 8] {
    [
        &report["preservation"]["unknown_nodes"],
        &report["preservation"]["unknown_attributes"],
        &report["preservation"]["xml_declaration"],
        &report["preservation"]["encoding"],
        &report["preservation"]["comments"],
        &report["preservation"]["cdata"],
        &report["preservation"]["namespace_prefixes"],
        &report["preservation"]["whitespace"],
    ]
}

fn assert_report_omits_sensitive_values(report: &Value) {
    let serialized = serde_json::to_string(report).unwrap();
    for forbidden in [
        "SPIKE_COMMENT_SENTINEL",
        "UNKNOWN_NODE_SENTINEL",
        "UNKNOWN_ATTR_SENTINEL",
        "SPIKE_CDATA_SENTINEL",
        "PREFIX_SENTINEL",
        "comp-main",
        "Main Reviewed",
        "Wrong Old",
        "aepx_writer_spike_preservation.aepx",
        "aepx_writer_spike_crlf_bom.aepx",
        "target/aepx-synthetic-preservation-proof",
        "CRLF_BOM_COMMENT_SENTINEL",
        "CRLF_BOM_NODE_SENTINEL",
        "comp-crlf-bom",
        "CRLF Main",
        "CRLF Reviewed",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "proof report should not echo {forbidden}"
        );
    }
}

fn assert_sentinels_survive(patched: &str) {
    for sentinel in [
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        "SPIKE_COMMENT_SENTINEL",
        "meta:UNKNOWN_ATTR_SENTINEL=\"keep\"",
        "<xmp:UNKNOWN_NODE_SENTINEL oddSpacing = \"  keep  \">",
        "<![CDATA[SPIKE_CDATA_SENTINEL <raw>&value</raw>]]>",
        "meta:prefixAttr=\"SPIKE_PREFIX_ATTR_SENTINEL\"",
        "<prefix:PREFIX_SENTINEL xmlns:prefix=\"urn:aviutlas:synthetic:prefix\"/>",
    ] {
        assert!(
            patched.contains(sentinel),
            "patched XML should preserve {sentinel}"
        );
    }
}

fn assert_single_replacement_only(source: &str, patched: &str, old: &str, new: &str) {
    let start = source.find(old).expect("old token should exist in source");
    assert_eq!(&patched[..start], &source[..start]);
    assert_eq!(&patched[start..start + new.len()], new);
    assert_eq!(&patched[start + new.len()..], &source[start + old.len()..]);
}

fn json_string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("expected array")
        .iter()
        .map(|item| item.as_str().expect("expected string"))
        .collect()
}

fn assert_object_has_fields(value: &Value, fields: &[&str], label: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{label} should be object"));
    for field in fields {
        assert!(
            object.contains_key(*field),
            "{label} should contain field {field}"
        );
    }
}

fn assert_report_matches_schema(report: &Value, schema: &Value) {
    assert_object_has_fields(
        report,
        &json_string_array(&schema["required_fields"]),
        "report",
    );
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(&report[field], expected, "field {field} diverged");
    }
    assert!(
        json_string_array(&schema["allowed_statuses"])
            .contains(&report["status"].as_str().expect("status should be string")),
        "unexpected status"
    );
    assert_object_has_fields(
        &report["preservation"],
        &json_string_array(&schema["preservation_required_fields"]),
        "preservation",
    );
    assert_object_has_fields(
        &report["hardening_gate"],
        &json_string_array(&schema["hardening_gate_required_fields"]),
        "hardening_gate",
    );
    for (field, expected) in schema["privacy_gate_required_values"].as_object().unwrap() {
        assert_eq!(
            &report["privacy_gate"][field], expected,
            "privacy gate field {field} diverged"
        );
    }
    for note in json_string_array(&schema["required_notes"]) {
        assert!(
            report["notes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == note),
            "missing note {note}"
        );
    }
    let serialized = serde_json::to_string(report).unwrap();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "report should not contain forbidden token {token}"
        );
    }
}

#[test]
fn synthetic_exact_id_proof_writes_create_new_output_and_reports_preserved_without_echo() {
    let output = target_path("ready.aepx");
    let _ = std::fs::remove_file(&output);
    let source = fixture_path("aepx_writer_spike_preservation.aepx");
    let source_before = std::fs::read_to_string(&source).unwrap();

    let report = run(&request(&output));
    let schema: Value = serde_json::from_str(PROOF_SCHEMA).unwrap();

    assert_report_matches_schema(&report, &schema);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["report_kind"], "aepx_synthetic_preservation_proof");
    assert_eq!(report["status"], "synthetic_preservation_proof_ready");
    assert_eq!(report["synthetic_fixture_only"], true);
    assert_eq!(report["operation_kind"], "rename_comp");
    assert_eq!(report["selector_kind"], "xml_id");
    assert_eq!(report["target_count"], 1);
    assert!(preservation_fields(&report)
        .iter()
        .all(|value| value.as_str() == Some("preserved")));
    assert_eq!(report["io_gate"]["synthetic_xml_body_read_performed"], true);
    assert_eq!(report["io_gate"]["xml_body_embedded_in_report"], false);
    assert_eq!(report["io_gate"]["external_process_invoked"], false);
    assert_eq!(report["io_gate"]["after_effects_invoked"], false);
    assert_eq!(report["write_gate"]["output_write_performed"], true);
    assert_eq!(
        report["write_gate"]["output_write_mode"],
        "create_new_synthetic_target_only"
    );
    assert_eq!(report["write_gate"]["source_overwrite_performed"], false);
    assert_eq!(report["write_gate"]["production_apply_enabled"], false);
    assert_eq!(report["hardening_gate"]["output_path_extension_aepx"], true);
    assert_eq!(
        report["hardening_gate"]["output_path_has_no_parent_traversal"],
        true
    );
    assert_eq!(
        report["hardening_gate"]["output_path_under_generated_target_root"],
        true
    );
    assert_eq!(
        report["hardening_gate"]["output_parent_canonical_under_generated_target_root"],
        true
    );
    assert_eq!(report["hardening_gate"]["replacement_value_xml_safe"], true);
    assert_eq!(report["hardening_gate"]["exact_byte_diff_verified"], true);
    assert_eq!(report["hardening_gate"]["approved_span_count"], 1);
    assert_eq!(report["privacy_gate"]["selector_value_embedded"], false);
    assert_eq!(report["privacy_gate"]["expected_old_value_embedded"], false);
    assert_eq!(report["privacy_gate"]["new_value_embedded"], false);
    assert_eq!(report["privacy_gate"]["xml_sentinels_embedded"], false);
    assert!(report["blocked_reasons"].as_array().unwrap().is_empty());
    assert_report_omits_sensitive_values(&report);
    for (field, expected) in schema["ready_required_values"].as_object().unwrap() {
        assert_eq!(&report[field], expected, "ready field {field} diverged");
    }
    for (field, expected) in schema["preservation_ready_required_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["preservation"][field], expected,
            "preservation field {field} diverged"
        );
    }
    for (field, expected) in schema["io_gate_required_values"].as_object().unwrap() {
        assert_eq!(
            &report["io_gate"][field], expected,
            "io gate field {field} diverged"
        );
    }
    for (field, expected) in schema["write_gate_ready_required_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["write_gate"][field], expected,
            "write gate field {field} diverged"
        );
    }
    for (field, expected) in schema["hardening_gate_ready_required_values"]
        .as_object()
        .unwrap()
    {
        assert_eq!(
            &report["hardening_gate"][field], expected,
            "hardening gate field {field} diverged"
        );
    }

    let patched = std::fs::read_to_string(&output).unwrap();
    assert!(patched.contains(r#"id="comp-main" name="Main Reviewed""#));
    assert!(patched.contains(r#"data-id="comp-main" display-name="Main""#));
    assert_single_replacement_only(
        &source_before,
        &patched,
        r#"id="comp-main" name="Main""#,
        r#"id="comp-main" name="Main Reviewed""#,
    );
    assert_sentinels_survive(&patched);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), source_before);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn synthetic_proof_hardens_scanner_and_exact_byte_diff_for_quoted_tag_edges() {
    let output = target_path("scanner-edges.aepx");
    let _ = std::fs::remove_file(&output);
    let source = fixture_path("aepx_writer_spike_scanner_edges.aepx");
    let source_before = std::fs::read_to_string(&source).unwrap();
    let mut edge = request(&output);
    edge["input_fixture_name"] = json!("aepx_writer_spike_scanner_edges.aepx");
    edge["operation"]["new_value"] = json!("Edge Reviewed");

    let report = run(&edge);

    assert_eq!(report["status"], "synthetic_preservation_proof_ready");
    assert_eq!(report["hardening_gate"]["exact_byte_diff_verified"], true);
    assert_eq!(report["hardening_gate"]["approved_span_count"], 1);
    assert_report_omits_sensitive_values(&report);
    let patched = std::fs::read_to_string(&output).unwrap();
    assert!(patched.contains("name='Edge Reviewed' id='comp-main'"));
    assert!(patched.contains(r#"<xmp:composition id="comp-main" name="False Positive"/>"#));
    assert!(patched.contains(r#"<xmp:companion id="comp-main" name="False Companion"/>"#));
    assert!(patched.contains(r#"data-note="quoted > marker""#));
    assert!(patched.contains("EDGE_COMMENT_SENTINEL"));
    assert!(patched.contains("EDGE_CDATA_SENTINEL <xmp:comp id=\"comp-main\" name=\"CData Fake\">"));
    assert!(patched.contains("edge:EDGE_NODE_SENTINEL"));
    assert_single_replacement_only(
        &source_before,
        &patched,
        "name='Main'",
        "name='Edge Reviewed'",
    );
    let _ = std::fs::remove_file(&output);
}

#[test]
fn synthetic_proof_preserves_non_ascii_fixture_bytes_without_echoing_values() {
    let output = target_path("unicode-edges.aepx");
    let _ = std::fs::remove_file(&output);
    let source = fixture_path("aepx_writer_spike_unicode_edges.aepx");
    let source_before = std::fs::read_to_string(&source).unwrap();
    let mut unicode = request(&output);
    unicode["input_fixture_name"] = json!("aepx_writer_spike_unicode_edges.aepx");
    unicode["operation"]["selector_value"] = json!("comp-unicode");
    unicode["operation"]["expected_old_value"] = json!("メイン");
    unicode["operation"]["new_value"] = json!("メイン レビュー済み");

    let report = run(&unicode);
    let schema: Value = serde_json::from_str(PROOF_SCHEMA).unwrap();

    assert_report_matches_schema(&report, &schema);
    assert_eq!(report["status"], "synthetic_preservation_proof_ready");
    assert_eq!(report["hardening_gate"]["exact_byte_diff_verified"], true);
    assert_eq!(report["hardening_gate"]["approved_span_count"], 1);
    let patched = std::fs::read_to_string(&output).unwrap();
    assert!(patched.contains(r#"id="comp-unicode" name="メイン レビュー済み""#));
    assert!(patched.contains(r#"display-name="メイン表示""#));
    assert!(patched.contains("UNICODE_COMMENT_SENTINEL: 日本語コメントは保持される"));
    assert!(patched.contains("UNICODE_CDATA_SENTINEL: こんにちは世界"));
    assert!(patched.contains(r#"meta:note="保持値""#));
    assert_single_replacement_only(
        &source_before,
        &patched,
        r#"name="メイン""#,
        r#"name="メイン レビュー済み""#,
    );
    assert_eq!(std::fs::read_to_string(&source).unwrap(), source_before);
    let serialized = serde_json::to_string(&report).unwrap();
    for forbidden in [
        "aepx_writer_spike_unicode_edges.aepx",
        "comp-unicode",
        "メイン",
        "メイン レビュー済み",
        "こんにちは世界",
        "保持値",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "report should not echo unicode/private token {forbidden}"
        );
    }
    let _ = std::fs::remove_file(&output);
}

#[test]
fn synthetic_proof_preserves_crlf_and_utf8_bom_fixture_bytes_without_echoing_values() {
    let output = target_path("crlf-bom.aepx");
    let _ = std::fs::remove_file(&output);
    let source = fixture_path("aepx_writer_spike_crlf_bom.aepx");
    let source_bytes = std::fs::read(&source).unwrap();
    assert!(
        source_bytes.starts_with(&[0xef, 0xbb, 0xbf]),
        "fixture should carry a UTF-8 BOM"
    );
    assert!(
        source_bytes.windows(2).any(|window| window == b"\r\n"),
        "fixture should carry CRLF line endings"
    );
    let source_before = String::from_utf8(source_bytes.clone()).unwrap();
    let mut crlf_bom = request(&output);
    crlf_bom["input_fixture_name"] = json!("aepx_writer_spike_crlf_bom.aepx");
    crlf_bom["operation"]["selector_value"] = json!("comp-crlf-bom");
    crlf_bom["operation"]["expected_old_value"] = json!("CRLF Main");
    crlf_bom["operation"]["new_value"] = json!("CRLF Reviewed");

    let report = run(&crlf_bom);
    let schema: Value = serde_json::from_str(PROOF_SCHEMA).unwrap();

    assert_report_matches_schema(&report, &schema);
    assert_eq!(report["status"], "synthetic_preservation_proof_ready");
    assert_eq!(report["hardening_gate"]["exact_byte_diff_verified"], true);
    assert_eq!(report["hardening_gate"]["approved_span_count"], 1);
    assert_report_omits_sensitive_values(&report);
    let patched_bytes = std::fs::read(&output).unwrap();
    assert!(
        patched_bytes.starts_with(&[0xef, 0xbb, 0xbf]),
        "patched fixture should preserve the UTF-8 BOM"
    );
    assert!(
        patched_bytes.windows(2).any(|window| window == b"\r\n"),
        "patched fixture should preserve CRLF line endings"
    );
    let patched = String::from_utf8(patched_bytes).unwrap();
    assert!(patched.contains(r#"id="comp-crlf-bom" name="CRLF Reviewed""#));
    assert!(patched.contains("CRLF_BOM_COMMENT_SENTINEL"));
    assert!(patched.contains("<xmp:CRLF_BOM_NODE_SENTINEL"));
    assert_single_replacement_only(
        &source_before,
        &patched,
        r#"name="CRLF Main""#,
        r#"name="CRLF Reviewed""#,
    );
    assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn synthetic_proof_applies_multi_operation_all_or_nothing_after_resolving_all_spans() {
    let output = target_path("multi-comp.aepx");
    let _ = std::fs::remove_file(&output);
    let source = fixture_path("aepx_writer_spike_multi_comp.aepx");
    let source_before = std::fs::read_to_string(&source).unwrap();
    let multi = json!({
        "schema_version": 1,
        "proof_kind": "aepx_synthetic_preservation_proof",
        "input_fixture_name": "aepx_writer_spike_multi_comp.aepx",
        "output_aepx": abs_string(&output),
        "operations": [
            {
                "kind": "rename_comp",
                "selector_kind": "xml_id",
                "selector_value": "comp-one",
                "expected_old_value": "One",
                "new_value": "One Reviewed"
            },
            {
                "kind": "rename_comp",
                "selector_kind": "xml_id",
                "selector_value": "comp-two",
                "expected_old_value": "Two",
                "new_value": "Two Reviewed"
            }
        ],
        "options": {
            "synthetic_fixture_only": true,
            "create_new_only": true,
            "preserve_unknown_xml": "required",
            "report_private_payloads": false
        }
    });

    let report = run(&multi);

    assert_eq!(report["status"], "synthetic_preservation_proof_ready");
    assert_eq!(report["operation_kind"], "rename_comp");
    assert_eq!(report["selector_kind"], "xml_id");
    assert_eq!(report["target_count"], 2);
    assert_eq!(report["hardening_gate"]["approved_span_count"], 2);
    assert_eq!(
        report["hardening_gate"]["all_operations_resolved_before_write"],
        true
    );
    assert_eq!(
        report["hardening_gate"]["approved_spans_non_overlapping"],
        true
    );
    assert_report_omits_sensitive_values(&report);
    let patched = std::fs::read_to_string(&output).unwrap();
    assert!(patched.contains(r#"id="comp-one" name="One Reviewed""#));
    assert!(patched.contains(r#"id="comp-two" name="Two Reviewed""#));
    assert!(patched.contains("MULTI_COMMENT_SENTINEL"));
    assert!(patched.contains("MULTI_UNKNOWN_NODE_SENTINEL"));
    let expected = source_before
        .replace(r#"name="One""#, r#"name="One Reviewed""#)
        .replace(r#"name="Two""#, r#"name="Two Reviewed""#);
    assert_eq!(patched, expected);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), source_before);
    let serialized = serde_json::to_string(&report).unwrap();
    for forbidden in [
        "aepx_writer_spike_multi_comp.aepx",
        "comp-one",
        "comp-two",
        "One Reviewed",
        "Two Reviewed",
        "MULTI_COMMENT_SENTINEL",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "report should not echo multi-operation token {forbidden}"
        );
    }
    let _ = std::fs::remove_file(&output);

    let blocked_output = target_path("multi-comp-blocked.aepx");
    let _ = std::fs::remove_file(&blocked_output);
    let mut blocked = multi;
    blocked["output_aepx"] = json!(abs_string(&blocked_output));
    blocked["operations"][1]["expected_old_value"] = json!("Wrong Two");

    let report = run(&blocked);

    assert_eq!(report["status"], "expected_value_mismatch");
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert_eq!(report["hardening_gate"]["approved_span_count"], 0);
    assert!(!blocked_output.exists());
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.contains("Wrong Two"));
}

#[test]
fn synthetic_proof_fails_closed_for_ambiguous_selector_and_expected_mismatch() {
    let ambiguous_output = target_path("ambiguous.aepx");
    let _ = std::fs::remove_file(&ambiguous_output);
    let mut ambiguous = request(&ambiguous_output);
    ambiguous["input_fixture_name"] = json!("aepx_writer_spike_ambiguous.aepx");
    ambiguous["operation"]["selector_kind"] = json!("name");
    ambiguous["operation"]["selector_value"] = json!("Main");

    let report = run(&ambiguous);

    assert_eq!(report["status"], "ambiguous_target");
    assert_eq!(report["target_count"], 0);
    assert_eq!(report["io_gate"]["synthetic_xml_body_read_performed"], true);
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert!(!ambiguous_output.exists());
    assert!(preservation_fields(&report)
        .iter()
        .all(|value| value.as_str() == Some("not_written")));
    assert_report_omits_sensitive_values(&report);

    let mismatch_output = target_path("mismatch.aepx");
    let _ = std::fs::remove_file(&mismatch_output);
    let mut mismatch = request(&mismatch_output);
    mismatch["operation"]["expected_old_value"] = json!("Wrong Old");

    let report = run(&mismatch);

    assert_eq!(report["status"], "expected_value_mismatch");
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert!(!mismatch_output.exists());
    assert_report_omits_sensitive_values(&report);

    let duplicate_id_output = target_path("duplicate-id.aepx");
    let _ = std::fs::remove_file(&duplicate_id_output);
    let mut duplicate_id = request(&duplicate_id_output);
    duplicate_id["input_fixture_name"] = json!("aepx_writer_spike_duplicate_id.aepx");

    let report = run(&duplicate_id);

    assert_eq!(report["status"], "ambiguous_target");
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert!(!duplicate_id_output.exists());
    assert_report_omits_sensitive_values(&report);
}

#[test]
fn synthetic_proof_uses_create_new_and_preserves_existing_output() {
    let output = target_path("existing.aepx");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&output, "existing").unwrap();

    let report = run(&request(&output));

    assert_eq!(report["status"], "output_exists");
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "existing");
    assert_report_omits_sensitive_values(&report);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn synthetic_proof_rejects_private_or_non_synthetic_inputs_before_reading_or_writing() {
    let output = target_path("private-input.aepx");
    let _ = std::fs::remove_file(&output);
    let mut private = request(&output);
    private["input_fixture_name"] = json!("D:/private/project.aepx");

    let report = run(&private);

    assert_eq!(report["status"], "invalid_request");
    assert_eq!(
        report["io_gate"]["synthetic_xml_body_read_performed"],
        false
    );
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert!(!output.exists());
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.contains("D:/private"));
    assert!(!serialized.contains("project.aepx"));

    let outside_output = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("outside-proof.aepx");
    let _ = std::fs::remove_file(&outside_output);
    let report = run(&request(&outside_output));

    assert_eq!(report["status"], "invalid_request");
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert!(!outside_output.exists());
}

#[test]
fn synthetic_proof_rejects_traversal_non_aepx_and_unsafe_replacement_values() {
    let traversal_output = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-synthetic-preservation-proof")
        .join("..")
        .join("outside-proof.aepx");
    let _ = std::fs::remove_file(&traversal_output);
    let report = run(&request(&traversal_output));

    assert_eq!(report["status"], "invalid_request");
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert_eq!(
        report["hardening_gate"]["output_path_has_no_parent_traversal"],
        false
    );

    let non_aepx_output = target_path("wrong-extension.txt");
    let _ = std::fs::remove_file(&non_aepx_output);
    let report = run(&request(&non_aepx_output));

    assert_eq!(report["status"], "invalid_request");
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert_eq!(
        report["hardening_gate"]["output_path_extension_aepx"],
        false
    );
    assert!(!non_aepx_output.exists());

    for unsafe_value in [
        "Needs < escape",
        "Needs & escape",
        "Needs \" quote",
        "Needs ' quote",
    ] {
        let output = target_path("unsafe-value.aepx");
        let _ = std::fs::remove_file(&output);
        let mut unsafe_request = request(&output);
        unsafe_request["operation"]["new_value"] = json!(unsafe_value);

        let report = run(&unsafe_request);

        assert_eq!(report["status"], "invalid_request");
        assert_eq!(report["write_gate"]["output_write_performed"], false);
        assert_eq!(
            report["hardening_gate"]["replacement_value_xml_safe"],
            false
        );
        assert!(!output.exists());
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(
            !serialized.contains(unsafe_value),
            "report should not echo unsafe replacement value"
        );
    }
}

#[test]
fn synthetic_proof_validates_report_output_policy_without_writing_private_paths() {
    let valid_report = target_path("proof.local.json");
    assert!(
        aepx_synthetic_preservation_proof::validate_synthetic_preservation_report_output_path(
            &valid_report
        )
        .is_ok()
    );

    let non_json_report = target_path("proof.local.txt");
    assert!(
        aepx_synthetic_preservation_proof::validate_synthetic_preservation_report_output_path(
            &non_json_report
        )
        .is_err()
    );

    let traversal_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-synthetic-preservation-proof")
        .join("..")
        .join("private-report.json");
    assert!(
        aepx_synthetic_preservation_proof::validate_synthetic_preservation_report_output_path(
            &traversal_report
        )
        .is_err()
    );

    let outside_report = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("outside-proof-report.json");
    assert!(
        aepx_synthetic_preservation_proof::validate_synthetic_preservation_report_output_path(
            &outside_report
        )
        .is_err()
    );
    assert!(!traversal_report.exists());
    assert!(!outside_report.exists());
}

#[cfg(windows)]
#[test]
fn synthetic_proof_rejects_symlink_parent_that_resolves_outside_target_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-synthetic-preservation-proof");
    std::fs::create_dir_all(&root).unwrap();
    let outside = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("{}-aepx-proof-outside", std::process::id()));
    let link = root.join(format!("{}-outside-link", std::process::id()));
    let output = link.join("escape.aepx");
    let _ = std::fs::remove_file(&output);
    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();

    if std::os::windows::fs::symlink_dir(&outside, &link).is_err() {
        let _ = std::fs::remove_dir_all(&outside);
        return;
    }

    let report = run(&request(&output));

    assert_eq!(report["status"], "write_failed");
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert_eq!(
        report["hardening_gate"]["output_parent_canonical_under_generated_target_root"],
        false
    );
    assert!(!output.exists());
    assert!(!outside.join("escape.aepx").exists());
    let serialized = serde_json::to_string(&report).unwrap();
    for forbidden in [abs_string(&outside), abs_string(&link), abs_string(&output)] {
        assert!(
            !serialized.contains(&forbidden),
            "report should not echo symlink escape path {forbidden}"
        );
    }

    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_dir_all(&outside);
}

#[cfg(windows)]
fn create_dir_junction(target: &Path, junction: &Path) -> bool {
    std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(junction)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(windows)]
#[test]
fn synthetic_proof_rejects_junction_parent_that_resolves_outside_target_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-synthetic-preservation-proof");
    std::fs::create_dir_all(&root).unwrap();
    let outside = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!(
            "{}-aepx-proof-junction-outside",
            std::process::id()
        ));
    let junction = root.join(format!("{}-outside-junction", std::process::id()));
    let output = junction.join("escape-through-junction.aepx");
    let _ = std::fs::remove_file(&output);
    let _ = std::fs::remove_dir(&junction);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();

    if !create_dir_junction(&outside, &junction) {
        let _ = std::fs::remove_dir_all(&outside);
        return;
    }

    let report = run(&request(&output));

    assert_eq!(report["status"], "write_failed");
    assert_eq!(report["io_gate"]["external_process_invoked"], false);
    assert_eq!(report["write_gate"]["output_write_performed"], false);
    assert_eq!(
        report["hardening_gate"]["output_parent_canonical_under_generated_target_root"],
        false
    );
    assert!(!output.exists());
    assert!(!outside.join("escape-through-junction.aepx").exists());
    let serialized = serde_json::to_string(&report).unwrap();
    for forbidden in [
        abs_string(&outside),
        abs_string(&junction),
        abs_string(&output),
    ] {
        assert!(
            !serialized.contains(&forbidden),
            "report should not echo junction escape path {forbidden}"
        );
    }

    let _ = std::fs::remove_dir(&junction);
    let _ = std::fs::remove_dir_all(&outside);
}
