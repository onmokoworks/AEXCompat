#[allow(dead_code)]
#[path = "../examples/aex_probe_readiness.rs"]
mod aex_probe_readiness;

use serde_json::Value;

const CAPABILITY_DRAFT_SCHEMA: &str =
    include_str!("../../analysis/AEX_READINESS_CAPABILITY_DRAFT_SCHEMA_2026-06-01.json");
const IMAGE_PROBE_REQUEST_SCHEMA: &str =
    include_str!("../../analysis/AEX_IMAGE_PROBE_REQUEST_SCHEMA_2026-05-31.json");

type JsonArtifact = (String, Value);
type PlanOutput = (Value, Value, Value, Vec<JsonArtifact>, Vec<JsonArtifact>);
type PlanWithGateOutput = (
    Value,
    Value,
    Value,
    Vec<JsonArtifact>,
    Vec<JsonArtifact>,
    Option<JsonArtifact>,
    Option<JsonArtifact>,
);

fn plan(input: &str) -> PlanOutput {
    let output = aex_probe_readiness::plan_probe_readiness_json(input)
        .expect("readiness planner should run");
    let allowlist = serde_json::from_str(&output.allowlist_json).expect("allowlist should be JSON");
    let readiness = serde_json::from_str(&output.readiness_json).expect("readiness should be JSON");
    let loader_gate =
        serde_json::from_str(&output.loader_gate_json).expect("loader gate should be JSON");
    let requests = output
        .requests
        .into_iter()
        .map(|request| {
            (
                request.file_name,
                serde_json::from_str(&request.json).expect("request should be JSON"),
            )
        })
        .collect();
    let capabilities = output
        .capabilities
        .into_iter()
        .map(|capability| {
            (
                capability.file_name,
                serde_json::from_str(&capability.json).expect("capability should be JSON"),
            )
        })
        .collect();
    (allowlist, readiness, loader_gate, requests, capabilities)
}

fn plan_with_gate(input: &str, gate: &str) -> PlanWithGateOutput {
    let output =
        aex_probe_readiness::plan_probe_readiness_json_with_fixture_gate(input, Some(gate))
            .expect("readiness planner should run with fixture gate");
    let allowlist = serde_json::from_str(&output.allowlist_json).expect("allowlist should be JSON");
    let readiness = serde_json::from_str(&output.readiness_json).expect("readiness should be JSON");
    let loader_gate =
        serde_json::from_str(&output.loader_gate_json).expect("loader gate should be JSON");
    let requests = output
        .requests
        .into_iter()
        .map(|request| {
            (
                request.file_name,
                serde_json::from_str(&request.json).expect("request should be JSON"),
            )
        })
        .collect();
    let capabilities = output
        .capabilities
        .into_iter()
        .map(|capability| {
            (
                capability.file_name,
                serde_json::from_str(&capability.json).expect("capability should be JSON"),
            )
        })
        .collect();
    let fixture_review = output.fixture_review.map(|review| {
        (
            review.file_name,
            serde_json::from_str(&review.json).expect("fixture review should be JSON"),
        )
    });
    let loader_preflight = output.loader_preflight.map(|preflight| {
        (
            preflight.file_name,
            serde_json::from_str(&preflight.json).expect("preflight should be JSON"),
        )
    });
    (
        allowlist,
        readiness,
        loader_gate,
        requests,
        capabilities,
        fixture_review,
        loader_preflight,
    )
}

fn capability_draft_schema() -> Value {
    serde_json::from_str(CAPABILITY_DRAFT_SCHEMA).expect("capability draft schema should parse")
}

fn image_probe_request_schema() -> Value {
    serde_json::from_str(IMAGE_PROBE_REQUEST_SCHEMA)
        .expect("image probe request schema should parse")
}

fn json_string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("expected JSON array")
        .iter()
        .map(|item| item.as_str().expect("expected string array item"))
        .collect()
}

