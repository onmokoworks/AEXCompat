# ScatterMap AE Parameter Bounds Result

`tools/ae_scattermap_param_bounds_probe.jsx` created a new unsaved AE 25.2
project, applied the fixed ScatterMap fixture, and attempted ten values outside
the parameter definitions exposed by the production host. No render or input
file was required.

| Property | Attempts | Valid range | AE behavior |
| --- | --- | --- | --- |
| Scatter Amount | -1, 501 | 0 to 500 | both rejected; value remained 5 |
| Direction | 0, 4 | 1 to 3 | both rejected; value remained 3 |
| Random Seed | -1, 10001 | 0 to 10000 | both rejected; value remained 0 |
| Mix with Original | -0.1, 100.1 | 0 to 100 | both rejected; value remained 100 |
| Invert Map | -1, 2 | 0 to 1 | both rejected; value remained 0 |

AE raised an `out of range` exception for every attempt and reported the exact
valid range. It did not clamp, wrap, coerce, or commit any attempted value.
`tools/ae_scattermap_param_bounds_verify.py` independently checked the ordered
matrix and reported `attempt_count: 10`, `rejected_count: 10`,
`values_unchanged: true`, and `passed: true`.

The probe used only a new project and closed it with
`CloseOptions.DO_NOT_SAVE_CHANGES`. Its report was create-new, and the target
AEX remained the fixed owner-authored fixture. Repeat Edge was not invented as
a production property because AE does not enumerate that malformed descriptor.

This establishes a host-side validation boundary: AEXCompat must reject these
out-of-range values before dispatch rather than passing them to ScatterMap or
silently clamping them.
