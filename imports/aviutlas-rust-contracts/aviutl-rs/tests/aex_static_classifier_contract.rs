#[allow(dead_code)]
#[path = "../examples/aex_static_classifier.rs"]
mod aex_static_classifier;

use serde_json::Value;
use std::fs;
use std::path::Path;

fn classify(input: &str) -> Value {
    let output =
        aex_static_classifier::classify_inventory_json(input).expect("classifier should run");
    serde_json::from_str(&output).expect("classifier should emit JSON")
}

fn classify_with_pe_inspection(input: &str) -> Value {
    let output = aex_static_classifier::classify_inventory_json_with_options(
        input,
        aex_static_classifier::ClassifierOptions {
            inspect_pe: true,
            ..Default::default()
        },
    )
    .expect("classifier should run");
    serde_json::from_str(&output).expect("classifier should emit JSON")
}

fn classify_with_adjacent_source(input: &str) -> Value {
    let output = aex_static_classifier::classify_inventory_json_with_options(
        input,
        aex_static_classifier::ClassifierOptions {
            inspect_adjacent_source: true,
            ..Default::default()
        },
    )
    .expect("classifier should run");
    serde_json::from_str(&output).expect("classifier should emit JSON")
}

fn classify_with_pe_adjacent_source_and_pipl_payload(input: &str) -> Value {
    let output = aex_static_classifier::classify_inventory_json_with_options(
        input,
        aex_static_classifier::ClassifierOptions {
            inspect_pe: true,
            inspect_adjacent_source: true,
            inspect_pipl_payload: true,
        },
    )
    .expect("classifier should run");
    serde_json::from_str(&output).expect("classifier should emit JSON")
}

fn assert_neutral_capability_schema_sections(effect: &Value) {
    for section in [
        "source",
        "identity",
        "classification",
        "frame",
        "host_surfaces",
        "execution",
        "risk",
    ] {
        assert!(
            effect.get(section).is_some_and(Value::is_object),
            "effect should expose neutral capability section: {section}"
        );
    }
    assert!(
        effect.get("last_probe").is_some_and(Value::is_null),
        "static classifier should expose an explicit null last_probe"
    );

    assert_eq!(effect["source"]["kind"], "aex");
    assert_eq!(effect["source"]["path_publication"], "local-only");
    assert_eq!(effect["execution"]["load_status"], "not_loaded");
    assert_eq!(effect["execution"]["broker_may_load_plugin"], false);
    assert_eq!(effect["frame"]["preferred_pixel_format"], "rgba8");
    assert!(effect["frame"]["pixel_formats"]
        .as_array()
        .expect("pixel formats should be an array")
        .iter()
        .any(|format| format == "rgba8"));
    assert_eq!(effect["host_surfaces"]["aex_worker"]["supported"], false);
    assert_eq!(
        effect["host_surfaces"]["aviutlas_external_effect"]["supported"],
        false
    );
    assert_eq!(effect["host_surfaces"]["ofx_facade"]["supported"], false);
    assert_eq!(effect["render_capable"], false);
}

#[test]
fn catalog_declares_static_local_only_boundary() {
    let catalog = classify(
        r#"{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": ["metadata-only inventory", "no file hashes recorded"],
          "aex_candidates": []
        }"#,
    );

    assert_eq!(catalog["schema_version"], 1);
    assert_eq!(catalog["catalog_id"], "external-effects-local");
    assert_eq!(catalog["publication_status"], "local-only");
    assert_eq!(catalog["status"], "metadata-only-static-inference");
    assert!(catalog["effects"].as_array().unwrap().is_empty());

    let notes = catalog["notes"]
        .as_array()
        .expect("notes should be present");
    assert!(notes.iter().any(|note| note
        .as_str()
        .unwrap()
        .contains("No .aex file was opened, hashed, loaded, or executed.")));
    assert!(notes
        .iter()
        .any(|note| note.as_str().unwrap().contains("not render-capable")));
}

