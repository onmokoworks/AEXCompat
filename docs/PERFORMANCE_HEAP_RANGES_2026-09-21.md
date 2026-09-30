# Dense CRT heap search

Issue #21; baseline merged #20 at
`b0fe6c94b0af92805abca123dbc06ae0636c69e7`. Actual baseline worker SHA-256:
`5ff4f7089113e4e5ffe07dc3655d67fe401f36bea4e4ef90a167b84277a835ae`.

Valid post-#19 profiles identified `first_fit_aligned` as a significant host
cost. #20 changes only bounded string scanning. Fresh macOS sampling currently
hangs even on an owned sleep process; do not change OS security settings or
present that failed sampling attempt as evidence.

Candidate: maintain a coalesced occupied-run index alongside the authoritative
allocation ownership BTreeMap. Adjacent, non-overlapping allocations can then be
skipped as one run. Overlap or overflow detected during index initialization or
subsequent insertions permanently disables that index, reverting to existing search.
This avoids incorrectly treating an overlapped freed interval as empty, without
adding coverage-count maintenance to the common path. Ownership, limits,
allocation kinds, cursor semantics and error behavior remain unchanged.

Acceptance: existing heap tests plus differential address/error/cursor checks,
dense holes, overlap fallback, realloc transitions and overflow edge cases;
bounded synthetic search measurements followed by actual jobs=1 resident ABBA
against #20, Release workspace tests, 292-plugin sweep and independent review.
Reject if actual rendering does not benefit. Synthetic-only gains are not
acceptance. Initial actual-render results are mixed (see below); not accepted.

Initial standalone optimized heap tests: 21 passed, with one manual measurement
ignored by the normal run. The manual measurement performs 2,000 searches with
low-address insert/remove churn, including index maintenance. For 256 / 4,096 /
16,384 densely adjacent allocations, legacy ABBA endpoints took respectively
1,176–1,199 / 17,615–17,678 / 68,235–69,319 microseconds total; indexed runs took
176 / 186 / 191 microseconds. This demonstrates the intended dense-search
scaling only, not whole-frame or fragmented-heap performance.

Initial independent code review found no blocking correctness issue. Actual
render comparison and full integration validation remain required before adoption.

Initial Release build completed in 173.51 seconds, with sampled peak group RSS
833,200 KiB, minimum free disk 37,280,579,584 bytes and no remaining group members.
Candidate worker SHA-256:
`5a786b6a16fd1d2124b183440333261dc4849976fb7a04eb5f1422aee0b98ae8`.

Resident comparison: jobs=1, 256x144 ARGB8, 1,000 frames per session, first 10
excluded from timing, ABBA twice per plugin (24 sessions / 24,000 frames).
Median of the four session medians, in microseconds:

| Effect | Before | Candidate | Change |
| --- | ---: | ---: | ---: |
| PrismLens | 5300 | 4917 | -7.23% |
| Blur | 4172.75 | 4288.5 | +2.77% |
| ColorKeep | 2744.25 | 2770.25 | +0.95% |

All frames matched SHA, strict metadata/guard validation passed, bounded diagnostic
excerpts matched, all workers closed cleanly and all 24 process groups were absent.
Evidence: private `resident-abba.jsonl`, empty runner stderr, and
`summarize_resident.py`. Blur candidate session medians were all above the baseline
range, so its slowdown is not dismissed as noise.

Confirmation used the same binary and settings with 2,000 frames per session
(48,000 total): PrismLens 5273 -> 4908 us (-6.92%), Blur 4171.75 -> 4278.25 us
(+2.55%), ColorKeep 2745.25 -> 2756.75 us (+0.42%). All output/metadata/guard/
diagnostic/cleanup checks passed and all process groups were absent. Evidence:
`resident-confirm.jsonl` and empty runner stderr. The Blur slowdown reproduced,
so this first candidate is not accepted.