fn assert_capability_matches_draft_schema(capability: &Value, schema: &Value) {
    for field in json_string_array(&schema["required_fields"]) {
        assert!(
            capability.get(field).is_some(),
            "capability draft missing required field {field}: {capability:?}"
        );
    }

    for (field, expected) in schema["required_values"]
        .as_object()
        .expect("required_values should be an object")
    {
        assert_eq!(
            &capability[field], expected,
            "capability draft field {field} diverged from schema"
        );
    }

    let selector_names = capability["selectors"]
        .as_array()
        .expect("selectors should be an array")
        .iter()
        .map(|selector| {
            assert_eq!(
                selector["status"], schema["selector_status"],
                "selector should stay not_run"
            );
            selector["name"]
                .as_str()
                .expect("selector name should be a string")
        })
        .collect::<Vec<_>>();
    assert_eq!(selector_names, json_string_array(&schema["selector_names"]));

    assert_eq!(
        capability["aex_worker"]["supported"],
        schema["aex_worker"]["supported"]
    );
    assert_eq!(
        capability["aex_worker"]["status"],
        schema["aex_worker"]["status"]
    );
    assert_eq!(
        capability["ofx_facade"]["supported"],
        schema["ofx_facade"]["supported"]
    );
    assert_eq!(
        capability["ofx_facade"]["status"],
        schema["ofx_facade"]["status"]
    );

    for surface in json_string_array(&schema["required_unsupported_or_deferred_surfaces"]) {
        assert!(
            capability["unsupported_or_deferred_surfaces"]
                .as_array()
                .expect("unsupported/deferred surfaces should be an array")
                .iter()
                .any(|entry| entry == surface),
            "capability should keep {surface} deferred"
        );
    }

    for field in json_string_array(&schema["forbidden_fields"]) {
        assert!(
            capability.get(field).is_none(),
            "capability draft should not contain forbidden field {field}"
        );
    }

    let serialized = serde_json::to_string(capability)
        .expect("capability should serialize")
        .to_ascii_lowercase();
    for token in json_string_array(&schema["forbidden_serialized_tokens"]) {
        assert!(
            !serialized.contains(token),
            "capability draft should not contain serialized token {token}"
        );
    }
}

#[test]
fn describe_requests_match_image_probe_request_schema_guard() {
    let input = include_str!("fixtures/aex_probe_readiness_inventory.synthetic.json");
    let schema = image_probe_request_schema();
    let (_, _, _, requests, _) = plan(input);
    let schema_request = &schema["request"];

    assert_eq!(schema["schema_version"], 1);
    assert!(schema_request["operation"]
        .as_str()
        .expect("schema operation should be a string")
        .split('|')
        .map(str::trim)
        .any(|operation| operation == "describe"));
    assert_eq!(schema_request["pixel_format"], "rgba8");
    for field in ["plugin_path", "allowlist"] {
        assert!(
            schema_request[field]
                .as_str()
                .expect("schema field should document request boundary")
                .contains("required for describe/render_png"),
            "schema should require {field} for describe requests"
        );
    }

    assert!(
        !requests.is_empty(),
        "readiness should emit describe requests"
    );
    for (file_name, request) in requests {
        assert_eq!(request["operation"], "describe", "{file_name}");
        assert!(
            request["plugin_path"]
                .as_str()
                .expect("describe request should include plugin_path")
                .ends_with(".aex"),
            "{file_name} plugin_path should target a .aex"
        );
        assert!(
            request.get("allowlist").is_some(),
            "{file_name} should include an allowlist boundary"
        );
        assert_eq!(
            request["pixel_format"], schema_request["pixel_format"],
            "{file_name}"
        );
        for field in ["worker_exe", "input_png", "output_png", "loader_intent"] {
            assert!(
                request.get(field).is_none(),
                "{file_name} describe request should not include {field}"
            );
        }
    }
}

