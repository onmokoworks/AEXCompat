use std::io::ErrorKind;
use std::path::Path;

use serde_json::Value;

const WORKER_CAPABILITY_SCHEMA: &str =
    "analysis/AEX_WORKER_CAPABILITY_REPORT_SCHEMA_2026-05-31.json";

fn load_analysis_json(relative_path: &str) -> Option<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .join(relative_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!(
                "skipping AEX worker capability schema guard; artifact is absent: {}",
                path.display()
            );
            return None;
        }
        Err(err) => panic!("failed to read analysis artifact {}: {err}", path.display()),
    };

    Some(serde_json::from_str(&text).expect("analysis artifact should be JSON"))
}

fn string_field<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("expected string field {field} in {value:?}"))
}

fn string_array(value: &Value, field: &str) -> Vec<String> {
    value[field]
        .as_array()
        .unwrap_or_else(|| panic!("expected array field {field} in {value:?}"))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| panic!("expected string array item in {field}: {item:?}"))
                .to_owned()
        })
        .collect()
}

fn pipe_options(value: &Value) -> Vec<&str> {
    value
        .as_str()
        .unwrap_or_default()
        .split('|')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect()
}

fn assert_pipe_options(value: &Value, expected: &[&str]) {
    assert_eq!(
        pipe_options(value),
        expected,
        "unexpected option vocabulary for {value:?}"
    );
}

#[test]
fn worker_capability_schema_freezes_metadata_shape_and_blocked_boundaries() {
    let Some(schema) = load_analysis_json(WORKER_CAPABILITY_SCHEMA) else {
        return;
    };

    assert_eq!(schema["schema_version"], 1);
    assert_eq!(
        string_field(&schema, "name"),
        "AEX worker capability report schema"
    );
    assert_eq!(
        string_field(&schema, "publication_status"),
        "local-only design artifact"
    );

    let capability = &schema["capability"];
    for field in [
        "plugin_id",
        "plugin_path",
        "plugin_class",
        "publication_status",
        "load_status",
        "entrypoint",
        "selectors",
        "params",
        "pixel_formats",
        "required_suites",
        "unsupported",
        "last_result",
    ] {
        assert!(
            capability.get(field).is_some(),
            "worker capability report should document field {field}"
        );
    }

    assert_eq!(
        string_field(capability, "plugin_id"),
        "stable local id from allowlist or catalog"
    );
    assert_eq!(
        string_field(capability, "plugin_path"),
        "absolute path to .aex"
    );
    assert_pipe_options(
        &capability["plugin_class"],
        &["classic-effect", "aegp", "aeio", "smartfx", "unknown"],
    );
    assert_pipe_options(
        &capability["publication_status"],
        &["local-only", "public-candidate", "blocked", "unknown"],
    );
    assert_pipe_options(
        &capability["load_status"],
        &["not_loaded", "loaded", "failed", "timeout", "crash"],
    );
    assert_pipe_options(
        &capability["entrypoint"],
        &["resolved-from-pipl", "export", "unknown", "null"],
    );
    assert_eq!(
        string_array(capability, "pixel_formats"),
        vec!["rgba8".to_owned()],
        "v0 worker capability reports should only claim rgba8"
    );

    let selectors = capability["selectors"]
        .as_array()
        .expect("selectors should document report item shape");
    let selector = selectors
        .first()
        .expect("selector item example should stay present");
    assert_eq!(string_field(selector, "name"), "PF_Cmd_GLOBAL_SETUP");
    assert_pipe_options(
        &selector["status"],
        &["ok", "unsupported", "failed", "not_run"],
    );
    assert_eq!(string_field(selector, "error"), "optional short error");

    let params = capability["params"]
        .as_array()
        .expect("params should document report item shape");
    let param = params
        .first()
        .expect("param item example should stay present");
    for field in [
        "id", "label", "kind", "default", "min", "max", "choices", "animated",
    ] {
        assert!(
            param.get(field).is_some(),
            "parameter report shape should document field {field}"
        );
    }
    assert_pipe_options(
        &param["kind"],
        &[
            "float", "int", "bool", "choice", "color", "point", "path", "unknown",
        ],
    );
    assert_pipe_options(&param["animated"], &["supported", "unsupported", "unknown"]);

    let unsupported = string_array(capability, "unsupported");
    for feature in [
        "SmartFX",
        "GPU",
        "AEGP suites",
        "AEIO",
        "audio",
        "layer checkout",
        "custom UI",
    ] {
        assert!(
            unsupported.iter().any(|item| item == feature),
            "unsupported or blocked host surface should stay explicit: {feature}"
        );
    }

    let last_result = &capability["last_result"];
    assert_pipe_options(
        &last_result["status"],
        &[
            "ok",
            "allowlist_denied",
            "unsupported_selector",
            "unsupported_suite",
            "timeout",
            "plugin_exception",
            "worker_crash",
            "internal_error",
        ],
    );
    assert_eq!(string_field(last_result, "elapsed_ms"), "integer");
    assert_eq!(string_field(last_result, "warnings"), "array of strings");

    let notes = string_array(&schema, "notes");
    assert!(notes
        .iter()
        .any(|note| { note.contains("Unsupported features must be reported instead of faked.") }));
    assert!(notes.iter().any(|note| {
        note.contains("Crash and timeout history must not include binary payloads")
            && note.contains("private image contents")
    }));

    let serialized = serde_json::to_string(&schema).unwrap().to_ascii_lowercase();
    assert!(!serialized.contains("sha256"));
    assert!(!serialized.contains("base64"));
}