Revision 2 retains the left tree entry when extending, shortening or splitting
an occupied run, rather than removing/reinserting a key which did not change.
This targets index maintenance overhead generically, without effect-name logic
or changing the search algorithm. Standalone optimized tests again passed 21/21
(one manual measurement ignored); independent full-diff review found no blocking
correctness issue. Its Release build succeeded (66.79 seconds, empty process
group). Worker SHA-256:
`257c2b3d99d026e09f8eac61b3914ed5268ee7ae9066ea2dda11cd17a66ef431`.
The same 1,000-frame/session ABBA comparison passed all 24,000 frame checks and
all process groups were absent: PrismLens 5325.75 -> 4881.75 us (-8.34%), Blur
4162.25 -> 4203.5 us (+0.99%), ColorKeep 2751 -> 2760.5 us (+0.35%). Blur ranges
overlapped, but this does not establish absence of regression. Evidence:
`resident-v2-abba.jsonl`, empty runner stderr. Not accepted yet.

Revision 3 additionally updates the predecessor through its mutable range lookup,
avoiding a second lookup by key. All 21 standalone heap tests passed again;
independent full-diff re-review found no blocking correctness issue. Release build
succeeded in 67.79 seconds with an empty process group. Worker SHA-256:
`f065a5a355b9eed59ca77467a789e3f9136c27544572f0b26282140a15cda9be`.

Revision 3 used 2,000 frames/session, jobs=1, ABBA twice (48,000 total):

| Effect | Before (us) | Revision 3 (us) | Change |
| --- | ---: | ---: | ---: |
| PrismLens | 5249.75 | 4815.25 | -8.28% |
| Blur | 4163.25 | 4131 | -0.77% |
| ColorKeep | 2743 | 2732.5 | -0.38% |

PrismLens session medians were 5128.5–5372 before and 4722.5–4851 after. Blur
was 4162.5–4200 before and 4099–4153.5 after; the earlier Blur regression did
not reproduce in this revision. ColorKeep ranges overlap, so no meaningful
ColorKeep gain is claimed. All frames passed SHA/metadata/guard checks, bounded
diagnostics matched, all closes were clean and all 24 process groups were absent.
The worker SHA remained unchanged. Evidence: `resident-v3-abba.jsonl` and empty
runner stderr. These results justify proceeding to integration validation, not
a claim of the complete one-third goal or all-effect speedup.

Release workspace tests passed: 740 passed, zero failed, six explicitly ignored
(five existing manual/platform cases plus the new manual dense-search measurement).
The sampled guard reported 275.63 seconds, peak group RSS 1,082,080 KiB,
minimum free disk 36,574,482,432 bytes and an empty process group. Evidence:
`workspace-tests.log`. The added code was then rustfmt-formatted; the final-source
workspace rerun also passed all 740 tests, zero failures, six ignored in 131.56
seconds, with an empty process group (`workspace-formatted-tests.log`). The normal
worker rebuild succeeded in 66.85 seconds with an empty process group. Final
worker SHA-256 is
`9887dc0cf6825d66a049a6304532c8af56497630d0c7b9c93738ed1dce5aa6f3`.
The 292-plugin jobs=1 sweep passed on this final binary. Strict comparison against
the saved #20 sweep found zero mismatches across input/manifests, plugin identity,
status/exit code, encoded PNG files, raw pixel SHA/extent/format, guards and cleanup.
The guard reported 514.49 seconds, minimum free disk 36,091,179,008 bytes, peak
group RSS 977,424 KiB and an empty process group. Per-plugin elapsed-time sum was
501.6685 seconds before versus 500.679 after; median 1.62305 versus 1.61725 seconds.
These non-interleaved cold-sweep times are essentially unchanged and are not
claimed as a causal speedup. Evidence: private `issue21-heap-j1-20260921/report.json`,
`sweep.log`, comparison via the strict `compare_sweep.py` against
`issue20-string-j1-20260921/report.json`.

