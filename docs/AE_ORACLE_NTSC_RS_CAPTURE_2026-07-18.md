# AE Oracle Capture: ntsc-rs (2026-07-18)

First successful After Effects oracle comparison for a real third-party AEX.
This note records the capture-tooling failures found on the way, the fixes,
and the comparison result. Observations are labeled as such; hypotheses carry
explicit hedging.

## Environment

- After Effects 25.3.1x3 (installed as "Adobe After Effects 2025"), Windows 11.
- Plug-in: ntsc-rs v0.9.4 AE build, `ntsc-rs-ae.aex`, SHA-256
  `ad129f8029914a09346de0d1ed60ee5d5a7ad5d3df4a17782260ef2421fd354a`
  (release artifact `ntsc-rs-windows-afterfx.zip`), installed in the shared
  `MediaCore` plug-in folder and byte-identical to the copy the host tested.
- Host side: AEXCompat SmartFX render (`--render-experimental-smart`) of the
  same `.aex`, same input, frame 0, default parameters, 8 bpc.

## Timeline of observations

1. **Every earlier capture timeout had one cause: `AfterFX.exe` does not run
   `-r` scripts.** Launching `AfterFX.exe -m -noui -r <script>` starts AE,
   loads plug-ins, and exits with code 0 after roughly 15 seconds without
   executing the script; a script that writes to a hard-coded path produces
   nothing. The same invocation through `AfterFX.com` executes the script in
   about 16 seconds. Verified with minimal marker scripts on 2026-07-18.
2. **Correction of the 2026-07-18 earlier-session claims.** A prior working
   session asserted that the effect had resolved to the Premiere GPU variant
   (`Pr GPU MediaCore ntsc-rs`) and that `saveFrameToPng` hung. The session
   transcript contains no executed command supporting either claim (the smoke
   script was written but never run); both statements are retracted as
   unsubstantiated. The observed timeouts are fully explained by item 1.
3. **AE 25.3 ExtendScript has no `JSON` global** (`typeof JSON` is
   `undefined`), and referencing the missing global aborts the script mid-write,
   leaving a truncated result file. `tools/ae-reference-capture.jsx` previously
   depended on `JSON.stringify`, so it could not have produced a valid result
   on this AE version even via `AfterFX.com`. It now serializes by hand.
