# Typed parameter capacity (#1831)

Released OLM Color Key 2.3.3 declares 1,623 controls. The native registry already
accepted 4,096 controls, but the broker rejected slots above 1,024 and parameter
payloads above 16 KiB. Increasing only the encoder limit would still exceed the
Windows command-line limit for dense requests.

## Resource budgets

These are AEXCompat resource budgets, not a claim about Adobe's absolute maximum.
The count matches the existing native registry capacity from #1003 and covers
the observed 1,623-control OLM effect and 1,265-control Universe Text Tile.

| Resource | Bound | Rationale |
| --- | ---: | --- |
| Controls / non-input slots | 4,096 / slots 1..4,096 | Existing native capacity; input slot zero is separate |
| Encoded parameter payload | 1,048,576 bytes | Aggregate budget of 4,096 * 256 bytes |
| Typed request / descriptor document | 6,356,992 bytes | Six-byte JSON escaping allowance per payload byte, plus 64 KiB context |
| Video session control message | 1,114,112 bytes | Payload budget plus 64 KiB framing/context |
| Individual arbitrary text | 4,096 bytes | Existing per-value bound; also consumes aggregate payload budget |

The aggregate payload budget is not a promise that all 4,096 arbitrary values
can simultaneously use their individual maximum. Oversized input is rejected,
never truncated. Duplicate slots remain errors. Non-parameter budgets such as
mask vertices, image sizes, BEE vtable slots, the 128-case independent batch and
its 1 MiB manifest, and the 4 MiB cluster manifest remain separate limits.

Rust constants live in `broker/crates/broker/src/parameter_limits.rs`; native
constants mirror them in `minihost/src/worker_parameter_limits.hpp`. Inspection,
encoding, animation, descriptor/fixture loading, typed request files, per-case
batch files, video messages, layer slots, alpha-coverage slots, cluster payloads,
promotion/conformance tools and their JSON schemas use the updated budgets.

## Transport and defaults

Large launch payloads (over 8,192 ASCII bytes) travel in an anonymous mapping.
The broker fills it, duplicates a read-only inheritable handle, and includes
that handle in the existing explicit inheritance list. The worker receives
`param-map:v1|HANDLE,LENGTH`, validates the length, copies printable ASCII, closes
the mapping handle and parses the ordinary v2-v7 payload. Nested mapping tokens
are rejected. No filesystem path is reopened for this transport. Video sessions
continue using bounded length-prefixed messages. Job/memory/private-desktop
containment is unchanged.

