# ScatterMap Extended Classic Render Result (2026-07-13)

Status: **all eight extended cases passed**.

The continuous authorization and receipt
`scattermap-extended-render-20260713-001` covered the fixed self-authored
fixture. Every case ran in two disposable broker workers. All 16 executions
returned error 0, were deterministic, preserved outer guards and row padding,
and matched the independent Python oracle SHA-256 exactly.

Passed coverage:

- amount-zero identity;
- horizontal displacement with nonzero seed;
- vertical displacement with transparent out-of-bounds behavior;
- both-axis displacement with 37.5% mix;
- odd 13x9 dimensions;
- padded 64-byte stride at 13x9;
- connected 5x3 map resampled to 11x7;
- connected 11x7 map with luminance inversion.

Oracle hashes are fixed in
`analysis/SCATTERMAP_EXTENDED_ORACLE_HASHES_2026-07-13.json`. Local create-new
runtime reports remain under `target/render-results/` and contain no private
plug-in path or raw pixels.

Remaining compatibility scope includes broader randomized classic cases,
SmartFX selectors, GPU capability behavior, 16/32-bpc worlds, and an actual
After Effects reference trace.

