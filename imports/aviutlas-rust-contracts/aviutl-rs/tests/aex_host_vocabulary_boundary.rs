use std::path::Path;

use serde_json::Value;

const BOUNDARY_SCHEMA: &str =
    include_str!("../../analysis/AEX_HOST_VOCABULARY_BOUNDARY_SCHEMA_2026-06-01.json");

fn boundary_schema() -> Value {
    serde_json::from_str(BOUNDARY_SCHEMA).expect("AEX host vocabulary boundary schema should parse")
}

fn string_array(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("expected JSON array")
        .iter()
        .map(|item| item.as_str().expect("expected string array item"))
        .collect()
}

fn read_repo_file(repo_relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .join(repo_relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .to_path_buf()
}

#[test]
fn aex_host_vocabulary_boundary_schema_pins_cleanroom_policy() {
    let schema = boundary_schema();

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        schema["compatibility_classification"],
        "Cleanroom planning vocabulary guard"
    );
    assert_eq!(
        schema["allowed_policy"]["native_loader_calls_allowed"],
        false
    );
    assert_eq!(schema["allowed_policy"]["adobe_sdk_headers_allowed"], false);
    assert_eq!(schema["allowed_policy"]["bindgen_allowed"], false);
    assert_eq!(
        schema["allowed_policy"]["reuse_existing_aviutl_libloading_path_for_aex_allowed"],
        false
    );
    assert_eq!(
        schema["allowed_policy"]["pf_names_are_planning_labels_only"],
        true
    );
    assert_eq!(
        schema["allowed_policy"]["metadata_labels_do_not_define_abi"],
        true
    );
    assert_eq!(
        schema["allowed_policy"]["worker_os_isolation_ffi_allowed"],
        true
    );

    let planning_labels = string_array(&schema["allowed_planning_labels"]);
    for label in [
        "PF_Cmd_GLOBAL_SETUP",
        "PF_Cmd_PARAMS_SETUP",
        "PF_Cmd_SEQUENCE_SETUP",
        "PF_Cmd_FRAME_SETUP",
        "PF_Cmd_RENDER",
        "PF_Cmd_FRAME_SETDOWN",
        "PF_Cmd_SEQUENCE_SETDOWN",
        "PF_Cmd_GLOBAL_SETDOWN",
        "PF_InData",
        "PF_OutData",
        "PF_ParamDef[]",
        "PF_LayerDef source",
        "PF_LayerDef destination",
    ] {
        assert!(
            planning_labels.contains(&label),
            "planning label {label} should be explicitly allowed"
        );
    }

    let metadata_labels = string_array(&schema["allowed_metadata_labels"]);
    for label in [
        "EffectMain",
        "AEEffect",
        "PIPLType::AEEffect",
        "Property::AE_Effect_Match_Name",
        "Property::CodeWin64X86",
    ] {
        assert!(
            metadata_labels.contains(&label),
            "metadata label {label} should be explicitly allowed"
        );
    }

    let forbidden = string_array(&schema["source_forbidden_substrings"]);
    for token in [
        "libloading",
        "libloading::",
        "LoadLibrary",
        "LoadLibraryA",
        "LoadLibraryW",
        "LoadLibraryEx",
        "GetProcAddress",
        "windows::Win32::System::LibraryLoader",
        "bindgen",
        "after_effects",
        "after_effects::",
        "pipl::",
        "#[repr(C)]",
        "extern \"C\" fn EffectMain",
        "extern \"system\" fn EffectMain",
        "struct PF_InData",
        "std::mem::transmute",
    ] {
        assert!(
            forbidden.contains(&token),
            "forbidden source token {token} should be guarded"
        );
    }
}

#[test]
fn aex_source_files_do_not_cross_native_loader_or_sdk_boundary() {
    let schema = boundary_schema();
    let forbidden = string_array(&schema["source_forbidden_substrings"]);
    let source_files = string_array(&schema["source_files"]);
    assert!(
        !source_files.is_empty(),
        "boundary schema should list source files to scan"
    );

    for repo_relative in source_files {
        let text = read_repo_file(repo_relative);
        for token in &forbidden {
            assert!(
                !text.contains(token),
                "{repo_relative} should not contain forbidden AEX cleanroom token {token}"
            );
        }
    }
}

#[test]
fn aex_source_files_cover_all_aex_and_ofx_example_sources() {
    let schema = boundary_schema();
    let source_files = string_array(&schema["source_files"]);
    let source_set: std::collections::BTreeSet<_> = source_files.iter().copied().collect();
    let examples_dir = repo_root().join("aviutl-rs/examples");

    let mut discovered = std::fs::read_dir(&examples_dir)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", examples_dir.display()))
        .map(|entry| entry.expect("example entry should read").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            (name.starts_with("aex") || name.starts_with("ofx"))
                .then(|| format!("aviutl-rs/examples/{name}"))
        })
        .collect::<Vec<_>>();
    discovered.sort();

    let missing = discovered
        .iter()
        .filter(|path| !source_set.contains(path.as_str()))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "AEX/OFX example sources missing from cleanroom boundary schema: {missing:?}"
    );
}

#[test]
fn native_stage_plan_uses_pf_names_only_as_no_load_labels() {
    let schema = boundary_schema();
    let stage_plan_source = read_repo_file("aviutl-rs/examples/aex_native_stage_plan.rs");
    let stage_plan_schema = read_repo_file("analysis/AEX_NATIVE_STAGE_PLAN_SCHEMA_2026-06-01.json");

    for label in string_array(&schema["allowed_planning_labels"]) {
        assert!(
            stage_plan_source.contains(label) || stage_plan_schema.contains(label),
            "allowed planning label {label} should be visible in native stage planning artifacts"
        );
    }

    assert!(stage_plan_source.contains("planned_not_run"));
    assert!(stage_plan_source.contains("declared_not_allocated"));
    assert!(stage_plan_source.contains("selectors_executed: false"));
    assert!(stage_plan_source.contains("native_load_performed: false"));
}
