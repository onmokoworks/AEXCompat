# checkout_param out-of-table index: AE observation and host contract (2026-08-17)

Issue #1251. `RollingShutter.aex` (AE 2026 `Support Files\Plug-ins\Effects`,
304-AEX corpus) failed the render sweep with `frame_error:4` after two
`checkout_param` calls for indices 29 and 31 that the host refused as
`unknown_layer`; its PARAMS_SETUP had registered only 4 params. This note
records what the plug-in is doing (RE), what AE answers (oracle capture), and
what the host now does. Observation and inference are marked.

## 1. Which effect the host is running (observation)

`RollingShutter.aex` carries no PiPL. It registers through
`PluginDataEntryFunction` (v1) three effects in this order
(`tools/aex_plugindata_probe.py`):

| order | name | match name | entrypoint |
| --- | --- | --- | --- |
| 1 | Pixel Motion Blur | `ADBE OFMotionBlur` | `MotionBlurMain` |
| 2 | Rolling Shutter Repair | `ADBE Rolling Shutter` | `RollingShutterMain` |
| 3 | Timewarp | `ADBE Timewarp` | `TimewarpMain` |

The worker keeps the first registration of a multi-effect bundle (issue #326,
`record_plugin_data_registration`), so the sweep record named
`RollingShutter.aex` is **Pixel Motion Blur**, not Rolling Shutter Repair. The
trace confirms it: PARAMS_SETUP adds Shutter Control (popup) / Shutter Angle /
Shutter Samples / Vector Detail, the four params `FUN_180052610` (the
`KronosParamType == 2` PARAMS_SETUP) registers.

## 2. Where indices 29 / 31 come from (RE, Ghidra headless on `/RollingShutter.aex`)

Observed in the decompilation (image base `0x180000000`):

- The three exports are one-line wrappers around
  `AE_ROLLINGSHUTTER::rollingshutter::TWorRS(cmd, in_data, out_data, params, output, KronosParamType)`
  with type 0 = Timewarp, 1 = Rolling Shutter Repair, 2 = Pixel Motion Blur
  (`0x180003470/80/90`, `TWorRS` at `0x1800727a0`).
- `TWorRS` case 0xb (RENDER) picks the depth by `PF_WorldSuite2::PF_GetPixelFormat`
  and calls a per-depth render core; for type != 1 the cores are
  `FUN_18005b230` (8-bit), `FUN_180061ce0` (16-bit), `FUN_180068720` (32-bit).
- In `FUN_180061ce0` and `FUN_180068720` (the 8-bit core timed out in the
  decompiler; the 16/32-bit ones are the same code shape) the render core, for
  every Kronos type, `memset`s a 176-byte `PF_ParamDef` to zero and calls
  `in_data->inter.checkout_param(effect_ref, 0x1d, current_time, local_time_step, time_scale, &def)`
  and then the same for `0x1f` into a second zeroed def
  (`0x180061ce0` region, decompiler lines around the two literal calls). It
  reads `def.u.ld.data` (offset 0x50) as "is there a layer": non-null data
  enables the matte / warp-layer path, null means none. The return value of
  the `0x1f` call is kept and its main loop is gated on it
  (`if (... || (int)uVar13 != 0) goto <exit>`), so a non-zero answer ends the
  frame with that error, which is exactly the observed `frame_error:4`.
- The host trace shows what the core does with those defs afterwards: in the
  failing run the two refused checkouts are followed by two `checkin_param`
  calls (answered `4 (not_checked_out)`), and in the rendered run the last two
  `checkin_param` entries of the history are the same pair after the frame's
  layer checkouts, so both defs are checked in whether or not the checkout
  succeeded (trace observation; the checkin call sites were not read in the
  decompilation).
- 0x1d = 29 and 0x1f = 31 are Timewarp's Matte Layer / Warp Layer slots:
  `FUN_1800516a0(params, timewarp_index, kronos_type)` maps Timewarp's
  0x01..0x25 index space onto each effect's own slot table (for type 2:
  0x19->params[1], 0x1a->params[2], 0x1b->params[3], 0x06->params[4], the
  rest constants), and the render core reads all its numeric params through
  that map; only the two layer checkouts use the raw Timewarp index.

Inference: the shared Kronos render core asks for the two layer slots
unconditionally and relies on the host to answer "no such layer" for the
effects that do not have them. Pixel Motion Blur ships and renders in AE, so
AE's answer must be one the core reads as "no layer" with return code 0.

## 3. What AE answers (oracle observation)

Probe: `instruments/pf-checkout-index-probe` (build with
`tools/build-pf-checkout-index-probe.ps1`). It registers the same 5-slot table
(popup + three sliders) and, in RENDER, for each of the indices 29, 31, 5, 0:
checks out into a zero-filled def and checks it back in; then checks out again
into a def seeded with 0xAB in every byte to see whether the host writes it.
Answers are encoded as solid RGB bands (see the source header for the layout).

