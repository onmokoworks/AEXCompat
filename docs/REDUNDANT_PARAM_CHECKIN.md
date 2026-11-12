# Redundant PF_CHECKIN_PARAM compatibility

A successful explicit checkin retires the `(effect_ref, PF_ParamDef address)`
checkout key. Repeating that key returns `PF_Err_NONE`, increments
`redundant_param_checkins`, and neither dereferences the parameter nor consumes
another outstanding checkout. A new checkout of the same key removes its retired
state; multiple live checkouts still require matching checkins. Null, unknown,
foreign-effect and previous-case keys return error 4. The old arbitrary-live-entry
fallback is removed: an unknown address cannot discharge someone else's checkout.

Classic contexts own their ledgers. Hosted SmartFX ledgers reset at parameter
preparation; independent cases clear retired keys at begin/finish. Retired keys
are diagnostics, not resources, so they do not affect fresh-case ownership or
`param_checkouts_balanced`. The redundant counter survives case finish long enough
for the case report and resets before the next case. Existing automatic cleanup
and outstanding-checkout checks remain; implicit cleanup is not an explicit
checkin that authorizes a retired key.

Both native report formats and the broker image-report projection expose the
counter. `param_checkin_calls` counts actual discharges only, preserving its
balance with successful checkout calls.

## Evidence and limits

The public OLM Anti Anti Alias 2025/2026 releases check out slot 1 and return it,
then slot 2 at the same address and return it, then check that address in again.
Retained OLM-for-mac `VALIDATION.md` and trace-0/trace-1 experiments isolate the
last callback: returning error 4 prevents rendering; suppressing it allows the
original to finish. AE tolerance is inferred from these releases working in AE;
no native AE callback measurement was made, and this change is not an AE pixel
parity certification.

The behavioral worker self-test `--self-test-redundant-param-checkin` covers both
Classic and hosted callbacks, separate live addresses, repeated live checkouts,
unchanged parameter bytes, invalid pointers, foreign effects, nested contexts,
fresh-case cleanup, previous-case rejection and a live-checkout leak. The existing
temporal test now checks out directly into its checkin destination, instead of
relying on the arbitrary-live-entry fallback. Broker unit tests cover report
projection and acceptance of redundant counts with strict lifecycle failure.

Local validation results for issue #1829 are recorded in the PR draft. Proprietary
AEX binaries, raw images, process traces and machine-specific paths remain local.

## Local verification (2026-11-13, source base 7418be04e)

| Check | Result |
| --- | --- |
| Native Release worker and affected standalone self-tests | Passed |
| Enabled worker routes and temporal checkout pytest | 34 passed |
| Classic standalone runtime self-test | Passed |
| Broker resident-session integration | 80 passed after Release fixture preparation |
| Broker / host-core / FFI library tests | 405 / 33 / 8 passed; 3 broker tests ignored |
| Harness unit tests | 112 passed, 2 ignored |
| Whole tracked-Rust rustfmt check | Passed |
| Anti Anti Alias 2025/2026 direct Smart8/16/32 CPU | 24/24 successful; 12 repeated pairs bit-exact |
| Anti Anti Alias independent batches | Six batches, three cases each; 18/18 successful; all equal the matching single request |
| Older originals against current-harness baseline | 20/20 successful and pixel-exact (two inputs per effect) |
| RadialBlur Type 1 control, three CPU depths | Successful before and after, unchanged pixels |
| RadialBlur Type 3, three CPU depths | Still fails before and after; see below |

Anti Anti Alias used an 11x7 synthetic RGBA input, defaults and an explicit
slot-1=50 / slot-2=1 request, each repeated twice. Each successful single and
batch case reports `redundant_param_checkins=1`; batch sequence is default,
explicit, default in one resident process. Deep comparisons use native raw
sidecars (float bit patterns), not PNG previews. The old-harness Anti Anti Alias
baseline fails for both releases at all three depths.

The older cohort is ColorKeep, DistanceGradation, OLMBlur, OLMColorKey,
OLMDirectionalBlur, OLMKiraKira, OLMRadialBlur, OLMSmoother, OLMSmoother2 and
OLMToonDilate. Each used default parameters and 3x7/11x7 synthetic RGBA inputs.
OLMSmoother used Classic8; the other nine used Smart8. This is a bounded
regression sample, not an exhaustive parameter or depth campaign.

RadialBlur used the retained 1x1 input/noise-layer reproduction, Strength=1 and
Noise Type=3, with Type=1 as the single-setting control. It still fails on all
three depths. The retained batch stderr has 22 successful checkins and then
`checkin_param -> 4 (not_checked_out)` in both builds: its final callback does
not match a live or explicitly completed `(effect_ref, address)` key, unlike
Anti Anti Alias. The strict rejection remains. The earlier report's independent
uninitialized Noise Layer destination and invalid-handle evidence is retained
context, not a newly measured fault in this job. The batch failure also retains
its pre-existing `frame_done_mismatch`/unclean-close classification; no successful
Radial Type-3 render or final counter report is claimed.

No AE or interactive GUI was launched. Native AEX dispatch used the project's
required process/Job/private-desktop containment. The local runner verified its
owned Job's active-process count reached zero on every return. All requests,
outputs and copied baseline executables were confined to this worktree. JSON
reports were parsed with duplicate-key rejection. No original AEX was copied
or committed.

| Executable | Baseline SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| harness | `68a251fed8131d3a32cdcbd8797aba9959be417d8da68d1e80f44d087c75e9ba` | `9681bc8c574578b4def12e67fc6f55f582597158395dfa9d2368366c56f48ceb` |
| worker | `186f0f00b1e9ff793f6ff5f9f23d6167188af3276d298b2ce6764bb6cf9b94f1` | `fe39b4c38ddadc04befdee1e6975d95156b7e32a79f2937c5d7d6132f280f124` |

The first Release broker integration attempt had 2 passes and 78 failures because
`session_protocol_worker.exe` was absent in the Release directory; its fallback
build produced Debug fixtures. After explicitly building `dummy-workers --bins
--release`, the same integration test executable passed all 80 tests. Both logs
are retained locally. The integration set includes
`modal_ui_worker_uses_a_private_desktop_before_session_timeout`, the required
private-desktop containment check; it did not use the interactive desktop.
Full Python, SDK fixture and AE oracle suites were not run. The enabled focused
Python set, workspace library tests, harness unit tests and broker session
integration are the selected regression coverage; no broader suite pass is
claimed. Independent review and CI remain pending the requested pre-push handoff.