#[test]
fn inventory_input_generates_describe_only_draft_allowlist() {
    let input = include_str!("fixtures/aex_probe_readiness_inventory.synthetic.json");
    let (allowlist, readiness, loader_gate, requests, capabilities) = plan(input);
    let capability_schema = capability_draft_schema();

    assert_eq!(allowlist["schema_version"], 1);
    assert_eq!(allowlist["allowlist_publication_status"], "local-only");
    assert_eq!(allowlist["draft_status"], "not-approved");
    assert_eq!(allowlist["entries"].as_array().unwrap().len(), 2);
    assert_eq!(readiness["candidate_count"], 4);
    assert_eq!(readiness["allowlist_entry_count"], 2);
    assert_eq!(readiness["blocked_count"], 2);
    assert!(readiness["generated_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file == "loader-gate.local.json"));
    assert!(readiness["generated_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file.as_str().unwrap().starts_with("capabilities/")));
    assert_eq!(loader_gate["status"], "loader_gate_not_opened");
    assert_eq!(loader_gate["approved"], false);
    assert_eq!(loader_gate["loader_enabled"], false);
    assert_eq!(loader_gate["real_aex_load_enabled"], false);
    assert_eq!(loader_gate["candidate_count"], 2);
    assert_eq!(loader_gate["open_candidate_count"], 0);
    assert_eq!(loader_gate["blocked_count"], 2);
    assert!(loader_gate["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note.as_str().unwrap().contains("not loader approval")));
    assert_eq!(requests.len(), 2);
    assert_eq!(capabilities.len(), 2);

    for entry in allowlist["entries"].as_array().unwrap() {
        assert_eq!(entry["expected_class"], "classic-effect");
        assert_eq!(entry["allowed_operations"], serde_json::json!(["describe"]));
        assert_eq!(entry["fixture_status"], "local-build-candidate");
        assert_eq!(entry["publication_status"], "local-only");
        assert_eq!(entry["license_status"], "local-only-unpublished");
        assert_eq!(entry["classifier_status"], "candidate_for_contract_probe");
        assert_eq!(
            entry["classifier_inferred_class"],
            "classic-effect-candidate"
        );
        assert!(entry["plugin_path"].as_str().unwrap().ends_with(".aex"));
    }

    for (_, request) in requests {
        assert_eq!(request["schema_version"], 1);
        assert_eq!(request["operation"], "describe");
        assert_eq!(request["allowlist"], "../allowlist.local.draft.json");
        assert_eq!(request["pixel_format"], "rgba8");
        assert!(request.get("worker_exe").is_none());
        assert!(request.get("input_png").is_none());
        assert!(request.get("output_png").is_none());
    }

    for (_, capability) in capabilities {
        assert_capability_matches_draft_schema(&capability, &capability_schema);
    }

    for entry in loader_gate["entries"].as_array().unwrap() {
        assert_eq!(entry["pre_loader_status"], "blocked_pending_review");
        assert_eq!(entry["loader_approval_status"], "not-approved");
        assert_eq!(entry["allowlist_operation_status"], "describe-only");
        assert_eq!(entry["sandbox_preflight_required"], "passed");
        assert_eq!(entry["job_object_required"], "assigned-with-kill-on-close");
        assert_eq!(
            entry["handle_inheritance_required"],
            "sentinel_not_inherited-with-explicit-handle-list"
        );
        assert_eq!(entry["worker_identity_revalidation_required"], "passed");
        assert_eq!(entry["worker_attestation_required"], "passed");
        assert_eq!(
            entry["ofx_facade_status"],
            "deferred-same-broker-worker-contract"
        );
        assert!(entry["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .contains("render_png is not approved")));
        assert!(entry["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("job_object_status")));
    }
}

#[test]
fn static_classifier_catalog_input_preserves_schema_bridge_fields() {
    let catalog = r#"{
      "schema_version": 1,
      "catalog_id": "external-effects-local",
      "publication_status": "local-only",
      "effects": [
        {
          "schema_version": 1,
          "effect_id": "adaptivefilter-local",
          "path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "pipl_name": "AdaptiveFilter",
          "pipl_match_name": "ONMK_AdaptiveFilter",
          "source": {
            "path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
            "origin": "local-build-candidate"
          },
          "identity": {
            "display_name": "AdaptiveFilter",
            "match_name": "ONMK_AdaptiveFilter"
          },
          "classification": {
            "plugin_class": "classic-effect-candidate"
          },
          "pipl_content_scan": {
            "status": "semantic_matches",
            "contents_emitted": false
          }
        },
        {
          "schema_version": 1,
          "effect_id": "unknown-local",
          "path": "D:\\AviUtlas\\local\\Unknown.aex",
          "fixture_status": "defer",
          "plugin_class": "unknown",
          "status": "unknown_needs_resource_scan"
        }
      ]
    }"#;

    let (allowlist, readiness, loader_gate, requests, capabilities) = plan(catalog);
    let capability_schema = capability_draft_schema();

    assert_eq!(allowlist["entries"].as_array().unwrap().len(), 1);
    let entry = &allowlist["entries"][0];
    assert_eq!(entry["id"], "adaptivefilter-local");
    assert_eq!(
        entry["plugin_path"],
        "D:\\AviUtlas\\local\\AdaptiveFilter.aex"
    );
    assert_eq!(requests[0].0, "ONMK-AdaptiveFilter.describe.json");
    assert_eq!(capabilities[0].0, "ONMK-AdaptiveFilter.capability.json");
    assert_capability_matches_draft_schema(&capabilities[0].1, &capability_schema);
    assert_eq!(
        loader_gate["entries"][0]["declared_size_bytes"],
        Value::Null
    );
    assert!(loader_gate["entries"][0]["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason
            .as_str()
            .unwrap()
            .contains("declared_size_bytes is missing")));
    assert_eq!(readiness["entries"][0]["status"], "draft_allowlisted");
    assert_eq!(
        readiness["entries"][0]["pipl_content_scan_status"],
        "semantic_matches"
    );
    assert_eq!(readiness["entries"][0]["pipl_content_scan_ready"], true);
    assert_eq!(readiness["entries"][1]["status"], "blocked_or_deferred");
    assert!(readiness["entries"][1]["blocked_reason"]
        .as_str()
        .unwrap()
        .contains("fixture_status"));
}

#[test]
fn static_classifier_catalog_requires_semantic_pipl_scan_for_readiness() {
    let catalog = r#"{
      "schema_version": 1,
      "catalog_id": "external-effects-local",
      "publication_status": "local-only",
      "effects": [
        {
          "effect_id": "semantic-local",
          "path": "D:\\AviUtlas\\local\\SemanticReady.aex",
          "size": 207360,
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "SemanticReady", "match_name": "ONMK_SemanticReady"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "semantic_matches", "contents_emitted": false}
        },
        {
          "effect_id": "partial-local",
          "path": "D:\\AviUtlas\\local\\PartialScan.aex",
          "size": 207360,
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "PartialScan", "match_name": "ONMK_PartialScan"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "partial_semantic_matches", "contents_emitted": false}
        },
        {
          "effect_id": "missing-local",
          "path": "D:\\AviUtlas\\local\\MissingScan.aex",
          "size": 207360,
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "MissingScan", "match_name": "ONMK_MissingScan"},
          "classification": {"plugin_class": "classic-effect-candidate"}
        },
        {
          "effect_id": "emits-local",
          "path": "D:\\AviUtlas\\local\\EmitsContents.aex",
          "size": 207360,
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "EmitsContents", "match_name": "ONMK_EmitsContents"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "semantic_matches", "contents_emitted": true}
        }
      ]
    }"#;

    let (allowlist, readiness, loader_gate, requests, capabilities) = plan(catalog);

    assert_eq!(allowlist["entries"].as_array().unwrap().len(), 1);
    assert_eq!(allowlist["entries"][0]["id"], "semantic-local");
    assert_eq!(requests.len(), 1);
    assert_eq!(capabilities.len(), 1);
    assert_eq!(loader_gate["candidate_count"], 1);
    assert_eq!(loader_gate["entries"][0]["effect_id"], "semantic-local");

    let entries = readiness["entries"].as_array().unwrap();
    let semantic = entries
        .iter()
        .find(|entry| entry["effect_id"] == "semantic-local")
        .expect("semantic scan candidate should be present");
    assert_eq!(semantic["status"], "draft_allowlisted");
    assert_eq!(semantic["pipl_content_scan_status"], "semantic_matches");
    assert_eq!(semantic["pipl_content_scan_ready"], true);

    for (effect_id, expected_status) in [
        ("partial-local", "partial_semantic_matches"),
        ("missing-local", "missing"),
        ("emits-local", "semantic_matches"),
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry["effect_id"] == effect_id)
            .unwrap_or_else(|| panic!("{effect_id} entry should be present"));
        assert_eq!(entry["status"], "blocked_or_deferred", "{effect_id}");
        assert_eq!(
            entry["pipl_content_scan_status"], expected_status,
            "{effect_id}"
        );
        assert_eq!(entry["pipl_content_scan_ready"], false, "{effect_id}");
        assert!(entry["blocked_reason"]
            .as_str()
            .unwrap()
            .contains("pipl_content_scan.status"));
        assert!(entry["blocked_reason"]
            .as_str()
            .unwrap()
            .contains("contents_emitted"));
    }
}

#[test]
fn generated_outputs_share_normalized_effect_path_and_artifact_policy() {
    let catalog = r#"{
      "effects": [
        {
          "effect_id": " ACME Alpha Filter ",
          "path": "D:/AviUtlas/local/Alpha.Filter.aex",
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "Alpha Filter", "match_name": "ACME/Alpha Filter"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "semantic_matches", "contents_emitted": false}
        }
      ]
    }"#;

    let (allowlist, readiness, loader_gate, requests, capabilities) = plan(catalog);
    let normalized_path = "D:\\AviUtlas\\local\\Alpha.Filter.aex";

    assert_eq!(allowlist["entries"][0]["id"], "acme-alpha-filter-local");
    assert_eq!(allowlist["entries"][0]["plugin_path"], normalized_path);
    assert_eq!(
        readiness["entries"][0]["effect_id"],
        "acme-alpha-filter-local"
    );
    assert_eq!(readiness["entries"][0]["plugin_path"], normalized_path);
    assert_eq!(
        loader_gate["entries"][0]["effect_id"],
        "acme-alpha-filter-local"
    );
    assert_eq!(loader_gate["entries"][0]["plugin_path"], normalized_path);
    assert_eq!(requests[0].0, "ACME-Alpha-Filter.describe.json");
    assert_eq!(requests[0].1["plugin_path"], normalized_path);
    assert_eq!(capabilities[0].0, "ACME-Alpha-Filter.capability.json");
    assert_eq!(capabilities[0].1["effect_id"], "acme-alpha-filter-local");
    assert_eq!(capabilities[0].1["plugin_path"], normalized_path);
    assert!(readiness["generated_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file == "requests/ACME-Alpha-Filter.describe.json"));
    assert!(readiness["generated_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file == "capabilities/ACME-Alpha-Filter.capability.json"));
}

#[test]
fn duplicate_artifact_stems_fail_closed_without_suffixing() {
    let catalog = r#"{
      "effects": [
        {
          "effect_id": "first-local",
          "path": "D:\\AviUtlas\\local\\First.aex",
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "First", "match_name": "ACME_Duplicate"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "semantic_matches", "contents_emitted": false}
        },
        {
          "effect_id": "second-local",
          "path": "D:\\AviUtlas\\local\\Second.aex",
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "Second", "match_name": "ACME Duplicate"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "semantic_matches", "contents_emitted": false}
        }
      ]
    }"#;

    let (allowlist, readiness, loader_gate, requests, capabilities) = plan(catalog);

    assert_eq!(allowlist["entries"].as_array().unwrap().len(), 0);
    assert_eq!(readiness["candidate_count"], 2);
    assert_eq!(readiness["allowlist_entry_count"], 0);
    assert_eq!(readiness["blocked_count"], 2);
    assert_eq!(loader_gate["candidate_count"], 0);
    assert!(requests.is_empty());
    assert!(capabilities.is_empty());
    assert!(readiness["generated_files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|file| {
            let file = file.as_str().unwrap();
            !file.starts_with("requests/") && !file.starts_with("capabilities/")
        }));
    assert!(readiness["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| {
            entry["status"] == "blocked_or_deferred"
                && entry["allowed_operations"].as_array().unwrap().is_empty()
                && entry["blocked_reason"]
                    .as_str()
                    .unwrap()
                    .contains("artifact_stem ACME-Duplicate is duplicated")
        }));
}

