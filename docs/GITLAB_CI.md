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
