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

This is load and parameter-construction evidence only. It does not prove pixel
parity. H-4 remains open until an 8-bpc reference frame is rendered in After
Effects from a fixed input, decoded to a documented channel order, and compared
with the minihost oracle. Parameter value/name capture and non-default cases
should be included in that render trace.