#[test]
fn synthetic_inventory_effects_emit_neutral_capability_schema_sections() {
    let catalog = classify(
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
        }"#,
    );

    let effects = catalog["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 4);
    for effect in effects {
        assert_neutral_capability_schema_sections(effect);
        assert_eq!(effect["pe_machine"], Value::Null);
        assert!(effect["exports"].as_array().unwrap().is_empty());
        assert_eq!(effect["resource_summary"], "not-inspected");
        assert_eq!(effect["pipl_resource_status"], "not-inspected");
        assert!(effect["pipl_resource_entries"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(
            ["required", "denied"]
                .contains(&effect["execution"]["allowlist_status"].as_str().unwrap()),
            "static metadata should require or deny loader allowlisting"
        );
    }
}

#[test]
fn adjacent_source_whitelist_candidates_are_static_probe_candidates_only() {
    let catalog = classify(
        r#"{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": [],
          "aex_candidates": [
            {
              "path": "D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins\\AdaptiveFilterRust\\rust\\target\\release\\AdaptiveFilter.aex",
              "bytes": 207360,
              "inferred_class": "likely-classic-effect",
              "fixture_status": "local-build-candidate"
            },
            {
              "path": "D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins\\MedianProRust\\rust\\target\\release\\MedianPro.aex",
              "bytes": 207360,
              "inferred_class": "likely-classic-effect",
              "fixture_status": "local-build-candidate"
            }
          ]
        }"#,
    );

    let effects = catalog["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 2);
    for effect in effects {
        assert_eq!(effect["plugin_class"], "classic-effect-candidate");
        assert_eq!(effect["status"], "candidate_for_contract_probe");
        assert_eq!(effect["confidence"], "inferred");
        assert_eq!(effect["render_capable"], false);
        assert_eq!(effect["entrypoint"]["name"], "EffectMain");
        assert_eq!(effect["entrypoint"]["verified_from_binary"], false);
        assert_eq!(effect["pipl_kind"], "AEEffect");
        assert_eq!(effect["source"]["kind"], "aex");
        assert_eq!(effect["source"]["path_publication"], "local-only");
        assert_eq!(
            effect["source"]["binary_publication"],
            "blocked-until-review"
        );
        assert_eq!(effect["source"]["origin"], "local-build-candidate");
        assert_eq!(effect["identity"]["entrypoint"]["name"], "EffectMain");
        assert_eq!(
            effect["classification"]["plugin_class"],
            "classic-effect-candidate"
        );
        assert_eq!(effect["params"].as_array().unwrap().len(), 0);
        assert_eq!(effect["frame"]["preferred_pixel_format"], "rgba8");
        assert_eq!(
            effect["host_surfaces"]["aex_worker"]["status"],
            "contract-only"
        );
        assert_eq!(effect["execution"]["load_status"], "not_loaded");
        assert_eq!(effect["execution"]["allowlist_status"], "required");
        assert_eq!(effect["execution"]["broker_may_load_plugin"], false);
        assert_eq!(effect["execution"]["selectors"][0]["name"], "EffectMain");
        assert_eq!(effect["risk"]["cleanroom"], "local-only");
        assert_eq!(effect["risk"]["publication"], "do-not-publish-binary");
        assert_eq!(effect["pe_machine"], Value::Null);
        assert!(effect["exports"].as_array().unwrap().is_empty());
        assert_eq!(effect["resource_summary"], "not-inspected");
        assert_eq!(effect["pipl_resource_status"], "not-inspected");
        assert!(effect["pipl_resource_entries"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(effect["deferred_features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature == "SmartFX"));
    }
}

#[test]
fn aegp_entries_are_blocked_and_not_render_capable() {
    let catalog = classify(
        r#"{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": [],
          "aex_candidates": [
            {
              "path": "C:\\AEPluginBuild\\ExEditRemoteAEGP.aex",
              "bytes": 189952,
              "inferred_class": "aegp",
              "fixture_status": "not-first-render-fixture"
            }
          ]
        }"#,
    );

    let effect = &catalog["effects"][0];
    assert_eq!(effect["plugin_class"], "aegp");
    assert_eq!(effect["status"], "blocked_aegp");
    assert_eq!(effect["render_capable"], false);
    assert_eq!(effect["host_surfaces"]["aex_worker"]["status"], "blocked");
    assert_eq!(effect["execution"]["allowlist_status"], "denied");
    assert!(effect["execution"]["unsupported"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "AEGP"));
    assert_eq!(effect["pipl_kind"], "AEGP");
    assert!(effect["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason
            .as_str()
            .unwrap()
            .contains("not v0 image render fixtures")));
    assert!(effect["deferred_features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "AEGP"));
}

#[test]
fn heavy_and_unknown_entries_are_conservative() {
    let catalog = classify(
        r#"{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": [],
          "aex_candidates": [
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
        }"#,
    );

    let heavy = &catalog["effects"][0];
    assert_eq!(heavy["plugin_class"], "blocked");
    assert_eq!(heavy["status"], "defer_heavy_dependency");
    assert_eq!(heavy["render_capable"], false);
    assert_eq!(heavy["execution"]["allowlist_status"], "denied");
    assert!(heavy["execution"]["unsupported"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "heavy/specialized effect"));

    let unknown = &catalog["effects"][1];
    assert_eq!(unknown["plugin_class"], "unknown");
    assert_eq!(unknown["status"], "unknown_needs_resource_scan");
    assert_eq!(unknown["confidence"], "unverified");
    assert_eq!(unknown["render_capable"], false);
    assert!(unknown["execution"]["unsupported"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "unverified plugin class"));
}

#[test]
fn first_queue_inventory_candidates_remain_unverified() {
    let catalog = classify(
        r#"{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": [],
          "aex_candidates": [
            {
              "path": "C:\\AEPluginBuild\\fin\\DistortChroma.aex",
              "bytes": 234496,
              "inferred_class": "likely-classic-effect",
              "fixture_status": "local-build-candidate"
            }
          ]
        }"#,
    );

    let effect = &catalog["effects"][0];
    assert_eq!(effect["plugin_class"], "classic-effect-candidate");
    assert_eq!(effect["status"], "classified_from_inventory");
    assert_eq!(effect["confidence"], "inferred");
    assert_eq!(effect["entrypoint"], Value::Null);
    assert_eq!(effect["fixture_status"], "local-build-candidate");
    assert_eq!(effect["host_surfaces"]["aex_worker"]["status"], "blocked");
    assert_eq!(effect["execution"]["allowlist_status"], "denied");
    assert_eq!(
        effect["recommended_next_action"],
        "confirm adjacent source/PiPL metadata before image probe"
    );
}

#[test]
fn optional_pe_inspection_reads_headers_exports_and_resource_names_only() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-static-classifier-test");
    fs::create_dir_all(&fixture_root).expect("fixture root should be writable");
    let fixture_path = fixture_root.join("SyntheticEffect.aex");
    fs::write(&fixture_path, synthetic_pe64_with_export_and_pipl())
        .expect("synthetic PE fixture should be written under target");
    let fixture_json_path = fixture_path.to_string_lossy().replace('\\', "\\\\");

    let catalog = classify_with_pe_inspection(&format!(
        r#"{{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": ["synthetic PE metadata fixture"],
          "aex_candidates": [
            {{
              "path": "{fixture_json_path}",
              "bytes": 5120,
              "inferred_class": "likely-classic-effect",
              "fixture_status": "synthetic-test-fixture"
            }}
          ]
        }}"#,
    ));

    assert!(catalog["notes"].as_array().unwrap().iter().any(|note| note
        .as_str()
        .unwrap()
        .contains("PE inspection reads headers/export names/resource names only")));
    let effect = &catalog["effects"][0];
    assert_eq!(effect["pe_machine"], "x86_64");
    assert_eq!(effect["exports"][0], "EffectMain");
    assert!(effect["resource_summary"]
        .as_str()
        .unwrap()
        .contains("types=PiPL"));
    assert_eq!(effect["pipl_resource_status"], "present_metadata_only");
    assert_eq!(effect["pipl_content_scan"]["status"], "not_requested");
    let pipl_entries = effect["pipl_resource_entries"].as_array().unwrap();
    assert_eq!(pipl_entries.len(), 1);
    assert_eq!(pipl_entries[0]["resource_id"], 16000);
    assert_eq!(pipl_entries[0]["resource_name"], Value::Null);
    assert_eq!(pipl_entries[0]["language_id"], 1033);
    assert_eq!(pipl_entries[0]["data_size"], 64);
    assert_eq!(pipl_entries[0]["code_page"], 1252);
    assert_eq!(pipl_entries[0]["contents_read"], false);
    assert_eq!(effect["execution"]["load_status"], "not_loaded");
    assert_eq!(effect["execution"]["broker_may_load_plugin"], false);
    assert_eq!(effect["render_capable"], false);
    assert!(effect["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|evidence| {
            evidence
                .as_str()
                .unwrap()
                .contains("read-only PE metadata inspection completed")
        }));
    assert!(effect["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|evidence| {
            evidence
                .as_str()
                .unwrap()
                .contains("PE PiPL resource metadata entries observed: 1")
        }));
    assert!(effect["risk"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|note| { note.as_str().unwrap().contains("without loading code") }));
    let serialized = serde_json::to_string(&catalog)
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in [
        "sha256",
        "base64",
        "binary_payload",
        "input_png",
        "output_png",
        "worker_exe",
        "loadlibrary",
        "libloading",
        "rendered_pixels",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "classifier output should not contain {forbidden}"
        );
    }
}