Since its identity differs after formatting, the final binary was also measured
in jobs=1 ABBA twice, 2,000 frames/session, 48,000 total. PrismLens was 5297 ->
4812.5 us (-9.15%); Blur 4154.25 -> 4158.75 us (+0.11%); ColorKeep 2754.5 ->
2798.5 us (+1.60%). All frame SHA/metadata/guard/diagnostic comparisons passed,
all closes were clean, all 24 process groups were absent, and the binary SHA
remained unchanged. Evidence: `resident-final-abba.jsonl` and empty runner stderr.
Blur is effectively unchanged, rather than claimed faster. ColorKeep's positive
delta was not dismissed merely because ranges overlap: a focused 4,000-frame
session ABBA confirmation (32,000 total) reproduced it, 2754.5 -> 2788.5 us
(+1.23%), with all frame/diagnostic/cleanup checks passing and all eight process
groups absent. Evidence: `resident-final-color.jsonl`. This always-maintained
index is therefore not accepted despite the PrismLens benefit. Earlier revision 3 timing
numbers above refer only to the explicitly recorded earlier binary. Independent
review checked the formatted code, measurement records and initial workspace log
and found no blocking issue.

Next candidate: adaptively build the occupied-run index only after a real legacy
search visits 128 allocation entries. Heaps with short searches do not maintain
the auxiliary tree. The triggering search finishes on the original path, preserving
its result/error order; subsequent searches can use the index. Initialization
audits current allocations for overlap/overflow and permanently falls back when
needed. This is a generic workload threshold, not effect-name logic. New behavioral
cases cover activation boundaries, post-activation mutation, preexisting overlap
and activation during exhaustion with cursor preservation. Performance and full
integration evidence above belongs to earlier binaries, not this new candidate.

Adaptive candidate standalone tests: 23 passed, one manual measurement ignored.
Independent correctness review found no blocking issue; wording about activation
and overlap detection was clarified. Release worker SHA-256:
`d1539d54b4c5c6099d6c05e5f83817a2abdf55885e0c68549ca939eccc4039e2`.
Jobs=1 ABBA twice, 2,000 frames/session (48,000 total): PrismLens 5255 -> 4869.25
us (-7.34%), Blur 4236.5 -> 4218.75 us (-0.42%), ColorKeep 2748 -> 2775 us
(+0.98%). All frame/diagnostic/cleanup checks passed and all process groups were
absent. Evidence: `resident-adaptive-abba.jsonl`, empty runner stderr. ColorKeep
still needs investigation; this candidate is not accepted yet. A temporary,
explicitly diagnostic-only build will report whether its index actually activated;
diagnostic-build timings must not be used as performance evidence, and its probe
must be removed before production acceptance.

The temporary probe completed 24 sessions / 4,800 frames, all checks passed and
all process groups were absent. Same diagnostic binary in both labels; timings
are not performance evidence. At teardown ColorKeep's index was active and not
disabled (10,899 live allocations / 92 runs); Blur 17,371 / 915 and PrismLens
35,906 / 2,704 were also active. Thus activation gating did not avoid ColorKeep's
index maintenance in this workload. Probe binary SHA-256:
`ad6da694b73893944c01f0f42d170e3946abcb0828bf741ea19077614bfb4970`;
evidence `heap-activation-probe.jsonl`. The temporary Drop/environment/stderr probe
has been removed from source.

Next within-heap optimization uses BTreeMap entry APIs for insert and remove_kind,
eliminating duplicate ownership-tree lookups while retaining duplicate-pointer,
foreign/free-kind checks before any mutation. Added a behavioral duplicate/wrong-kind
failure test preserving original allocation, live bytes and search cursor with
both inactive and active indexes. This new candidate requires fresh measurements.

The full additional-one-third goal starts after #3, merge
`4ed8c0c6847df101977d1d3f623deae7357fa6cd`. Historical CPU-class differences mean
per-issue percentages cannot be multiplied as proof; a controlled comparison
against that source remains required. CI is waived by owner, not claimed green.

## Continuation on 2026-09-23

