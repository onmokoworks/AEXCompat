# ScatterMap Extended Render Matrix (2026-07-13)

Status: **prepared; not approved or executed**.

Each case requires two disposable worker processes, byte-identical output,
intact guards, broker survival, and equality with the independent Python oracle.

| Case | Dimensions | Parameter change | Purpose |
| --- | --- | --- | --- |
| identity | 16x12 | amount 0 | exact input pass-through |
| horizontal | 16x12 | amount 9, horizontal, seed 17 | horizontal hash path |
| vertical-no-repeat | 16x12 | amount 7, vertical, repeat false | transparent OOB path |
| mixed | 16x12 | amount 12, both, seed 991, mix 37.5 | f32 blend path |
| odd-dimensions | 13x9 | amount 4, seed 3 | non-power-of-two indexing |
| padded-stride | 13x9 | rowbytes 64 | stride handling and padding guards |
| connected-map | 11x7 + 5x3 map | map layer connected | checkout and resampling |
| inverted-map | 11x7 + 11x7 map | map connected, invert true | luma inversion |

The first six cases remain classic CPU/ARGB8. Connected-map cases additionally
require a bounded checkout callback and a separate review because they expose a
second image world to the plug-in. SmartFX, GPU, project access, and 16/32-bpc
remain outside this matrix.
