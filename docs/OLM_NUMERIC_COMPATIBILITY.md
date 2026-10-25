# OLM numeric compatibility port

Local draft stacked on F54; OLM's default oracle is unchanged.

| Patch | Implementation |
|---|---|
| F29 | SSE first-NaN operand selection, separate x87 policy |
| F30 | UCRT expf, atan2f, powf imports |
| F32 | Legacy DEEP_PIXELS selects PF16 with existing bounds/ownership |
| F44 | UCRT sinf, cosf, log2f imports |
| F45 | UCRT double sin, cos, exp imports |
| F46 | UCRT double pow import |
| F49 | Optional resident Smart pointer watches with caller-selected RVA |
| F50 | Finite instruction budget per scheduler slice; no production wall deadline |

F47/F47b/c/d and F51 are OLM native-plugin bridges/standalone hosts, not guest
AEX emulation patches, and are not copied into the guest worker. F52/F52c/F53
are inherited from F54. C53b and R08 are later experiments outside this scope.

## Math contract

The olm-ucrtmath crate bundles OLM implementations. AMD-derived files retain BSD
notices and upstream provenance. The ms/ files retain their OLM-internal
restriction: **do not publish standalone**. This is a local integration draft,
not publication authorization. Cargo publishing is disabled. No Microsoft
executable or disassembly is included.

OLM_UCRT_FMA=0|1 selects reference mode once per process (default 1). Qualification
concerns raw return bits under round-to-nearest and gradual underflow, not errno,
floating-point flags, alternate rounding, GPU or native Adobe-host equivalence.
C contraction/fast-math is disabled; explicit fma calls remain explicit.
Portable unsigned limb operations replace compiler-specific 128-bit multiply
and CLZ without changing floating-point operations. tests/wide_integer.c checks
512 boundary and 100,000 deterministic random combinations against a compiler
128-bit oracle. MSVC compilation still requires its own platform verification.

## Resident Smart watches

Set AEXCOMPAT_SMART_TRACE to an owned output JSON path and
AEXCOMPAT_SMART_TRACE_WATCH to hex-rva,decimal-bytes,first|all.
For the original F49 diagnostic use 1ec0,<plane-bytes>,all (or first).
Watches capture RCX and RDX at the caller-supplied RVA during SMART_RENDER.
Each watch accepts 1..=1,048,576 bytes; existing trace event bounds still apply.
The file contains the most recent completed SMART_RENDER; use a fresh path and
process per observation. Invalid configuration fails explicitly. Unset output
means no tracing. No OLM function address is embedded in the implementation.

## Verification

Set TMPDIR to owned scratch and CARGO_TARGET_DIR to a space-free owned build
directory; check memory_pressure for at least 30% free before building.

    cargo test --manifest-path guest/Cargo.toml --release -p aex-guest-worker -p olm-ucrtmath -j 1
    cargo build --manifest-path guest/Cargo.toml --release -p aex-guest-worker --bin aex-guest-worker --example f30_import_probe -j 1
    clang -std=c11 -O2 -Iguest/crates/olm-ucrtmath/ucrtmath/include guest/crates/olm-ucrtmath/tests/wide_integer.c -o "$TMPDIR/wide-integer-test"
    "$TMPDIR/wide-integer-test"

OLM tools/emulation/aexcompat_patches/verify_F54b.py replays all saved corpus sets
against F53. Successful raw bytes/rectangles, matching non-success statuses,
byte differences and status differences are distinct categories. Matching
rejections are not successful numerical equivalence. The OLM ticket report
records actual counts and remaining differences.
