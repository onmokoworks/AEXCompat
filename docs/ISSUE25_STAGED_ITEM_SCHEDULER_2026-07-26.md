# Issue #25 staged item scheduler evidence

## Contract

The staged item runtime treats an item render as a bounded dependency plan made
from immutable stage worlds. A stage identity binds:

- stable item identity and project generation;
- normalized rational source time and time step;
- explicit `exact`, `hold`, or `nearest` sampling policy;
- `upstream`, `all_effects`, `downstream`, or `final_item` stage kind;
- effect instance, quality, guide-layer state, pixel format, dimensions, and
  tight rowbytes.

`exact` never falls through to `hold`, `nearest`, or synthetic production for a
registered item. `hold` selects the latest source not later than the request.
`nearest` compares rational distances exactly and chooses the earlier source on
an exact tie. These are deterministic AEXCompat host policies, not claims about
undocumented After Effects scheduling.

The scheduler resolves child items in dependency post-order, then each declared
effect instance in upstream/all-effects/downstream order, and registers a
receipt only after the complete plan and final item stage resolve. Direct and
indirect cycles, excessive depth, excessive resolved stages, elapsed-time
budget, and cache/receipt byte limits fail closed. No effect callback is invoked
recursively by the resolver.

## Independent native fixtures

`--self-test-aegp-item-staged-worlds` runs the following through the same
production runtime used by the three workers:

1. `nested_exact`: three registered items form `A -> B -> C`. The receipt proves
   dependency depth 2, three resolved final stages, exact rational identity, a
   nonzero trace hash, the stable root identity, and the root's minimal pixel
   oracle. A missing exact frame fails without synthetic fallback.
2. `multiple_policy_boundaries`: independent hold and nearest items feed a root
   item. The nearest item carries two effect instances, each with distinct
   upstream/all-effects/downstream identities. An intentionally omitted
   downstream stage fails before receipt publication; the complete ordered plan
   resolves nine stages. Hold and nearest source times, tie behavior, identity
   noncollision, and trace separation are asserted.

The same self-test also covers ARGB8/16/32F, ROI/downsample/field transforms,
generation invalidation, deterministic eviction, direct/indirect cycles, depth
and stage-count overflow, and failure cleanup. Eight simultaneously released
threads render a 4 MiB root stage; all receipts check in, the observed in-flight
count exceeds one, and live/reserved ownership returns to zero.

Pixel assertions are deliberately minimal: they distinguish selected source,
depth conversion, field/ROI behavior, and effect boundaries. Structural state,
execution order, receipt identity, hashes, and cleanup are the primary oracle.

## Real fixture and After Effects boundary

The machine has `AfterFX.com` for After Effects 2026 version 26.3 and After
Effects 2025 version 25.2. The repository does not currently contain an AEX/JSX
fixture that asks After Effects to exercise this staged item registry with a
two-level nested composition and explicit hold/nearest scheduler policy.
Launching AE with an unrelated probe would not establish this contract.

The existing authenticated fixtures still provide adjacent evidence:

- `REAL_AEX_TIMED_LAYER_INPUT_RESULT_2026-07-17.json` proves an exact supplied
  non-current rational-time selection through the secure worker path.
- `REAL_AEX_SMART_TIMED_MULTI_LAYER_RESULT_2026-07-17.json` records a real
  SDK-built SmartFX AEX checking out current, past, and future exact rational
  times at ARGB8/16/32F with balanced ownership and an independent pixel oracle.

The default focused evidence tests pass, while forcing the machine-bound SmartFX
artifact tests exposes the expected boundary: the historical source identity no
longer matches current `l2_main.cpp`, and the untracked output/report files are
absent in this worktree. Therefore this change does not report a fresh AE pixel
equivalence or authenticate the historical bundle against the new worker. A
future fresh comparison requires a current host-facing nested scheduler probe,
project generator, and recaptured identity-bound outputs.
