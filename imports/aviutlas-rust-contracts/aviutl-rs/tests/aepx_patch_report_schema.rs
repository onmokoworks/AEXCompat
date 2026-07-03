#[allow(dead_code)]
#[path = "../examples/aepx_patch_probe.rs"]
mod aepx_patch_probe;

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aepx_patch_probe::run_aepx_patch_request_text;
use serde_json::Value;

const REPORT_SCHEMA: &str = "analysis/AEPX_PATCH_REPORT_SCHEMA_2026-06-01.json";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .to_path_buf()
}

fn load_report_schema() -> Option<Value> {
    let path = repository_root().join(REPORT_SCHEMA);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!(
                "skipping AEPX patch report schema guard; artifact is absent: {}",
                path.display()
            );
            return None;
        }
        Err(err) => panic!("failed to read analysis artifact {}: {err}", path.display()),
    };
    Some(serde_json::from_str(&text).expect("report schema should be JSON"))
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn unique_target_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after UNIX_EPOCH")
        .as_nanos();
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-patch-report-schema")
        .join(format!("{name}-{}-{nanos}.aepx", std::process::id()))
}

fn abs_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn request(mode: &str, input_aepx: &Path, output_aepx: Option<&Path>) -> String {
    let output_field = output_aepx
        .map(|path| format!(r#","output_aepx":"{}""#, abs_string(path)))
        .unwrap_or_default();
    format!(
        r#"{{
  "schema_version": 1,
  "operation": "{mode}",
  "input_aepx": "{}"{output_field},
  "patch_id": "report-schema-test",
  "publication_status": "local-only",
  "operations": [
    {{
      "id": "op_001",
      "kind": "rename_comp",
      "target": {{
        "kind": "comp",
        "selector": {{
          "name": "PRIVATE_SELECTOR"
        }}
      }},
      "expected_old_value": "PRIVATE_OLD_VALUE",
      "new_value": "PRIVATE_NEW_VALUE",
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
}}"#,
        abs_string(input_aepx)
    )
}

fn string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("schema field should be an array")
        .iter()
        .map(|item| item.as_str().expect("schema array item should be a string"))
        .collect()
}

fn object_keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("value should be a JSON object")
        .keys()
        .map(String::as_str)
        .collect()
}

fn required_fields<'a>(schema: &'a Value, path: &[&str]) -> BTreeSet<&'a str> {
    let mut value = schema;
    for segment in path {
        value = &value[*segment];
    }
    string_array(value).into_iter().collect()
}

fn assert_value_in_vocabulary(value: &Value, vocabulary: &[&str]) {
    let text = value.as_str().expect("report status should be a string");
    assert!(
        vocabulary.contains(&text),
        "report value {text:?} should be in {vocabulary:?}"
    );
}

fn assert_preservation_not_written(report: &Value, schema: &Value) {
    let expected = schema["response"]["preservation"]["current_probe_must_equal"]
        .as_str()
        .expect("current preservation status should be a string");
    for field in string_array(&schema["response"]["preservation"]["required_fields"]) {
        assert_eq!(
            report["preservation"][field], expected,
            "preservation field {field} should stay metadata-only"
        );
    }
}

fn assert_write_gate_no_write(report: &Value, schema: &Value) {
    assert_eq!(
        object_keys(&report["write_gate"]),
        required_fields(schema, &["response", "write_gate", "required_fields"])
    );
    assert_eq!(
        report["write_gate"]["patch_application_status"],
        schema["response"]["write_gate"]["patch_application_status"]["current_probe_value"]
    );
    assert_eq!(
        report["write_gate"]["xml_writer_status"],
        schema["response"]["write_gate"]["xml_writer_status"]["current_probe_value"]
    );
    assert_eq!(
        report["write_gate"]["user_approval_status"],
        schema["response"]["write_gate"]["user_approval_status"]["current_probe_value"]
    );
    assert_eq!(
        report["write_gate"]["output_write_performed"],
        schema["response"]["write_gate"]["output_write_performed"]["current_probe_value"]
    );
    assert_eq!(
        report["write_gate"]["source_overwrite_performed"],
        schema["response"]["write_gate"]["source_overwrite_performed"]["current_probe_value"]
    );
}

fn assert_io_gate_metadata_only(report: &Value, schema: &Value) {
    assert_eq!(
        object_keys(&report["io_gate"]),
        required_fields(schema, &["response", "io_gate", "required_fields"])
    );
    for field in string_array(&schema["response"]["io_gate"]["required_fields"]) {
        assert_eq!(
            report["io_gate"][field], false,
            "io_gate field {field} should stay false for the current probe"
        );
        assert_eq!(
            schema["response"]["io_gate"][field]["current_probe_value"], false,
            "schema current_probe_value for {field} should be false"
        );
    }
}

fn assert_no_private_payloads(report: &Value) {
    let text = serde_json::to_string(report).expect("report should serialize");
    for forbidden in [
        "PRIVATE_SELECTOR",
        "PRIVATE_OLD_VALUE",
        "PRIVATE_NEW_VALUE",
        "UNKNOWN_NODE_SENTINEL",
        "UNKNOWN_ATTR_SENTINEL",
        "COMMENT_SENTINEL",
        "CDATA_SENTINEL",
        "PREFIX_SENTINEL",
    ] {
        assert!(
            !text.contains(forbidden),
            "report should not echo private payload sentinel {forbidden}"
        );
    }
}

