use crate::windows_process::{ProcessResult, run_isolated, run_sentinel_check};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Workers {
    pub exit0: PathBuf,
    pub sleep: PathBuf,
    pub abort: PathBuf,
}

struct Scenario {
    name: &'static str,
    result: ProcessResult,
    passed: bool,
}

fn escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            ch if ch.is_control() => "?".chars().collect(),
            ch => vec![ch],
        })
        .collect()
}

fn scenario(
    name: &'static str,
    result: ProcessResult,
    expected: crate::ExitClassification,
    stdout_contains: Option<&str>,
) -> Scenario {
    let passed = result.classification == expected
        && stdout_contains
            .map(|text| result.stdout.contains(text))
            .unwrap_or(true);
    Scenario {
        name,
        result,
        passed,
    }
}

pub fn run(workers: &Workers, output: &Path) -> io::Result<bool> {
    let scenarios = vec![
        scenario(
            "normal_exit",
            run_isolated(&workers.exit0, &[], Some(Duration::from_secs(2)))?,
            crate::ExitClassification::Ok,
            None,
        ),
        scenario(
            "timeout",
            run_isolated(
                &workers.sleep,
                &["500".into()],
                Some(Duration::from_millis(25)),
            )?,
            crate::ExitClassification::TimeoutKilled,
            None,
        ),
        scenario(
            "crash",
            run_isolated(&workers.abort, &[], Some(Duration::from_secs(2)))?,
            crate::ExitClassification::Crashed,
            None,
        ),
        scenario(
            "hang_kill",
            run_isolated(
                &workers.sleep,
                &["30000".into()],
                Some(Duration::from_millis(40)),
            )?,
            crate::ExitClassification::TimeoutKilled,
            None,
        ),
        scenario(
            "sentinel_not_inherited",
            run_sentinel_check(&workers.exit0, Some(Duration::from_secs(2)))?,
            crate::ExitClassification::Ok,
            Some("sentinel_inherited=false"),
        ),
    ];
    let passed = scenarios.iter().all(|item| item.passed);
    let mut rows = String::new();
    for (index, item) in scenarios.iter().enumerate() {
        if index > 0 {
            rows.push(',');
        }
        rows.push_str(&format!("{{\"name\":\"{}\",\"result\":\"{}\",\"exit_code\":{},\"passed\":{},\"stdout\":\"{}\",\"stderr\":\"{}\",\"stdout_truncated\":{},\"stderr_truncated\":{}}}",
            item.name, item.result.classification.as_str(), item.result.exit_code, item.passed, escape(&item.result.stdout), escape(&item.result.stderr), item.result.stdout_truncated, item.result.stderr_truncated));
    }
    let report = format!(
        "{{\n  \"schema_version\": 1,\n  \"report_kind\": \"broker_selftest\",\n  \"selftest_state\": \"{}\",\n  \"scenario_count\": 5,\n  \"scenarios\": [{}],\n  \"accepts_aex_path\": false,\n  \"dll_load_performed\": false,\n  \"native_load_enabled\": false\n}}\n",
        if passed { "passed" } else { "failed" },
        rows
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(report.as_bytes())?;
    Ok(passed)
}
