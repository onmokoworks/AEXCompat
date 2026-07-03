use serde_json::Value;

const REFRESH: &str = include_str!("../../analysis/AEX_WIZTREE_AEX_REFRESH_2026-06-01.json");
const SCHEMA: &str = include_str!("../../analysis/AEX_WIZTREE_AEX_REFRESH_SCHEMA_2026-06-01.json");

fn refresh() -> Value {
    serde_json::from_str(REFRESH).expect("AEX WizTree refresh should parse")
}

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("AEX WizTree refresh schema should parse")
}

fn string_array<'a>(value: &'a Value, label: &str) -> Vec<&'a str> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{label} should be an array"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("{label} items should be strings"))
        })
        .collect()
}

fn array_contains_string(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item.as_str() == Some(expected))
}

fn object_has_fields(value: &Value, fields: &[&str], label: &str) {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{label} should be an object"));
    for field in fields {
        assert!(
            object.contains_key(*field),
            "{label} should contain field {field}"
        );
    }
}

#[test]
fn wiztree_aex_refresh_matches_schema_and_preserves_metadata_only_boundary() {
    let refresh = refresh();
    let schema = schema();

    for field in string_array(&schema["required_fields"], "required_fields") {
        assert!(
            refresh.get(field).is_some(),
            "refresh missing schema-required field {field}"
        );
    }
    for (field, expected) in schema["required_values"].as_object().unwrap() {
        assert_eq!(
            refresh.get(field).unwrap_or(&Value::Null),
            expected,
            "refresh field {field} differs from schema"
        );
    }
    for note in string_array(&schema["required_safety_notes"], "required_safety_notes") {
        assert!(
            array_contains_string(&refresh["safety_notes"], note),
            "refresh missing safety note {note}"
        );
    }

    assert_eq!(
        refresh["total_aex_scan"],
        schema["total_aex_scan_required_values"]
    );
    assert_eq!(
        refresh["canonical_non_generated_aex"]["count"],
        schema["canonical_non_generated_aex_required_values"]["count"]
    );
    assert_eq!(
        refresh["canonical_non_generated_aex"]["bytes"],
        schema["canonical_non_generated_aex_required_values"]["bytes"]
    );
    assert_eq!(
        refresh["generated_target_test_artifacts"]["count"],
        schema["generated_target_test_artifacts_required_values"]["count"]
    );
    assert_eq!(
        refresh["generated_target_test_artifacts"]["bytes"],
        schema["generated_target_test_artifacts_required_values"]["bytes"]
    );
    assert_eq!(
        refresh["canonical_non_generated_aex"]["count"]
            .as_u64()
            .unwrap()
            + refresh["generated_target_test_artifacts"]["count"]
                .as_u64()
                .unwrap(),
        refresh["total_aex_scan"]["count"].as_u64().unwrap()
    );
    assert_eq!(
        refresh["canonical_non_generated_aex"]["bytes"]
            .as_u64()
            .unwrap()
            + refresh["generated_target_test_artifacts"]["bytes"]
                .as_u64()
                .unwrap(),
        refresh["total_aex_scan"]["bytes"].as_u64().unwrap()
    );

    let serialized = serde_json::to_string(&refresh)
        .expect("refresh should serialize")
        .to_ascii_lowercase();
    for forbidden in string_array(&schema["forbidden_serialized_tokens"], "forbidden tokens") {
        assert!(
            !serialized.contains(forbidden),
            "refresh should not serialize forbidden token {forbidden}"
        );
    }
}

#[test]
fn wiztree_refresh_excludes_generated_target_artifacts_from_fixture_review() {
    let refresh = refresh();
    let schema = schema();

    let group_fields = string_array(
        &schema["generated_name_group_required_fields"],
        "generated_name_group_required_fields",
    );
    let name_groups = refresh["generated_target_test_artifacts"]["name_groups"]
        .as_array()
        .expect("generated target name groups should be an array");
    for group in name_groups {
        object_has_fields(group, &group_fields, "generated name group");
    }
    for (name, expected_count) in schema["required_generated_name_groups"]
        .as_object()
        .expect("required generated name groups should be object")
    {
        assert!(
            name_groups
                .iter()
                .any(|group| group["name"] == name.as_str() && group["count"] == *expected_count),
            "generated target groups missing {name} count {expected_count}"
        );
    }

    assert!(refresh["generated_target_test_artifacts"]["policy"]
        .as_str()
        .unwrap_or_default()
        .contains("exclude from first-loader fixture review"));
    assert_eq!(
        refresh["recommended_fixture_gate_delta"]
            ["do_not_expand_first_loader_queue_from_generated_target_artifacts"],
        true
    );
    assert_eq!(
        refresh["recommended_fixture_gate_delta"]["do_not_select_fixture_without_manual_approval"],
        true
    );
}

#[test]
fn wiztree_refresh_keeps_current_fixture_gate_candidates_present_but_unapproved() {
    let refresh = refresh();
    let schema = schema();

    let required_fields = string_array(
        &schema["fixture_gate_candidate_required_fields"],
        "fixture_gate_candidate_required_fields",
    );
    let candidates = refresh["fixture_gate_candidates_verified_present"]
        .as_array()
        .expect("fixture gate candidates should be an array");
    assert_eq!(
        candidates.len(),
        schema["fixture_gate_candidate_required_ids"]
            .as_array()
            .unwrap()
            .len()
    );
    for candidate in candidates {
        object_has_fields(candidate, &required_fields, "fixture gate candidate");
        assert!(array_contains_string(
            &schema["fixture_gate_candidate_required_ids"],
            candidate["id"].as_str().unwrap()
        ));
        assert_eq!(
            candidate["current_review_status"],
            schema["fixture_gate_candidate_required_values"]["current_review_status"]
        );
        assert!(candidate["path"]
            .as_str()
            .expect("candidate path")
            .ends_with(".aex"));
        assert!(candidate["bytes"].as_u64().unwrap_or_default() > 0);
    }

    for name in string_array(
        &schema["additional_small_local_build_required_names"],
        "additional small local builds",
    ) {
        assert!(
            refresh["additional_small_local_builds_for_later_review"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| candidate["name"] == name),
            "missing later-review candidate {name}"
        );
    }
}

#[test]
fn wiztree_refresh_pins_blocked_first_loader_classes() {
    let refresh = refresh();
    let schema = schema();
    let blocked = refresh["blocked_first_loader_classes_observed"]
        .as_array()
        .expect("blocked classes should be an array");

    for (class_name, expected_count) in schema["blocked_first_loader_class_required_values"]
        .as_object()
        .expect("blocked class required values should be object")
    {
        assert!(
            blocked.iter().any(|entry| {
                entry["class"] == class_name.as_str() && entry["count"] == *expected_count
            }),
            "missing blocked class {class_name} count {expected_count}"
        );
    }
    assert!(blocked.iter().any(|entry| entry["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("not a first render fixture")));
    assert!(blocked.iter().any(|entry| entry["reason"]
        .as_str()
        .unwrap_or_default()
        .contains("defer until sandbox")));
}