#[test]
fn report_schema_sidecar_pins_metadata_only_contract() {
    let Some(schema) = load_report_schema() else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["contract_classification"],
        "Measured metadata/report contract"
    );
    assert_eq!(schema["producer"]["implementation_writes_aepx"], false);

    assert_eq!(
        string_array(&schema["response"]["required_fields"]),
        vec![
            "schema_version",
            "status",
            "aepx_patch_report_binding",
            "input_aepx",
            "output_aepx",
            "metadata",
            "operations",
            "preservation",
            "io_gate",
            "write_gate",
            "warnings",
            "unsupported",
            "elapsed_ms",
        ]
    );

    for status in [
        "ok",
        "dry_run_ok",
        "invalid_request",
        "schema_error",
        "source_not_found",
        "output_exists",
        "output_same_as_source",
        "unsupported_operation",
        "ambiguous_target",
        "target_not_found",
        "expected_value_mismatch",
        "parse_error",
        "preservation_failed",
        "write_failed",
        "internal_error",
    ] {
        assert!(
            string_array(&schema["response"]["status"]["vocabulary"]).contains(&status),
            "missing report status vocabulary item {status}"
        );
    }

    assert_eq!(
        schema["response"]["metadata"]["xml_body_read"]["must_equal_for_current_probe"],
        false
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["algorithm"],
        "fnv1a64-v1-noncryptographic"
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["payloads_embedded"],
        false
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["covers_xml_or_private_patch_payloads"],
        false
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["cryptographic_digest"]["algorithm"],
        "sha256-v1"
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["cryptographic_digest"]
            ["payloads_embedded"],
        false
    );
    assert_eq!(
        schema["response"]["aepx_patch_report_binding"]["cryptographic_digest"]
            ["covers_xml_or_private_patch_payloads"],
        false
    );
    assert_eq!(
        schema["response"]["operations"]["current_probe_success_status"],
        "planned"
    );
    assert_eq!(
        schema["response"]["preservation"]["current_probe_must_equal"],
        "not_written"
    );
    assert_eq!(
        string_array(&schema["response"]["io_gate"]["required_fields"]),
        vec![
            "xml_body_read_performed",
            "xml_body_write_performed",
            "xml_body_embedded_in_report",
            "private_patch_payloads_embedded_in_report",
            "external_process_invoked",
            "after_effects_invoked",
        ]
    );
    for field in string_array(&schema["response"]["io_gate"]["required_fields"]) {
        assert_eq!(
            schema["response"]["io_gate"][field]["current_probe_value"], false,
            "io_gate schema should pin {field}=false"
        );
    }
    assert_eq!(
        string_array(&schema["response"]["write_gate"]["required_fields"]),
        vec![
            "patch_application_status",
            "xml_writer_status",
            "user_approval_status",
            "output_write_performed",
            "source_overwrite_performed",
        ]
    );
    assert_eq!(
        schema["response"]["write_gate"]["patch_application_status"]["current_probe_value"],
        "not_applied"
    );
    assert_eq!(
        schema["response"]["write_gate"]["xml_writer_status"]["current_probe_value"],
        "not_implemented"
    );
    assert_eq!(
        schema["response"]["write_gate"]["user_approval_status"]["current_probe_value"],
        "required_before_write"
    );
    assert_eq!(
        schema["response"]["write_gate"]["output_write_performed"]["current_probe_value"],
        false
    );
    assert_eq!(
        schema["response"]["write_gate"]["source_overwrite_performed"]["current_probe_value"],
        false
    );

    for field in string_array(&schema["response"]["preservation"]["required_fields"]) {
        assert!(
            string_array(&schema["response"]["preservation"]["status_by_field"][field])
                .contains(&"not_written"),
            "preservation field {field} should expose not_written"
        );
    }

    assert_eq!(
        schema["request_apply_boundary"]["apply_requires_output_aepx"],
        true
    );
    assert_eq!(
        schema["request_apply_boundary"]["current_apply_writes_aepx"],
        false
    );
    assert_eq!(
        schema["request_apply_boundary"]["current_apply_new_output_status"],
        "unsupported_operation"
    );
    assert_eq!(
        schema["request_output_path_boundary"]["output_aepx_optional_for_non_apply_modes"],
        true
    );
    assert_eq!(
        schema["request_output_path_boundary"]["when_supplied_must_not_exist"],
        true
    );
    assert_eq!(
        schema["request_output_path_boundary"]["existing_output_status"],
        "output_exists"
    );
    assert_eq!(
        schema["request_output_path_boundary"]["current_probe_writes_aepx"],
        false
    );

    for invariant in [
        "ae_launch",
        "jsx_execution",
        "aepx_write",
        "source_overwrite",
        "xml_body_mutation",
        "external_process",
        "report_private_payloads",
        "xml_body_read",
    ] {
        assert_eq!(
            schema["metadata_only_invariants"][invariant], false,
            "metadata-only invariant {invariant} should be false"
        );
    }
    assert_eq!(
        schema["metadata_only_invariants"]["native_oracle"],
        "not_run"
    );
    assert_eq!(
        schema["review_packet_boundary"]["optional_aepx_dry_run_reports_require_binding"],
        true
    );
    assert_eq!(
        schema["review_packet_boundary"]["cryptographic_digest_algorithm"],
        "sha256-v1"
    );
}

