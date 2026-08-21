pub mod cli_operation;
pub mod cluster_manifest;
pub mod companion_manifest;
pub mod cuda_compute_probe;
pub mod fixture_profiles;
pub mod gpu_platform_collector;
pub mod host_core;
#[cfg(windows)]
pub mod image_render;
pub mod installed_runtime_roots;
#[cfg(windows)]
pub mod l2;
pub mod minidump_policy;
pub mod observability;
pub mod opencl_icd_adapter_binding;
pub mod opencl_icd_collector;
pub mod opencl_runtime_probe;
pub mod parameter_animation;
pub mod plugin_dependency_closure;
pub mod pnp_opencl_runtime_collector;
#[cfg(windows)]
pub mod render;
pub mod render_approval;
#[path = "image_render/artifacts.rs"]
pub mod render_artifacts;
pub mod render_fixture;
pub mod render_pixel_format;
#[cfg(windows)]
pub mod render_request;
#[cfg(windows)]
pub mod render_session;
pub mod runtime_module_authorization;
pub mod runtime_module_identity;
pub mod runtime_module_policy;
pub mod secure_image_dispatch;
pub mod secure_launch;
#[cfg(windows)]
pub mod selftest;
pub mod session_dependency_manifest;
#[cfg(windows)]
pub mod smart;
pub mod staging_trust;
#[cfg(any(test, feature = "test-pe-fixtures"))]
pub mod test_pe;
pub mod trace_policy;
pub mod trusted_worker_stage;
pub mod wgpu_dx12_pf_probe;
#[cfg(windows)]
pub mod windows_process;
pub mod worker_dialog;
pub mod worker_module_audit;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitClassification {
    Ok,
    NonzeroExit,
    TimeoutKilled,
    Crashed,
}

impl ExitClassification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NonzeroExit => "nonzero_exit",
            Self::TimeoutKilled => "timeout_killed",
            Self::Crashed => "crashed",
        }
    }
}

pub fn classify_exit(exit_code: u32, timed_out: bool) -> ExitClassification {
    if timed_out {
        ExitClassification::TimeoutKilled
    } else if exit_code == 0 {
        ExitClassification::Ok
    } else if exit_code & 0xC000_0000 == 0xC000_0000 {
        ExitClassification::Crashed
    } else {
        ExitClassification::NonzeroExit
    }
}

pub fn redact_windows_paths(text: &str, limit: usize) -> (String, bool) {
    let truncated = text.len() > limit;
    let mut boundary = limit.min(text.len());
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let bounded = &text[..boundary];
    let chars: Vec<char> = bounded.chars().collect();
    let mut output = String::new();
    let mut index = 0;
    let mut inside_json_string = false;
    // Marker-shaped suffix validation shares one capture-sized inspection
    // budget. Valid compact envelopes consume only their own JSON span, while
    // repeated unterminated objects cannot each rescan the remaining capture
    // (issue #1306).
    let mut structured_marker_budget = chars.len();
    while index < chars.len() {
        let starts_path = index + 2 < chars.len()
            && chars[index].is_ascii_alphabetic()
            && chars[index + 1] == ':'
            && matches!(chars[index + 2], '\\' | '/');
        if starts_path {
            // A path that is the complete value of a compact JSON string
            // needs a schema-safe replacement so redaction cannot corrupt
            // the report or cause an otherwise valid Suite event to vanish.
            output.push_str(
                if inside_json_string && index > 0 && chars[index - 1] == '"' {
                    "redacted-path"
                } else {
                    "<redacted-path>"
                },
            );
            index += 3;
            while index < chars.len() {
                if inside_json_string && chars[index] == '"' {
                    let preceding_backslashes = chars[..index]
                        .iter()
                        .rev()
                        .take_while(|character| **character == '\\')
                        .count();
                    if preceding_backslashes % 2 == 0 {
                        break;
                    }
                } else if !inside_json_string {
                    if chars[index].is_whitespace() || chars[index] == '"' {
                        break;
                    }
                    if structured_marker_budget > 0 && chars[index] == ',' {
                        let marker_shaped = [",diagnostics=", ",report="].iter().any(|marker| {
                            let mut actual = chars[index..].iter();
                            marker
                                .chars()
                                .all(|expected| actual.next() == Some(&expected))
                        });
                        if marker_shaped {
                            let (valid, inspected) = validated_structured_marker(
                                &chars,
                                index,
                                structured_marker_budget,
                            );
                            structured_marker_budget =
                                structured_marker_budget.saturating_sub(inspected);
                            if valid {
                                break;
                            }
                        }
                    }
                }
                index += 1;
            }
        } else {
            output.push(chars[index]);
            if chars[index] == '"' {
                let preceding_backslashes = chars[..index]
                    .iter()
                    .rev()
                    .take_while(|character| **character == '\\')
                    .count();
                if preceding_backslashes % 2 == 0 {
                    inside_json_string = !inside_json_string;
                }
            }
            index += 1;
        }
    }
    let redaction_truncated = output.len() > limit;
    if redaction_truncated {
        let mut boundary = limit;
        while !output.is_char_boundary(boundary) {
            boundary -= 1;
        }
        output.truncate(boundary);
    }
    (output, truncated || redaction_truncated)
}

