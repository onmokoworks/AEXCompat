use crate::parameter_gate::{validate, ValidationError, SCATTERMAP_DESCRIPTORS};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

const REQUEST_LIMIT: u64 = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    plugin_id: String,
    assignments: Assignments,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assignments {
    #[serde(rename = "Scatter Amount")]
    scatter_amount: Option<f64>,
    #[serde(rename = "Direction")]
    direction: Option<f64>,
    #[serde(rename = "Random Seed")]
    random_seed: Option<f64>,
    #[serde(rename = "Mix with Original")]
    mix: Option<f64>,
    #[serde(rename = "Invert Map")]
    invert_map: Option<f64>,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    gate: &'static str,
    plugin_id: &'static str,
    assignment_count: usize,
    accepted: bool,
    native_dispatch_permitted: bool,
    native_process_started: bool,
    errors: Vec<ValidationError>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn resolve_inside(
    repository: &Path,
    path: &Path,
    relative_root: &str,
    create_parent: bool,
) -> io::Result<PathBuf> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid("path traversal forbidden"));
    }
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repository.join(path)
    };
    if resolved.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(invalid("JSON path required"));
    }
    let root = repository.join(relative_root);
    if create_parent {
        fs::create_dir_all(&root)?;
        fs::create_dir_all(
            resolved
                .parent()
                .ok_or_else(|| invalid("path parent missing"))?,
        )?;
    }
    let parent = resolved
        .parent()
        .ok_or_else(|| invalid("path parent missing"))?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid("path outside broker-owned root"));
    }
    if !create_parent && !resolved.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid("input resolves outside broker-owned root"));
    }
    Ok(resolved)
}

pub fn run(repository: &Path, request_path: &Path, output_path: &Path) -> io::Result<bool> {
    let request_path = resolve_inside(repository, request_path, "target/render-requests", false)?;
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/render-request-results",
        true,
    )?;
    let metadata = fs::metadata(&request_path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > REQUEST_LIMIT {
        return Err(invalid("render request size invalid"));
    }
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)
        .map_err(|error| invalid(format!("invalid render request: {error}")))?;
    if request.schema_version != 1 || request.plugin_id != "scattermap" {
        return Err(invalid("render request identity mismatch"));
    }
    let values = [
        request.assignments.scatter_amount,
        request.assignments.direction,
        request.assignments.random_seed,
        request.assignments.mix,
        request.assignments.invert_map,
    ];
    let mut errors = Vec::new();
    let mut assignment_count = 0;
    for (descriptor, value) in SCATTERMAP_DESCRIPTORS.into_iter().zip(values) {
        if let Some(value) = value {
            assignment_count += 1;
            if let Some(error) = validate(descriptor, value) {
                errors.push(error);
            }
        }
    }
    let accepted = errors.is_empty();
    let report = Report {
        schema_version: 1,
        gate: "pre_dispatch_parameter_validation",
        plugin_id: "scattermap",
        assignment_count,
        accepted,
        native_dispatch_permitted: accepted,
        native_process_started: false,
        errors,
    };
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(accepted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn repository() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-render-request-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("target/render-requests")).unwrap();
        root
    }

    #[test]
    fn accepted_request_still_does_not_start_native_code() {
        let root = repository();
        let request = root.join("target/render-requests/valid.json");
        let output = root.join("target/render-request-results/valid.json");
        fs::write(&request, br#"{"schema_version":1,"plugin_id":"scattermap","assignments":{"Scatter Amount":500,"Direction":1,"Random Seed":10000,"Mix with Original":0,"Invert Map":1}}"#).unwrap();
        assert!(run(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(report["native_dispatch_permitted"], true);
        assert_eq!(report["native_process_started"], false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejected_request_records_no_native_process_and_create_new_output() {
        let root = repository();
        let request = root.join("target/render-requests/rejected.json");
        let output = root.join("target/render-request-results/rejected.json");
        fs::write(
            &request,
            br#"{"schema_version":1,"plugin_id":"scattermap","assignments":{"Direction":4}}"#,
        )
        .unwrap();
        assert!(!run(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(report["errors"][0]["code"], "parameter_out_of_range");
        assert_eq!(report["native_process_started"], false);
        assert!(run(&root, &request, &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strict_json_rejects_unknown_and_duplicate_assignments() {
        for (name, body) in [
            ("unknown", br#"{"schema_version":1,"plugin_id":"scattermap","assignments":{"Other":1}}"#.as_slice()),
            ("duplicate", br#"{"schema_version":1,"plugin_id":"scattermap","assignments":{"Direction":1,"Direction":2}}"#.as_slice()),
        ] {
            let root = repository();
            let request = root.join(format!("target/render-requests/{name}.json"));
            let output = root.join(format!("target/render-request-results/{name}.json"));
            fs::write(&request, body).unwrap();
            assert!(run(&root, &request, &output).is_err());
            assert!(!output.exists());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn requires_json_paths_inside_broker_owned_roots() {
        let root = repository();
        let request = root.join("target/render-requests/request.txt");
        let output = root.join("target/render-request-results/report.json");
        fs::write(&request, b"{}").unwrap();
        assert!(run(&root, &request, &output).is_err());
        assert!(!output.exists());
        assert!(run(&root, &root.join("outside.json"), &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