#[test]
fn optional_pe_inspection_reports_absent_pipl_for_other_resource_types() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-static-classifier-test");
    fs::create_dir_all(&fixture_root).expect("fixture root should be writable");
    let fixture_path = fixture_root.join("SyntheticNoPipl.aex");
    let mut bytes = synthetic_pe64_with_export_and_pipl();
    put_utf16(&mut bytes, 0x422, "TEXT");
    fs::write(&fixture_path, bytes).expect("synthetic PE fixture should be written under target");
    let fixture_json_path = fixture_path.to_string_lossy().replace('\\', "\\\\");

    let catalog = classify_with_pe_inspection(&format!(
        r#"{{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": ["synthetic PE metadata fixture"],
          "aex_candidates": [
            {{
              "path": "{fixture_json_path}",
              "bytes": 5120,
              "inferred_class": "likely-classic-effect",
              "fixture_status": "synthetic-test-fixture"
            }}
          ]
        }}"#,
    ));

    let effect = &catalog["effects"][0];
    assert_eq!(effect["pe_machine"], "x86_64");
    assert!(effect["resource_summary"]
        .as_str()
        .unwrap()
        .contains("types=TEXT"));
    assert_eq!(effect["pipl_resource_status"], "absent");
    assert!(effect["pipl_resource_entries"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(effect["execution"]["load_status"], "not_loaded");
    assert_eq!(effect["render_capable"], false);
}

#[test]
fn bounded_pipl_content_scan_matches_expected_fields_without_emitting_bytes() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-static-classifier-test")
        .join("PayloadSource")
        .join("rust");
    fs::create_dir_all(source_root.join("target").join("release"))
        .expect("source fixture root should be writable");
    fs::write(
        source_root.join("build.rs"),
        r#"use pipl::*;

fn main() {
    pipl::plugin_build(vec![
        Property::Kind(PIPLType::AEEffect),
        Property::Name("SyntheticEffect"),
        Property::Category("Filter"),
        Property::CodeWin64X86("EffectMain"),
        Property::AE_Effect_Match_Name("ONMK_SyntheticEffect"),
    ]);
}
"#,
    )
    .expect("synthetic build.rs should be written under target");
    let fixture_path = source_root
        .join("target")
        .join("release")
        .join("SyntheticEffect.aex");
    fs::write(&fixture_path, synthetic_pe64_with_export_and_pipl())
        .expect("synthetic PE fixture should be written under target");
    let fixture_json_path = fixture_path.to_string_lossy().replace('\\', "\\\\");

    let catalog = classify_with_pe_adjacent_source_and_pipl_payload(&format!(
        r#"{{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": ["synthetic bounded PiPL scan fixture"],
          "aex_candidates": [
            {{
              "path": "{fixture_json_path}",
              "bytes": 5120,
              "inferred_class": "likely-classic-effect",
              "fixture_status": "synthetic-test-fixture"
            }}
          ]
        }}"#,
    ));

    let effect = &catalog["effects"][0];
    let scan = &effect["pipl_content_scan"];
    assert_eq!(scan["status"], "semantic_matches");
    assert_eq!(scan["scan_mode"], "bounded-expected-string-match-only");
    assert_eq!(scan["bytes_limit"], 16 * 1024);
    assert_eq!(scan["bytes_read"], 64);
    assert_eq!(scan["truncated"], false);
    assert_eq!(scan["contents_emitted"], false);
    assert!(scan["unmatched_expected_fields"]
        .as_array()
        .unwrap()
        .is_empty());
    let matched = scan["matched_fields"].as_array().unwrap();
    for field in [
        "pipl_name",
        "pipl_category",
        "pipl_match_name",
        "entrypoint",
    ] {
        assert!(
            matched.iter().any(|item| item["field"] == field),
            "bounded scan should match {field}"
        );
    }
    assert_eq!(effect["execution"]["load_status"], "not_loaded");
    assert_eq!(effect["execution"]["broker_may_load_plugin"], false);
    assert_eq!(effect["render_capable"], false);
    assert_eq!(effect["last_probe"], Value::Null);
    let serialized = serde_json::to_string(&catalog)
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in [
        "sha256",
        "base64",
        "binary_payload",
        "payload_bytes",
        "payload_hash",
        "binary_bytes",
        "raw_pipl",
        "pipl_bytes",
        "hex_dump",
        "input_png",
        "output_png",
        "worker_exe",
        "native_load_result",
        "rendered_pixels",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "bounded scan output should not contain {forbidden}"
        );
    }
}

