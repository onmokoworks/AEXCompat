#[allow(dead_code)]
#[path = "../examples/aex_probe_readiness.rs"]
mod aex_probe_readiness;

use serde_json::Value;

const READINESS_REPORT_SCHEMA: &str =
    include_str!("../../analysis/AEX_PROBE_READINESS_REPORT_SCHEMA_2026-06-01.json");

fn readiness_report_schema() -> Value {
    serde_json::from_str(READINESS_REPORT_SCHEMA)
        .expect("AEX probe readiness report schema should parse")
}

fn json_string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("expected JSON array")
        .iter()
        .map(|item| item.as_str().expect("expected string array item"))
        .collect()
}

fn assert_object_has_fields(value: &Value, fields: &[&str], label: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{label} should be an object"));
    for field in fields {
        assert!(
            object.contains_key(*field),
            "{label} missing required field {field}: {value:?}"
        );
    }
}

fn assert_required_values(value: &Value, required_values: &Value, label: &str) {
    for (field, expected) in required_values
        .as_object()
        .expect("required_values should be an object")
    {
        assert_eq!(
            &value[field], expected,
            "{label} field {field} diverged from schema"
        );
    }
}

fn assert_array_contains(value: &Value, expected: &str, label: &str) {
    assert!(
        value
            .as_array()
            .unwrap_or_else(|| panic!("{label} should be an array"))
            .iter()
            .any(|item| item == expected),
        "{label} missing expected entry {expected}"
    );
}

fn assert_no_forbidden_top_level_fields(report: &Value, schema: &Value, label: &str) {
    for field in json_string_array(&schema["forbidden_top_level_fields"]) {
        assert!(
            report.get(field).is_none(),
            "{label} should not expose top-level field {field}"
        );
    }
}

fn assert_no_forbidden_field_names(value: &Value, forbidden: &[&str], label: &str) {
    match value {
        Value::Object(object) => {
            for (field, child) in object {
                assert!(
                    !forbidden.contains(&field.as_str()),
                    "{label} should not expose forbidden field name {field}"
                );
                assert_no_forbidden_field_names(child, forbidden, label);
            }
        }
        Value::Array(array) => {
            for child in array {
                assert_no_forbidden_field_names(child, forbidden, label);
            }
        }
        _ => {}
    }
}

fn assert_no_forbidden_serialized_tokens(report: &Value, schema: &Value, label: &str) {
    let serialized = serde_json::to_string(report)
        .expect("report should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "{label} should not contain serialized token {token}"
        );
    }
}

fn assert_fixture_review_gate_summary(summary: &Value, schema: &Value, label: &str) {
    assert_object_has_fields(
        summary,
        &json_string_array(&schema["fixture_review_gate_summary_required_fields"]),
        label,
    );
    assert_required_values(
        summary,
        &schema["fixture_review_gate_summary_required_values"],
        label,
    );
    for field in json_string_array(&schema["fixture_review_gate_summary_forbidden_fields"]) {
        assert!(
            summary.get(field).is_none(),
            "{label} should remain a summary and should not expose {field}"
        );
    }
    assert_eq!(
        summary["allowed_candidate_count"],
        summary["candidate_ids"]
            .as_array()
            .expect("candidate_ids should be an array")
            .len()
    );
}

