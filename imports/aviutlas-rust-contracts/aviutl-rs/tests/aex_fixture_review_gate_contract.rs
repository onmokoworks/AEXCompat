use std::collections::BTreeSet;

use serde_json::Value;

const FIXTURE_REVIEW_GATE: &str =
    include_str!("../../analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json");
const FIXTURE_REVIEW_GATE_SCHEMA: &str =
    include_str!("../../analysis/AEX_FIXTURE_REVIEW_GATE_SCHEMA_2026-06-01.json");

fn gate() -> Value {
    serde_json::from_str(FIXTURE_REVIEW_GATE).expect("fixture review gate should parse")
}

fn schema() -> Value {
    serde_json::from_str(FIXTURE_REVIEW_GATE_SCHEMA)
        .expect("fixture review gate schema should parse")
}

fn array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .expect("expected array")
        .iter()
        .any(|item| item.as_str() == Some(expected))
}

fn schema_string_array<'a>(schema: &'a Value, key: &str) -> Vec<&'a str> {
    schema[key]
        .as_array()
        .unwrap_or_else(|| panic!("schema {key} should be an array"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("schema {key} entries should be strings"))
        })
        .collect()
}

#[test]
fn fixture_review_gate_schema_matches_current_closed_gate() {
    let gate = gate();
    let schema = schema();

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["artifact"],
        "analysis/AEX_FIXTURE_REVIEW_GATE_2026-05-31.json"
    );

    for field in schema_string_array(&schema, "required_fields") {
        assert!(
            gate.get(field).is_some(),
            "fixture review gate missing schema-required field {field}"
        );
    }
    let required_values = schema["required_values"]
        .as_object()
        .expect("schema required_values should be an object");
    for (field, expected) in required_values {
        assert_eq!(
            gate.get(field).unwrap_or(&Value::Null),
            expected,
            "fixture review gate field {field} differs from schema"
        );
    }

    for flag in schema_string_array(&schema, "approval_flags_must_be_false") {
        assert_eq!(gate["approval"][flag], false, "approval flag {flag}");
    }

    let policy_values = schema["single_fixture_policy_required_values"]
        .as_object()
        .expect("schema single_fixture_policy_required_values should be an object");
    for (field, expected) in policy_values {
        assert_eq!(
            &gate["single_fixture_policy"][field], expected,
            "single fixture policy field {field}"
        );
    }

    let runtime_values = schema["runtime_evidence_required_values"]
        .as_object()
        .expect("schema runtime_evidence_required_values should be an object");
    for (field, expected) in runtime_values {
        assert_eq!(
            &gate["required_runtime_evidence_before_loader"][field], expected,
            "runtime evidence field {field}"
        );
    }

    let candidates = gate["candidates"]
        .as_array()
        .expect("candidates should be an array");
    let candidate_required_values = schema["candidate_required_values"]
        .as_object()
        .expect("schema candidate_required_values should be an object");
    for candidate in candidates {
        for field in schema_string_array(&schema, "candidate_required_fields") {
            assert!(
                candidate.get(field).is_some(),
                "candidate missing schema-required field {field}"
            );
        }
        for (field, expected) in candidate_required_values {
            assert_eq!(candidate.get(field).unwrap_or(&Value::Null), expected);
        }
        for field in schema_string_array(&schema, "optional_static_evidence_required_fields") {
            assert!(
                candidate["optional_static_evidence"].get(field).is_some(),
                "optional_static_evidence missing schema-required field {field}"
            );
        }
        for reason in schema_string_array(&schema, "required_candidate_blocked_reasons") {
            assert!(array_contains(&candidate["blocked_reasons"], reason));
        }
    }

    for class_name in schema_string_array(&schema, "allowed_rejected_first_loader_classes") {
        assert!(array_contains(
            &gate["rejected_first_loader_classes"],
            class_name
        ));
    }
    for note in schema_string_array(&schema, "required_notes") {
        assert!(array_contains(&gate["notes"], note));
    }

    let serialized = serde_json::to_string(&gate)
        .expect("gate should serialize")
        .to_ascii_lowercase();
    for forbidden in schema_string_array(&schema, "forbidden_serialized_tokens") {
        assert!(
            !serialized.contains(&forbidden.to_ascii_lowercase()),
            "fixture review gate should not contain serialized token {forbidden}"
        );
    }
}