Capture: `tools/capture-ae-probe-oracle.ps1`, After Effects 26.3
(`AfterFX.exe` product version 26.3), 8 bpc, plug-in installed under the
user-writable `MediaCore\AEXCompatOracle` folder, effect added by match name
`AEXCompat CheckoutIndex`, input a 240x60 solid PNG
(sha256 `9161c6eb70fd81bbdac187d23608a30c892006e6e8d9a2fa58d0947883230271`),
probe sha256 `25d444d8af45f947dc25d7a717427ac4f79c63aefc2c1a8992a8887a4100bd8f`,
output PNG sha256 `fcdaabf67ed43ada9ea4e5e47d122b99cb01800b6b073c9f70a4e974a594b555`,
`loaded_aex_identity.state = verified`, `effect_provenance.state = verified`.

Decoded (band centre pixels, RGB):

| index | zeroed def: checkout err / checkin err | zeroed def: `u.ld.data` non-null / `param_type` / `u.ld.width` | seeded def: written by AE / checkout err |
| --- | --- | --- | --- |
| 29 | 0 / 0 | no / 0 (LAYER) / 0 | yes / 0 |
| 31 | 0 / 0 | no / 0 (LAYER) / 0 | yes / 0 |
| 5 (first slot past the table) | 0 / 0 | no / 9 (NO_DATA) / 0 | yes / 0 |
| 0 (input layer, control) | 0 / 0 | yes / 0 (LAYER) / 240 | yes / 0 |

Observation: for each of the three past-the-table indices queried (5, 29, 31;
Classic `PF_Cmd_RENDER`, 8 bpc, one 5-slot table) AE returns `PF_Err_NONE`,
writes the def (the 0xAB seed does not survive), leaves `u.ld.data` NULL, and
accepts the checkin with `PF_Err_NONE`. Index 5 comes back typed `PF_Param_NO_DATA`
while 29 / 31 come back typed 0; the probe does not resolve what AE keeps at
slot 5 (an internal stream past the effect's own params is one reading), and
the host does not reproduce that distinction. Not observed: negative indices,
what AE writes into the def beyond `param_type` / `u.ld.data` / `u.ld.width`
(the probe reports "written", not "zero-filled"), other depths, and the
SmartFX / lifecycle selectors. Extending the rule to every index past the
table, to the hosted (lifecycle / smart) table, and to a full zero-fill of the
def is inference from this Classic RENDER capture.

## 4. Host contract after this change

`checkout_param` (`minihost/src/worker_param_checkout_runtime.cpp`) answers a
slot that is past the published definition table (index > last published
slot, on both the classic-context path and the hosted lifecycle / smart table)
with `PF_Err_NONE` and a zero-filled def (param_type LAYER, `u.ld.data` NULL),
records the checkout so the matching `checkin_param` returns `PF_Err_NONE` and
the balance check holds, and stamps the callback-history entry with reason
`beyond_param_table` (a result-0 entry: not counted as a denial; it marks in
`callback_history` that a checkout resolved past the table, without the slot
number, and only within the 32-entry history ring; the slot is on the
`AEXCOMPAT_EXTENDED_DIAG=1` trace line `checkout_param -> 0 (beyond_param_table)`). Still refused with
`unknown_layer`: negative slots, slots inside the table that have no
definition (a timed layer slot asked at a time the host does not hold, or a
gap), and any slot while no table has been published. Nothing in the
host-protection floor changed: no host memory is handed out (the def is the
caller's, zero-filled), and ownership stays fail-closed for stale / foreign /
double checkins.

Self-test: `--self-test-checkout-param-beyond-table` on all three workers
(`tests/test_worker_selftest_routes.py::test_checkout_param_beyond_table_answers_like_ae_on_all_workers`).

Result on the corpus (AE 2026 `Support Files\Plug-ins\Effects` as the sweep
argument, 304 AEX, default depth 8 / 256x144 / time 0, worker 3 exe built from
this change): `RollingShutter.aex` (Pixel Motion Blur) goes from
`frame_error:4` to `rendered` and no other plug-in changes bucket against a
sweep of the same folder with workers built from `2a11b8b9`.

## 5. Open observations (not changed here)

- The sweep record for a multi-effect PluginData bundle carries the file name
  and only the first registered effect renders; Rolling Shutter Repair and
  Timewarp in this bundle are not exercised by the sweep at all. Filed as
  #1260 (refs #326's first-wins rule).
- AE types the first slot past the table (`index == num_params`) as
  `PF_Param_NO_DATA` and the far slots as 0; the host answers 0 for all of them.
  Immaterial for the Kronos core (it reads only `u.ld.data`) but a real
  divergence at that one slot.
- The SmartFX `pre_checkout_layer` path (`worker_smart_runtime.cpp`) keeps its
  own `unknown_layer` refusal for unknown layer indices; the AE observation
  above is for `PF_InteractCallbacks.checkout_param` only and was not carried
  over.
