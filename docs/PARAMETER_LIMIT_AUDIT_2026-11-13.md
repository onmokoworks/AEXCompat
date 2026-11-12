# Parameter limits and default audit, second review of #1831

Scope: the existing issue1831 worktree, after review of `4d760b6e`. No main
integration, push, AE launch or other worktree modification. This records a
search and code audit, not a proof that every possible effect/value is supported.

## Review fixes and behavioral evidence

| Finding | Tests | Before | After |
| --- | --- | --- | --- |
| Fixed-slider default precision | Native `fixed_slider_omitted_matches_explicit_full_range` | Omitted 32767.5 encoded as 2147418112 instead of 2147450880; omitted INT32_MAX/65536 encoded as 2147418112 instead of 2147483647; self-test exit 75 | Omitted and explicit assignments have identical exact bits at both positive values, -32768 and -32767.5 |
| Corpus capacities and slot bounds | `test_large_corpus_matrix_validation_and_evidence_generation`, `test_large_public_evidence_shape_boundaries`, `test_corpus_matrix_and_gap_slot_overflow_rejected` | Legal 1,025/1,623/4,096-control documents rejected; single slot 4,097 accepted | Matrix loading and generated v2 evidence preserve every legal slot; gaps and v1/v2 evidence validate; excessive count/slot rejected before evidence publication |
| macOS sweep framing | `test_large_guest_setup_response_roundtrip`, `test_guest_sweep_request_boundary_no_partial_write` | Rejected a 4,096-descriptor setup response and legal 65,537/1,114,112-byte requests | Entire setup preserved; request maximum accepted exactly; maximum+1 rejected without writing |
| Additional audit finding: promotion report | `test_large_promotion_generated_report_capacity`, `test_promotion_review_budget_covers_disjoint_inventories` | Generated large reports rejected by the old 64-descriptor schema | 1,025/1,623/4,096 accepted; overflow rejected; 4,096 new + 4,096 missing review entries preserved |

Fixed sliders now clamp in encoded signed 16.16 units before rounding, using the
full INT32_MIN..INT32_MAX range. Point/angle geometry conversion was not widened.
The corpus matrix, gaps and both public evidence schemas now allow 4,096 entries
and an upper index of 4,096. Their historical lower index zero is retained; a
schema-valid evidence shape is not a promise that input-slot zero is an editable
typed assignment (typed assignment slots remain 1..4,096).

The macOS sweep uses 4,096*256 + 64 KiB request bytes and 4,096*256*6 + 64 KiB
response bytes, matching guest resident framing. The promotion report supports
4,096 observed/assignable descriptors. Its review list is bounded at 8,192 because
disjoint supported inventories can generate both one new and one missing entry
per descriptor. No product source-string assertions were added.

## Search coverage

The tracked inventory contained 2,130 files. All 1,971 tracked text files with
source/config/schema/document extensions were passed to `rg` in bounded batches,
including frozen evidence, documentation and vendor text. Binary assets were not
searched as text. The initial candidate scan found 235 lines in 83 files. Follow-up
searches covered smaller caps (16/32/64/128/256/512), numeric spellings, payload and
message constants, and all schema `maxItems`, `maxProperties` and `maximum` fields
under parameter/descriptor/assignment/shape paths. Runtime default functions,
their explicit assignment counterparts, animation conversion, UI reset and
discovery-default normalization were then read separately.

Local evidence: `.local-limits/r2-whole-tracked-audit.txt`,
`r2-whole-tracked-audit-summary.json`, `r2-structured-schema-audit.json`, and the
secondary active-source searches. Generated build trees, local binaries/evidence
and external worktrees were outside this source audit.

## Remaining capacities and distinct budgets (not changed)

| Location | Remaining limit / disposition |
| --- | --- |
| `tools/run_targeted_aex_trace.py:load_manifest` | Diagnostic CLI permits 64 parameter strings, 256 UTF-8 bytes each. This route is **not** aligned to 4,096 typed assignments. |
| `blender_addon/aexcompat_blender/session_wrapper.py`, `__init__.py`, `contracts/blender/aexcompat_blender_session.schema.json` | Blender catalog is limited to 256 entries; multi-override/selection to 16; choice lists to 16. These integrations are **not** qualified for a complete 4,096-control catalog. |
| Broker/native parameter animation | 4,096 slots, but separate limits of 256 keys per parameter, 4,096 total keys, 1 MiB JSON and aggregate arbitrary key bytes, 64 KiB per arbitrary key. These are key/data budgets, not a guarantee of 256 keys for all controls simultaneously. |
| Broker/native discovery and cluster | Discovery response and cluster manifest retain 4 MiB budgets. Typed documents/guest setup responses use a different 6 MiB + 64 KiB budget; this is not universal message-size equality. |
| Independent batches | 128 cases and a 1 MiB manifest; per-case typed documents have their own larger budget. |
| Native audio control protocol | Retains a 64 KiB audio message bound. It is not the video typed-parameter frame protocol. |
| Per-value / metadata resources | Arbitrary text 4 KiB, popup choices 64 in the ordinary encoder, mask vertices, suite/handle telemetry, PiPL, OpenFX image transport, exception arguments and thread counts retain their respective independent bounds. |