fn assert_readiness_matches_schema(report: &Value, schema: &Value, fixture_gate_expected: bool) {
    assert_object_has_fields(
        report,
        &json_string_array(&schema["readiness_required_fields"]),
        "readiness report",
    );
    assert_required_values(
        report,
        &schema["readiness_required_values"],
        "readiness report",
    );
    assert!(
        json_string_array(&schema["readiness_allowed_statuses"]).contains(
            &report["status"]
                .as_str()
                .expect("readiness status should be string")
        ),
        "unexpected readiness status"
    );
    assert_no_forbidden_top_level_fields(report, schema, "readiness report");

    let generated_files = report["generated_files"]
        .as_array()
        .expect("generated_files should be an array");
    for file in json_string_array(&schema["generated_files_required"]) {
        assert!(
            generated_files.iter().any(|item| item == file),
            "readiness report missing generated file {file}"
        );
    }
    for prefix in json_string_array(&schema["generated_files_required_prefixes"]) {
        assert!(
            generated_files
                .iter()
                .any(|item| item.as_str().unwrap_or_default().starts_with(prefix)),
            "readiness report missing generated file prefix {prefix}"
        );
    }
    if fixture_gate_expected {
        for file in json_string_array(&schema["generated_files_fixture_gate_required"]) {
            assert!(
                generated_files.iter().any(|item| item == file),
                "fixture-gated readiness missing generated file {file}"
            );
        }
        assert_fixture_review_gate_summary(
            &report["fixture_review_gate"],
            schema,
            "readiness fixture_review_gate",
        );
    }

    let entries = report["entries"]
        .as_array()
        .expect("entries should be array");
    let ready_count = entries
        .iter()
        .filter(|entry| entry["status"] == "draft_allowlisted")
        .count();
    let blocked_count = entries
        .iter()
        .filter(|entry| entry["status"] == "blocked_or_deferred")
        .count();
    assert_eq!(report["candidate_count"], entries.len());
    assert_eq!(report["allowlist_entry_count"], ready_count);
    assert_eq!(report["blocked_count"], blocked_count);
    assert_eq!(
        generated_files
            .iter()
            .filter(|file| file.as_str().unwrap_or_default().starts_with("requests/"))
            .count(),
        ready_count
    );
    assert_eq!(
        generated_files
            .iter()
            .filter(|file| {
                file.as_str()
                    .unwrap_or_default()
                    .starts_with("capabilities/")
            })
            .count(),
        ready_count
    );

    let allowed_statuses = json_string_array(&schema["readiness_entry_allowed_statuses"]);
    for entry in entries {
        assert_object_has_fields(
            entry,
            &json_string_array(&schema["readiness_entry_required_fields"]),
            "readiness entry",
        );
        let status = entry["status"]
            .as_str()
            .expect("entry status should be string");
        assert!(
            allowed_statuses.contains(&status),
            "unexpected entry status"
        );
        assert!(
            entry["plugin_path"]
                .as_str()
                .expect("plugin_path should be string")
                .ends_with(".aex"),
            "readiness entries should only point at candidate .aex paths"
        );
        if status == "draft_allowlisted" {
            assert_required_values(
                entry,
                &schema["readiness_ready_entry_required_values"],
                "ready readiness entry",
            );
            assert_eq!(entry["blocked_reason"], Value::Null);
        } else {
            assert_required_values(
                entry,
                &schema["readiness_blocked_entry_required_values"],
                "blocked readiness entry",
            );
            assert!(
                entry["blocked_reason"].is_string(),
                "blocked entries should carry a blocked_reason"
            );
        }
    }

    for note in json_string_array(&schema["required_readiness_notes"]) {
        assert_array_contains(&report["notes"], note, "readiness notes");
    }
}

fn assert_loader_gate_matches_schema(gate: &Value, schema: &Value, fixture_gate_expected: bool) {
    assert_object_has_fields(
        gate,
        &json_string_array(&schema["loader_gate_required_fields"]),
        "loader gate",
    );
    assert_required_values(gate, &schema["loader_gate_required_values"], "loader gate");
    assert_no_forbidden_top_level_fields(gate, schema, "loader gate");
    assert_eq!(
        gate["candidate_count"],
        gate["entries"].as_array().unwrap().len()
    );
    assert_eq!(gate["blocked_count"], gate["candidate_count"]);

    if fixture_gate_expected {
        assert_fixture_review_gate_summary(
            &gate["fixture_review_gate"],
            schema,
            "loader gate fixture_review_gate",
        );
    }

    for entry in gate["entries"]
        .as_array()
        .expect("entries should be an array")
    {
        assert_object_has_fields(
            entry,
            &json_string_array(&schema["loader_gate_entry_required_fields"]),
            "loader gate entry",
        );
        assert_required_values(
            entry,
            &schema["loader_gate_entry_required_values"],
            "loader gate entry",
        );
        assert!(
            entry["plugin_path"]
                .as_str()
                .expect("plugin_path should be string")
                .ends_with(".aex"),
            "loader gate entries should only point at candidate .aex paths"
        );
        assert!(
            entry["blocked_reasons"]
                .as_array()
                .expect("blocked_reasons should be an array")
                .iter()
                .any(|reason| reason
                    .as_str()
                    .unwrap_or_default()
                    .contains("render_png is not approved")),
            "loader gate entry should explicitly keep render approval closed"
        );
        assert!(
            entry["required_before_loader"]
                .as_array()
                .expect("required_before_loader should be an array")
                .iter()
                .any(|requirement| requirement
                    .as_str()
                    .unwrap_or_default()
                    .contains("explicit later slice")),
            "loader gate entry should require an explicit later slice"
        );
    }

    for note in json_string_array(&schema["required_loader_gate_notes"]) {
        assert_array_contains(&gate["notes"], note, "loader gate notes");
    }
}

