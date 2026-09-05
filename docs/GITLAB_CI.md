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

Two GitLab caches are used, both under `.ci-cache/`:

- Cargo registry package archives are keyed by `broker/Cargo.lock`.
- A branch-specific, 2 GB sccache store caches Rust and minihost C++ compilation,
  with the default branch as a fallback when available. Rust incremental
  compilation is disabled for sccache. VSLANG and the existing dependency check
  remain enabled. Cache entries are saved even after a failed job so a later
  fix can reuse completed compilation.

Rustup installs no default stable toolchain. The repository-pinned version is
explicitly installed with the minimal profile plus rustfmt/clippy. Toolchains
are not archived: job 16324927682 spent about 28 minutes archiving 51,000 Rust
files, while installation took about 98 seconds. Managed Python is not archived
either: GitLab's Windows ZIP archiver failed on its directory link. uv recreates
the environment from the lock file. This leaves only cache stores that saved
successfully and avoids both expensive archives and filesystem-link failures.

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

Observed on job 16325304596: 391 cache hits / 409 hit-or-miss requests
(95.60%; C++ 99.59%, Rust 89.82%). Native build fell from about 627 seconds
to about three minutes; Rust build fell from 647 seconds to 418 seconds.
Job duration fell from roughly 66 minutes to 25 minutes, including removing
the expensive Rust archive. These were failing runs at different test points,
not comparable full-suite success benchmarks. The CLI fixture test remains
under investigation. Rust uses `--no-fail-fast`, and Python runs even after
Rust test failures; any failure still fails the final job.