The scene-probe schema's 1,024 identities, OpenCL device/resource arrays, suite
versions and diagnostic memory-snapshot sizes are not effect parameter counts.
Historical observations/test fixtures were not rewritten to resemble new runs.
No other stale 1,024-count/16-KiB cap was found in the audited ordinary typed-render,
corpus, promotion and guest-resident/sweep paths; this statement excludes the
explicitly listed trace/Blender and other resource limits above.

## Remaining default differences (code-audit findings, not fixed here)

The earlier statement that no other default gap existed was too broad. The
ordinary-value fixture did not cover all ranges or all guest initialization paths.

| Path | Difference / qualification |
| --- | --- |
| Guest `classic.rs:materialize_default` versus `apply_parameter_value_for_layer`, POINT | Defaults round and saturate into the full i32 range; explicit points clamp pixel coordinates at 32767.0 first. A default producing 32767.5 pixels therefore differs from an explicit same point. At zero dimensions, default points multiply by zero while explicit points retain the supplied percentages. These are source-derived findings; no new guest rendering qualification is claimed. |
| Guest CHECKBOX | Default initialization copies the declared byte; explicit assignment normalizes nonzero to 1. Noncanonical declared bytes such as 2 differ. Ordinary 0/1 declarations are unaffected. |
| Native PATH | An invalid/unresolvable declared mask index initializes to zero; an explicit invalid index is rejected. This does not establish equivalence for invalid defaults. |
| Native numeric range normalization | Omitted scalar defaults are clamped to declared numeric ranges; out-of-range explicit edits are rejected. Equality is claimed only for supported, in-range representable values. |
| Native ANGLE/POINT and animation | Native default and direct edit use the same narrower 32767.0 geometry clamp. Angle animation uses checked full-range conversion, so animation is not claimed equivalent at that upper fractional interval. The fixed-slider fix does not change geometry policy. |
| Arbitrary and layer values | Omission uses default COPY/null or host layer initialization. PRINT text and filesystem layer assignments are different operations, not equivalent serializations of those defaults. |

Guest fixed-slider/angle initialization copies their declared fixed bits, and
guest float initialization widens the declared f32 to f64. Native colour now
materializes its declaration; the prior single/batch pixel regression still
passes. Native fixed-slider boundaries are covered by this review's new test.
There is no repository-wide omitted/explicit equivalence claim.

## Validation in this pass

- Native Release worker, Classic self-test and parameter checkout/time self-test
  built; both self-test executables passed. Header dependency tracking: 147 units.
- Broker workspace libraries: 408 + 33 + 8 passed, 3 ignored. Harness units:
  115 passed, 2 ignored; independent CLI 5 and parameter/default CLI 3 passed.
- Broker resident-session integration: 80 passed, including contained modal test.
- Focused Python suites: 300 passed, 3 skipped, 15 subtests passed. The new schema
  and sweep regressions first produced 15 failures / 2 passes; the two additional
  promotion tests also failed before their schema fix, then passed.
- Tracked Rust rustfmt and diff whitespace checks passed. No Rust product source
  changed in this pass. The full guest workspace's 801-test result belongs to
  the previous pass; it is not a fresh guest run.
- Original recheck: Color Key 2.3.3 completed 36 typed singles (18 exact repeated
  pairs) and 18 batch cases across all three depths and 1/1, 1/2, 1/4. Ten older
  originals matched the previously recorded shared Release snapshot on 20/20
  samples. Local evidence is under `.local-limits/r2-originals` and
  `r2-shared-older`; all 86 final owned launches exited with empty Jobs.
  Candidate harness SHA-256:
  `730947c937edac7abe10f7b1515de0b882e032d3054a2d6142c8eddd658c5e1c`;
  worker SHA-256:
  `d549ebfb29aa2aef4946dddb5c40085b3c2a031a4263e127e70b00cc8d0cb7ea`.

Full Python/SDK/AE suites and macOS end-to-end execution were not run. Fixes still
require independent re-review. Main remains unmerged and nothing was pushed.
