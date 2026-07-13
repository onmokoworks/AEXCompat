# After Effects Reference Trace (2026-07-13)

## Scope

This trace is an actual Adobe After Effects host observation, not minihost or
mock evidence. It used the owner-authored `ScatterMap.aex` fixed at 201216 bytes
and SHA-256
`223FF5EC542DD74374C727F16AA6C068073D1C2D7A5CABF20512CB289F0716EB`.

The plug-in was installed only in the current user's Adobe common MediaCore
plug-in directory. The probe created a new unsaved project, added one temporary
composition and solid, and closed without saving. It did not open or modify an
existing project.

## Observed Result

Adobe After Effects `25.2x131` reported:

```json
{"schema_version":1,"app_version":"25.2x131","found":true,"added":true,"match_name":"ScatterMap","display_name":"ScatterMap","property_count":7,"error":""}
```

The append-only stage marker reached `effect added properties=7`. This proves
that the production host discovered the fixed fixture and completed effect
construction on a layer without a script-visible error.

## Reproduction

Run `tools/ae_scattermap_probe.jsx` with `AEXCOMPAT_AE_REPORT` set to a new JSON
path and, optionally, `AEXCOMPAT_AE_MARKER` set to a new text path. On this
Windows host, After Effects drops a script argument containing spaces when it
relaunches itself, so the script was copied byte-for-byte to the temporary
space-free path `D:\AEXCompatProbe\ae_scattermap_probe.jsx` before launch.

## Remaining H-4 Work

The default 8-bpc reference render is now captured in After Effects 25.2. A
16x12 lossless PNG input with SHA-256
`5BB62CF97158128743D79E0D867DECBBA25FE17162F8507067645AAAD9254538`
was rendered through ScatterMap. The output PNG SHA-256 was
`542B9E5D0BCFEC8D9A4D077C72F7A5B5839738521368B04BA3F7C6A8C27ED9C7`.
After decoding PNG RGBA and normalizing to `PF_Pixel8` ARGB, both the AE output
and independent oracle had SHA-256
`19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9`.
The comparison found zero differing bytes, zero differing pixels, and maximum
channel delta zero. The output also differs from the source hash, proving the
comparison did not pass through identity behavior.

The AE property trace exposed these defaults: Scatter Amount 5, Direction 3,
Random Seed 0, Mix with Original 100, Scatter Map 0, and Invert Map 0. It then
reported AE's built-in Compositing Options group. `Repeat Edge Pixels` was not
enumerated through ExtendScript even though the exact default render parity
shows edge-repeat behavior. This visibility discrepancy remains a specific
follow-up item rather than being inferred away.

`tools/ae_scattermap_render_probe.jsx` reproduces the host render and
`tools/ae_scattermap_render_verify.py` performs the channel normalization and
strict comparison. H-4 default-render capture is complete. Non-default AE
parameter cases and the Repeat Edge enumeration discrepancy remain broader
compatibility work.
