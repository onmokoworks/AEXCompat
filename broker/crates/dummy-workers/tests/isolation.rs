#[cfg(windows)]
#[test]
fn five_isolation_scenarios_pass() {
    use aexcompat_broker::selftest::{Workers, run};
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
        Some(Duration::from_millis(200)),
    )
    .expect("run sleeping worker");
    assert_eq!(result.classification.as_str(), "timeout_killed");
    assert_eq!(result.kill_reason, Some("timeout"));
    assert!(!result.memory_limit_reached);
    assert_eq!(result.process_memory_limit_bytes, 512 * 1024 * 1024);
    let peak = result.peak_process_memory_bytes.expect("peak recorded");
    assert!(peak > 0 && peak < result.process_memory_limit_bytes);
}

#[cfg(windows)]
#[test]
fn no_deadline_waits_for_a_worker_the_watchdog_would_have_killed() {
    // The same worker and the same wall-clock, with and without a deadline
    // (issue #354): discovery passes `None` because a plug-in that is merely
    // slow to map its sealed closure must not be recorded as a timeout.
    use aexcompat_broker::windows_process::run_isolated;
    use std::path::Path;
    use std::time::Duration;

    let worker = Path::new(env!("CARGO_BIN_EXE_dummy_sleep"));
    let killed = run_isolated(worker, &["400".into()], Some(Duration::from_millis(50)))
        .expect("run sleeping worker under a deadline");
    assert_eq!(killed.classification.as_str(), "timeout_killed");
    assert_eq!(killed.kill_reason, Some("timeout"));

    let waited = run_isolated(worker, &["400".into()], None).expect("run sleeping worker");
    assert_eq!(waited.classification.as_str(), "ok");
    assert_eq!(waited.kill_reason, None);
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
        Some(Duration::from_secs(60)),
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
fn descendant_oom_does_not_implicate_the_worker() {
    use aexcompat_broker::windows_process::run_isolated;
    use std::path::Path;
    use std::time::Duration;

    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_oom_descendant")),
        &[env!("CARGO_BIN_EXE_dummy_oom").to_string()],
        Some(Duration::from_secs(60)),
    )
    .expect("run descendant-oom worker");
    assert_eq!(result.classification.as_str(), "nonzero_exit");
    assert_eq!(result.exit_code, 7);
    // The child peaked at the cap; the job aggregate shows that, but the
    // worker's own peak stays low, so no memory-limit kill reason.
    assert_eq!(result.kill_reason, None);
    assert!(!result.memory_limit_reached);
    let job_peak = result.peak_process_memory_bytes.expect("job peak recorded");
    assert!(job_peak >= result.process_memory_limit_bytes - 16 * 1024 * 1024);
    let worker_peak = result
        .worker_peak_commit_bytes
        .expect("worker peak recorded");
    assert!(worker_peak < result.process_memory_limit_bytes / 2);
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
        Some(Duration::from_secs(10)),
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
    // Generous bounds: the parallel OOM tests commit ~1 GiB while this runs,
    // and the point here is "does not block on the orphaned pipe", not speed.
    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_descendant")),
        &[],
        Some(Duration::from_secs(10)),
    )
    .expect("run descendant worker");
    assert_eq!(result.classification.as_str(), "ok");
    assert!(result.stdout.contains("parent completed"));
    assert!(started.elapsed() < Duration::from_secs(20));
}

/// The wiring half of issue #1290: worker stderr has to reach the broker as
/// the *end* of the stream, and the bytes that are dropped have to leave whole
/// lines behind. Every unit test around the capture names its own retention and
/// bound, so only this can catch the two `reader` arguments being swapped back,
/// or the post-read step going missing - which is the bug itself, not a
/// refactor of it.
#[cfg(windows)]
#[test]
fn production_stderr_capture_keeps_the_end_of_an_over_long_trace() {
    use aexcompat_broker::windows_process::{STDERR_CAPTURE_LIMIT, run_isolated};
    use std::path::Path;
    use std::time::Duration;

    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_large_stderr")),
        &[
            (STDERR_CAPTURE_LIMIT * 2).to_string(),
            STDERR_CAPTURE_LIMIT.to_string(),
        ],
        Some(Duration::from_secs(30)),
    )
    .expect("run large-stderr worker");
    assert_eq!(result.classification.as_str(), "ok");
    assert!(result.stderr_truncated, "the stream ran past the bound");
    assert!(result.stderr.len() <= STDERR_CAPTURE_LIMIT);
    assert!(
        result.stderr.ends_with("stage:the_last_thing_it_did\n"),
        "the last thing the worker did is what the capture is for"
    );
    assert!(
        !result.stderr.contains("stage:the_first_thing_it_did"),
        "the head is what a trace can afford to lose"
    );
    // The worker reports on stdout where it put the path it arranged to
    // straddle the capture boundary. Assert that arrangement before the
    // conclusion that rests on it: a boundary that happened to fall on a line
    // break would make the next assertion pass for the wrong reason.
    let arrangement: Vec<usize> = result
        .stdout
        .split_whitespace()
        .map(|value| value.parse().expect("the worker reports three counts"))
        .collect();
    let (total, straddling_at, straddling_len) = (arrangement[0], arrangement[1], arrangement[2]);
    let boundary = total - STDERR_CAPTURE_LIMIT;
    assert!(
        boundary > straddling_at && boundary < straddling_at + straddling_len,
        "the boundary has to fall inside the path line: {boundary} vs {straddling_at}..{}",
        straddling_at + straddling_len
    );
    // So the drive letter is on the dropped side, and `redact_windows_paths`
    // recognizes a path only there. Realigning the capture to a line boundary
    // is the only thing stopping the rest of that path from reaching the report
    // verbatim.
    assert!(
        !result.stderr.contains("private"),
        "an unredacted path fragment survived the capture"
    );
    // Whole paths inside the kept region are still redacted, so the alignment
    // did not simply throw the redaction's work away.
    assert!(result.stderr.contains("<redacted-path>"));
}

#[cfg(windows)]
#[test]
fn production_stdout_capture_preserves_large_bounded_worker_reports() {
    use aexcompat_broker::windows_process::{STDOUT_CAPTURE_LIMIT, run_isolated};
    use std::path::Path;
    use std::time::Duration;

    let report_bytes = 17 * 1024 * 1024;
    assert!(report_bytes < STDOUT_CAPTURE_LIMIT);
    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_large_stdout")),
        &[report_bytes.to_string()],
        Some(Duration::from_secs(30)),
    )
    .expect("run large-report worker");
    assert_eq!(result.classification.as_str(), "ok");
    assert_eq!(result.stdout.len(), report_bytes);
    assert!(!result.stdout_truncated);
}