fn validated_structured_marker(
    chars: &[char],
    index: usize,
    inspection_budget: usize,
) -> (bool, usize) {
    if chars.get(index) != Some(&',') {
        return (false, 0);
    }
    for marker in [",diagnostics=", ",report="] {
        let marker = marker.chars().collect::<Vec<_>>();
        if !chars[index..].starts_with(&marker) {
            continue;
        }
        let json_start = index + marker.len();
        if chars.get(json_start) != Some(&'{') {
            return (false, 0);
        }
        let mut depth = 0usize;
        let mut inside_string = false;
        let mut escaped = false;
        let mut inspected = 0usize;
        for (offset, character) in chars[json_start..].iter().enumerate() {
            if inspected == inspection_budget {
                break;
            }
            inspected += 1;
            if inside_string {
                if escaped {
                    escaped = false;
                } else if *character == '\\' {
                    escaped = true;
                } else if *character == '"' {
                    inside_string = false;
                }
                continue;
            }
            match character {
                '"' => inside_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let json = chars[json_start..=json_start + offset]
                            .iter()
                            .collect::<String>();
                        return (
                            serde_json::from_str::<serde_json::Value>(&json).is_ok(),
                            inspected,
                        );
                    }
                }
                _ => {}
            }
        }
        return (false, inspected);
    }
    (false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_exit_codes() {
        assert_eq!(classify_exit(0, false), ExitClassification::Ok);
        assert_eq!(classify_exit(7, false), ExitClassification::NonzeroExit);
        assert_eq!(
            classify_exit(0xC000_0005, false),
            ExitClassification::Crashed
        );
        assert_eq!(classify_exit(0, true), ExitClassification::TimeoutKilled);
    }

    #[test]
    fn redacts_paths_and_bounds_output() {
        let (redacted, truncated) = redact_windows_paths("failed at D:\\private\\file.bin", 1024);
        assert_eq!(redacted, "failed at <redacted-path>");
        assert!(!truncated);
        let (bounded, truncated) = redact_windows_paths("abcdef", 3);
        assert_eq!(bounded, "abc");
        assert!(truncated);
        let (expanded, truncated) = redact_windows_paths("C:\\x", 4);
        assert!(expanded.len() <= 4);
        assert!(truncated);
    }

    #[test]
    fn redaction_preserves_compact_json_boundaries() {
        let input = r#"{"suite_timeline":[{"name":"C:\\private,secret]}tail","result":25}]}"#;
        let (redacted, truncated) = redact_windows_paths(input, 1024);
        assert!(!truncated);
        let value: serde_json::Value = serde_json::from_str(&redacted).unwrap();
        assert_eq!(value["suite_timeline"][0]["name"], "redacted-path");
        assert_eq!(value["suite_timeline"][0]["result"], 25);
        assert!(!redacted.contains("private"));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("tail"));
    }

    #[test]
    fn redaction_preserves_compact_diagnostic_markers_after_paths() {
        let input = r#"worker failed at C:\private\worker.exe,diagnostics={"classification":"nonzero_exit"},report={"render_error":25}"#;
        let (redacted, truncated) = redact_windows_paths(input, 1024);
        assert!(!truncated);
        assert_eq!(
            redacted,
            r#"worker failed at <redacted-path>,diagnostics={"classification":"nonzero_exit"},report={"render_error":25}"#
        );
    }

    #[test]
    fn redaction_consumes_legal_punctuation_in_unquoted_windows_paths() {
        for path in [
            r"C:\private\customer,secret.txt",
            r"C:\private\customer}secret.txt",
            r"C:\private\customer]secret.txt",
        ] {
            let input = format!("worker failed at {path}");
            let (redacted, truncated) = redact_windows_paths(&input, 1024);
            assert!(!truncated);
            assert_eq!(redacted, "worker failed at <redacted-path>");
            assert!(!redacted.contains("secret"));
        }
    }

    #[test]
    fn marker_like_path_suffix_is_redacted_unless_it_contains_json() {
        let input = r#"worker failed at C:\private\customer,report=secret.txt"#;
        let (redacted, truncated) = redact_windows_paths(input, 1024);
        assert!(!truncated);
        assert_eq!(redacted, "worker failed at <redacted-path>");
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn repeated_marker_shaped_paths_do_not_revalidate_capture_suffixes() {
        let mut input = r#"worker failed at C:\private\first,report={}"#.to_owned();
        input.push_str(&r#" C:\private\later,report={}"#.repeat(4_096));
        let (redacted, truncated) = redact_windows_paths(&input, input.len());
        assert!(!truncated);
        assert_eq!(redacted.matches(",report={}").count(), 4_097);
        assert!(!redacted.contains("private"));
        assert!(!redacted.contains("later"));
    }

    #[test]
    fn unterminated_marker_json_shares_one_capture_inspection_budget() {
        let input = r#"C:\one,report={ C:\two,report={ C:\three,report={"#;
        let (redacted, truncated) = redact_windows_paths(input, input.len());
        assert!(!truncated);
        assert_eq!(redacted, "<redacted-path> <redacted-path> <redacted-path>");
    }
}