#[test]
fn optional_adjacent_source_inspection_reads_build_rs_pipl_metadata_only() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-static-classifier-test")
        .join("SourceOnlyRust")
        .join("rust");
    fs::create_dir_all(&source_root).expect("source fixture root should be writable");
    fs::write(
        source_root.join("build.rs"),
        r#"use pipl::*;

fn main() {
    pipl::plugin_build(vec![
        Property::Kind(PIPLType::AEEffect),
        Property::Name("SourceOnly"),
        Property::Category("Filter"),
        Property::CodeWin64X86("EffectMain"),
        Property::AE_Effect_Global_OutFlags_2(OutFlags2::SupportsSmartRender),
        Property::AE_Effect_Match_Name("ONMK_SourceOnly"),
    ]);
}
"#,
    )
    .expect("synthetic build.rs should be written under target");
    let plugin_path = source_root
        .join("target")
        .join("release")
        .join("SourceOnly.aex");
    let plugin_json_path = plugin_path.to_string_lossy().replace('\\', "\\\\");

    let catalog = classify_with_adjacent_source(&format!(
        r#"{{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": ["synthetic adjacent-source fixture"],
          "aex_candidates": [
            {{
              "path": "{plugin_json_path}",
              "bytes": 42,
              "inferred_class": "likely-classic-effect",
              "fixture_status": "local-build-candidate"
            }}
          ]
        }}"#,
    ));

    assert!(catalog["notes"].as_array().unwrap().iter().any(|note| note
        .as_str()
        .unwrap()
        .contains("Adjacent source inspection reads build.rs")));
    let effect = &catalog["effects"][0];
    assert_eq!(effect["status"], "candidate_for_contract_probe");
    assert_eq!(effect["confidence"], "observed-adjacent-source");
    assert_eq!(effect["pipl_kind"], "AEEffect");
    assert_eq!(effect["pipl_name"], "SourceOnly");
    assert_eq!(effect["pipl_category"], "Filter");
    assert_eq!(effect["pipl_match_name"], "ONMK_SourceOnly");
    assert_eq!(effect["entrypoint"]["name"], "EffectMain");
    assert_eq!(effect["entrypoint"]["source"], "adjacent-build-rs");
    assert_eq!(effect["entrypoint"]["verified_from_binary"], false);
    assert_eq!(effect["pe_machine"], Value::Null);
    assert!(effect["exports"].as_array().unwrap().is_empty());
    assert_eq!(effect["resource_summary"], "not-inspected");
    assert_eq!(effect["pipl_resource_status"], "not-inspected");
    assert!(effect["pipl_resource_entries"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(effect["execution"]["load_status"], "not_loaded");
    assert_eq!(effect["execution"]["allowlist_status"], "required");
    assert_eq!(effect["execution"]["broker_may_load_plugin"], false);
    assert_eq!(effect["render_capable"], false);
    assert!(effect["deferred_features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "SmartFX"));
    assert!(effect["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|evidence| {
            evidence
                .as_str()
                .unwrap()
                .contains("adjacent build.rs PiPL kind: AEEffect")
        }));
}

#[test]
fn adjacent_source_identity_mismatch_does_not_promote_candidate() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-static-classifier-test")
        .join("MismatchedSourceRust")
        .join("rust");
    fs::create_dir_all(&source_root).expect("source fixture root should be writable");
    fs::write(
        source_root.join("build.rs"),
        r#"use pipl::*;

fn main() {
    pipl::plugin_build(vec![
        Property::Kind(PIPLType::AEEffect),
        Property::Name("DifferentEffect"),
        Property::Category("Filter"),
        Property::CodeWin64X86("EffectMain"),
        Property::AE_Effect_Match_Name("ONMK_DifferentEffect"),
    ]);
}
"#,
    )
    .expect("synthetic build.rs should be written under target");
    let plugin_path = source_root
        .join("target")
        .join("release")
        .join("RenamedBinary.aex");
    let plugin_json_path = plugin_path.to_string_lossy().replace('\\', "\\\\");

    let catalog = classify_with_adjacent_source(&format!(
        r#"{{
          "schema_version": 1,
          "publication_status": "local-only",
          "safety_notes": ["synthetic adjacent-source fixture"],
          "aex_candidates": [
            {{
              "path": "{plugin_json_path}",
              "bytes": 42,
              "inferred_class": "likely-classic-effect",
              "fixture_status": "local-build-candidate"
            }}
          ]
        }}"#,
    ));

    let effect = &catalog["effects"][0];
    assert_eq!(effect["status"], "classified_from_inventory");
    assert_eq!(effect["confidence"], "inferred");
    assert_eq!(effect["entrypoint"], Value::Null);
    assert_eq!(effect["execution"]["allowlist_status"], "denied");
    assert!(effect["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .any(|evidence| {
            evidence
                .as_str()
                .unwrap()
                .contains("adjacent build.rs identity mismatch")
        }));
}

