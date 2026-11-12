# Independent case review regression record

The independent owner review of `067df6c996c25f05681aea2b4c72b2bdbb9bb986` found five blocking issues. All five findings were accepted; none were rejected. This revision addresses them as follows:

| Finding | Regression / required check | Before fix | After fix |
| --- | --- | --- | --- |
| P1-1 Classic on a SmartFX effect | `classic_request_rejects_smart_effect_before_render_worker` | FAILED: Classic succeeded and published output | PASS: capability rejection before render-worker open |
| P1-2 separate ownership registries | `aegp_memory_leak_invalidates_before_next_case` | FAILED: leaking case returned a normal reply | PASS: invalidates and rejects next case |
| P1-3 hidden SmartFX retries | `independent_smart_error_is_not_retried` | FAILED: original error 14 replaced | PASS: first errors 14/512/516 preserved, one selector call per case, one attempt, no fallback |
| P2-4 cumulative compatibility failures | `compatibility_diagnostics_are_local_to_each_case` | FAILED: good case inherited missing suite | PASS: missing/unsupported diagnostics local; repeated failures still recorded; final close retains cumulative evidence |
| P2-5 Rust formatting | `rustfmt --edition 2024 --check --config skip_children=true` on every tracked `.rs` | exit 1 | exit 0 |

The red runs used the original production implementation, with only regression fixtures/tests added. Logs are retained locally in `target/review-red-{route,native,smart,rustfmt}.log`. Formatting has a mechanical formatter regression check, not a fabricated behavioral test.

Additional behavioral coverage: `pf_path_checkout_and_preparation_leaks_invalidate_next_case` checks both a leaked checkout and a leaked preparation, with a balanced control that continues; `ordinary_smart_session_keeps_premiere_retry` confirms ordinary sessions still call the selector twice and return the retry's error. The SmartFX fixture records each real selector invocation through a numbered suite diagnostic and returns a different error on a second invocation; the test does not merely trust a fixed success field.

Ownership certification now snapshots outstanding identities/reference or lock counts in the separate AEGP memory, PF path/preparation, AEGP/PF world, render-option, render-receipt, GPU memory/world, extended-inter memory, color-profile, Drawbot/progress, mask/stream/value/keyframe-transaction, compute-cache receipt/computation, async-request, audio and legacy-sampling registries. It compares them after sequence setdown and host cleanup, against the pre-instance baseline, while retaining module-owned globals/defaults and compute-cache values. PF state is explicitly reset and auxiliary chunks explicitly reclaimed; host facade/tombstone metadata is retained by its existing bounded host lifetime. Registry implementations expose small snapshots in their owning translation units; this accounts for the wider file list without changing ordinary rendering behavior.

Case-local diagnostic collectors run alongside the cumulative collectors, including repeated names/slots in later cases. SmartFX fresh cases disable both ordinary-session retries and report the first render error, actual attempt count and fallback observation. Broker reports copy fallback fields from the native report.

Validation after fixes:

- Native Release build passed; 215 translation units recorded with header dependencies.
- Focused independent native tests: 8 passed; existing render-session integration: 80 passed.
- Harness crate: 124 passed, 7 existing ignored. Workspace lib/bin: 582 passed, 10 existing ignored (includes broker library tests).
- Native/Python suites: 52 passed plus 15 subtests, with `PYTHONUTF8=1`.
- All tracked Rust files pass the canonical rustfmt check. `git diff --check` passed.
- Full AE/GUI qualification was intentionally not run. No AE launch, GUI, daemon or proprietary binary is included. CI is not claimed for an unpublished branch.

Requalification on the revised native worker and Release harness: all ten external originals, 29 route/depth conditions, six cases per condition, **one repetition** (174 case comparisons). Nine effects use Smart8/16/32; OLMSmoother uses Classic8/16. Every PNG and native deep sidecar was byte-identical to the unchanged single-request route. One resident PID per batch, with process exit verified. Inputs are 97x61 deterministic opaque images, A/B/A/edited-C/B/A. This is AEXCompat evidence, **not native AE evidence**.

The timing scope includes launches, discovery, rendering, I/O, reports and cleanup. Filesystem caches were warm; no CPU affinity or system-wide load isolation was imposed. These are single samples, not stable benchmark medians despite the shared measurement schema's `_median_s` keys. All samples are retained. Speed ratios range from 2.60x to 5.09x; ratio of summed per-condition single-request times to summed batch times is 4.22x.

| Plug-in | Route | Six single requests, ms | Batch, ms | Speed-up |
| --- | --- | ---: | ---: | ---: |
| ColorKeep | smart8 | 2680.5 | 581.4 | 4.61x |
| ColorKeep | smart16 | 2640.8 | 584.9 | 4.52x |
| ColorKeep | smart32 | 2641.0 | 569.3 | 4.64x |
| DistanceGradation | smart8 | 3776.5 | 786.8 | 4.80x |
| DistanceGradation | smart16 | 3639.0 | 842.5 | 4.32x |
| DistanceGradation | smart32 | 3382.3 | 769.0 | 4.40x |
| OLMBlur | smart8 | 2578.3 | 568.4 | 4.54x |
| OLMBlur | smart16 | 2721.9 | 554.9 | 4.91x |
| OLMBlur | smart32 | 2711.6 | 560.7 | 4.84x |
| OLMColorKey | smart8 | 2708.0 | 626.3 | 4.32x |
| OLMColorKey | smart16 | 2741.8 | 584.6 | 4.69x |
| OLMColorKey | smart32 | 2805.4 | 581.6 | 4.82x |
| OLMDirectionalBlur | smart8 | 2843.0 | 634.3 | 4.48x |
| OLMDirectionalBlur | smart16 | 2800.8 | 562.8 | 4.98x |
| OLMDirectionalBlur | smart32 | 2811.3 | 560.1 | 5.02x |
| OLMKiraKira | smart8 | 5510.4 | 1733.3 | 3.18x |
| OLMKiraKira | smart16 | 4940.6 | 1896.6 | 2.60x |
| OLMKiraKira | smart32 | 4819.6 | 1651.8 | 2.92x |
| OLMRadialBlur | smart8 | 2617.6 | 588.8 | 4.45x |
| OLMRadialBlur | smart16 | 2816.0 | 553.7 | 5.09x |
| OLMRadialBlur | smart32 | 2700.3 | 563.5 | 4.79x |
| OLMSmoother | classic8 | 2606.8 | 559.7 | 4.66x |
| OLMSmoother | classic16 | 2734.7 | 568.4 | 4.81x |
| OLMSmoother2 | smart8 | 2710.7 | 595.8 | 4.55x |
| OLMSmoother2 | smart16 | 2626.3 | 587.2 | 4.47x |
| OLMSmoother2 | smart32 | 2676.3 | 550.1 | 4.87x |
| OLMToonDilate | smart8 | 2843.0 | 597.1 | 4.76x |
| OLMToonDilate | smart16 | 2728.5 | 579.7 | 4.71x |
| OLMToonDilate | smart32 | 2723.0 | 594.0 | 4.58x |

Worker SHA-256: `fff751528e0bc4857206118f6411b1de8bd8e8b51ea6776d275b9cf2fe7a739b`. Harness SHA-256: `273674aba26ce152427cd08fda6ba2714f259b11914adfbc15939178f843a23f`.
Current evidence: `docs/evidence/independent-case-batch-review-2026-11-12.json` (all samples, hashes, identities and PID-exit checks). Earlier evidence remains separately named and is not mixed into this revision's timings. Reproduce with `python tools/measure_independent_cases.py --originals <external-directory> --output <new-directory> --repetitions 1`.
