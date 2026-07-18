#[cfg(windows)]
#[test]
fn five_isolation_scenarios_pass() {
    use aexcompat_broker::selftest::{run, Workers};
    use std::path::PathBuf;
    let output = std::env::temp_dir().join(format!(
        "aexcompat-broker-selftest-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&output);
    let workers = Workers {
        exit0: PathBuf::from(env!("CARGO_BIN_EXE_dummy_exit0")),
        sleep: PathBuf::from(env!("CARGO_BIN_EXE_dummy_sleep")),
        abort: PathBuf::from(env!("CARGO_BIN_EXE_dummy_abort")),
    };
    assert!(run(&workers, &output).expect("run selftest"));
    let report = std::fs::read_to_string(&output).expect("read report");
    assert!(report.contains("\"scenario_count\": 5"));
    assert!(report.contains("sentinel_not_inherited"));
    assert!(report.contains("\"accepts_aex_path\": false"));
    std::fs::remove_file(output).expect("remove report");
}

#[cfg(windows)]
#[test]
fn timeout_kill_reports_timeout_reason_and_memory_peaks() {
    use aexcompat_broker::windows_process::run_isolated;
    use std::path::Path;
    use std::time::Duration;

    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_sleep")),
        &["30000".into()],
        Duration::from_millis(200),
    )
    .expect("run sleeping worker");
    assert_eq!(result.classification.as_str(), "timeout_killed");
    assert_eq!(result.kill_reason, Some("timeout"));
    assert!(!result.memory_limit_reached);
    let peak = result.peak_process_memory_bytes.expect("peak recorded");
    assert!(peak > 0 && peak < result.process_memory_limit_bytes);
}

#[cfg(windows)]
#[test]
fn memory_cap_death_reports_memory_limit_reason() {
    use aexcompat_broker::windows_process::run_isolated;
    use std::path::Path;
    use std::time::Duration;

    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_oom")),
        &[],
        Duration::from_secs(60),
    )
    .expect("run oom worker");
    assert_eq!(result.classification.as_str(), "nonzero_exit");
    assert_eq!(result.exit_code, 42);
    assert_eq!(result.kill_reason, Some("memory_limit"));
    assert!(result.memory_limit_reached);
    let peak = result.peak_process_memory_bytes.expect("peak recorded");
    assert!(peak <= result.process_memory_limit_bytes);
    assert!(peak >= result.process_memory_limit_bytes - 16 * 1024 * 1024);
}

#[cfg(windows)]
#[test]
fn clean_exit_reports_no_kill_reason() {
    use aexcompat_broker::windows_process::run_isolated;
    use std::path::Path;
    use std::time::Duration;

    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_exit0")),
        &[],
        Duration::from_secs(10),
    )
    .expect("run exiting worker");
    assert_eq!(result.classification.as_str(), "ok");
    assert_eq!(result.kill_reason, None);
    assert!(!result.memory_limit_reached);
}

#[cfg(windows)]
#[test]
fn pipe_holding_descendant_does_not_block_capture() {
    use aexcompat_broker::windows_process::run_isolated;
    use std::path::Path;
    use std::time::{Duration, Instant};

    let started = Instant::now();
    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_descendant")),
        &[],
        Duration::from_secs(2),
    )
    .expect("run descendant worker");
    assert_eq!(result.classification.as_str(), "ok");
    assert!(result.stdout.contains("parent completed"));
    assert!(started.elapsed() < Duration::from_secs(5));
}
