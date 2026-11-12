# Independent case re-review corrections

The separate review of `067df6c..78d71a5` cleared four initial findings, but
found P1-2 only partially resolved and an audio cleanup regression. The prior
completion claim was too broad. Both findings were accepted.

## Behavioral regressions

Production baseline: `78d71a544709599e8390267c69c3d18277d5cced`.
Before production changes, the expanded SDK-free fixture and Rust tests gave
9 passed / 2 failed (`target/rereview-red.log`):

- `scene_effect_leak_rejects_next_case`: FAILED because an undisposed
  AEGP_GetNewEffectForEffect handle was accepted as clean. After the fix:
  PASS; cleanup invalidates the session and rejects the following case.
- `automatic_audio_cleanup_allows_successful_next_case`: FAILED on frame 0
  with `frame_done_mismatch`, despite Classic automatic audio check-in. After
  the fix: PASS; two audio-checkout cases and a following plain case render,
  automatic check-in counts advance, and final close is clean.
- `balanced_scene_effect_cases_can_reacquire`: PASS before and after;
  disposed effect handles can be reacquired in later independent instances.

The SceneRuntimeState snapshot covers live effect ownership, effect leases,
transform streams/values, legacy effect streams/values, and selection
collections. It excludes retained scene identity records and dead leases.
`worker_aegp_scene_model_selftest` additionally exercises each live record,
every legacy stream slot, generation changes, independently live values,
retained tombstones, and a nonempty global ownership baseline. These are
assertions against actual snapshot transitions, not source-text checks.

Audio now snapshots checked-out handle identities, preserving the separate
invalid-operation check. Automatically released slots are absent from that
snapshot, consistent with Runtime::lifetimes_balanced().

## Validation

- Native Release build passed (216 translation units with header dependencies).
- Broker native integration: 11 independent-case tests and 80 render-session
  tests passed.
- Harness crate: 124 passed, 7 existing ignored.
- Workspace lib/bin: 582 passed, 10 existing ignored.
- Native/Python suites: 52 passed plus 15 subtests (`PYTHONUTF8=1`).
- Direct scene-model and host-audio native self-tests passed.
- Canonical rustfmt check over all tracked Rust files and git diff --check passed.

The prior 29-condition originals measurement belongs to the previous worker
hash recorded in `independent-case-batch-review-2026-11-12.json`. It was not
rerun for this focused revision and is not new-head qualification evidence.
No main integration, AE/GUI launch, push, PR creation or CI run occurred.
The new local commit awaits the owner's separate re-review.