#[test]
fn real_inventory_shape_is_accepted_without_binary_inspection() {
    let inventory = include_str!("../../analysis/AE_AEX_AEP_STATIC_INVENTORY_2026-05-31.json");
    let catalog = classify(inventory);

    let effects = catalog["effects"].as_array().unwrap();
    assert!(
        effects.len() >= 20,
        "real static inventory should expose local AEX candidates"
    );
    assert!(effects.iter().all(|effect| effect["pe_machine"].is_null()));
    assert!(effects
        .iter()
        .all(|effect| effect["exports"].as_array().unwrap().is_empty()));
    assert!(effects
        .iter()
        .all(|effect| effect["resource_summary"] == "not-inspected"));
    assert!(effects
        .iter()
        .all(|effect| effect["pipl_resource_status"] == "not-inspected"));
    assert!(effects.iter().all(|effect| effect["pipl_resource_entries"]
        .as_array()
        .unwrap()
        .is_empty()));
    assert!(effects
        .iter()
        .all(|effect| effect["render_capable"] == false));
    assert!(effects
        .iter()
        .all(|effect| effect["last_probe"] == Value::Null));
    assert!(effects
        .iter()
        .all(|effect| effect["source"]["path_publication"] == "local-only"));
    assert!(effects
        .iter()
        .all(|effect| effect["execution"]["broker_may_load_plugin"] == false));
    for effect in effects {
        assert_neutral_capability_schema_sections(effect);
    }
}