#[test]
fn fixture_review_gate_stays_metadata_only_and_unapproved() {
    let gate = gate();

    assert_eq!(gate["schema_version"], 1);
    assert_eq!(gate["status"], "review_queue_not_approved");
    assert_eq!(
        gate["metadata_mode"],
        "path-and-size-only-no-hash-no-binary-payload"
    );
    assert_eq!(gate["selected_fixture"], Value::Null);
    assert_eq!(gate["recommended_first_review"], "adaptive-filter-local");
    assert_eq!(
        gate["recommendation_status"],
        "queue-order-only-not-approval"
    );

    let approval = gate["approval"]
        .as_object()
        .expect("approval should be an object");
    for (field, value) in approval {
        assert_eq!(
            value, false,
            "approval flag {field} must remain false until an explicit loader slice"
        );
    }

    let policy = &gate["single_fixture_policy"];
    assert_eq!(policy["max_selected_fixtures"], 1);
    for required in [
        "selection_requires_manual_user_approval",
        "selection_requires_local_only_license_review",
        "selection_requires_source_tree_review",
        "selection_requires_binary_redistribution_review",
        "selection_requires_loader_gate_opened_by_separate_slice",
        "no_parallel_first_loader_fixtures",
    ] {
        assert_eq!(
            policy[required], true,
            "single fixture policy field {required} must stay true"
        );
    }
}

#[test]
fn fixture_review_gate_candidates_are_unique_local_not_approved_entries() {
    let gate = gate();
    let candidates = gate["candidates"]
        .as_array()
        .expect("candidates should be an array");
    assert_eq!(
        candidates.len(),
        2,
        "review queue should stay small and explicit"
    );

    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut first_review_count = 0usize;
    for candidate in candidates {
        let id = candidate["id"]
            .as_str()
            .expect("candidate id should be a string");
        let path = candidate["path"]
            .as_str()
            .expect("candidate path should be a string");
        assert!(ids.insert(id), "duplicate candidate id {id}");
        assert!(paths.insert(path), "duplicate candidate path {path}");
        assert!(
            path.ends_with(".aex"),
            "candidate should point to .aex metadata only"
        );
        assert_eq!(candidate["fixture_status"], "local-build-candidate");
        assert_eq!(candidate["plugin_class"], "classic-effect-candidate");
        assert_eq!(candidate["review_status"], "not-approved");
        assert!(
            candidate["observed_size_bytes"]
                .as_u64()
                .is_some_and(|size| size > 0),
            "candidate should have nonzero observed size metadata"
        );
        assert!(
            candidate["source_license_evidence"]
                .as_str()
                .is_some_and(|text| text.contains("MIT")),
            "candidate should carry local source license evidence"
        );
        assert!(array_contains(
            &candidate["blocked_reasons"],
            "manual user approval has not selected this fixture"
        ));
        assert!(array_contains(
            &candidate["blocked_reasons"],
            "real loader slice is not opened"
        ));
        if candidate["review_priority"] == 1 {
            first_review_count += 1;
            assert_eq!(id, gate["recommended_first_review"]);
        }
    }
    assert_eq!(first_review_count, 1, "exactly one first review candidate");
}

#[test]
fn fixture_review_gate_serialization_excludes_payload_and_loader_success_tokens() {
    let gate = gate();
    let serialized = serde_json::to_string(&gate)
        .expect("gate should serialize")
        .to_ascii_lowercase();

    for forbidden in [
        "sha256",
        "base64",
        "binary_payload",
        "payload_bytes",
        "loadlibrary",
        "native_load_performed",
        "\"approved\":true",
        "\"loader_enabled\":true",
        "\"real_aex_load_enabled\":true",
        "\"render_png_enabled\":true",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "fixture review gate should not contain serialized token {forbidden}"
        );
    }

    assert!(array_contains(
        &gate["notes"],
        "This artifact is a review queue, not permission to load .aex."
    ));
    assert!(array_contains(
        &gate["rejected_first_loader_classes"],
        "smartfx-only"
    ));
    assert_eq!(
        gate["required_runtime_evidence_before_loader"]["worker_identity_revalidation"],
        "passed"
    );
    assert_eq!(
        gate["required_runtime_evidence_before_loader"]["sandbox_preflight"],
        "passed"
    );
    assert_eq!(
        gate["required_runtime_evidence_before_loader"]["ofx_facade"],
        "not-a-loader-and-not-a-bypass"
    );
}
