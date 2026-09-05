# GitLab CI

`.gitlab-ci.yml` runs on GitLab.com's ephemeral Windows hosted runner
(`saas-windows-medium-amd64`). Branch pushes and merge requests trigger it;
when a merge request is open, its pipeline replaces the duplicate push pipeline.

The initial pipeline installs Rust, uv, PowerShell and Ninja, follows
`rust-toolchain.toml`, `.python-version` and `uv.lock`, checks tracked Rust
formatting, builds the native workers in Release, verifies header dependencies,
builds the trace writer self-test, builds/tests the broker Rust workspace, and
runs the existing SDK-independent Python CI partition. Native self-test wrappers
can use the executables built in the same job. Command failures fail the job.
Python JUnit results and its log are retained for seven days, including failures.

This is not yet parity with `.github/workflows/windows-clean-clone.yml`:
SDK downloads, SDK-generated probes/bridges, built-artifact opt-in tests,
classic failure evidence replay, license audit and public probe publishing are
not enabled. No SDK credentials are required or copied into this pipeline.
Guest/macOS builds and real After Effects integration are outside this job.
Runner availability and account compute quota still determine whether it runs.

The entry point is `tools/gitlab/windows-tests.ps1`. It installs tools and is
intended for a disposable Windows CI VM, not an existing development machine.

## Cache and timing policy

Three GitLab caches are used, all under `.ci-cache/`:

- Rustup and Cargo proxy binaries are keyed by `rust-toolchain.toml`.
  Bootstrap installs no default stable toolchain; the repository-pinned version
  is installed with the minimal profile. This avoids installing two toolchains
  and local Rust documentation on every clean VM.
- Cargo downloads, uv's package cache and managed Python are keyed by the lock
  files. The virtual environment is recreated with `uv sync --locked`.
- A branch-specific, 2 GB sccache store caches Rust and minihost C++ compilation,
  with the default branch as a fallback when available. Rust incremental
  compilation is disabled for sccache. VSLANG and the existing dependency check
  remain enabled. Cache entries are saved even after a failed job so a later
  fix can reuse completed compilation.

No CMake build tree or Cargo target tree is restored; compiler/content-based
cache keys avoid relying on checkout timestamps or stale build configuration.
GitLab's protected/unprotected cache separation is retained. Caches are optional:
a miss rebuilds normally. Fast ZIP compression limits cache transfer overhead.
The small instruments trace writer still builds directly.

`gitlab-timings.jsonl` records each command's wall time, including failures, and
`gitlab-sccache.log` records cache statistics. Compare a cold run and a later
run on the same branch before claiming a speedup; include restore/upload time.
Windows VM provisioning remains outside this optimization. No test selection
or automatic pipeline trigger was removed.
