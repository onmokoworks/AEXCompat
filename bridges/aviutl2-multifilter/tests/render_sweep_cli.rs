use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

const PROCESS_DEADLINE: Duration = Duration::from_secs(15);

fn executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aexcompat-render-sweep"))
}

fn temporary_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "aexcompat-render-sweep-cli-{label}-{}-{nonce:032x}",
        std::process::id()
    ))
}

fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .env_remove("AEXCOMPAT_MULTIFILTER_REPOSITORY")
        .env_remove("AEXCOMPAT_MULTIFILTER_DIR")
        .env_remove("AEXCOMPAT_MULTIFILTER_DEPENDENCY_DIRS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn output_with_deadline(mut command: Command) -> Output {
    let mut child = command.spawn().expect("spawn sweep CLI");
    let started = Instant::now();
    loop {
        if child.try_wait().expect("poll sweep CLI").is_some() {
            return child.wait_with_output().expect("collect sweep CLI output");
        }
        if started.elapsed() >= PROCESS_DEADLINE {
            let _ = child.kill();
            let output = child.wait_with_output().expect("collect timed-out output");
            panic!(
                "sweep CLI exceeded {PROCESS_DEADLINE:?}: stderr={}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn final_error(stderr: &[u8]) -> Value {
    let stderr = String::from_utf8_lossy(stderr);
    let line = stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .expect("structured error line");
    serde_json::from_str(line)
        .unwrap_or_else(|error| panic!("final stderr line is not JSON: {error}; stderr={stderr}"))
}

fn write_packaged_layout(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root.join("target/minihost-build")).expect("create worker layout");
    let packaged = root.join("aexcompat-render-sweep.exe");
    std::fs::copy(executable(), &packaged).expect("copy sweep executable");
    std::fs::write(
        root.join("target/minihost-build/aex_worker.exe"),
        b"fingerprint-only worker fixture",
    )
    .expect("write worker fixture");
    packaged
}

fn write_worker(root: &Path, bytes: &[u8]) {
    let worker = root.join("target/minihost-build/aex_worker.exe");
    std::fs::create_dir_all(worker.parent().unwrap()).unwrap();
    std::fs::write(worker, bytes).unwrap();
}

#[test]
fn help_succeeds_without_worker_resolution() {
    let root = temporary_root("help");
    std::fs::create_dir_all(&root).unwrap();
    let mut command = command(&executable());
    command
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", root.join("missing.toml"))
        .arg("--help");
    let output = output_with_deadline(command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AEXCompat batch render sweep"));
    for option in [
        "--blocked-path",
        "--no-layer",
        "--force-classic",
        "--include-scan-paths",
        "--close-report",
        "--plugin-defaults",
        "--verify-pixel-determinism",
        "--clean-prefix-cluster-salvage",
    ] {
        assert!(stdout.contains(option), "help omitted {option}");
    }
    assert!(output.stderr.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_arguments_return_usage_json_without_side_effects() {
    let root = temporary_root("usage");
    std::fs::create_dir_all(&root).unwrap();
    let dump = root.join("must-not-exist");
    let mut command = command(&executable());
    command
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", root.join("missing.toml"))
        .args(["--dump-frames"])
        .arg(&dump)
        .args(["--render-jobs", "0"]);
    let output = output_with_deadline(command);
    assert_eq!(output.status.code(), Some(64));
    assert!(
        !dump.exists(),
        "pure validation must precede filesystem changes"
    );
    let error = final_error(&output.stderr);
    assert_eq!(error["program"], "aexcompat-render-sweep");
    assert_eq!(error["ok"], false);
    assert_eq!(error["error"]["kind"], "usage");
    assert_eq!(error["error"]["code"], "invalid_render_jobs");
    assert_eq!(error["error"]["option"], "--render-jobs");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn value_option_cannot_consume_the_following_flag() {
    let root = temporary_root("missing-option-value");
    std::fs::create_dir_all(&root).unwrap();
    let accidentally_consumed = root.join("--inventory-only");
    let mut command = command(&executable());
    command
        .current_dir(&root)
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", root.join("missing.toml"))
        .args(["--json", "--inventory-only"]);
    let output = output_with_deadline(command);
    assert_eq!(output.status.code(), Some(64));
    assert!(!accidentally_consumed.exists());
    let error = final_error(&output.stderr);
    assert_eq!(error["error"]["kind"], "usage");
    assert_eq!(error["error"]["code"], "missing_option_value");
    assert_eq!(error["error"]["option"], "--json");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_repository_without_worker_fails_before_scan() {
    let root = temporary_root("missing-worker");
    let scan = root.join("scan");
    let repository = root.join("repository");
    std::fs::create_dir_all(&scan).unwrap();
    std::fs::create_dir_all(&repository).unwrap();
    std::fs::write(scan.join("must-not-be-scanned.aex"), b"not a PE").unwrap();
    let mut command = command(&executable());
    command
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", root.join("missing.toml"))
        .arg("--repository")
        .arg(&repository)
        .arg(&scan);
    let output = output_with_deadline(command);
    assert_eq!(output.status.code(), Some(1));
    let error = final_error(&output.stderr);
    assert_eq!(error["error"]["kind"], "environment");
    assert_eq!(error["error"]["code"], "worker_root_unavailable");
    assert!(String::from_utf8_lossy(&output.stderr).contains("explicit repository"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn packaged_cli_resolves_worker_and_publishes_complete_zero_record_report() {
    let root = temporary_root("package");
    let packaged = write_packaged_layout(&root);
    let scan = root.join("scan");
    std::fs::create_dir_all(&scan).unwrap();
    std::fs::write(scan.join("selected-but-limited.aex"), b"never loaded").unwrap();
    let report = root.join("zero.json");
    let mut command = command(&packaged);
    command
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", root.join("missing.toml"))
        .args(["--clean-prefix-cluster-salvage", "--limit", "0", "--json"])
        .arg(&report)
        .arg(&scan);
    let output = output_with_deadline(command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(value["mode"], "render");
    assert_eq!(value["scan"]["seen"], 1);
    assert_eq!(value["scan"]["after_ignore"], 1);
    assert_eq!(value["scan"]["swept"], 0);
    assert_eq!(value["scan"]["selection"]["limit"], 0);
    assert_eq!(value["plugins"], serde_json::json!([]));
    assert_eq!(value["buckets"], serde_json::json!({}));
    assert_eq!(value["render"]["dependency_lane_count"], 0);
    assert_eq!(value["render"]["render_shard_count"], 0);
    assert_eq!(value["render"]["render_group_count"], 0);
    assert_eq!(value["render"]["effective_render_jobs"], 0);
    assert_eq!(value["render"]["effective_same_closure_render_jobs"], 0);
    assert_eq!(value["render"]["distinct_dependency_closure_count"], 0);
    assert_eq!(
        value["render"]["clean_prefix_cluster_salvage"]["effective"],
        false
    );
    assert_eq!(value["build"]["complete"], true);
    assert_eq!(value["build"]["verification"], "run_boundary_verified");
    assert!(!report.with_extension("partial.jsonl").exists());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        serde_json::json!({})
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_worker_root_is_used_and_environment_override_wins() {
    let root = temporary_root("configured-worker");
    let scan = root.join("scan");
    let configured = root.join("configured");
    let overridden = root.join("overridden");
    std::fs::create_dir_all(&scan).unwrap();
    write_worker(&configured, b"configured-worker");
    write_worker(&overridden, b"environment-worker-is-longer");
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        format!("repository = '{}'\n", configured.display()),
    )
    .unwrap();
    let report = root.join("root-order.json");

    let mut configured_command = command(&executable());
    configured_command
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", &config)
        .args(["--limit", "0", "--json"])
        .arg(&report)
        .arg(&scan);
    let configured_output = output_with_deadline(configured_command);
    assert!(
        configured_output.status.success(),
        "{}",
        String::from_utf8_lossy(&configured_output.stderr)
    );
    let configured_report: Value =
        serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        configured_report["build"]["l2_worker"]["size_bytes"],
        b"configured-worker".len()
    );

    let mut overridden_command = command(&executable());
    overridden_command
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", &config)
        .env("AEXCOMPAT_MULTIFILTER_REPOSITORY", &overridden)
        .args(["--limit", "0", "--json"])
        .arg(&report)
        .arg(&scan);
    let overridden_output = output_with_deadline(overridden_command);
    assert!(
        overridden_output.status.success(),
        "{}",
        String::from_utf8_lossy(&overridden_output.stderr)
    );
    let overridden_report: Value =
        serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        overridden_report["build"]["l2_worker"]["size_bytes"],
        b"environment-worker-is-longer".len()
    );
    assert_eq!(overridden_report["build"]["complete"], true);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn report_publish_failure_is_nonzero_and_removes_temporary_file() {
    let root = temporary_root("publish-failure");
    let packaged = write_packaged_layout(&root);
    let scan = root.join("scan");
    std::fs::create_dir_all(&scan).unwrap();
    let blocked_report = root.join("blocked.json");
    std::fs::create_dir(&blocked_report).unwrap();
    let mut command = command(&packaged);
    command
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", root.join("missing.toml"))
        .args(["--limit", "0", "--json"])
        .arg(&blocked_report)
        .arg(&scan);
    let output = output_with_deadline(command);
    assert_eq!(output.status.code(), Some(1));
    let error = final_error(&output.stderr);
    assert_eq!(error["error"]["kind"], "output");
    assert_eq!(error["error"]["code"], "report_publish_failed");
    assert!(!blocked_report.with_extension("json.tmp").exists());
    assert!(!blocked_report.with_extension("partial.jsonl").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_partial_in_render_mode_fails_before_aex_load() {
    let root = temporary_root("stale-partial");
    let packaged = write_packaged_layout(&root);
    let scan = root.join("scan");
    std::fs::create_dir_all(&scan).unwrap();
    std::fs::write(scan.join("must-not-load.aex"), b"not a PE").unwrap();
    let report = root.join("render.json");
    let partial = report.with_extension("partial.jsonl");
    std::fs::create_dir(&partial).unwrap();
    let mut command = command(&packaged);
    command
        .env("AEXCOMPAT_MULTIFILTER_CONFIG", root.join("missing.toml"))
        .arg("--json")
        .arg(&report)
        .arg(&scan);
    let output = output_with_deadline(command);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("discovery progress:"),
        "inspection callback ran before stale-sidecar rejection: {stderr}"
    );
    assert!(
        !stderr.contains(" inspected in "),
        "inspection summary ran before stale-sidecar rejection: {stderr}"
    );
    let error = final_error(&output.stderr);
    assert_eq!(error["error"]["kind"], "output");
    assert_eq!(error["error"]["code"], "partial_prepare_failed");
    assert!(partial.is_dir(), "the stale evidence must not be replaced");
    assert!(!report.exists());
    std::fs::remove_dir_all(root).unwrap();
}
