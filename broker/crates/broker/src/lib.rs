pub mod conformance;
pub mod fixture_profiles;
pub mod host_core;
#[cfg(windows)]
pub mod image_render;
#[cfg(windows)]
pub mod l1;
#[cfg(windows)]
pub mod l2;
#[cfg(windows)]
pub mod render;
#[cfg(windows)]
pub mod render_request;
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
    while index < chars.len() {
        let starts_path = index + 2 < chars.len()
            && chars[index].is_ascii_alphabetic()
            && chars[index + 1] == ':'
            && matches!(chars[index + 2], '\\' | '/');
        if starts_path {
            // A path that is the complete value of a compact JSON string
            // needs a schema-safe replacement so redaction cannot corrupt
            // the report or cause an otherwise valid Suite event to vanish.
            output.push_str(if index > 0 && chars[index - 1] == '"' {
                "redacted-path"
            } else {
                "<redacted-path>"
            });
            index += 3;
            while index < chars.len()
                && !chars[index].is_whitespace()
                && !matches!(chars[index], '"' | ',' | '}' | ']')
            {
                index += 1;
            }
        } else {
            output.push(chars[index]);
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
        let input = r#"{"suite_timeline":[{"name":"C:\\private","result":25}]}"#;
        let (redacted, truncated) = redact_windows_paths(input, 1024);
        assert!(!truncated);
        let value: serde_json::Value = serde_json::from_str(&redacted).unwrap();
        assert_eq!(value["suite_timeline"][0]["name"], "redacted-path");
        assert_eq!(value["suite_timeline"][0]["result"], 25);
        assert!(!redacted.contains("private"));
    }
}
