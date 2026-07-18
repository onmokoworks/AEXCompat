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
Neither produces a clean equivalence claim yet; the observations and the
concrete blockers are recorded here so the gaps stay explicit.

- **16 bpc: bounded, but dominated by export-conversion conventions.**
  AE 16 bpc capture vs the host's `--render-experimental-smart-16` output
  (both exported as 8-bit PNGs): every difference is again exactly ±1 LSB
  (nothing at ±2 or more, alpha exact), but 882,040 of 2,073,600 pixels
  differ, in mixed directions. The same comparison between AE's own 16 bpc
  and 8 bpc captures differs at 1,393,980 pixels, so the spread is largely
  the down-conversion convention (AE's 0–32768 pipeline vs the host's
  16-bit path, each rounding to 8 bits differently), not effect behavior.
  A meaningful 16 bpc equivalence claim needs both sides to export at
  16-bit precision; the worker-to-broker transport (8-bit RGBA) and the
  capture JSX (`saveFrameToPng` writes 8-bit) both lack that today.
- **32 bpc: not comparable as captured; color management interferes.**
  The AE 32 bpc capture comes back compressed into the 0–63 range
  (mean 14.2 vs host 132.9) and is not explained by any per-pixel
  post-transform of the host output (best fit, sRGB-linearize plus 1/4
  scale, still leaves mean error ~12 LSB). The likely reading is that the
  32 bpc project applies color transforms on the effect's input side, so
  the effect itself renders different data. Hypothesis, unverified: a
  32 bpc project linearizes or reinterprets the working space and
  `saveFrameToPng` exports without the display transform. Before any
  32 bpc comparison, the capture JSX must pin the project color pipeline
  (working space, linearization) explicitly and the result must be
  validated against a no-effect passthrough capture first.

## Tooling changes shipped with this note

- `tools/ae-reference-capture.jsx`: hand-rolled JSON serialization (no
  `JSON` global on AE 25.3) and asynchronous-save polling.
- `tools/capture-ae-reference.ps1`: refuses the silent `AfterFX.exe` no-op;
  substitutes the sibling `AfterFX.com` with a warning when given the `.exe`.