fn assert_metadata_only_boundaries(report: &Value, schema: &Value, label: &str) {
    let forbidden = json_string_array(&schema["forbidden_field_names"]);
    assert_no_forbidden_field_names(report, &forbidden, label);
    assert_no_forbidden_serialized_tokens(report, schema, label);
}

#[test]
fn probe_readiness_top_level_reports_match_sidecar_schema() {
    let schema = readiness_report_schema();
    let input = include_str!("fixtures/aex_probe_readiness_inventory.synthetic.json");
    let gate = r#"{
      "schema_version": 1,
      "status": "review_queue_not_approved",
      "selected_fixture": null,
      "recommended_first_review": "adaptive-filter-local",
      "recommendation_status": "queue-order-only-not-approval",
      "candidates": [
        {"id": "adaptive-filter-local", "path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex"},
        {"id": "median-pro-local", "path": "D:\\AviUtlas\\local\\MedianPro.aex"}
      ]
    }"#;
    let output =
        aex_probe_readiness::plan_probe_readiness_json_with_fixture_gate(input, Some(gate))
            .expect("readiness planner should run with a fixture review gate");
    let readiness: Value =
        serde_json::from_str(&output.readiness_json).expect("readiness should parse");
    let loader_gate: Value =
        serde_json::from_str(&output.loader_gate_json).expect("loader gate should parse");

    assert_eq!(
        schema["compatibility_classification"],
        "Measured readiness metadata/report contract"
    );
    assert_array_contains(
        &schema["excluded_scopes"],
        "AEX loader preflight preflight.local.json",
        "excluded scopes",
    );
    assert_array_contains(
        &schema["excluded_scopes"],
        "AEX image probe worker receipt/report",
        "excluded scopes",
    );

    assert_readiness_matches_schema(&readiness, &schema, true);
    assert_loader_gate_matches_schema(&loader_gate, &schema, true);
    assert_metadata_only_boundaries(&readiness, &schema, "readiness report");
    assert_metadata_only_boundaries(&loader_gate, &schema, "loader gate");

    assert!(
        output.fixture_review.is_some(),
        "fixture-gated readiness should emit a fixture review packet side output"
    );
    assert!(
        output.loader_preflight.is_some(),
        "fixture-gated readiness should emit a loader preflight side output"
    );
}

#[test]
fn probe_readiness_report_shape_is_not_preflight_or_image_probe_receipt() {
    let schema = readiness_report_schema();
    let input = include_str!("fixtures/aex_probe_readiness_inventory.synthetic.json");
    let output = aex_probe_readiness::plan_probe_readiness_json(input)
        .expect("readiness planner should run");
    let readiness: Value =
        serde_json::from_str(&output.readiness_json).expect("readiness should parse");
    let loader_gate: Value =
        serde_json::from_str(&output.loader_gate_json).expect("loader gate should parse");

    assert_eq!(
        schema["metadata_only_invariants"]["loader_gate_must_remain_closed"],
        true
    );
    assert_eq!(
        schema["metadata_only_invariants"]["native_aex_load_render_pixel_parity_out_of_scope"],
        true
    );
    assert_readiness_matches_schema(&readiness, &schema, false);
    assert_loader_gate_matches_schema(&loader_gate, &schema, false);

    for field in json_string_array(&schema["forbidden_top_level_fields"]) {
        assert!(
            readiness.get(field).is_none(),
            "readiness report should not use preflight/image-probe top-level field {field}"
        );
        assert!(
            loader_gate.get(field).is_none(),
            "loader gate should not use preflight/image-probe top-level field {field}"
        );
    }
    assert_metadata_only_boundaries(&readiness, &schema, "readiness report");
    assert_metadata_only_boundaries(&loader_gate, &schema, "loader gate");
}