The entry-API candidate built in Release and passed 24 focused heap tests (one
manual timing case ignored). Its jobs=1, 2,000-frame/session ABBA twice passed
48,000 frame, pixel, metadata, diagnostic and cleanup checks: PrismLens 5380 ->
4984.5 us (-7.35%), Blur 4344.75 -> 4228.75 us (-2.67%), ColorKeep 2780 ->
2803 us (+0.83%). The ColorKeep cost remained a concern.

Temporary diagnostic-only workers established that after 100 frames the index
received 547,064 searches in PrismLens, 54,834 in Blur, but only 222 in
ColorKeep. With index activation suppressed, the original tree saw respectively
52,803, 3,810 and 4 searches visiting at least 128 allocations. ColorKeep still
had only four such searches after 1,000 frames. All diagnostic-only probe code
was removed before performance measurement of later candidates.

The index now activates on the eighth long search on the same heap, rather than
the first. Focused Release tests passed 27/27 (one manual timing case ignored).
The first jobs=1, 2,000-frame/session ABBA twice passed all 48,000 frame and
cleanup checks: PrismLens 5365.75 -> 4952.25 us (-7.71%), Blur 4231.75 ->
4108.75 us (-2.91%), ColorKeep 2773.5 -> 2786.5 us (+0.47%). A separate
4,000-frame/session ColorKeep ABBA twice reproduced a +1.17% delta (2773.5 ->
2806 us), so that binary was not accepted.

Moving the occupied-run search out of the normal instruction path while keeping
the inactive check inline reduced the ColorKeep delta in a 4,000-frame/session
ABBA twice to +0.34% (2768.5 -> 2778 us), with overlapping session ranges.
Moving occupied-run insert/remove out of line gave the same +0.34% in another
32,000-frame ColorKeep run (2776.5 -> 2786 us). Removing the ownership BTreeMap
entry-API change instead worsened ColorKeep to +0.80% (2766 -> 2788.25 us).
These trials each passed pixel, metadata, diagnostic and process-group cleanup
checks. Their binary identities and timings must not be mixed with the final
candidate. The final combination keeps the entry-API change and out-of-line
occupied-run methods. Its worker SHA-256 is
`b4f2320b91576d9a75b59e8b839848dcb40621857486c3b70991f568c14d6fa2`.

Final-source Release workspace tests passed: 743 passed, zero failed, six
explicitly ignored. The final worker passed jobs=1 ABBA twice at 2,000
frames/session (48,000 frames total), with matching frame SHA, metadata,
bounded diagnostics and clean process-group termination:

| Effect | #20 baseline (us) | Final #21 (us) | Change |
| --- | ---: | ---: | ---: |
| PrismLens | 5372.25 | 4925.5 | -8.32% |
| Blur | 4293.5 | 4172.25 | -2.82% |
| ColorKeep | 2775.5 | 2776.25 | +0.03% |

The separate 4,000-frame/session ColorKeep ABBA on the same SHA measured
2776.5 -> 2786 us (+0.34%). Its session ranges overlap; no ColorKeep speedup
is claimed, and a small regression cannot be excluded. The final 48,000-frame
run had zero frame or cleanup mismatches and empty runner stderr. Evidence:
`resident-final-eight-abba.jsonl` and `resident-outlined-all-color.jsonl`.

The same final binary completed the full 292-plugin jobs=1 Sapphire sweep.
Strict comparison with the saved #20 report found zero mismatches in plugin
identity, status, exit code, encoded PNG, raw pixel SHA/format/extent, guards,
and cleanup. The summed one-shot elapsed times were 501.6685 seconds before
and 500.5782 seconds after, with medians 1.62305 and 1.61873 seconds. These
non-interleaved cold times do not establish a sweep speedup. Evidence:
`issue21-heap-final-j1-20260923/report.json` and `sweep-final-eight.log`.
Independent full-diff review found no blocking correctness or evidence issue.
The controlled comparison against the original post-#3 source for the overall
additional-one-third goal remains pending; this issue claims only the measured
PrismLens and Blur resident gains.
