pub mod conformance;
pub mod fixture_profiles;
pub mod host_core;
#[cfg(windows)]
pub mod image_render;
#[cfg(windows)]
pub mod l1;
#[cfg(windows)]
pub mod l2;
pub mod minidump_policy;
#[cfg(windows)]
pub mod render;
#[cfg(windows)]
pub mod render_request;
#[cfg(windows)]
pub mod render_session;
pub mod restricted_worker_acl;
pub mod restricted_worker_token;
pub mod runtime_module_authorization;
pub mod runtime_module_identity;
pub mod runtime_module_policy;
pub mod sealed_load_tree;
pub mod secure_image_dispatch;
pub mod secure_launch;
#[cfg(windows)]
pub mod selftest;
pub mod session_dependency_manifest;
#[cfg(windows)]
pub mod smart;
pub mod trace_policy;
pub mod trusted_worker_stage;
#[cfg(windows)]
pub mod windows_process;
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
    while index < chars.len() {
        let starts_path = index + 2 < chars.len()
            && chars[index].is_ascii_alphabetic()
            && chars[index + 1] == ':'
            && matches!(chars[index + 2], '\\' | '/');
        if starts_path {
            // A path that is the complete value of a compact JSON string
            // needs a schema-safe replacement so redaction cannot corrupt
            // the report or cause an otherwise valid Suite event to vanish.
            output.push_str(if inside_json_string && index > 0 && chars[index - 1] == '"' {
                "redacted-path"
            } else {
                "<redacted-path>"
            });
            index += 3;
            // Validate at most one marker-shaped suffix per path. This keeps
            // hostile punctuation-heavy diagnostics linear in the capture
            // bound while still preserving the broker's compact envelope.
            let mut structured_marker_checked = false;
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
                    if !structured_marker_checked && chars[index] == ',' {
                        let marker_shaped = [",diagnostics=", ",report="]
                            .iter()
                            .any(|marker| {
                                let mut actual = chars[index..].iter();
                                marker
                                    .chars()
                                    .all(|expected| actual.next() == Some(&expected))
                            });
                        if marker_shaped {
                            structured_marker_checked = true;
                            if validated_structured_marker(&chars, index) {
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

fn validated_structured_marker(chars: &[char], index: usize) -> bool {
    if chars.get(index) != Some(&',') {
        return false;
    }
    for marker in [",diagnostics=", ",report="] {
        let marker = marker.chars().collect::<Vec<_>>();
        if !chars[index..].starts_with(&marker) {
            continue;
        }
        if chars.get(index + marker.len()) != Some(&'{') {
            return false;
        }
        let json = chars[index + marker.len()..].iter().collect::<String>();
        if serde_json::Deserializer::from_str(&json)
            .into_iter::<serde_json::Value>()
            .next()
            .is_some_and(|result| result.is_ok())
        {
            return true;
        }
    }
    false
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
}
