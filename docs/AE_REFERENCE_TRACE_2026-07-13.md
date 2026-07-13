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

## Project Roundtrip

`tools/ae_scattermap_roundtrip_probe.jsx` created a new 16x12 project, applied
ScatterMap, set Random Seed to 10000, saved a new AEPX, closed the project,
reopened that AEPX, located the effect by match name, and observed Seed 10000.
It then rendered the reopened composition without modifying any existing user
project. AE 25.2 reported every stage successful with no script-visible error.

The reopened render normalized to ARGB8 SHA-256
`E31BA13264E801DE7CCCE4D6863215E54C0DC0C7FF4A918E45EE75BC59E817EC`,
exactly matching the seed-max oracle with zero differing bytes and pixels. This
proves non-default parameter persistence, effect reconstruction, and
production-host render equivalence after project serialization/resetup.

## Deep And Float Observation

`tools/ae_scattermap_depth_probe.jsx` rendered the same fixed input twice at
8, 16, and 32 project bits per channel. `saveFrameToPng` emitted RGBA8 for the
8-bpc project and RGBA16 PNGs for both 16- and 32-bpc projects. The custom
lossless decoder in `tools/ae_png_depth_inspect.py` compared unfiltered samples
without Pillow's 8-bit conversion:

| AE project depth | PNG depth | Stable samples | Different samples | Result |
| --- | --- | --- | --- | --- |
| 8 bpc | 8 | 768 | 0 | deterministic and ARGB8 oracle-exact |
| 16 bpc | 16 | 429 | 339 | nondeterministic unwritten tail |
| 32 bpc | 16 | 766 | 2 | nondeterministic unwritten content |

The two 16-bpc decoded hashes were
`B367114389C4553E90262C59C3A3DA0293D9A272874A416824B570D1C12AB8AB`
and `90557F84830B20463E432E3D775EC3D2BDB9A154B74FA1AEBBCEE4AFC5811A74`.
The two 32-bpc-derived hashes were
`8E1451ECBC236683746890A1A29DCB2B6B5B952BCBEE1F4DC71A4EE128F4C45C`
and `97C4AAD492FAD3DB14CB87F420F0526C0E2542EDB31F80C50235120BBF0B6CE9`.

This confirms that exact full-frame deep/float hashes are not a valid AE oracle
for this malformed fixture. AEXCompat reports the deterministic `width*4`
bytes written per row and the remaining undefined tail separately. Its `0xCC`
tail is a containment sentinel, not an invented claim about AE pixel values.

## Time Independence

`tools/ae_scattermap_time_probe.jsx` rendered a three-frame 24-fps composition
at time 0 and time 1/24 second. Both AE 25.2 PNG files were byte-identical with
SHA-256
`542B9E5D0BCFEC8D9A4D077C72F7A5B5839738521368B04BA3F7C6A8C27ED9C7`.
After RGBA-to-ARGB normalization, each frame matched the default oracle hash
`19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9`
with zero differing bytes and pixels. This confirms in production AE that the
fixture does not introduce frame-time variation when parameters are constant.

## Downsample Contract

`tools/ae_scattermap_downsample_probe.jsx` set a 16x12 composition resolution
factor to `[2,2]`. AE 25.2 produced 8x6 frames for both Amount 0 and Amount 5.
The Amount 0 frame was used as the exact host-downsampled input rather than
assuming a resampling kernel. `tools/ae_scattermap_downsample_verify.py` then
applied the fixture algorithm directly to those 8x6 ARGB pixels.

The actual Amount 5 output and independent arbitrary-source oracle both had
ARGB SHA-256
`B115AC29862C051E7F87D340B115EC1197DB8F96B3F6766478EACB05604DAD51`,
with zero differing bytes and maximum channel delta zero. This proves that the
fixture operates on the downsampled world dimensions and, as its source
indicates, does not scale the five-pixel amount by `downsample_x/y`.

## Variable Alpha

`tools/ae_scattermap_alpha_probe.jsx` imported a fixed 16x12 RGBA8 image with
192 distinct alpha values spanning 0 through 254. Amount 0 captured AE's exact
interpretation of that source, including any alpha interpretation and
premultiplication policy; Amount 5 captured the effect output.

Applying the arbitrary-source oracle to the identity-rendered ARGB bytes
produced SHA-256
`B60B162009FB9D2A78662FE9B3B2F4032F5CF4D859508794031D12642C86539F`,
exactly equal to the AE effect output with zero differing bytes. The output
retained alpha values from 0 through 254 across 117 distinct levels. This proves
that production AE behavior follows the fixture's four-channel ARGB coordinate
copy, including transparent and partially transparent pixels.

A third case set Mix with Original to 37.5%. Directly mixing the exported
identity PNG is intentionally not equivalent because AE applies output
premultiplication after the effect. The exact host model is: run the fixture's
four-channel mix on straight ARGB, then multiply each RGB channel by the mixed
alpha using round-to-nearest. This model and AE output both had SHA-256
`9FDE646043AE9B54B38014226F3430E9F5EDF5649321BF406A921B1E454CEB03`
with zero differing bytes. Alpha alone also matched before adding the host
output transform, which independently confirms the plug-in's alpha mix.

## Odd Dimensions

`tools/ae_scattermap_odd_probe.jsx` imported a fixed opaque 13x9 RGBA8 image
and rendered Amount 0 and Amount 5 in AE 25.2. The Amount 0 frame captured the
host's exact source interpretation; the arbitrary-source oracle then processed
all 117 pixels at the production world's odd width and height.

The actual Amount 5 output and oracle both had ARGB SHA-256
`61183666BA5A70E7B1B5A6D2E591EE8A532A3EA152790C90035ABC981CA0B725`,
with zero differing bytes and maximum channel delta zero. This verifies row
transitions and the final pixel in production AE rather than relying only on
the minihost's padded-stride 13x9 coverage.

## Parameter Bounds

Production AE 25.2 rejected all ten fixed out-of-range assignments across
Scatter Amount, Direction, Random Seed, Mix with Original, and Invert Map.
Every call raised an `out of range` exception with the exact descriptor range,
and every property retained its prior value. The host therefore rejects rather
than clamps invalid values before effect dispatch. See
`SCATTERMAP_AE_PARAM_BOUNDS_RESULT_2026-07-13.md`.
