//! Shared environment probe for the integration tests that launch a real
//! isolated worker (issue #335).
//!
//! GitHub's hosted Windows runners execute as a service account whose token,
//! once restricted, cannot initialize even a statically linked Rust binary: the
//! process starts and immediately exits with `STATUS_DLL_INIT_FAILED`
//! (0xC0000142). Every test that drives a live worker therefore fails there for
//! a reason that has nothing to do with the code under test.
//!
//! The workflow used to name individual tests in a `--skip` list. That list did
//! not follow the tests that were added afterwards, which is how `main` ended up
//! red with 32 failures. This probe replaces the list: it asks the environment
//! once, through the same sealed/restricted launch the tests use, and reports
//! whether a restricted-token worker can run at all.
//!
//! It is deliberately narrow about what counts as "the environment cannot do
//! this". Only the documented 0xC0000142 exit suppresses the tests. A staging,
//! ACL, or token failure (`Err`), or any other nonzero exit, returns `true` so
//! the tests run and fail loudly.
//!
//! That narrowness is not enough on its own, because 0xC0000142 is also what a
//! *regression in the restricted token itself* would produce: drop an entry from
//! `COMPATIBILITY_SIDS`, or change the `CreateRestrictedToken` flags, and the
//! worker stops being able to load its system DLLs on every machine, not just a
//! hosted runner. The probe cannot tell those apart -- they are the same signal.
//! So the skip additionally requires an explicit opt-in
//! (`AEXCOMPAT_ALLOW_RESTRICTED_TOKEN_SKIP=1`), which only the CI workflows set.
//! A developer machine never has it, so a token regression there fails 58 tests
//! instead of quietly skipping them.

#![allow(dead_code)]

#[cfg(windows)]
mod windows_probe {
    use aexcompat_broker::sealed_load_tree::{LoadEntry, SealedLoadTree};
    use aexcompat_broker::secure_launch::{SecureLaunchRequest, secure_launch};
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::time::Duration;

    /// `STATUS_DLL_INIT_FAILED`. A process that the restricted token cannot
    /// initialize exits with exactly this.
    const STATUS_DLL_INIT_FAILED: u32 = 0xC000_0142;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn build_probe_worker() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(manifest)
            .args(["-p", "dummy-workers", "--bin", "dummy_exit0"])
            .status()
            .expect("run cargo build for the restricted-token probe worker");
        assert!(status.success(), "restricted-token probe build failed");
        // The build above has no --profile, so the binary lands under debug/,
        // while current_exe() sits in whatever profile the tests were built with.
        // Look in both rather than assuming they agree: under `cargo test
        // --release` the debug/ copy is the one that exists.
        let deps = std::env::current_exe().unwrap();
        let profile_dir = deps.parent().unwrap().parent().unwrap();
        let target_dir = profile_dir.parent().unwrap();
        [
            target_dir.join("debug").join("dummy_exit0.exe"),
            profile_dir.join("dummy_exit0.exe"),
        ]
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| profile_dir.join("dummy_exit0.exe"))
    }

    /// Set by the CI workflows, which run on hosts that provably cannot launch a
    /// restricted-token worker. Absent everywhere else, so the skip cannot hide a
    /// regression in the token construction on a developer machine.
    const ALLOW_SKIP_ENV: &str = "AEXCOMPAT_ALLOW_RESTRICTED_TOKEN_SKIP";

    fn probe() -> bool {
        if std::env::var(ALLOW_SKIP_ENV).ok().as_deref() != Some("1") {
            return true;
        }
        let worker = build_probe_worker();
        let worker_bytes = match std::fs::read(&worker) {
            Ok(bytes) => bytes,
            // Unreadable probe binary is a build problem, not an environment
            // limitation. Let the tests run and report it themselves.
            Err(_) => return true,
        };
        let repository = TempDir(std::env::temp_dir().join(format!(
            "aexcompat-restricted-token-probe-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        )));
        if std::fs::create_dir_all(&repository.0).is_err() {
            return true;
        }
        let payload = repository.0.join("probe.plugin");
        let payload_bytes = b"restricted token launch probe";
        if std::fs::write(&payload, payload_bytes).is_err() {
            return true;
        }
        let entry = LoadEntry {
            source: payload,
            relative_basename: "probe.plugin".into(),
            expected_sha256: Sha256::digest(payload_bytes).into(),
            expected_size: payload_bytes.len() as u64,
        };
        let tree = match SealedLoadTree::create(entry, vec![]) {
            Ok(tree) => tree,
            Err(_) => return true,
        };
        let request = SecureLaunchRequest {
            worker_program: &worker,
            worker_expected_sha256: Sha256::digest(&worker_bytes).into(),
            worker_expected_size: worker_bytes.len() as u64,
            plugin_basename: "probe.plugin",
            args_before_plugin: &[],
            args_after_plugin: &[],
            repository: &repository.0,
            require_module_audit: false,
        };
        match secure_launch(tree, request, Duration::from_secs(60)) {
            Ok(result) if result.exit_code == STATUS_DLL_INIT_FAILED => {
                // println!, not eprintln!: libtest's --show-output is what makes
                // these lines survive into the CI log, and stdout is the stream
                // it is documented to replay for a passing test.
                println!(
                    "restricted-token launch is unavailable in this environment: the probe worker \
                     exited with STATUS_DLL_INIT_FAILED (0x{STATUS_DLL_INIT_FAILED:08X}). Tests \
                     that drive a live worker will report themselves as skipped (issue #335)."
                );
                false
            }
            // Any other outcome is not the documented environment limitation.
            // Run the tests so a real regression surfaces as a failure rather
            // than as a silently green suite.
            _ => true,
        }
    }

    pub fn restricted_token_launch_available() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(probe)
    }
}

#[cfg(windows)]
pub use windows_probe::restricted_token_launch_available;

#[cfg(not(windows))]
pub fn restricted_token_launch_available() -> bool {
    true
}

/// Returns true (after printing the skip line) when this environment cannot
/// launch a restricted-token worker, so a test can `return` on it:
///
/// ```ignore
/// if common::skip_without_restricted_token_launch("my_test") { return; }
/// ```
///
/// Costs nothing where launches work: the probe runs once per test binary and
/// returns `true`, so no test is suppressed on a normal user token.
pub fn skip_without_restricted_token_launch(test: &str) -> bool {
    if restricted_token_launch_available() {
        return false;
    }
    println!("skipping {test}: this environment cannot launch a restricted-token worker (#335)");
    true
}
