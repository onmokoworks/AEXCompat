#[allow(dead_code)]
#[path = "../examples/aex_static_classifier.rs"]
mod aex_static_classifier;

use serde_json::Value;
use std::collections::BTreeSet;

const REPORT_SCHEMA: &str =
    include_str!("../../analysis/AEX_STATIC_CLASSIFIER_REPORT_SCHEMA_2026-06-01.json");

fn classify(input: &str) -> Value {
    let output =
        aex_static_classifier::classify_inventory_json(input).expect("classifier should run");
    serde_json::from_str(&output).expect("classifier should emit JSON")
}

fn report_schema() -> Value {
    serde_json::from_str(REPORT_SCHEMA).expect("report schema sidecar should parse")
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
        .unwrap_or_else(|| panic!("{label} required values should be an object"))
    {
        assert_eq!(
            &value[field], expected,
            "{label} field {field} diverged from report schema"
        );
    }
}

fn assert_string_in(value: &Value, allowed: &[&str], label: &str) {
    let actual = value
        .as_str()
        .unwrap_or_else(|| panic!("{label} should be a string"));
    assert!(
        allowed.contains(&actual),
        "{label} {actual:?} is outside schema vocabulary {allowed:?}"
    );
}

fn assert_array_contains_all(value: &Value, expected: &[&str], label: &str) {
    let actual = value
        .as_array()
        .unwrap_or_else(|| panic!("{label} should be an array"));
    for expected_item in expected {
        assert!(
            actual.iter().any(|item| item == expected_item),
            "{label} missing required item {expected_item}"
        );
    }
}

fn assert_no_forbidden_fields(value: &Value, schema: &Value) {
    let forbidden = json_string_array(&schema["forbidden_fields"])
        .into_iter()
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    assert_no_forbidden_fields_inner(value, &forbidden);
}

fn assert_no_forbidden_fields_inner(value: &Value, forbidden: &BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                assert!(
                    !forbidden.contains(&key.to_ascii_lowercase()),
                    "report should not expose forbidden field {key}"
                );
                assert_no_forbidden_fields_inner(nested, forbidden);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_no_forbidden_fields_inner(item, forbidden);
            }
        }
        _ => {}
    }
}

fn assert_no_forbidden_serialized_tokens(value: &Value, schema: &Value) {
    let serialized = serde_json::to_string(value)
        .expect("report should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "report should not contain serialized token {token}"
        );
    }
}

fn assert_entrypoint_matches_schema(entrypoint: &Value, schema: &Value, label: &str) {
    if entrypoint.is_null() {
        return;
    }
    assert_object_has_fields(
        entrypoint,
        &json_string_array(&schema["entrypoint_required_fields"]),
        label,
    );
    assert_required_values(entrypoint, &schema["entrypoint_required_values"], label);
}

