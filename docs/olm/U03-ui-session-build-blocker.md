> Historical first-attempt report. The physical space-free cache build resolved this blocker; see [the implemented UI session](U03-ui-session.md).

# U03 UI-session oracle: blocked before guest compilation

Status: blocked. Base: `8d102ae455ecab5483a243f459c3cc529823b70c`.
Branch: `olm/U03-ui-session`. No UI-session implementation is delivered.

## Observed results

On the assigned macOS host, two sequential `cargo build --release --locked
--manifest-path guest/Cargo.toml -p aex-guest-worker -j 1` attempts exited 101
in the vendored `unicorn-engine-sys` build script. The required OLM scratch
location has a space in an ancestor directory. First, `CARGO_TARGET_DIR`
pointed directly at that scratch tree. Second, it pointed at a space-free
symlink to the same tree. Neither attempt reached the guest Rust compile.

Both logs contain, in order:

```text
ERROR: main directory cannot contain spaces nor colons
error: typedef redefinition with different types ('Int128' (aka 'struct Int128') vs '__int128')
make: *** [all] Error 2
```

`qemu/configure` checks both `source_path` and `$PWD` for whitespace/colons.
The source checkout has no space in its path. The symlink was used in the
second CMake invocation, but that attempt still emitted the same diagnostic.
Inference: the symlink does not hide the physical build path from configure.
The exact subprocess path-resolution mechanism was not traced.

Rust toolchain pin: 1.93.1. Compiler: Apple clang 17.0.0
(clang-1700.6.4.2), arm64-apple-darwin24.6.0. `memory_pressure` showed 55%
free before attempt 1 and 56% before attempt 2. Scratch target size after
both failures: 172 MiB (`du -sh`). Build outputs were subsequently removed.

## Reproduction

Set `OLM_SCRATCH` to the assigned ticket scratch directory (with a space in
an ancestor), and `U03_TARGET_ALIAS` to an unused space-free symlink path.
Run from this AEXCompat worktree:

```sh
mkdir -p "$OLM_SCRATCH/build-tmp"
memory_pressure
TMPDIR="$OLM_SCRATCH/build-tmp" CARGO_TARGET_DIR="$OLM_SCRATCH/target" \
  cargo build --release --locked --manifest-path guest/Cargo.toml \
  -p aex-guest-worker -j 1 > "$OLM_SCRATCH/build.log" 2>&1
ln -s "$OLM_SCRATCH/target" "$U03_TARGET_ALIAS"
memory_pressure
TMPDIR="$OLM_SCRATCH/build-tmp" CARGO_TARGET_DIR="$U03_TARGET_ALIAS" \
  cargo build --release --locked --manifest-path guest/Cargo.toml \
  -p aex-guest-worker -j 1 > "$OLM_SCRATCH/build2.log" 2>&1
```

Expected on the observed host: both builds exit 101 with the diagnostics
above. This is a reproduction of a build failure, not a passing test.
Proceed with a Rust build only when memory free percentage is at least 30%.

## Stop boundary and next work

The ticket requires stopping after two refuted hypotheses without a new
boundary. Ordinary build compatibility and symlink-based path avoidance
were both refuted; no guest execution was obtained. An uncompiled UI draft
was removed. No UI dispatch, arbitrary serialization, Drawbot capture,
UPDATE matrix, behavioral self-test, or original-AEX witness was completed.
No cached worker was modified, installed, or registered.

A follow-up must establish a build recipe compatible with the mandated
scratch location (or an explicitly revised scratch constraint), then resume
U03. Do not diagnose this as a UI regression or promote a hypothetical trace
into oracle evidence. The OLM ticket report contains the draft claim/PR text;
no claim, PR, push, or external comment was posted.
