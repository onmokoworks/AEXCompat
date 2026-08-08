# Render sweep baseline, 2026-08-09

What the installed AE corpus does when every plug-in is asked for one frame, and
what the failures are made of. The point of the file is that the next session
does not have to re-derive the shape of the problem: the cohorts below are split
by cause, each says what is observed and what is inferred, and each names its
members so the first command is a `--filter` rather than a query.

Measured on `main` at `5c807e7d` (the merge of #974) with workers built from that
tree. The sweep report carries no host-build fingerprint, so that pairing is
asserted here and not verifiable from the report itself; giving the report a
`BuildFingerprint` the way the multifilter cache has one would fix that.

## Running it

```powershell
$env:AEXCOMPAT_MULTIFILTER_REPOSITORY = "C:\path\to\AEXCompat"
cargo run --release --manifest-path bridges\aviutl2-multifilter\Cargo.toml `
  --example render_sweep -- "C:\Program Files\Adobe\Adobe After Effects 2026\Support Files\Plug-ins\Effects" `
  --json sweep.json
```

With no folder argument it sweeps what the AviUtl2 registration would register
(the configured or default scan folders, 575 AEX here) rather than AE's Effects
folder alone (304). Enumeration and discovery are the shipping code, and neither
touches the discovery cache file, so a sweep cannot demote what a running AviUtl2
depends on.

Useful axes: `--depth 8|16|32`, `--time`, `--frames`, `--no-layer`,
`--force-classic`, `--plugin-defaults`, `--filter`, `--limit`/`--skip`.
`--close-report` carries the worker's whole close report for one plug-in, and
with `AEXCOMPAT_EXTENDED_DIAG=1` that includes the host-callback trace
(`worker.diagnostics.stderr_tail`). Diffing two of those traces is how #958's two
regressions and #962's copy defect were each found; it is the sharpest tool here.

`--close-report` output is not shareable: the trace is unbounded in shape and can
carry absolute paths.

## Baseline: AE 2026 `Support Files\Plug-ins\Effects`, 304 AEX, one frame each

| bucket | n |
| --- | --- |
| `rendered` | 170 |
| `frame_error:512:PF_Err_INTERNAL_STRUCT_DAMAGED` | 31 |
| `render_frame_failed:worker_exited` | 22 |
| `session_open_failed` | 20 |
| `frame_error:516:PF_Err_BAD_CALLBACK_PARAM` | 16 |
| `frame_error:4:PF_Err_OUT_OF_MEMORY` | 9 |
| `not_discovered:exit_12_unknown_no_effect_entrypoint` | 9 |
| `render_frame_failed:worker_invariant_failure` | 8 |
| `not_discovered:unknown` | 6 |
| `frame_error:514:PF_Err_UNRECOGNIZED_PARAM_TYPE` | 4 |
| `frame_error:14` | 3 |
| `rendered_empty` | 2 |
| `not_discovered:exit_12` | 2 |
| `not_discovered:cluster_session_invalidated` | 1 |
| `not_discovered:exit_12_aegp_candidate` | 1 |

## The 512 cohort turned over almost completely since #704

#704 (filed 2026-08-04) listed 41 plug-ins answering error 512. Where those 41
are now:

| | n |
| --- | --- |
| `rendered` | 25 |
| still `frame_error:512` | 14 |
| `frame_error:516` | 1 (Matte_Choker) |
| `render_frame_failed:worker_invariant_failure` | 1 (Corner_Pin) |

The 25 that render: Arithmetic, Bevel_Alpha, Change_Color, Circle, Color_Emboss,
Color_HLS, Emboss, Equalize, EyedropperFill, Foam, Gpg, Invert, Lightning2,
Magnify, Mirror, Mosaic, Noise, PSL_Bevel_Emboss, PSL_Drop_Shadow,
PSL_Inner_Glow, PSL_Inner_Shadow, PSL_Outer_Glow, Texturize, TurbulentNoise,
Unmult.

The 14 that have not moved: Basic_3D, Brush_Strokes, Bulge, Channel_Blur,
DirectionalBlur, Fractal, Glow, Lens_Flare, Radial_Blur, SmartBlur, Spherize,
Strobe_Light, Time_Displace, Wave_Warp.

And 17 plug-ins that were not on #704's list are in the 512 bucket now:
BallAction, CannedWarp, Curl_Noise, Cylinder, Echo, PW, Rainfall, ShapeBlur,
Smear, Snowfall, Sphere, Spill2, Transform, Vignette, and the three `VR*` below.
Some of those sit in subfolders #704's sweep did not descend into, so not all of
the turnover is movement; the 25 that now render and the 14 that did not move are
the parts that are directly comparable, because #704 named them.

Reading "31 today against 41 then" as "barely changed" is the mistake this
section exists to prevent.

## Cohorts

### 1. `session_open_failed`, 20 — nothing reaches the plug-in

| reason | n | members |
| --- | --- | --- |
| `arbitrary parameter has no printable text` | 15 | ColorAndContrast, ColorShift, Colorama, Cryptomatte, Curves, EXtractoR, Hue_Sat, IDentifier, Liquify, Lumetri, MochaAE, MshWrp_New, OCIOCDLTransform, OCIOFileTransform, mochashape4ae_adobe |
| `interactive parameter is out of range` | 5 | Block_Dissolve, Card Dance, Card Wipe, Iris_Wipe, Smear_New |

Observed: the session is refused before the worker is spawned, so no selector
runs and there is no close report to read. The sweep hands the session exactly
what discovery reported for that plug-in.

Not established: *why* the value is missing or out of range. The arbitrary
summary discovery reports comes from the plug-in's own `PF_Arbitrary_PRINT_FUNC`
(`worker_parameter_execution.cpp`), so "the host rejects what it produced" may
bottom out in an ARB round-trip that failed at discovery time — which is AE
semantics, not a loop closed inside this repository. Do not start from the
assumption that it is purely host-side validation.

This is the largest cohort whose members all fail at the same, very early place.
That is the reason to start here; it is not that the cause is understood.

### 2. `frame_error:512`, 31 — mostly not a missing suite

The sweep records `suite_acquire_failures` per plug-in from the worker's own
`stage:suite_acquire_failed` lines. **28 of the 31 have none**: every suite they
ask for, they get.

The three that do are the VR trio, which looks like a separate story answering
the same code:

| plug-in | could not acquire |
| --- | --- |
| VRConverter | `AEGP Item Suite v13` |
| VRSphereToPlane | `AEGP Item Suite v13` |
| VRPlaneToSphere | `AEGP Item Suite v13`, `AE VR Effects Video Attributes Suite v1` |

All 31 split 17 `smart_render` / 11 `classic_render` / 3 `frame_setup` by the
stage they fail in, and 17 smart / 14 classic by the path discovery says they
take; the VR trio are three of the seventeen `smart_render`, leaving the other 28
at 14 / 11 / 3. Twenty-six of the 31 declare no layer parameter, and all 31 have
a null `return_message`.

Inferred, not observed: that one cause common to both render paths is likelier
than several independent ones. The three `frame_setup` failures (Basic_3D, Bulge,
Spherize) do not fit that framing at all — they fail before either render
selector runs — which is also what makes them the cheapest entry point, because
their traces are the shortest.

Take one of those three with `--close-report` and `AEXCOMPAT_EXTENDED_DIAG=1` and
read the last callbacks before the stage ends.

#704's "Adobe defines private codes based at 512" hypothesis is disproved:
`AE_Effect.h` defines `PF_FIRST_ERR` as 512 and the value is
`PF_Err_INTERNAL_STRUCT_DAMAGED`.

### 3. `render_frame_failed:worker_exited`, 22 — three groups

| exit | n | stage reached | members |
| --- | --- | --- | --- |
| 21 | 13 | `render` | every `Aud_*` in the corpus (BT, Compressor, Delay, Distortion, Flange, Gate, HiLo, Mixer, Modulator, ParamEQ, Reverb, Reverse, Tone) |
| 3 | 7 | none | AudSpect, AudWave, Inner-Outer-Key, Radio_Waves, Scribble, Stroke, Vegas |
| `0xC0000409` | 2 | `smart_render_cpu` (Fog_3d), `smart_render` (Levels2) | Fog_3d, Levels2 |

Observed: the exit code partitions these 22 exactly, and the 13 exit-21s are
exactly the `Aud_*` audio processors.

Not established: that exit 21 *means* "audio effect". Cohort 3b has four exit-21s
that are plainly not audio (Basic_Text, Lightning, Numbers, Path_Text), so
whatever exit 21 is, it is not audio-specific. Setting the 13 aside because they
process audio is a judgement about the corpus, not a reading of the exit code.

AudSpect and AudWave draw audio rather than process it and sit in the exit-3
group, not with the thirteen.

`0xC0000409` is `STATUS_STACK_BUFFER_OVERRUN`, which `__fastfail` also raises for
other checks, so naming it a stack cookie specifically is a guess. These two want
a minidump (`AEXCOMPAT_MINIDUMP_DIR`).

### 3b. `render_frame_failed:worker_invariant_failure`, 8 — two host refusals

The session's own error code splits these cleanly, and the report already carries
it:

| session error | exit | members |
| --- | --- | --- |
| -47 `kSessionSequenceSetupFailed` | 21 | Basic_Text, Lightning, Numbers, Path_Text |
| -45 `kSessionOutputValidationError` | 24 | BezWarp_New, Corner_Pin, PageTurn, Tile |

Two different failures, not one cohort of eight. The first four never got through
sequence setup; the second four produced output the host refused. The -47 four
are all text or path effects, which is a thread.

### 4. `frame_error:516`, 16 — eight classic, eight smart

Blobbylize, Cartoon, Compound_Blur, CrossBlur, Drizzle, Glass, GlueGun,
LightSweep, Matte_Choker, MrSmoothie, PowerPin, Slant, SolidComposite, Timecode,
Tritone, Unmultiply. Four declare one layer parameter each (Blobbylize at slot 2,
Compound_Blur at slot 1, Glass at slot 2, MrSmoothie at slot 1).

`PF_Err_BAD_CALLBACK_PARAM` is what a plug-in answers when a host callback refused
it, and #962 is the worked example: 3DGlasses answered 516 because `PF_COPY`
refused a rectangle AE would have clipped. The refusing callback is in the trace.

**Timecode is already answered by the report.** It is the only plug-in in the
whole corpus with a non-null `return_message`:

```json
{"selector": "RENDER", "error": 516, "display_requested": true,
 "text": "Not able to acquire AEFX Suite."}
```

and it is also the one that fails to acquire `AE Timecode Helper Suite v1`. Two
things follow. Its story needs no further tracing, and the per-frame `return_msg`
transport #704 suspected of being broken demonstrably works — the other 15 are
silent because their plug-ins said nothing, not because the host dropped it.

### 5. `not_discovered:*`, 18 — the entrypoint cohort and the remainder

| bucket | n | members |
| --- | --- | --- |
| `exit_12_unknown_no_effect_entrypoint` | 9 | VRChromaticAberration, VRColorGradient, VRDenoise, VRDigitalGlitch, VRFractalNoise, VRGaussianBlur, VRGlow, VRRotateSphere, VRSharpen |
| `exit_12` | 2 | Fast_Blur, Sharpen |
| `exit_12_aegp_candidate` | 1 | MochaAEAEGP (an AEGP, correctly not an effect) |
| `unknown` | 6 | 3D Camera Tracker, PSL_Adjustments, Particle_Playground, ProfileToProfile, Reshape_New, Stabilizer |
| `cluster_session_invalidated` | 1 | |

The nine `VR*` here are different plug-ins from the three `VR*` in the 512
cohort, which discover fine and fail at render. The names do not separate them;
the bucket does.

Fast_Blur and Sharpen are not unclassified: their records carry
`cluster_error_kind: entrypoint_unresolved` with exit 12, the same conclusion as
the nine. They land in a coarser bucket only because they went through the
cluster path, whose diagnostics carry no `plugin_kind`, which is what the sweep
keys on. Eleven plug-ins, one cause — #326's `PluginDataEntryFunction`
registration.

The six `unknown` carry no classification at all and are what is left after #960.

## Suites nothing could acquire

Across all 304 — eleven records over ten plug-ins:

| suite | n | asked by | that plug-in's bucket |
| --- | --- | --- | --- |
| `Premiere Memory Manager Suite v4` | 3 | OCIOColorSpaceTransform, OCIODisplayTransform, OCIOLookTransform | `rendered` |
| `AEGP Item Suite v13` | 3 | VRConverter, VRPlaneToSphere, VRSphereToPlane | `frame_error:512` |
| `AE CPU Data Suite v1` | 2 | Dust, Invert (#711) | `rendered` |
| `Private AE InData Internal Query Suite v1` | 1 | Posterize_Time | `rendered` |
| `AE Timecode Helper Suite v1` | 1 | Timecode | `frame_error:516` |
| `AE VR Effects Video Attributes Suite v1` | 1 | VRPlaneToSphere | `frame_error:512` |

Only four of the ten are in a failing cohort at all: the VR trio and Timecode.
Six ask for a suite they do not get and render anyway.

## Adding suites did not move this corpus

Between `2924e94b` and `5c807e7d`, seven suites landed — AEGP Utility v5
(8e6ef533), Layer v8 (854e8853), Comp v4 (9679c798), Item v3 and the legacy World
suite (031e94a0), Stream v8 and Iterate v1 (8f1784d9) — plus callback history
diagnostics and, separately, #960's discovery classification (8d71b8f7).

Across the same 304 plug-ins: **170 rendered before and after, and not one
plug-in changed render bucket.** Twelve rows did change and all twelve are
`not_discovered:*` renaming from #960 — the nine `VR*` and MochaAEAEGP out of
`unknown`, Fast_Blur and Sharpen out of `nonzero_exit`. That is a diagnostic
improvement, not a render one.

Implementing the next suite is cheap and legible, and on this evidence it is not
what moves this number. The cohorts above are.

## What a re-measurement should hold fixed

`--depth 8`, `--time 0`, `--frames 1`, a secondary layer supplied to the first
layer parameter, 256x144. Changing any of those changes the buckets: #777 is a
plug-in that fails at 16 bits and not at 8 or 32, #828 is a class that renders
only at time 0, and #961 is DeepGlow2 taking the worker down at 16 bits alone.
The report records all of them under `render`, so a report always says what it
was measured with.

## Related

- #957 the sweep, #958 the two regressions it found first, #962 the empty layer
  parameter and the `PF_COPY` clipping
- #704 the 512 cohort, #326 the entrypoint cohort, #711 AE CPU Data Suite
- #960 discovery failure classification (landed; `unknown` 16 → 6)
- #961 DeepGlow2 at 16 bits, #978 the `session_open_failed` twenty