fn assert_effect_matches_schema(effect: &Value, schema: &Value) {
    assert_object_has_fields(
        effect,
        &json_string_array(&schema["effect_required_fields"]),
        "effect",
    );
    assert_required_values(effect, &schema["effect_required_values"], "effect");
    assert_string_in(
        &effect["pipl_resource_status"],
        &json_string_array(&schema["allowed_pipl_resource_statuses"]),
        "effect.pipl_resource_status",
    );
    for entry in effect["pipl_resource_entries"]
        .as_array()
        .expect("pipl_resource_entries should be an array")
    {
        assert_object_has_fields(
            entry,
            &json_string_array(&schema["pipl_resource_entry_required_fields"]),
            "effect.pipl_resource_entries item",
        );
        assert_required_values(
            entry,
            &schema["pipl_resource_entry_required_values"],
            "effect.pipl_resource_entries item",
        );
    }
    assert_object_has_fields(
        &effect["pipl_content_scan"],
        &json_string_array(&schema["pipl_content_scan_required_fields"]),
        "effect.pipl_content_scan",
    );
    assert_string_in(
        &effect["pipl_content_scan"]["status"],
        &json_string_array(&schema["pipl_content_scan_allowed_statuses"]),
        "effect.pipl_content_scan.status",
    );
    assert_eq!(
        effect["pipl_content_scan"]["contents_emitted"], false,
        "PiPL content scan must not emit raw contents"
    );
    for matched in effect["pipl_content_scan"]["matched_fields"]
        .as_array()
        .expect("matched_fields should be an array")
    {
        assert_object_has_fields(
            matched,
            &json_string_array(&schema["pipl_content_match_required_fields"]),
            "effect.pipl_content_scan.matched_fields item",
        );
        assert_string_in(
            &matched["field"],
            &json_string_array(&schema["pipl_content_match_allowed_fields"]),
            "matched PiPL field",
        );
    }

    assert_object_has_fields(
        &effect["source"],
        &json_string_array(&schema["source_required_fields"]),
        "effect.source",
    );
    assert_required_values(
        &effect["source"],
        &schema["source_required_values"],
        "effect.source",
    );
    assert_eq!(
        effect["source"]["path"], effect["path"],
        "source.path should mirror effect.path"
    );
    assert_eq!(
        effect["source"]["origin"], effect["fixture_status"],
        "source.origin should mirror fixture_status"
    );

    assert_object_has_fields(
        &effect["identity"],
        &json_string_array(&schema["identity_required_fields"]),
        "effect.identity",
    );
    assert_entrypoint_matches_schema(
        &effect["identity"]["entrypoint"],
        schema,
        "effect.identity.entrypoint",
    );
    assert_entrypoint_matches_schema(&effect["entrypoint"], schema, "effect.entrypoint");

    assert_object_has_fields(
        &effect["classification"],
        &json_string_array(&schema["classification_required_fields"]),
        "effect.classification",
    );
    assert_eq!(
        effect["classification"]["plugin_class"], effect["plugin_class"],
        "classification.plugin_class should mirror effect.plugin_class"
    );
    assert_eq!(
        effect["classification"]["confidence"], effect["confidence"],
        "classification.confidence should mirror effect.confidence"
    );
    assert_eq!(
        effect["classification"]["evidence"], effect["evidence"],
        "classification.evidence should mirror effect.evidence"
    );
    assert_eq!(
        effect["classification"]["blocked_reasons"], effect["blocked_reasons"],
        "classification.blocked_reasons should mirror effect.blocked_reasons"
    );
    assert_eq!(
        effect["classification"]["deferred_features"], effect["deferred_features"],
        "classification.deferred_features should mirror effect.deferred_features"
    );

    assert_object_has_fields(
        &effect["frame"],
        &json_string_array(&schema["frame_required_fields"]),
        "effect.frame",
    );
    assert_required_values(
        &effect["frame"],
        &schema["frame_required_values"],
        "effect.frame",
    );
    assert_object_has_fields(
        &effect["frame"]["time_model"],
        &json_string_array(&schema["time_model_required_fields"]),
        "effect.frame.time_model",
    );
    assert_required_values(
        &effect["frame"]["time_model"],
        &schema["time_model_required_values"],
        "effect.frame.time_model",
    );

    assert_object_has_fields(
        &effect["host_surfaces"],
        &json_string_array(&schema["host_surfaces_required_fields"]),
        "effect.host_surfaces",
    );
    for surface in json_string_array(&schema["host_surfaces_required_fields"]) {
        let surface_value = &effect["host_surfaces"][surface];
        assert_object_has_fields(
            surface_value,
            &json_string_array(&schema["host_surface_required_fields"]),
            "effect.host_surfaces surface",
        );
        assert_required_values(
            surface_value,
            &schema["host_surface_required_values"],
            "effect.host_surfaces surface",
        );
        assert_string_in(
            &surface_value["status"],
            &json_string_array(&schema["host_surface_allowed_statuses"][surface]),
            "host surface status",
        );
    }

    assert_object_has_fields(
        &effect["execution"],
        &json_string_array(&schema["execution_required_fields"]),
        "effect.execution",
    );
    assert_required_values(
        &effect["execution"],
        &schema["execution_required_values"],
        "effect.execution",
    );
    assert_string_in(
        &effect["execution"]["allowlist_status"],
        &json_string_array(&schema["allowed_allowlist_statuses"]),
        "execution.allowlist_status",
    );
    for selector in effect["execution"]["selectors"]
        .as_array()
        .expect("execution.selectors should be an array")
    {
        assert_object_has_fields(
            selector,
            &json_string_array(&schema["execution_selector_required_fields"]),
            "execution selector",
        );
        assert_string_in(
            &selector["status"],
            &json_string_array(&schema["allowed_selector_statuses"]),
            "execution selector status",
        );
    }

    assert_object_has_fields(
        &effect["risk"],
        &json_string_array(&schema["risk_required_fields"]),
        "effect.risk",
    );
    assert_required_values(
        &effect["risk"],
        &schema["risk_required_values"],
        "effect.risk",
    );
    assert_string_in(
        &effect["risk"]["license"],
        &json_string_array(&schema["allowed_risk_licenses"]),
        "risk.license",
    );

    assert_string_in(
        &effect["status"],
        &json_string_array(&schema["allowed_effect_statuses"]),
        "effect.status",
    );
    assert_string_in(
        &effect["plugin_class"],
        &json_string_array(&schema["allowed_plugin_classes"]),
        "effect.plugin_class",
    );
    assert_string_in(
        &effect["classification"]["plugin_class"],
        &json_string_array(&schema["allowed_plugin_classes"]),
        "effect.classification.plugin_class",
    );
    assert_string_in(
        &effect["confidence"],
        &json_string_array(&schema["allowed_confidences"]),
        "effect.confidence",
    );
}