#[test]
fn dry_run_report_matches_sidecar_shape_without_xml_body_io() {
    let Some(schema) = load_report_schema() else {
        return;
    };
    let input = fixture_path("aepx_preservation_sentinels.aepx");
    let output = unique_target_path("dry-run-output");

    assert!(!output.exists(), "unique output path should start absent");
    let report = run_aepx_patch_request_text(&request("dry_run", &input, Some(&output)), None)
        .expect("probe should return a report");
    let report = serde_json::to_value(report).expect("report should serialize");

    assert_eq!(
        object_keys(&report),
        required_fields(&schema, &["response", "required_fields"])
    );
    assert_eq!(report["schema_version"], 1);
    assert_value_in_vocabulary(
        &report["status"],
        &string_array(&schema["response"]["status"]["current_probe_statuses"]),
    );
    assert_eq!(report["status"], "dry_run_ok");
    assert_eq!(
        report["aepx_patch_report_binding"]["algorithm"],
        "fnv1a64-v1-noncryptographic"
    );
    assert!(report["aepx_patch_report_binding"]["checksum_hex"]
        .as_str()
        .is_some_and(|value| value.len() == 16 && value.chars().all(|ch| ch.is_ascii_hexdigit())));
    assert_eq!(
        report["aepx_patch_report_binding"]["payloads_embedded"],
        false
    );
    assert_eq!(
        report["aepx_patch_report_binding"]["covers_xml_or_private_patch_payloads"],
        false
    );
    assert_eq!(
        report["aepx_patch_report_binding"]["cryptographic_digest"]["algorithm"],
        "sha256-v1"
    );
    assert!(
        report["aepx_patch_report_binding"]["cryptographic_digest"]["digest_hex"]
            .as_str()
            .is_some_and(
                |value| value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
            )
    );
    assert_eq!(
        report["aepx_patch_report_binding"]["cryptographic_digest"]["payloads_embedded"],
        false
    );
    assert_eq!(
        report["aepx_patch_report_binding"]["cryptographic_digest"]
            ["covers_xml_or_private_patch_payloads"],
        false
    );
    assert_eq!(report["metadata"]["xml_body_read"], false);
    assert_eq!(
        object_keys(&report["metadata"]),
        required_fields(&schema, &["response", "metadata", "required_fields"])
    );
    assert_eq!(
        object_keys(&report["operations"][0]),
        required_fields(&schema, &["response", "operations", "item_required_fields"])
    );
    assert_value_in_vocabulary(
        &report["operations"][0]["status"],
        &string_array(&schema["response"]["operations"]["status"]),
    );
    assert_eq!(report["operations"][0]["status"], "planned");
    assert_eq!(report["operations"][0]["target_count"], 0);
    assert_preservation_not_written(&report, &schema);
    assert_io_gate_metadata_only(&report, &schema);
    assert_write_gate_no_write(&report, &schema);
    assert_no_private_payloads(&report);
    assert!(!output.exists(), "dry_run should not write output_aepx");
}

#[test]
fn apply_boundary_reports_without_aepx_write() {
    let Some(schema) = load_report_schema() else {
        return;
    };
    let input = fixture_path("aepx_preservation_sentinels.aepx");
    let output = unique_target_path("apply-output");

    assert!(!output.exists(), "unique output path should start absent");
    let report = run_aepx_patch_request_text(&request("apply", &input, Some(&output)), None)
        .expect("probe should return a report");
    let report = serde_json::to_value(report).expect("report should serialize");

    assert_eq!(
        report["status"],
        schema["request_apply_boundary"]["current_apply_new_output_status"]
    );
    assert_eq!(report["metadata"]["xml_body_read"], false);
    assert!(report["unsupported"]
        .as_array()
        .expect("unsupported should be an array")
        .iter()
        .any(|item| item
            .as_str()
            .unwrap_or_default()
            .contains("not implemented")));
    assert_preservation_not_written(&report, &schema);
    assert_io_gate_metadata_only(&report, &schema);
    assert_write_gate_no_write(&report, &schema);
    assert_no_private_payloads(&report);
    assert!(
        !output.exists(),
        "apply boundary should not write output_aepx"
    );

    let missing_output =
        run_aepx_patch_request_text(&request("apply", &input, None), None).unwrap();
    let missing_output = serde_json::to_value(missing_output).unwrap();
    assert_eq!(missing_output["status"], "invalid_request");
    assert_eq!(missing_output["metadata"]["xml_body_read"], false);
    assert!(missing_output["warnings"]
        .as_array()
        .expect("warnings should be an array")
        .iter()
        .any(|item| item.as_str().unwrap_or_default().contains("output_aepx")));
}