#[test]
fn fixture_review_gate_filters_static_catalog_to_review_queue() {
    let catalog = r#"{
      "schema_version": 1,
      "catalog_id": "external-effects-local",
      "publication_status": "local-only",
      "effects": [
        {
          "schema_version": 1,
          "effect_id": "adaptivefilter-local",
          "path": "D:\\AviUtlas\\local\\AdaptiveFilter.aex",
          "size": 207360,
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "AdaptiveFilter", "match_name": "ONMK_AdaptiveFilter"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "semantic_matches", "contents_emitted": false}
        },
        {
          "schema_version": 1,
          "effect_id": "medianpro-local",
          "path": "D:\\AviUtlas\\local\\MedianPro.aex",
          "size": 207360,
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "MedianPro", "match_name": "ONMK_MedianPro"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "semantic_matches", "contents_emitted": false}
        },
        {
          "schema_version": 1,
          "effect_id": "minimaxmap-local",
          "path": "D:\\AviUtlas\\local\\MinimaxMap.aex",
          "size": 220000,
          "fixture_status": "local-build-candidate",
          "plugin_class": "classic-effect-candidate",
          "status": "candidate_for_contract_probe",
          "identity": {"display_name": "MinimaxMap", "match_name": "ONMK_MinimaxMap"},
          "classification": {"plugin_class": "classic-effect-candidate"},
          "pipl_content_scan": {"status": "semantic_matches", "contents_emitted": false}
        }
      ]
    }"#;
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

    let (
        allowlist,
        readiness,
        loader_gate,
        requests,
        capabilities,
        fixture_review,
        loader_preflight,
    ) = plan_with_gate(catalog, gate);

    assert_eq!(allowlist["entries"].as_array().unwrap().len(), 2);
    assert_eq!(requests.len(), 2);
    assert_eq!(capabilities.len(), 2);
    let (review_name, review) =
        fixture_review.expect("fixture-gated readiness should emit review packet");
    assert_eq!(review_name, "fixture-review.local.json");
    assert_eq!(review["status"], "manual_selection_required");
    assert_eq!(
        review["metadata_mode"],
        "path-size-static-evidence-only-no-hash-no-binary-payload"
    );
    assert_eq!(review["selected_fixture"], Value::Null);
    assert_eq!(review["candidate_count"], 2);
    assert_eq!(review["selected_candidate_count"], 0);
    assert_eq!(review["auto_approval_granted"], false);
    assert_eq!(review["queue"].as_array().unwrap().len(), 2);
    assert_eq!(review["queue"][0]["id"], "adaptive-filter-local");
    assert_eq!(
        review["queue"][0]["recommendation"],
        "recommended_first_review"
    );
    assert_eq!(review["queue"][0]["selection_status"], "not_selected");
    assert!(review["forbidden_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action.as_str().unwrap().contains("load .aex")));
    let (preflight_name, preflight) =
        loader_preflight.expect("fixture-gated readiness should emit preflight");
    assert_eq!(preflight_name, "loader-preflight.local.json");
    assert_eq!(preflight["status"], "blocked_no_selected_fixture");
    assert_eq!(preflight["preflight_passed"], false);
    assert_eq!(preflight["native_load_performed"], false);
    assert_eq!(preflight["broker_may_load_plugin"], false);
    assert_eq!(preflight["loader_gate_status"], "loader_gate_not_opened");
    assert_eq!(preflight["selected_fixture"], Value::Null);
    assert!(preflight["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason
            .as_str()
            .unwrap()
            .contains("selected_fixture is null")));
    assert_eq!(
        allowlist["fixture_review_gate"]["status"],
        "review_queue_not_approved"
    );
    assert_eq!(
        allowlist["fixture_review_gate"]["selected_fixture"],
        Value::Null
    );
    assert_eq!(
        allowlist["fixture_review_gate"]["effect"],
        "filter-describe-draft-to-review-queue"
    );
    assert_eq!(loader_gate["candidate_count"], 2);
    assert_eq!(loader_gate["open_candidate_count"], 0);
    assert_eq!(loader_gate["approved"], false);
    assert_eq!(loader_gate["loader_enabled"], false);
    assert_eq!(loader_gate["real_aex_load_enabled"], false);
    assert!(readiness["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["effect_id"] == "minimaxmap-local"
            && entry["status"] == "blocked_or_deferred"
            && entry["blocked_reason"]
                .as_str()
                .unwrap()
                .contains("not present in fixture review gate")));
    assert!(readiness["generated_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file == "fixture-review.local.json"));
    assert!(readiness["generated_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file == "loader-preflight.local.json"));
    assert!(allowlist["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note
            .as_str()
            .unwrap()
            .contains("Fixture review gate applied")));
}

#[test]
fn real_static_inventory_stays_draft_and_does_not_enable_render() {
    let input = include_str!("../../analysis/AE_AEX_AEP_STATIC_INVENTORY_2026-05-31.json");
    let (allowlist, readiness, loader_gate, requests, capabilities) = plan(input);
    let capability_schema = capability_draft_schema();

    assert_eq!(allowlist["draft_status"], "not-approved");
    assert!(allowlist["entries"].as_array().unwrap().len() >= 2);
    assert!(allowlist["entries"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| {
            entry["allowed_operations"] == serde_json::json!(["describe"])
                && entry["license_status"] == "local-only-unpublished"
        }));
    assert_eq!(readiness["publication_status"], "local-only");
    assert_eq!(
        readiness["status"], "probe_readiness_planned",
        "readiness planner must remain a plan, not worker approval"
    );
    assert_eq!(
        loader_gate["status"], "loader_gate_not_opened",
        "loader gate must remain closed until a separate loader slice is opened"
    );
    assert_eq!(loader_gate["approved"], false);
    assert_eq!(loader_gate["loader_enabled"], false);
    assert_eq!(loader_gate["real_aex_load_enabled"], false);
    assert!(loader_gate["requirements_before_open"]
        .as_array()
        .unwrap()
        .iter()
        .any(|requirement| requirement.as_str().unwrap().contains("OFX facade")));
    assert!(loader_gate["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| note
            .as_str()
            .unwrap()
            .contains("cannot bypass AEX allowlist gates")));
    assert!(loader_gate["requirements_before_open"]
        .as_array()
        .unwrap()
        .iter()
        .any(|requirement| requirement
            .as_str()
            .unwrap()
            .contains("job_object_status=assigned")));
    assert!(loader_gate["requirements_before_open"]
        .as_array()
        .unwrap()
        .iter()
        .any(|requirement| requirement.as_str().unwrap().contains("handle_inheritance")));
    assert!(loader_gate["requirements_before_open"]
        .as_array()
        .unwrap()
        .iter()
        .any(|requirement| requirement
            .as_str()
            .unwrap()
            .contains("handle_inheritance_status=sentinel_not_inherited")));
    assert!(loader_gate["requirements_before_open"]
        .as_array()
        .unwrap()
        .iter()
        .any(|requirement| requirement
            .as_str()
            .unwrap()
            .contains("explicit inherited-handle list")));
    assert_eq!(
        requests.len(),
        allowlist["entries"].as_array().unwrap().len(),
        "one describe scaffold per draft allowlist entry"
    );
    assert_eq!(
        capabilities.len(),
        allowlist["entries"].as_array().unwrap().len(),
        "one capability scaffold per draft allowlist entry"
    );
    for (_, capability) in capabilities {
        assert_capability_matches_draft_schema(&capability, &capability_schema);
    }
}
