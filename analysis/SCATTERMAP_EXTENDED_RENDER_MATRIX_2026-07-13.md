# ScatterMap Extended Render Matrix (2026-07-13)

Status: **executed through isolated classic and SmartFX brokers**.

Each case requires two disposable worker processes, byte-identical output,
intact guards, broker survival, and equality with the independent Python oracle.

| Case | Dimensions | Parameter change | Purpose |
| --- | --- | --- | --- |
| identity | 16x12 | amount 0 | exact input pass-through |
| horizontal | 16x12 | amount 9, horizontal, seed 17 | horizontal hash path |
| vertical-no-repeat | 16x12 | amount 7, vertical, repeat false | transparent OOB path |
| mixed | 16x12 | amount 12, both, seed 991, mix 37.5 | f32 blend path |
| amount-max | 16x12 | amount 500 | valid maximum displacement |
| seed-max | 16x12 | seed 10000 | valid maximum random seed |
| mix-zero | 16x12 | amount 500, seed 10000, mix 0 | exact original at blend minimum |
| odd-dimensions | 13x9 | amount 4, seed 3 | non-power-of-two indexing |
| padded-stride | 13x9 | rowbytes 64 | stride handling and padding guards |
| connected-map | 11x7 + 5x3 map | map layer connected | checkout and resampling |
| inverted-map | 11x7 + 11x7 map | map connected, invert true | luma inversion |

All cases passed twice per rendering path with deterministic output, intact
guards, broker survival, and exact independent-oracle parity. Connected-map
cases use the bounded checkout callback and a second image world.