4. **`CompItem.saveFrameToPng` is asynchronous.** It returns immediately;
   quitting right after discards the write. At 1920x1080 the PNG appeared
   within 500 ms of polling. The JSX now polls for the file (180 s bound).
   Stringifying its return value also throws ("Object of type Object found
   where a Number, Array, or Property is needed"), so the return is ignored.
5. **Effect resolution.** `addProperty("ntsc-rs")` (the PiPL
   `AE_Effect_Match_Name` of the AE variant, per ntsc-rs `v0.9.4`
   `crates/ae-plugin/build.rs`) resolves to `matchName "ntsc-rs"`, display
   name "NTSC-rs", 87 properties.

## Comparison result (observation)

Input `target/ntsc-rs-input.png` (1920x1080 gradient, SHA-256
`bb84e35ea6bdbfa178d13d6571dbff84aa2b78e57f3dbc9831aa7448c619c97e`),
frame 0, fps 24, 8 bpc, all parameters at defaults on both sides:

- AE reference `target/ae-ntsc-rs-reference.png`: SHA-256
  `2cd24acf039b61151438e6d4ddd14f1eef1ee28b66591893c6e1cf0309ac16bf`.
  Two independent AE captures were byte-identical, so the AE render is
  deterministic for this configuration.
- AEXCompat output `target/ntsc-rs-output.png`: SHA-256
  `c7da6b3a3ce00531c50ef891f6fb1d7a20584596f1fe35fe1d2f70ffcafa4bab`
  (byte-identical across two host renders in the earlier session).
- Pixel difference (RGBA, per channel): 178 of 2,073,600 pixels differ,
  every difference is exactly 1 LSB, mean absolute difference 2.1e-5.

Claim level: this is AE-oracle equivalence evidence for ntsc-rs SmartFX,
frame 0, default parameters, 8 bpc, within a ±1 LSB rounding tolerance. It
says nothing yet about parameter changes or 16/32 bpc; those need their own
captures. The ±1 LSB residue was initially hypothesized to be float-to-8-bit
rounding differences between AE's and the host's pipelines; the follow-up
below verified that reading as far as 8-bit data allows.

## Follow-up: the ±1 LSB residue is value-dependent, not structural
   (verified 2026-07-18, same day)

Two checks, both on otherwise identical configurations:

1. **Distribution of the frame-0 differences.** All 178 differing pixels
   differ in exactly one channel; the channel split is R 61 / G 62 / B 55 /
   A 0, the direction split is host-higher 78 vs AE-higher 100, and the
   locations scatter across the whole frame (161 distinct rows, at most 2
   hits per row). No spatial structure, no channel bias, no single-direction
   bias, alpha exact.
2. **A second capture at frame 12** (ntsc-rs seeds its noise from the frame
   number, so the noise content changes completely). AE reference
   `c561d9c30a213dd60e3e636f34562824a3d16d35efef266cd8e20ce0287ae73d`,
   host output `target/ntsc-rs-time-f12.png`
   `17a9958b4f7051bd8b2db0754b3afc8ef99794d2de91d31b600d12d9b82a6042`:
   162 differing pixels, again all exactly ±1 LSB, mixed directions, alpha
   exact. **The frame-0 and frame-12 difference locations overlap in zero
   pixels.**

A structural host defect (wrong coordinate handling, stride or edge errors,
a biased conversion) would produce differences correlated by location or
direction across frames. Instead the differences move entirely with the
noise content, which means they are value-dependent: pixels whose
pre-quantization value sits at an 8-bit rounding boundary resolve to
adjacent integers in the two pipelines. Float-level confirmation is not
currently possible because the worker-to-broker image transport is fixed at
8-bit RGBA; if that ever gains a deeper path, the check can be repeated at
16/32-bit precision.

The frame-12 capture doubles as the second AE-oracle equivalence sample
(same tolerance, different frame/noise seed).

## Follow-up: 16 and 32 bpc captures (2026-07-18, same day)

Both deeper-depth captures ran with the same input, frame 0, defaults.

An analysis-tooling correction first: at 16 and 32 bpc,
`CompItem.saveFrameToPng` exports **RGBA16 PNGs** (IHDR bit depth 16,
consistent with `docs/AE_REFERENCE_TRACE_2026-07-13.md`). A first pass of
this analysis read those files with PIL, which silently truncates 16-bit
RGB(A) PNGs to 8 bits (a `>>8`, not a rounding), and concluded that
`saveFrameToPng` writes 8-bit files and that 42% of 16 bpc pixels differ.
Both statements were artifacts of the truncation and are wrong; the
numbers below come from the true 16-bit samples (decoded via ffmpeg to
`rgba64le`).

- **16 bpc: equivalence holds at 8-bit precision, ±1 LSB.** Rounding the
  AE RGBA16 capture to 8 bits (`round(v * 255 / 65535)`) and comparing
  against the host's `--render-experimental-smart-16` 8-bit output:
  8,473 of 2,073,600 pixels differ, every one by exactly ±1 LSB, nothing
  at ±2 or more, alpha exact. The count is higher than the 8 bpc run
  (178) because the host side still quantizes through its own 16-bit
  path before the 8-bit export. A comparison at full 16-bit precision
  has exactly one remaining blocker: the worker-to-broker image
  transport is fixed at 8-bit RGBA, so the host cannot export deeper
  than 8 bits today. The AE side already can.
- **32 bpc: not comparable as captured; color management interferes.**
  The true 16-bit samples span 0–16195 of 65535 (a quarter of the
  scale; mean 5.7% of full scale vs the host's 52%), and no per-pixel
  post-transform of the host output explains them (best fit,
  sRGB-linearize plus 1/4 scale, still leaves ~5% of full scale as mean
  error). So the 32 bpc project is not merely exporting differently; the
  effect appears to receive transformed (likely linearized) input and
  renders genuinely different data. Hypothesis, unverified: the 32 bpc
  project linearizes or reinterprets the working space and
  `saveFrameToPng` exports without the display transform. Before any
  32 bpc comparison, the capture JSX must pin the project color pipeline
  (working space, linearization) explicitly and the result must be
  validated against a no-effect passthrough capture first.

## Follow-up: 32 bpc is comparable; the blocker was a fixed export range
   mapping, not color management (2026-07-18, later session)

The previous section hypothesized that the 32 bpc project linearizes the
working space. The observations below refute that hypothesis and replace it
with a verified mechanical explanation.

1. **Project color defaults are depth-independent (observation).** A scripted
   probe on AE 25.3.1x3 shows a fresh project has `workingSpace "None"`,
   `workingGamma 2.4`, `linearizeWorkingSpace false`, `linearBlending false`,
   `compensateForSceneReferredProfiles true`, and none of these change when
   `bitsPerChannel` is set to 8, 16, or 32. All of them are scriptable
   read/write; `listColorProfiles()` returns 102 profiles.
2. **`saveFrameToPng` at 32 bpc applies a fixed RGB range mapping
   (observation).** A no-effect passthrough capture of
   `target/ntsc-rs-input.png` (frame 0, fps 24, defaults) produced
   `target/ae-noeffect-32.png`, SHA-256
   `9d91f98d6eb78ffb91296d62d6d932ae90aeed7f6c1e7823f89355a7f6ceb8c0`.
   Every 8-bit input value v maps deterministically (all channels alike) to
   approximately `round(v/255 * 6553.5)` with an error of 0 to -2 uint16
   steps, i.e. RGB float 0..10 maps linearly onto uint16 0..65535 and the
   comp's float value v/255 is stored at one tenth of full scale. Alpha is
   not scaled (1.0 stores as 65535). The PNG carries no gAMA/iCCP/cICP
   chunk. Undoing the mapping (multiply by 10, round to 8 bits) reproduces
   the input **exactly, every pixel, RGB and alpha**.
3. **The mapping is independent of the project working space (observation).**
   The same capture with the working space pinned to `sRGB IEC61966-2.1`
   (`target/ae-noeffect-32-srgb.png`) is byte-identical to the
   default-project capture (same SHA-256 as above). Pinning the color
   pipeline neither causes nor removes the transform, so the earlier
   "effect receives linearized input" hypothesis is refuted: the earlier
   32 bpc numbers (span 0-16195/65535, mean 5.7% vs host 52%) are exactly
   the x0.1 export mapping, not linearization (0.52 / 10 = 0.052).
   Hypothesis, unverified: the fixed 0..10 range exists to preserve
   overbright float values in a 16-bit integer container; the residual
   0..-2-step error looks like an internal LUT/quantization artifact of the
   exporter and its cause is not identified.
4. **32 bpc effect comparison (observation).** AE capture
   `target/ae-ntsc-rs-32.png`, SHA-256
   `0f4194bf09ed9dca7ace548a74f2ae3beb40673912dfa4e671c468999a9dfa63`,
   same input/frame/defaults as the 8 bpc run. The AE float world (after
   undoing x0.1) spans 0..2.471 with mean 0.5703 - the effect emits
   overbrights above 1.0 (ringing), which the host's 8-bit output clamps.
   Comparing `clip(round(ae_float * 255), 0, 255)` against the host's
   `target/ntsc-rs-output-32f.png` (`--render-experimental-smart-32-cpu`,
   SHA-256 `7df6db96c0d05c05851269dd9365a729dcd64c13859198000ff0b615d8d2f296`):
   147,468 of 6,220,800 RGB samples differ, **all by exactly +/-1 LSB**
   (147,218 host-higher, 250 AE-higher), alpha exact, mean abs difference
   9.3e-5. The direction bias is fully explained by the exporter's downward
   0..-2-step quantization: 100% of the host-higher samples sit just below
   an 8-bit rounding boundary (median distance 0.021 LSB, p95 0.060) and
   all 250 AE-higher samples sit just above one, while matching samples are
   uniformly distributed. No spatial or channel structure beyond that.

Claim level: this is AE-oracle equivalence evidence for ntsc-rs SmartFX at
32 bpc (CPU path), frame 0, default parameters, at 8-bit precision within
+/-1 LSB, after undoing the documented saveFrameToPng x0.1 export mapping and
clamping to the host's 8-bit output range. Overbrights above 1.0 and
sub-8-bit precision remain unverified on the host side until the
worker-to-broker transport can carry more than 8-bit RGBA; the AE-side EXR
runner (`tools/capture-ae-exr-oracle.ps1`) is the deeper-precision path.

## Tooling changes shipped with this note

- `tools/ae-reference-capture.jsx`: hand-rolled JSON serialization (no
  `JSON` global on AE 25.3) and asynchronous-save polling.
- `tools/capture-ae-reference.ps1`: refuses the silent `AfterFX.exe` no-op;
  substitutes the sibling `AfterFX.com` with a warning when given the `.exe`.
- Later session: optional `-WorkingSpace` / `-LinearizeWorkingSpace`
  parameters pin the project color pipeline (readback-verified, fail-closed;
  unset leaves the fresh-project defaults untouched), and every capture
  result now records the observed working space, gamma, linearize, linear
  blending, and scene-referred-compensation state for evidence identity.
  The 8 bpc reference re-captured byte-identically after this change
  (SHA-256 `2cd24acf...` unchanged).

## Follow-up: the 8-bit transport blocker is resolved (2026-07-18, later
   the same day)

Correction to the two "transport is fixed at 8-bit RGBA" statements above:
they described the state at capture time and are no longer true.

1. The worker-to-broker image transport now carries the worker's native
   pixel depth for `argb16`/`argb32f` renders, and the broker preserves it
   as a raw sidecar (`<output>.rgba16le` / `<output>.rgba32f-le`) next to
   the 8-bit preview PNG ("Preserve native pixel depth across worker
   transport").
2. On top of that, `--render-experimental-16-deep` and
   `--render-experimental-smart-16-deep` now write the output PNG itself as
   full-range RGBA16 (IHDR bit depth 16), expanding the AE-range transport
   samples (white = 32768) with `round(v * 65535 / 32768)`. The expansion
   is exhaustively verified to be lossless for AE-range data and to
   reproduce the 8-bit preview under `round(v16 * 255 / 65535)`
   (broker unit test, all 32769 values).

Verification against this note's earlier capture (observations, same input
`bb84e35e...`, same plug-in `ad129f80...`, frame 0, defaults):

- `--render-experimental-smart-16` re-rendered with the current worker and
  broker produces a preview PNG pixel-identical to this note's
  `target/ntsc-rs-output-16.png` (8,294,400 RGBA8 samples, zero
  differences), so the native-depth transport did not change the 8-bit
  route.
- `--render-experimental-smart-16-deep` on the same input renders with an
  identical worker output hash
  (`94aaae84e573a090abd0c472fa79d548b1667beed2661be1fd63d61b548a74f6` for
  both routes), writes a bit-depth-16 RGBA PNG, and rounding its
  16,588,800 samples to 8 bits reproduces the preview PNG exactly (zero
  mismatches). The raw sidecars of both routes are byte-identical; all raw
  samples sit in 0..=32768 (no over-white values), and the 16-bit PNG is
  the exact lossless expansion of the raw data.

The float/16-bit-level AE comparison the earlier section called blocked is
therefore unblocked: the host can now export 16-bit PNGs and raw
`rgba16le` buffers, and `tools/compare-pixel-oracles.py` accepts them
(`--raw-format rgba16le --raw-integer-max 32768`). It still needs a fresh
AE 16 bpc capture (none is retained under `target/`), so the full-precision
comparison remains future work, not a claim of this note.

## Follow-up: fresh 16 bpc comparison detects a real unresolved difference
   (2026-07-19)

A fresh capture used a deterministic 1920x1080 RGBA gradient (PNG SHA-256
`29ebf3c2245a3b078bc430adc0b49b9906b092ab5f37c436895fc9c2035288cd`),
ntsc-rs defaults, frame 0, fps 1, one-frame duration, and project working
space `None`. Both AE and the host used the same plug-in binary and the host
used SmartFX ARGB16 with native-depth transport.

The comparison is **not equivalent** at 16-bit precision. Alpha is exact,
but 5,315,732 channels differ and 5,311,798 exceed one PNG16 integer step.
Mean absolute RGB error is R 0.01117, G 0.00896, B 0.02539; maximum error is
1.0. Clamping the host samples to 0..1 does not change those figures. The
images remain strongly correlated (R 0.9972, G 0.9980, B 0.9727), so this is
not a channel-order, gross color-space, alpha, or spatial-offset failure.

This observation supersedes no earlier equivalence claim because it uses a
new input and deeper comparison boundary. It establishes an unresolved
compatibility gap that needs parameter/world/timing-call tracing before the
host can claim ntsc-rs equivalence at 16 bpc. The artifacts are local under
`target/ntsc-rs-oracle16-v2-*`; hashes are recorded by the comparison report.

## Follow-up: corpus expansion to a popup parameter and input-shape
   variations (2026-07-19, issue #31)

All previous samples used one float slider and the single 1920x1080 opaque
gradient input. Four new samples extend the corpus along the parameter-kind
and input-shape axes (all observations; same plug-in `ad129f80...`, AE
25.3.1x3, frame 0, fps 24, 8 bpc, default parameters except where noted).
The machine-readable judgments live in
`analysis/NTSC_RS_ORACLE_CORPUS_RESULT_2026-07-19.json` (regenerated only
via `tools/refresh-ntsc-rs-oracle-corpus-evidence.ps1` from the executed
artifacts under `target/oracle-corpus/`; validated by
`tests/test_ntsc_rs_oracle_corpus_result.py`), produced with
`tools/compare-pixel-oracles.py --tolerance 0.004` (accepts +/-1 LSB at
8 bits, rejects +/-2). The host side ran the freshly rebuilt workers and
harness on this branch; the broker integration tests passed after the
rebuild.

1. **Popup parameter (`Use field` = `Both`).** First verification that
   `capture-ae-reference.ps1 -ParamName` drives a PF_Param_POPUP: the JSX
   `setValue(6)` applied cleanly (result JSON reads back `param_value: 6`).
   Host side used `--render-experimental-smart-param ... 4 6` (slot 4,
   1-based choice index; the harness `--inspect-experimental` listing pins
   `Use field` to slot 4 with choices Alternating/Upper only/Lower
   only/Interleaved upper/Interleaved lower/Both, default 4). Input is the
   existing gradient (`bb84e35e...`). Result: 151 of 8,294,400 channel
   samples differ, all +/-1 LSB, alpha exact.
2. **Alpha-gradient input.** New deterministic RGBA input with a vertical
   alpha ramp 0..255 (`tools/generate-oracle-rgba-input.py`; decoded RGBA
   SHA-256 `dcee3c8f...`). 178 differing channel samples, all +/-1 LSB;
   the alpha channel is exact everywhere, including fully and partially
   transparent rows.
3. **Odd dimensions (1919x1077).** Same generator, opaque (decoded RGBA
   `58be1365...`). 197 differing channel samples, all +/-1 LSB, alpha
   exact. No row-stride or edge structure: an incorrect stride would shear
   every row after the first, which would produce large structured errors,
   not boundary-value jitter.
4. **4K (3840x2160).** Same generator, opaque (decoded RGBA
   `582719c3...`). 729 of 33,177,600 channel samples differ, all +/-1 LSB,
   alpha exact - the same per-sample rate order as the 1080p samples
   (~2e-5), consistent with the value-dependent rounding jitter verified
   earlier, now at 4x the pixel count.

Generated inputs are identified by their decoded RGBA hash, not the PNG
file hash: the PNG container bytes depend on the local zlib build, so only
the decoded pixel stream is machine-portable (the evidence document
records both).

Claim level: with the four pre-existing samples this makes eight AE-oracle
equivalence samples for ntsc-rs SmartFX at +/-1 LSB (8-bit precision),
now spanning a popup parameter, a float parameter, alpha-carrying input,
odd dimensions, and 4K resolution. Field-dependent rendering with
non-`Both` interlacing choices and deeper-precision comparisons remain
uncovered here (the latter continues in the EXR/deep-transport work).

Tooling shipped with this follow-up: `tools/generate-oracle-rgba-input.py`
(deterministic RGBA oracle inputs; the input formula is documented in the
tool and locked by `tests/test_oracle_input_tools.py`),
`tools/png-to-rgba-raw.py` (lossless PNG-to-raw conversion so host PNG
outputs can feed `compare-pixel-oracles.py --raw`), and the evidence
refresh script named above.

## Follow-up: the 2026-07-19 16 bpc difference does not reproduce;
   replacement captures agree within full-precision tolerance (2026-07-19, issue #53)

The "fresh 16 bpc comparison detects a real unresolved difference" section
above is corrected by this follow-up. Its artifacts
(`target/ntsc-rs-oracle16-v2-*`) no longer exist on this machine, its input
(PNG SHA-256 `29ebf3c2...`) is not reproducible by
`tools/generate-oracle-rgba-input.py` (neither alpha mode yields that hash),
and a controlled re-run detects no such difference. Observations first,
then the corrected reading.

1. **Host determinism across a rebuild (observation).** On a fresh checkout
   of `main` (a283695) with rebuilt workers and harness (broker workspace
   tests pass), `--render-experimental-smart-16-deep` on the original
   gradient input `bb84e35e...` reproduces the earlier session's transport
   sidecar byte-identically (rgba16le SHA-256 `a5bfbb40...`).
2. **AE 16 bpc captures are fps-invariant (observation).** Fresh captures of
   the same input at comp fps 24 and fps 1 (one-frame duration) are
   byte-identical PNGs (SHA-256 `deb77139...`). The fps-1 configuration the
   unreproduced comparison used cannot have changed the AE render.
3. **Full-precision comparison matches (observation).** Host rgba16le
   (white = 32768) against the AE RGBA16 PNG: mean absolute error per RGB
   channel is at most 6.3e-6, the maximum error is 6.0e-5 (2 transport
   codes), and alpha is exact. A second, generator-produced opaque input
   (decoded RGBA `d6f2a543...`) gives the same scale (max 6.1e-5). About
   4.1-5.2 million of 8.29 million channels differ by 1-2 codes; none
   differ more.
4. **The residue is consistent with quantization boundary behavior
   (observation; mechanism evidence not retained).** The host's smart-input world was
   observed as `round(v * 32768 / 255)` of the 8-bit input (checked against
   the `AEXCOMPAT_DUMP_WORLDS_DIR` smart-input snapshot on all 8,294,400
   samples). AE's composed 8-bit-import -> 16 bpc -> `saveFrameToPng`
   chain, measured with a no-effect 16 bpc capture of the same input, is
   `v * 257` with a deterministic 0/-1-step deviation (one value +1) -
   the same downward exporter quantization family already documented for
   32 bpc. Sub-code input differences propagated through the effect's
   filters bound the observed 1-2-code output residue.
5. **The unreproduced difference has the signature of a noise-realization
   mismatch (observation + hypothesis).** Two host renders differing only
   in `Random seed` (0 vs 12345) differ with mean absolute RGB error
   0.010-0.016, maximum 1.0, alpha exact, correlation ~0.99. The
   unreproduced report's figures (mean 0.009-0.025, maximum 1.0, alpha
   exact, correlation 0.973-0.998, blue worst) are the same signature
   class; ntsc-rs's default chroma noise (intensity 0.1) lands on I/Q,
   whose largest RGB coefficient is blue, so a differing noise
   realization degrades blue first. Hypothesis, unverifiable now that the
   artifacts are gone: that comparison's AE and host sides rendered
   different noise realizations (a seed, frame, or parameter mismatch in
   that session), not different depth behavior.

Corrected claim level: the unresolved-16-bpc-gap observation above is
withdrawn as evidence; it is unreproduced and its artifacts are
unavailable. The replacement captures show export-tolerance agreement for
ntsc-rs SmartFX at full 16 bpc transport precision
(tolerance 4/32768, observed residue within 2/32768), frame 0, default
parameters, across two inputs and two comp fps values. They do not prove
which AEX module AE loaded because the old capture result did not record a
loaded-module identity. Therefore this is not yet identity-bound AE-oracle
equivalence. A fresh capture with `-RequireLoadedAexIdentity` is required
before promoting the claim again. The
machine-readable judgments live in
`analysis/NTSC_RS_ORACLE_DEEP16_RESULT_2026-07-19.json` (regenerated only
via `tools/refresh-ntsc-rs-oracle-deep16-evidence.ps1` from the executed
artifacts under `target/oracle-deep16/`; validated by
`tests/test_ntsc_rs_oracle_deep16_result.py`).

Two side findings from the same session, tracked separately:

- The worker's `fill_world8` promotes 8-bit fill colors to ARGB16 with
  `v * 128` (255 -> 32640), while the input-world path uses
  `round(v * 32768 / 255)` (255 -> 32768). Not implicated in this
  comparison (ntsc-rs does not use the fill callback on this path), but
  it is an inconsistent promotion inside one worker.
- `tools/capture-ae-reference.ps1`'s running-session gate checks the
  process names `AfterFX`, `aerender`, and `aerendercore`, but a
  lingering `AfterFX.com` shim (observed orphaned from an earlier
  session) is named `AfterFX.com` and passes the gate. The capture gate
  now also refuses on `AfterFX.com`.

## Follow-up: `-noui` restored for AE 25.3+ captures (2026-07-19, issue #54)

Commit `7085261` removed `-noui` from `tools/capture-ae-reference.ps1`
because AE 25.2 could abort before JSX execution when `-noui` hit a failed
GPU3 sanity state, which made every capture raise the AE GUI (and a hung
JSX left that GUI running indefinitely). Issue #54 asked whether 25.3 still
needs the UI launch. Measurements on this machine (AE 25.3.1x3, the same
install as every capture in this note):

1. **`AfterFX.com -m -noui -r <jsx>` executes the JSX on AE 25.3.1 even in
   the failed-GPU3-sanity state (observation).** This machine is in
   exactly the state that aborted 25.2: every launch prints
   `*** GPU Warning: GPU3 failed (previous) sanity test ***` on the
   wrapper's stderr (alongside an unrelated ORT version notice). A minimal
   marker JSX (write file, `app.quit()`) still completed in 13.9 s with no
   visible window (no process ever exposed a main-window title) and no
   surviving process, recording `app.version 25.3.1x3` and
   `app.availableGPUAccelTypes` raw enum values `1813,1816`. So 25.3.1
   demonstrably tolerates under `-noui` the same GPU3 sanity failure that
   made 25.2 abort before JSX execution. AE 25.2 itself is no longer
   installed here (only its leftover preferences folder), so the 25.2
   abort stands unrefuted for 25.2 and the fix branches on version instead
   of replacing the fallback.
2. **Two full `-noui` captures reproduce the GUI-path reference
   byte-for-byte (observation).** `ntsc-rs`, gradient input `bb84e35e...`,
   frame 0, fps 24, 8 bpc, defaults: both runs produced SHA-256
   `2cd24acf039b61151438e6d4ddd14f1eef1ee28b66591893c6e1cf0309ac16bf`,
   identical to this note's original UI-launch reference. The launch mode
   does not affect the rendered bytes for this configuration.
3. **A `-noui` launch runs entirely inside the `AfterFX.com` wrapper
   process (observation).** Sampling the process table at 200 ms intervals
   during a full capture found only a process literally named
   `AfterFX.com` - Windows keeps the `.com` extension in the process name
   (only `.exe` is stripped), and no `AfterFX`-named process ever existed.
   The previous timeout kill (`Get-Process AfterFX,aerendercore`) and the
   already-running gates (`Get-Process AfterFX,aerender,aerendercore`)
   were both blind to it: a hung `-noui` capture would have left AE
   running, exactly the leftover-GUI failure mode the issue reported for
   the UI path.
4. **The runner returns while AE is still quitting (observation).** The
   result JSON appears before AE finishes shutting down: one run held the
   PNG handle for under a second afterwards (an immediate `Get-FileHash`
   failed with a sharing violation and succeeded on retry), and another
   still showed the `AfterFX.com` process alive right after the runner
   returned; it exited on its own within seconds. The runner therefore now
   waits (bounded by the same `-TimeoutSeconds` knob as the capture
   itself) on the process it launched after the result appears,
   so callers can hash the PNG and start the next capture immediately; a
   quit that outlives the bound is killed and reported as a failure rather
   than returned as success. Both that kill and the no-result timeout kill
   are scoped to the launched process tree by PID (`taskkill /T`), never by
   process name, so an unrelated AE session started mid-capture is never
   touched; a launch that already exited is never killed at all, since its
   numeric PID could have been reused by an unrelated process.

Changes shipped: `capture-ae-reference.ps1` reads the `AfterFX.exe` file
version from the numeric `File*Part` fields (the `FileVersion` string can
carry non-numeric text, and the parts need no ETS-provided
`FileVersionRaw` property; measured on this machine both Windows
PowerShell 5.1 and pwsh 7 do expose `FileVersionRaw`, so the parts are a
robustness choice, not a bug fix) and launches `-m -noui -r` on 25.3+,
keeping the UI launch as the fallback for older versions (the 25.2
GPU3-abort path). Every
already-running gate across the AE oracle tools
(`capture-ae-reference.ps1`, `prepare-ae-oracle-project.ps1`,
`capture-ae-probe-oracle.ps1`, `capture-ae-exr-oracle.ps1`,
`manage-ae-oracle-bundle.ps1`) now includes `AfterFX.com`;
`capture-ae-reference.ps1` waits on and, on timeout, kills only the
process tree it launched (by PID), never processes matched by name, and
`capture-ae-probe-oracle.ps1`'s cleanup no longer kills by name either: it
reports lingering AE-named processes and lets the probe plug-in removal
fail explicitly instead.
`tests/test_ae_reference_capture_automation.py` locks the version branch,
the gate list, the PID-scoped shutdown, and the survival of an unrelated
AE-named process that appears mid-capture.

## Follow-up: evidence hardening after the PR #56 review (2026-07-19,
   issue #61)

The post-merge review of PR #56 found three gaps in the deep16 evidence
chain. Fresh captures were taken with an intermediate hardened runner (all four AE renders came
back byte-identical to the previous session's captures, which doubles as
another determinism observation).

1. **AE-loaded module identity.** The intermediate runner scanned the
   installed plug-in's folder tree and the AE application's own `Plug-ins`
   tree for any other `.aex` containing the effect name bytes (refusing on
   a collision), and re-hashes the installed plug-in after AE exits
   (refusing on change). Both hashes and the scan summary are recorded in
   the capture result; the refresh script fails closed unless every
   capture observed a stable installed hash, a collision-free scan, and the
   expected `effect_match_name`. That does not prove the module AE mapped:
   the scan covers only two roots and races launch. The current runner instead
   locks the installed file against replacement and compares its handle-derived
   final path/file ID with the launched process's loaded module. The committed
   captures predate that stronger proof, so `oracle_identity.state` remains
   `unverified` and exact claims remain disabled until a fresh recapture.
2. **Comparisons are recomputed at refresh time.** The refresh script now
   re-runs `tools/compare-pixel-oracles.py` with the frozen arguments for
   every case and requires the regenerated report to deep-equal the stored
   comparison JSON, so a stored comparison with rewritten hash fields can
   no longer smuggle judgment values into the evidence.
3. **The promotion/export mechanism is bound to local-only artifacts.** The smart-input
   world snapshot (`host-gradient-smart-input.rgba16le`, re-dumped and
   hash-verified during the refresh re-render) and the no-effect 16 bpc
   control capture (`ae-noeffect-16.png`, identity-checked like the effect
   captures) are corpus artifacts now, and
   `tools/verify-deep16-mechanism.py` recomputes both mechanism claims
   from them at refresh time: the host promotion
   `round(v * 32768 / 255)` holds on all 8,294,400 samples, and AE's
   composed import/export map is `v * 257 + d` with `d` in {-1, 0, +1}
   (histogram -1: 126, 0: 129, +1: 1 over all 256 values), deterministic
   and exactly invertible by `round(v16 / 257)`. The earlier section's
   mechanism statements were machine-checked instead of resting only on
   memory. These `target/oracle-deep16` raw artifacts are not committed, so
   a clean clone cannot independently rerun this verification; the evidence
   records `clean_clone_reproducible=false` until they are admitted into a
   portable conformance bundle. The export-map measurement (observation)
   remains separate from the residue attribution (explanation consistent
   with those bounds, not a per-sample proof).

The AE-side captures for this refresh also surfaced an operational
constraint worth recording: launching a capture while the previous AE
instance is still tearing down can hand the `-r` script to the dying
instance, which rejects it ("Attempt was made to run a second script
while another script was already running") and the new capture times out.
Waiting for process exit plus a grace period between captures avoids it.