fn synthetic_pe64_with_export_and_pipl() -> Vec<u8> {
    let mut bytes = vec![0u8; 0x1400];
    bytes[0..2].copy_from_slice(b"MZ");
    put_u32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");

    let coff = 0x84;
    put_u16(&mut bytes, coff, 0x8664);
    put_u16(&mut bytes, coff + 2, 1);
    put_u16(&mut bytes, coff + 16, 0xf0);
    put_u16(&mut bytes, coff + 18, 0x2022);

    let optional = 0x98;
    put_u16(&mut bytes, optional, 0x20b);
    put_u32(&mut bytes, optional + 108, 16);
    put_u32(&mut bytes, optional + 112, 0x1100);
    put_u32(&mut bytes, optional + 116, 0x80);
    put_u32(&mut bytes, optional + 112 + 16, 0x1200);
    put_u32(&mut bytes, optional + 112 + 20, 0x80);

    let section = optional + 0xf0;
    bytes[section..section + 6].copy_from_slice(b".rdata");
    put_u32(&mut bytes, section + 8, 0x1000);
    put_u32(&mut bytes, section + 12, 0x1000);
    put_u32(&mut bytes, section + 16, 0x1000);
    put_u32(&mut bytes, section + 20, 0x200);

    let export = 0x300;
    put_u32(&mut bytes, export + 24, 1);
    put_u32(&mut bytes, export + 32, 0x1140);
    put_u32(&mut bytes, 0x340, 0x1150);
    bytes[0x350..0x35b].copy_from_slice(b"EffectMain\0");

    let resource = 0x400;
    put_u16(&mut bytes, resource + 12, 1);
    put_u32(&mut bytes, resource + 16, 0x8000_0020);
    put_u32(&mut bytes, resource + 20, 0x8000_0030);
    put_u16(&mut bytes, resource + 0x20, 4);
    put_utf16(&mut bytes, resource + 0x22, "PiPL");
    put_u16(&mut bytes, resource + 0x30 + 14, 1);
    put_u32(&mut bytes, resource + 0x40, 16000);
    put_u32(&mut bytes, resource + 0x44, 0x8000_0050);
    put_u16(&mut bytes, resource + 0x50 + 14, 1);
    put_u32(&mut bytes, resource + 0x60, 1033);
    put_u32(&mut bytes, resource + 0x64, 0x70);
    put_u32(&mut bytes, resource + 0x70, 0x1280);
    put_u32(&mut bytes, resource + 0x74, 64);
    put_u32(&mut bytes, resource + 0x78, 1252);
    let pipl_payload = b"SyntheticEffect\0Filter\0ONMK_SyntheticEffect\0EffectMain\0";
    bytes[0x480..0x480 + pipl_payload.len()].copy_from_slice(pipl_payload);

    bytes
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_utf16(bytes: &mut [u8], offset: usize, value: &str) {
    for (index, unit) in value.encode_utf16().enumerate() {
        put_u16(bytes, offset + index * 2, unit);
    }
}