One-shot typed requests send only explicitly assigned slots after validating the
whole edit list. The worker initializes all other values from plugin defaults,
including its existing COPY of arbitrary data (#1824). UI inspection still
returns the full inventory. Tests alternate dense/sparse/dense requests in one
fresh-case batch to detect retained edits and lost defaults.

## Behavioral verification (Windows, 2026-11-13)

The SDK-free `worker_many_parameters_fixture.cpp` has variants declaring 1,025,
1,623, 4,096 and 4,097 controls. Every legal slot contributes to an output pixel
checksum. The harness integration test checks the complete inspected inventory,
single typed output, sparse defaults and repeated batch output; the 4,097-control
fixture fails inspection. Dense documents exceed the former 16 KiB limit. Broker,
harness and native tests also exercise payloads near/above 1 MiB, invalid slots,
large descriptor documents and read-only mapping contents/access rights.

| Check | Result |
| --- | --- |
| Rust workspace libraries | broker 408 passed / 3 ignored; host-core 33; FFI 8 |
| Harness unit tests | 113 passed / 2 ignored |
| Large-parameter CLI integration | 1 passed (all four fixture capacities) |
| Existing independent-case CLI integration | 5 passed |
| Broker resident-session integration | 80 passed |
| Enabled native-route / animation pytest | 38 passed |
| Focused schema, conformance and descriptor pytest | 153 passed / 1 skipped; 15 subtests passed |
| Native Release build | Worker, four new fixtures, existing independent fixtures and two self-test executables built |
| Native executables | Parameter checkout/time and Classic runtime self-tests passed |
| Formatting | Tracked Rust plus new Rust files pass rustfmt; whitespace check passes |

Representative reproduction commands, from the repository root:

```powershell
$env:PYTHONUTF8 = '1'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '4'
& tools/build-native.ps1 -Target aex_worker,worker_many_parameters_1025,worker_many_parameters_1623,worker_many_parameters_4096,worker_many_parameters_4097,worker_independent_case_fixture,worker_independent_smart_fixture,worker_param_checkout_time_selftest,worker_classic_runtime_selftest
& target/minihost-build/worker_param_checkout_time_selftest.exe
& target/minihost-build/worker_classic_runtime_selftest.exe
cargo build --manifest-path broker/Cargo.toml -p dummy-workers --bins --release --locked
cargo test --manifest-path broker/Cargo.toml --workspace --lib --locked
cargo test --manifest-path broker/Cargo.toml -p aexcompat-harness --bin aexcompat-harness --test many_parameters_cli --test independent_cases_cli --locked
cargo test --manifest-path broker/Cargo.toml -p aexcompat-broker --test render_session --locked
uv run python -m pytest -q --run-built-artifact-tests tests/test_worker_selftest_routes.py tests/test_pf_parameter_animation_transport.py
uv run python -m pytest -q tests/test_conformance_bundle_schema.py tests/test_run_conformance_bundle.py tests/test_aex_descriptor_manifest_promotion.py tests/test_aex_parameter_value_gate.py tests/test_render_parameter_gate_contract.py
```

The full Python, full workspace integration and Adobe SDK/AE suites were not
run. AE was not launched. The existing modal-worker integration test ran inside
the required private desktop. Existing ignored/skipped tests are not successes.
New schema tests validate generated documents with the schema validator; they
do not assert implementation-source substrings.

## Original effects

Original binaries and raw images remain external/local and are not committed.
The final Color Key run used the released 2025 OLM Color Key 2.3.3 binary:

| Artifact | SHA-256 |
| --- | --- |
| Color Key original | `9cd1645c8f75f45d9a6ea61bc61b7fd8bcbed198fb1124457de216e9e9b9d5ed` |
| Candidate harness | `d54f0986b01807b234171ca55b836332f4739e726c659fbf89d52ffffb4d2136` |
| Candidate worker | `14b37163556cae366a412e4956574e15f2e8062788f5075d1990e68374bacb42` |
| Baseline harness | `a0e5c6dd512cd9cb9c36d39a9cd27446fcc6fbd9b88bc6665b8af446dfa7cad6` |
| Baseline worker | `a39485007409df7205c9cf95fa01ccde452a9ed9ce4660ce6dbf5e3085621215` |

Inspection returned all 1,623 controls. Smart8, Smart16 and Smart32 CPU were run
at 1/1, 1/2 and 1/4 with a 31x19 input and explicit output request rectangle.
Full-resolution dimensions were 31*scale by 19*scale. Sparse requests contained
204 edits, dense requests 1,617 assignable edits. They enabled palette entry 200,
disabled earlier entries, and assigned slots 1,617/1,618/1,619. Two runs of each
combination produced 18 bit-exact pairs (36 renders). Nine batches containing
18 cases matched their corresponding single outputs exactly; batch cases shared
the same static spatial context. Six additional witnesses changed only high
slot 1,618 at 1/2 and 1/4 across all depths and changed the native output pixels.
Deep comparisons used native 16-bit/float bytes, not PNG previews.

Ten older originals were compared with a fresh build of the starting
`origin/main`, `7418be04e400c5020ea5b088e904db3fdf82b052`:
ColorKeep, DistanceGradation, OLMBlur, OLMColorKey, OLMDirectionalBlur, OLMKiraKira,
OLMRadialBlur, OLMSmoother, OLMSmoother2 and OLMToonDilate. Two default-state
3x7/11x7 inputs each matched exactly (20/20); Smoother used Classic8, the others
Smart8. This small regression sample is not AE parity or exhaustive compatibility.

Final raw records remain under local `.local-limits/originals-final`,
`high-slot-witness-final` and `older-originals-final`. They record executable and
plugin identities, native output hashes, owned process IDs and zero active Job
processes after each of the 92 launches. Earlier failed setup/test attempts are
retained separately rather than relabeled as passes.

## Review handoff

This is a local implementation commit; independent review, push, PR and CI are
pending as requested. No independent reviewed-HEAD boundary is claimed.
PR #1830 (issue #1829) merged while this job ran. The exact changed-file overlap
is `broker/crates/broker/src/image_render/tests.rs` (this job moves an invalid
slot assertion from 1,025 to 4,097). Its worktree and the main worktree were not
modified. This branch still starts at `7418be04e`; the reviewer must first review
the local diff, then merge current main, review any conflict resolution and
reverify before pushing. At handoff, fetched main is
`5763ac29409ac78586d398a9b530dfa10cc5d6ea`.
