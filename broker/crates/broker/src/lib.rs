#[cfg(windows)]
pub mod selftest;
#[cfg(windows)]
pub mod windows_process;
#[cfg(windows)]
pub mod l1;
#[cfg(windows)]
pub mod l2;

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
            output.push_str("<redacted-path>");
            index += 3;
            while index < chars.len() && !chars[index].is_whitespace() {
                index += 1;
            }
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    (output, truncated)
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
    }
}
