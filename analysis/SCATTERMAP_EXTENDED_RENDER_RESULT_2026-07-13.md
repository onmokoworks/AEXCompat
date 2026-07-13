# ScatterMap Extended Classic Render Result (2026-07-13)

Status: **all eleven extended cases passed in classic and SmartFX**.

The continuous authorization and receipt
`scattermap-extended-render-20260713-001` covered the fixed self-authored
fixture. Every case ran in two disposable workers per rendering path. The three
boundary additions contributed 12 new native executions; all returned error 0,
were deterministic, preserved guards, and matched the independent Python
oracle SHA-256 exactly.

Passed coverage:

- amount-zero identity;
- horizontal displacement with nonzero seed;
- vertical displacement with transparent out-of-bounds behavior;
- both-axis displacement with 37.5% mix;
- maximum amount 500;
- maximum random seed 10000;
- mix 0% with maximum amount and seed, producing exact input identity;
- odd 13x9 dimensions;
- padded 64-byte stride at 13x9;
- connected 5x3 map resampled to 11x7;
- connected 11x7 map with luminance inversion.

Oracle hashes are fixed in
`analysis/SCATTERMAP_EXTENDED_ORACLE_HASHES_2026-07-13.json`. Local create-new
runtime reports remain under `target/render-results/` and contain no private
plug-in path or raw pixels.

The classic broker now enforces oracle equality internally for every fixed
case, matching the SmartFX broker rather than relying only on post-run review.
Remaining scope includes broader randomized cases and an actual After Effects
reference trace.
