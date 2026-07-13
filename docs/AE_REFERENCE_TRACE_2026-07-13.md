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

## Parameter Matrix

`tools/ae_scattermap_matrix_probe.jsx` rendered seven cases in one AE 25.2
session. Every normalized ARGB8 output matched `render_case` with zero differing
bytes and zero differing pixels:

| Case | ARGB8 SHA-256 | Expected behavior |
| --- | --- | --- |
| default | `19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9` | non-identity |
| identity (amount 0) | `863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7` | source identity |
| horizontal | `10A2A95A0AE27CA5FE3A6F6F92EEDDFE611885FA72AFA0902A24E8BEA5D2198F` | non-identity |
| vertical | `6D6198506967E18F619E57CF79E65C52C8F8C65C0EF89710344AF2F1045E091C` | non-identity |
| amount 500 | `8E535435C74A9521D816A3B836DB578A2AE942EFBD80A55447B97610DC26B794` | non-identity |
| seed 10000 | `E31BA13264E801DE7CCCE4D6863215E54C0DC0C7FF4A918E45EE75BC59E817EC` | non-identity |
| mix 0 | `863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7` | source identity |

This closes AE reference parity for every script-visible scalar parameter at
its default plus representative direction and boundary values.

## Layer Map Matrix

`tools/ae_scattermap_map_probe.jsx` connected production-host Layer Control
inputs and rendered two additional 11x7 cases. A 5x3 grayscale map was checked
out at native dimensions and resampled by the fixture for `connected_map`; an
11x7 grayscale map was used with Invert Map enabled for `inverted_map`.

| Case | ARGB8 SHA-256 | Difference from oracle |
| --- | --- | --- |
| connected map (5x3 to 11x7) | `A38568761441C209940F81A8C2792DAD50566C66EDA1463BDCF071CCA614891B` | 0 bytes / 0 pixels |
| inverted map (11x7) | `3BC0C5172B880A8A83CEC24177B78721E9F0619D5330F6A26AAA02B9CC057A08` | 0 bytes / 0 pixels |

Production-host ARGB8 parity is therefore established for all parameters that
AE exposes to ExtendScript, including layer checkout, map resampling, and map
inversion. The sole remaining parameter-observation gap is Repeat Edge: the
source and minihost define it and default-render parity proves its default-true
behavior, but AE does not enumerate a `Repeat Edge Pixels` property. A
production-host no-repeat case cannot be authored through this scripting API
until that registration discrepancy is explained or another supported control
path is established.

## Repeat Edge Root Cause

An AE 25.2-created temporary AEPX reports `parn=8`, confirming the implicit
input plus all seven low-level plug-in parameters, but its parameter template
contains no match name for Repeat Edge (`ScatterMap--1083250007`). The rebuilt
L2 observer records the fixed AEX checkbox definition as `current=0`,
`default=1`, `current_default_mismatch=true`; Invert Map is consistently
`current=0`, `default=0`.

The external Adobe SDK `PF_ADD_CHECKBOX` macro initializes both `value` and
`dephault` from the requested default. The public Rust wrapper revision used by
the self-authored fixture implements `CheckBoxDef::set_default` by assigning
only `dephault`. Repeat Edge requests true and therefore becomes inconsistent;
Invert Map requests false and remains consistent by zero initialization. This
explains why AE retains the low-level parameter count but omits the Repeat Edge
stream. AEXCompat now preserves and reports this malformed descriptor instead
of silently replacing its current value with its default.