fn assert_catalog_matches_schema(catalog: &Value, schema: &Value) {
    assert_object_has_fields(
        catalog,
        &json_string_array(&schema["required_fields"]),
        "catalog",
    );
    assert_required_values(catalog, &schema["required_values"], "catalog");
    assert_object_has_fields(
        &catalog["input"],
        &json_string_array(&schema["input_required_fields"]),
        "catalog.input",
    );
    assert_array_contains_all(
        &catalog["notes"],
        &json_string_array(&schema["required_notes"]),
        "catalog.notes",
    );

    let effects = catalog["effects"]
        .as_array()
        .expect("catalog.effects should be an array");
    for effect in effects {
        assert_effect_matches_schema(effect, schema);
    }

    assert_no_forbidden_fields(catalog, schema);
    assert_no_forbidden_serialized_tokens(catalog, schema);
}

fn synthetic_status_inventory_json() -> &'static str {
    r#"{
      "schema_version": 1,
      "publication_status": "local-only",
      "safety_notes": ["metadata-only inventory"],
      "aex_candidates": [
        {
          "path": "D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins\\AdaptiveFilterRust\\rust\\target\\release\\AdaptiveFilter.aex",
          "bytes": 207360,
          "inferred_class": "likely-classic-effect",
          "fixture_status": "local-build-candidate"
        },
        {
          "path": "C:\\AEPluginBuild\\fin\\DistortChroma.aex",
          "bytes": 234496,
          "inferred_class": "likely-classic-effect",
          "fixture_status": "local-build-candidate"
        },
        {
          "path": "C:\\AEPluginBuild\\ExEditRemoteAEGP.aex",
          "bytes": 189952,
          "inferred_class": "aegp",
          "fixture_status": "not-first-render-fixture"
        },
        {
          "path": "C:\\Ae_Plugins\\FlowONNX\\target\\release\\FlowONNX.aex",
          "bytes": 600576,
          "inferred_class": "heavy-or-specialized-effect",
          "fixture_status": "defer"
        },
        {
          "path": "C:\\Ae_Plugins\\Mystery\\Mystery.aex",
          "bytes": 1234,
          "inferred_class": "unknown-effect-like",
          "fixture_status": "defer"
        }
      ]
    }"#
}

#[test]
fn report_schema_sidecar_declares_no_execution_static_metadata_boundary() {
    let schema = report_schema();

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["evidence_mode"],
        "default-no-binary-open-static-metadata"
    );
    assert_eq!(schema["default_options"]["inspect_pe"], false);
    assert_eq!(schema["default_options"]["inspect_adjacent_source"], false);

    for (name, value) in schema["no_execution_invariants"]
        .as_object()
        .expect("no_execution_invariants should be an object")
    {
        assert_eq!(
            value, false,
            "no-execution invariant {name} must stay false"
        );
    }

    for category in ["candidate", "deferred", "blocked"] {
        assert!(
            schema["status_vocabulary"][category]
                .as_array()
                .expect("status vocabulary category should be an array")
                .iter()
                .all(Value::is_string),
            "status vocabulary category {category} should contain strings"
        );
    }
}

#[test]
fn default_static_classifier_catalog_matches_report_schema_sidecar() {
    let schema = report_schema();
    let catalog = classify(synthetic_status_inventory_json());
    assert_catalog_matches_schema(&catalog, &schema);

    let statuses = catalog["effects"]
        .as_array()
        .expect("effects should be an array")
        .iter()
        .map(|effect| {
            effect["status"]
                .as_str()
                .expect("effect status should be a string")
        })
        .collect::<BTreeSet<_>>();
    for required_status in json_string_array(&schema["required_default_status_coverage"]) {
        assert!(
            statuses.contains(required_status),
            "synthetic catalog should cover default status {required_status}"
        );
    }
}

#[test]
fn real_inventory_default_static_catalog_matches_report_schema_sidecar() {
    let schema = report_schema();
    let inventory = include_str!("../../analysis/AE_AEX_AEP_STATIC_INVENTORY_2026-05-31.json");
    let catalog = classify(inventory);

    assert_catalog_matches_schema(&catalog, &schema);
    assert!(
        catalog["effects"]
            .as_array()
            .expect("effects should be an array")
            .len()
            >= 20,
        "real inventory should still expose local AEX candidates"
    );
}
