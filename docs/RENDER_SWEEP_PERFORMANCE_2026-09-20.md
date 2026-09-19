# Windows render sweep performance ledger (2026-09-20)

This is machine-local evidence for the `local-psoft-item11` performance cycle.
It records both accepted behavior and rejected experiments so a later cycle
does not infer safety from the fastest number alone. After Effects, AfterFX,
and aerender were not started.

## Fixed conditions

- Source HEAD before this note: `e9010d005` (`Accelerate SmartFX corpus rendering`).
- Shipping path: `bridges/aviutl2-multifilter/examples/render_sweep.rs`.
- Image: 256x144 ARGB8, time 0, one frame, generated secondary layer where
  declared, one render job.
- PSOFT corpus: 19 installed AEX files below the shipping scan root.
- Accepted baseline worker SHA-256:
  `f9494e5163cb3fd1e993617cc648b70fb817c14cf928141c90e6511b6fd36602`.
- Baseline report:
  `%TEMP%/aexcompat-psoft-known-worker-control-20260920.json`.

## Accepted measurements

The resident-session parameter-update behavioral test completed three real
frames in one session in 0.20 s. The manual 1920x1080 latency comparison used
12 parameter changes:

| path | measured latency |
|---|---:|
| one-shot frame, median | 183.8 ms |
| resident session open | 69.8 ms |
| resident frame, median | 47.5 ms |
| worker portion of resident frame, median | 43.0 ms |

The PSOFT clustered baseline rendered 19/19 effects. Its total was 3,398 ms:
598 ms discovery and 2,800 ms render. The earlier accepted clustered run was
3,316 ms total (610 ms discovery, 2,706 ms render). Both had identical bucket
and pixel SHA results for all 19 effects.

The safe non-vendor milestone report remains:
`%TEMP%/aexcompat-render-clustered-final-nonvendor-nodistort.json`.
It records 74 entries in 24,812 ms: 70 rendered, three AEGP/non-image entries,
and one explicit `DepthONNX.aex` frame error `-6`. Its worker fingerprint is the
accepted baseline hash above. Sapphire, Maxon/Red Giant/Trapcode, Boris FX, and
DistortChroma were excluded from execution, not from the larger inventory.

## Clean-build discriminator

Two PSOFT effects (`P_Texture.aex` and `P_BlurCelLayer.aex`) exited 22 after an
incremental native rebuild, while the accepted worker rendered both. A fresh
Ninja Release directory from the same source and MSVC 19.50 rendered both in
focused shipping sweeps. Fresh worker SHA-256 was
`936953504a33d92461fe52fcd642e6c0f488255fb049eccc2cccce5df7fe5edf`.

The failure did not reproduce in a fresh build and is therefore consistent
with an incremental-build-directory-dependent problem, not evidence of a
source or VS 2026 compatibility regression. The exact cause, including whether
a stale object was responsible, remains unidentified. Native performance
experiments must use a fresh build directory; copying an older known-good
worker back after each experiment remains the safe shipping rollback.

## Rejected module-audit cache

Profiling attributed about 50 ms to each record-only pre-unload audit and about
51 ms to each post-load audit in a PSOFT cluster swap. Caching canonical paths
for every loaded HMODULE produced the attractive result below:

| experiment | total | discovery | render | pixel/bucket differences |
|---|---:|---:|---:|---:|
| accepted baseline | 3,398 ms | 598 ms | 2,800 ms | 0 |
| all-module cache | 2,693 ms | 559 ms | 2,134 ms | 0 |

The all-module result is **rejected**. WorkerSession retains AEX images, but a
plug-in may load and unload arbitrary dependency DLLs. Windows can reuse a base
address/HMODULE and loader path, so a cached canonical or reparse result can
describe an earlier module lifetime. Record-only provenance would become
inaccurate even though enforced audit bypassed the cache.

Restricting the cache to the worker and explicitly retained AEX images removed
that lifetime ambiguity, but also removed the speedup: 3,683 ms total, 661 ms
discovery, 3,022 ms render (19/19 rendered). That implementation and its test
were fully reverted; the worktree returned to the accepted product code and
the shipping worker returned to the accepted `f9494e...` binary.

## Next safe performance boundary

A later optimization must avoid treating HMODULE plus path as a module-lifetime
identity. Plausible bounded directions are an unload-aware loader notification
index, or a snapshot-difference design with an independently verified lifetime
identity. Merely caching canonical paths or reparse classifications for every
observed module is not acceptable. Before another native optimization, retain
the validation ladder used here: focused behavioral self-test, clean Release
worker build, the two PSOFT regressions, one 19-effect PSOFT milestone, and
bucket/pixel SHA comparison.

## Full shipping-scan inventory

The shipping `scan_for_diagnostics` path found 984 AEX files after configured
ignore processing. An inventory-only pass hashed every candidate without
loading an AEX or starting `aex_worker`, After Effects, or aerender:

| classification | count |
|---|---:|
| external blocked (Sapphire) | 292 |
| external blocked (Trapcode) | 3 |
| unexecuted | 689 |
| total | 984 |

The pass took 17,937 ms. Every row has a 64-character canonical-path identity,
file SHA-256 and size, scan-root-relative identity, final stage, execution and
failure classification, and the boundary-verified CLI/worker build fingerprint.
The requested render conditions are recorded once as report-wide conditions.
Bucket counts sum to the record count. The report is
`%TEMP%/aexcompat-inventory-all-2026-09-20.json`; absolute scan paths remain
omitted. `--blocked-path` retains matching candidates in the denominator and
labels them instead of silently excluding them.

## Current 984-record reconciliation and clean-build recovery

After the OLM semantic cycles, the current shipping scan still found exactly
984 AEX files across the two default roots. The load-free inventory report is
`%TEMP%/aexcompat-inventory-fresh-worker-current-20260920.json`. It records 570
in-scope records and 414 external-blocked records: 292 Sapphire files plus 122
Maxon/Red Giant/Trapcode/Magic Bullet records. The blocked selectors are saved
in the report, and blocked rows remain in the 984-record denominator. The
inventory completed in 485 ms with CLI SHA-256
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`
and worker SHA-256
`2b00eb2793abcfce3cec7d9647bf09133c47c1a7507b813a456dcd1e71fbdac3`,
matching the render milestone below.

The reconciliation found one record present in the inventory but absent from
the MediaCore-root cohort: `LogiPlugin.aex` (SHA-256
`561b2971d2bf2350646b3496274a9bd9ae453b5034395f1ebba367649fa33f13`)
under the newest After Effects Plug-ins scan root. A default two-root shipping
probe classified it as `plugin_kind=aegp`, category `General Plugin`, with zero
parameters and no image-render route. The report is
`%TEMP%/aexcompat-logiplugin-focused-20260920.json`; it is now the third
evidence-backed non-image record rather than an unmeasured image failure.

The first current-fingerprint render milestone exposed a build-artifact
problem. Report
`%TEMP%/aexcompat-render-current-nonmaxon-nonsapphire-20260920.json` took
928,797 ms and recorded 425 rendered, seven transparent, three AEGP, and 135
`session_close_failed` rows. Every close failure exited 22 after producing a
decoded frame. Of those failures, 127 shared the main Continuum dependency
closure; the remaining eight occupied five smaller closures. A focused
`Beauty Studio.aex` close report showed a valid frame but one live output world
(`worlds_created=1`, `worlds_disposed=0`), which failed the worker's terminal
world-lifetime invariant.

This matched the already-recorded incremental-build discriminator. A fresh
Release build from the same source, isolated at
`target/minihost-build-fresh-boris-close-20260920`, produced worker SHA-256
`2b00eb2793abcfce3cec7d9647bf09133c47c1a7507b813a456dcd1e71fbdac3`.
The same `Beauty Studio.aex` then rendered with the identical pixel SHA-256,
one created and one disposed world, zero live worlds, and a clean session.
One representative from each of the six failing dependency closures also
rendered cleanly: `Primatte Studio`, `RefractionDispersion`, `FastBokeh`,
`P_Texture`, `P_BlurCelLayer`, and `onmk/DistortChroma`. The fresh worker was
then placed at the canonical shipping development path
`target/minihost-build/aex_worker.exe`; the replaced binary is retained in
`%TEMP%` under its full `68816d...` hash. Native stdout routing passed 2 tests
with 16 unrelated fixture-dependent skips, and broker render-session tests
passed 18/18.

The single post-recovery milestone report is
`%TEMP%/aexcompat-render-fresh-worker-nonmaxon-nonsapphire-20260920.json`:

| result | count |
|---|---:|
| rendered, nontransparent execution probes | 558 |
| rendered but fully transparent | 9 |
| non-image AEGP | 3 |
| session-open / frame / close / worker failures | 0 |
| executed in-scope records | 570 |
| external blocked, not executed | 414 |
| inventory total | 984 |

The merged per-record ledger is
`%TEMP%/aexcompat-ledger-fresh-worker-20260920.json` (1,802,696 bytes,
SHA-256
`26cc4ad9d5466711fad05325dfbb3793a57c81040c2e077aaa348dd10d01acf1`).
It contains exactly 984 records and preserves every inventory row, including
blocked and non-image entries. Each row records the relative AEX identity,
path hash, plug-in hash and size, host/worker build fingerprint, render
conditions reference, final stage, execution and failure classification,
plug-in kind, and—when executed—decoded-output size/hash/alpha validation,
session state, and worker exit. Its bucket counts are 558 rendered, nine
rendered-transparent, three AEGP, and 414 external-blocked, which sum to the
inventory total and use the same CLI/worker fingerprint as the milestone.

Discovery took 17,566 ms and the complete 570-record run took 724,866 ms. All
570 identities and AEX hashes match the failed milestone. The fresh worker
changed 133 close failures to `rendered` and two to `rendered_transparent`.
Only four pixel SHA-256 values differed. `ONMK_ParticleLab` and `ParticleKit`
returned exactly to their earlier accepted hashes. `DeBlock` matched its prior
accepted hash in the full run but produced the other observed hash in a later
fresh process. `signal` produced different hashes across prior, failed-worker,
fresh-milestone, and focused-process runs. Both pass same-run fresh-session
determinism checks, so these two remain recorded as process-dependent outputs,
not evidence that the clean rebuild changed their effect semantics.

The nine transparent execution probes are `BCCCartoonLight`,
`BCCLinearLumaKey`, `BCCMotionBlur`, `BCCParticleEmitter`, `BCCRadiantEdges`,
`Composite`, `Linear Luma Key`, `ColorKeep`, and `DistanceGradation`.
`ColorKeep` and `DistanceGradation` already have effect-appropriate semantic
fixtures below. The seven Boris defaults were subsequently resolved with
effect-appropriate inputs and parameters: Cartoon Light responds to line width,
BCC Linear Luma Key and Linear Luma Key move their alpha boundaries with the
threshold, Particle Emitter responds to time and birthrate, Radiant Edges tracks
the source boundary, and Composite follows a spatial graded matte. BCC Motion
Blur additionally receives explicit Host/Source layers and previous/next timed
samples; amount zero is input-identical while amounts two and eight change
increasing numbers of pixels only around the moving box's trajectory, with the
far background unchanged. At the same amount, static previous/current/next
samples produce the transparent default, proving that the timed samples affect
the result. The corresponding eight test modules passed
55/55 against the installed AEX files and the fingerprinted Release worker.
`test_bcc_motion_blur_response.py` and `test_linear_luma_key_response.py` add
the previously missing behavioral coverage, including validators that reject
fixed, empty, corrupt, and parameter-insensitive outputs. These results resolve
the seven transparent defaults as semantic successes for the tested behaviors,
not as exact AE pixel equivalence or exhaustive effect coverage. Likewise, the
other 558 rows are transport/decode successes, not blanket proof that every
default applied its intended effect; previously recorded demo overlays and
input-equal defaults remain separate semantic work.

One focused command intended for `onmk/DistortChroma.aex` used a basename
substring filter against both default roots and also executed Sapphire's
`S_DistortChroma.aex` once. This violated the execution exclusion even though
it did not start After Effects or change licensing. The corrected probe was
restricted to the exact `onmk` folder, and subsequent commands retain explicit
Sapphire/Maxon exclusions or exact cohort roots.

## Boris Continuum cohort

Shipping discovery inspected all 496 Continuum AEX files successfully in
13,408 ms. All share one dependency closure; 490 advertise the supported
SmartFX route. A focused layerless pair showed the resident-process benefit:
`BCCWoodPlanks.aex` fell from 1,687 ms in its own session to 505 ms after a
same-process plug-in swap, with identical bucket and pixel SHA-256.

The fresh 570-record milestone placed 94 effects in `BCC Transitions`; 93
returned the input-identical default hash because the generic probe did not
select manual progress. A focused inspection of all 94 succeeded in 42,031 ms
and grouped them into ten parameter signatures. The largest compatible
signature contains 42 effects with slot 2 `Layer to Reveal`, slot 3
`Animation = Pct. Done`, and slot 4 `Percent Done`. Report
`%TEMP%/aexcompat-bcc-transition-slot2-semantic-20260920.json` records 42/42
passing in 285,250 ms: percent zero equals the patterned source byte-for-byte,
percent 100 equals a distinct reveal image byte-for-byte, and percent 50 is a
valid non-endpoint image with nonzero alpha at every pixel. The report is
52,875 bytes with SHA-256
`46bc629112949df9938351b68c5236d0f2f89d04561ab38e554d263f3c70abfc`,
harness SHA-256
`b4b264a11629e7542231ebae73477b31cbbe62e09b2fba7ddf9bdfa47a36a26b`,
and worker SHA-256
`2b00eb2793abcfce3cec7d9647bf09133c47c1a7507b813a456dcd1e71fbdac3`.
`test_bcc_transition_slot2_response.py` preserves the exact 42-file cohort,
parameter contract, endpoints, midpoint response, and corruption-rejecting
validator. Its first full run passed 40 real effects and exposed an overstrict
opaque-alpha assertion on the two RGB Displacement variants; those variants
produce nonzero but slightly reduced midpoint alpha. After correcting the
oracle, the validator plus both affected installed AEX tests passed 8/8. No
full-corpus sweep was repeated. A focused 50-percent-only follow-up rendered
all 42 again in 89,672 ms and found no constant midpoint: the minimum was 137
distinct RGBA values in the two Lens Flash variants. The durable validator
therefore rejects a constant midpoint as well as fixed endpoints, transparency,
and truncation. This establishes the tested transition state change, not exact
midpoint parity with After Effects.

The next compatible signature contains 33 effects with slot 7
`Animation = Manual Pct Done`, slot 8 `Layer to Reveal`, and slot 9
`Percent Done`. A single focused 0/50/100 run completed 33/33 in 233,599 ms
with no session, frame, decode, empty-image, or alpha failures. Every effect
produced three distinct pixel hashes, a nonconstant midpoint containing pixels
from neither endpoint, a zero-percent result closer to the patterned source
than the reveal, and a 100-percent result closer to the reveal than the source.
Some animated transitions intentionally retain blur, glow, or particle tails
at an endpoint, so the shared contract uses endpoint proximity rather than
false byte-exact parity. `Atmospheric Glow Dissolve` explicitly selects its
documented `Style = Blend` mode; that mode produced byte-exact endpoints and a
21,269-color midpoint. A repeated `Smoke Wipe` zero-percent render had an
identical pixel SHA-256, establishing that its sparse residual is reproducible.
Separate visual inspection showed only faint blue particle-like pixels and no
text or logo watermark; this is not a general license-state assertion. The
report is
`%TEMP%/aexcompat-bcc-transition-slot8-semantic-20260920.json` with SHA-256
`2de31fab63e2296eb74ee18b68ec3cb447f85d848158b5aa35f7fc0e3ed95d1c`.
`test_bcc_transition_slot8_response.py` preserves the 33-file parameter and
image-response contract and includes mutations for wrong endpoints, fixed or
constant midpoints, transparency, and truncation. This is behavioral evidence
for the tested manual transition path, not exhaustive parameter coverage or
exact After Effects pixel equivalence.

Fourteen additional multi-layer wipe effects form seven AE/Premiere pairs.
They share slot 2 `Background`, slot 3 `Animation`, and slot 4 `Percent Done`,
but advertise four additional effect-specific layer inputs. The four
two-choice wipe pairs use `Animation = 2`; the three `Manual / Auto /
Pct. Done` pairs use `Animation = 3`. The focused installed-AEX test passed
14/14 in 94.18 seconds. Every zero-percent output matched the patterned source
byte-for-byte, every 100-percent output matched the gradient background
byte-for-byte, and every midpoint satisfied the existing nonconstant,
non-endpoint, all-pixels-nonzero-alpha effect validator. The test also asserts
that each
effect really exposes multiple layers, so it cannot silently collapse back to
the earlier single-secondary-layer cohort.
`test_bcc_transition_slot2_multilayer_response.py` preserves the exact cohort,
choice/value mapping, parameter receipt, and image response. This establishes
the tested first-background transition path; it does not claim behavior for
the four optional effect-specific layers.

The `BCCBurntFilm` AE/Premiere pair uses a distinct contract: slot 2
`Animation = Pct. Done` (value 3), slot 3 `Percent Done`, and slot 5
`Layer to Reveal`, followed by four optional Burn/Flare/Char inputs. Both
installed effects passed the shared transition oracle: exact source and reveal
endpoints, three distinct states, a nonconstant effect midpoint, and nonzero
alpha at every pixel. The pair plus the seven corruption mutations passed 9/9
in 13.98 seconds. `test_bcc_burnt_film_transition_response.py` preserves the
five-layer parameter layout, requested values, and image response while making
no claim about behavior of the four optional layer inputs.

`Prism Dissolve` uses slot 7 `Animation = Manual Pct Done`, slot 8
`Layer to Reveal`, and slot 9 `Percent Done`. Its effect processing remains
visible at the endpoints, so it uses the proximity-based transition oracle:
zero percent is closer to the patterned source, 100 percent is closer to the
gradient reveal, and the 7,431-color midpoint contains non-endpoint effect
pixels. All three states have distinct hashes and nonzero alpha at every pixel.
The installed effect plus the seven corruption mutations passed 8/8 in 7.79
seconds. `test_prism_dissolve_response.py` preserves the parameter receipt and
image response without claiming byte-exact endpoints or After Effects parity.

`Displacement Dissolve` uses slot 8 `Animation = Manual Pct Done`, slot 9
`Layer to Reveal`, and slot 10 `Percent Done`. Its inspected slot 12 default is
the built-in `Displacement Map`, so no external map behavior is claimed. The
installed effect produced byte-exact source and reveal endpoints and a
10,373-color displacement midpoint. The effect plus the seven corruption
mutations passed 8/8 in 8.52 seconds.
`test_displacement_dissolve_response.py` preserves the built-in-map contract,
requested values, and image response. The existing `Flutter Cut` five-frame
cut/reversal test was also rerun against the same Release worker and passed
10/10 including its nine mutations in 11.62 seconds.

Together, the 42 slot-2 transitions, 33 slot-8 transitions, 14 multi-layer
wipes, two Burnt Film variants, Prism Dissolve, Displacement Dissolve, and
Flutter Cut account for all 94 effects in the baseline `BCC Transitions`
category. Each now has a durable installed-AEX behavioral test for its selected
manual transition path. This closes the category's previously input-identical
generic-default ambiguity; it does not assert exhaustive parameter coverage or
pixel equivalence with After Effects. No full-corpus sweep was repeated while
developing these focused contracts.

The next input-identical-default cohort comes from the baseline `BCC Obsolete`
category. Inspection of its 73 input-equal rows completed in 33,796 ms and
found 53 complete parameter signatures. Grouping only by the leading operation
contract identified 24 legacy transition effects (12 AE/Premiere pairs) with
slot 2 `Layer to Reveal`, slot 3 `Animation = Pct. Done`, and slot 4
`Percent Done`; later controls remain effect-specific. All 24 installed effects
passed the byte-exact source/reveal endpoint and nonconstant midpoint oracle in
163.22 seconds, and the shared validator's seven corruption mutations passed
7/7 separately. `test_bcc_obsolete_transition_response.py` preserves the exact
cohort, parameter receipt, and image response. This resolves those 24 neutral
defaults for their selected manual transition path, not the remaining 49
input-equal `BCC Obsolete` effects or every effect-specific control.

A second legacy cohort contains ten Linear, Radial, Rectangular, Textured, and
Vignette wipe effects (five AE/Premiere pairs). They share slot 2 `Background`,
slot 3 `Animation = Pct. Done` (value 3), and slot 4 `Percent Done`. All ten
installed effects passed in 67.14 seconds with byte-exact source/reveal
endpoints and a nonconstant, non-endpoint midpoint; the shared validator's
seven corruption mutations also passed separately. The durable coverage is in
`test_bcc_obsolete_wipe_response.py`. This reduces the unresolved input-equal
`BCC Obsolete` set from 49 to 39 while leaving every effect-specific wipe
control outside the stated contract.

The legacy `BCCSwishPan` AE/Premiere pair has a third operation layout: slot 2
`Animation = Pct. Done`, slot 3 `Percent Done`, and slot 4 `Layer to Reveal`.
Both installed effects passed the byte-exact endpoint and nonconstant midpoint
oracle. The pair plus the seven corruption mutations passed 9/9 in 14.14
seconds. `test_bcc_obsolete_swish_pan_response.py` preserves this layout and
image response, reducing the unresolved input-equal `BCC Obsolete` set from 39
to 37.

The legacy `BCCLensTransition` AE/Premiere pair was exercised with its explicit
`Zoom In` type, slot 8 reveal layer, slot 9 percent-driven animation, and slot
10 progress. Both effects produced byte-exact endpoints and a 14,838-color
midpoint that differed from a plain 50/50 crossfade at all 36,864 pixels. The
validator also requires a dark outer frame around a nonblack center and a
large-magnitude difference from the plain blend. The pair plus fixed,
transparent, crossfade-only, one-pixel-noise, and uniform-color-transform
mutations passed 7/7 in 15.40 seconds.
`test_bcc_obsolete_lens_transition_response.py` verifies the type and progress
receipts as well as the spatial image response, reducing the unresolved
input-equal `BCC Obsolete` set from 37 to 35.

The legacy `BCCFlutterCut` AE/Premiere pair is time-driven rather than manually
percent-driven. With the saved 300-frame duration and the inspected two-frame
incoming/outgoing defaults, frames 0, 146, 148, 150, 152, 154, and 299 produced
the exact state sequence source, reveal, source, reveal, source, reveal,
reveal. `test_bcc_obsolete_flutter_cut_response.py` preserves that bounded
timeline and rejects source-only, reveal-only, monotonic, blended, corrupt,
truncated, missing, and reordered sequences. The pair plus eight mutations
passed 10/10 in 30.39 seconds, reducing the unresolved input-equal
`BCC Obsolete` set from 35 to 33. This is evidence for the saved timing
conditions, not a complete After Effects timing-equivalence claim.

Four remaining spatial blur effects required the inspected slot 6 `Host Layer`
as well as their effect-specific control. An initial Directional Blur probe
without that layer failed closed in Smart Pre-Render with a `missing_world`
checkout; the existing Directional/Gaussian behavioral test already documents
and supplies the required layer, so those effects were not duplicated. The new
cohort covers Fast Lens Blur and Lens Blur `Iris Scale`, Radial Blur `Blur
Amount`, and Spiral Blur's typed `Spin Angle`. For every effect, control zero
returned the patterned source byte-for-byte while the active value changed
4,768 to 8,981 localized pixels into grayscale intermediates, retained black
corners, and kept alpha nonzero. At least 95% of each effect's changed pixels
remain within 24 pixels of the original rectangle boundary, with intermediate
values on both its inside and outside. A remote-gradient mutation ensures that
unrelated localized changes do not satisfy the oracle. The four installed
effects plus eight corruption mutations passed 12/12 in 20.75 seconds.
`test_bcc_obsolete_spatial_blur_response.py` preserves the Host Layer,
numeric/angle receipts, and spatial response, reducing the unresolved
input-equal `BCC Obsolete` set from 33 to 29.

The existing `test_static_blur_response.py` already covers three other members
of that same input-equal baseline set: Directional Blur, Fast Blur, and
Gaussian Blur. It was rerun against the same Release worker and installed AEX
files; the three zero/active responses plus six corruption mutations passed
9/9 in 14.96 seconds. Accounting for this previously durable coverage reduces
the unresolved input-equal `BCC Obsolete` set from 29 to 26 without duplicating
the tests.

Three legacy glow effects form the next coherent semantic cohort. Fast Film
Glow and Film Glow use slot 10 `Glow Intensity`; Rough Glow uses slot 18
`Glow Opacity Scale`. All three also require the inspected slot 6 `Host Layer`.
For every effect, contribution zero returned the patterned source byte-for-byte,
while the installed default contribution kept every pixel of the white interior
unchanged and added a nonconstant halo outside it. The validator requires at
least 1,000 lit outside pixels, at least 500 intermediate-intensity outside
pixels, at least eight distinct outside colors, 95% of the lit pixels within
24 pixels of the original rectangle, no light beyond 32 pixels, opaque output,
and black corners. Its synthetic valid case uses additive light rather than
ordinary blur, and seven mutations cover copy, transparency, interior
darkening, remote light, whole-image uniform output, a constant-width and
constant-color ring, and truncation. The three installed effects plus those
mutations passed 10/10 in 15.61 seconds after the Release rebuild. The
parameter descriptors and applied
value receipts are checked by `test_bcc_obsolete_glow_response.py`. This
reduces the unresolved input-equal `BCC Obsolete` set from 26 to 23 without
claiming exact glow-kernel or After Effects pixel parity.

The legacy Glare and Glint pair was then exercised on a 32x32 white emitter.
An initial 6x6 emitter remained byte-identical even with an explicitly assigned
background-map layer and maximum-brightness controls; enlarging only the
emitter exposed the effect, and removing that optional layer left the resulting
pixels unchanged. The accepted minimal path therefore uses only the primary
input, threshold zero, brightness 300, scale 3, four rays, and explicit 0- and
45-degree angles; Glare's ring is disabled to isolate the shared ray contract.
`Mix With Original = 100` returns the emitter source byte-for-byte. At zero mix,
both effects produce opaque, multi-level ray images spanning at least 95 pixels
in each dimension, with at least 5,000 lit pixels. The 0-degree image has at
least 1.5 times as much axial as diagonal mean energy in the 30-to-60-pixel
annulus, and changing the angle changes at least 4,000 pixels. Eight mutations
reject copying, an ignored angle, isotropic blur, transparency, localized or
uniform output, truncation, and a wrong neutral frame. A ninth mutation dims
the zero-degree rays without changing their direction; the 90-degree-periodic
directional-energy profile must move its dominant direction by at least 20
degrees, so that false response is also rejected. The two installed AEX cases
plus those mutations passed 11/11 in 16.71 seconds in
`test_bcc_obsolete_glare_glint_response.py`, reducing the unresolved
input-equal `BCC Obsolete` set from 23 to 21. This is a bounded ray-control
contract, not exact ray-kernel or After Effects pixel parity.

The legacy Fast Film Process and Film Process pair shares slot 6 `Host Layer`
and slot 14 `Brightness`. On a 0-to-255 horizontal grayscale ramp, values -50,
0, and +50 produced mean levels 71.625, 127.5, and 183.375 in both installed
effects; zero returned the input byte-for-byte. The validator requires opaque
grayscale output, the per-pixel order dark <= source <= bright, strict changes
on at least three quarters of the pixels in each direction, monotonic rows,
identical behavior across rows, at least 64 output levels, and mean movement of
at least 40 levels in each direction. Nine mutations reject copied dark or
bright frames, transparency, color contamination, nonmonotonic and spatially
varying transforms, reversed direction, a wrong neutral frame, and truncation.
The two installed effects plus those mutations passed 11/11 in 15.29 seconds
in `test_bcc_obsolete_film_process_response.py`, reducing the unresolved
input-equal `BCC Obsolete` set from 21 to 19. This verifies the shared
brightness control, not every film-processing control or After Effects pixel
parity.

The remaining legacy `BCCBlur.aex` is already the installed target of
`test_bcc_blur_response.py`: its inspected slots 4 and 5 are the test's
`Horizontal Blur` and `Vertical Blur` controls. Rerunning that test with this
exact AEX path exercised zero, radius 2, radius 20, and independent horizontal
and vertical blur. The installed case and ten corruption mutations passed
11/11 in 12.16 seconds against the same Release worker. Accounting for this
existing behavioral coverage reduces the unresolved input-equal `BCC Obsolete`
set from 19 to 18 without adding a duplicate test.

Legacy `BCCMosaic.aex` requires its inspected slot 6 `Host Layer` before the
pixelation controls produce their semantic output. On a per-pixel color ramp,
slot 9/10 `Pixelate X/Y = 0` returned the input byte-for-byte, while value 25
reduced 36,864 input colors to 5,184 and produced 26,496 equal horizontal and
18,432 equal vertical neighbor pairs. The validator requires opaque,
nonconstant output with at most one quarter as many colors as pixels, at least
two-thirds equal horizontal neighbors, and at least half equal vertical
neighbors. The ramp also binds output to input location: every output red and
green value remains within four and two levels of its pixel's x and y
coordinate, respectively (the installed maxima were three and one). Ten
mutations reject copying, blur, uniform output, transparency, horizontal-only
or vertical-only blocking, RGB inversion, horizontal mirroring, a wrong neutral
frame, and truncation. The installed AEX plus those mutations passed 11/11 in
5.86 seconds in `test_bcc_obsolete_mosaic_response.py`, reducing the unresolved
input-equal `BCC Obsolete` set from 18 to 17. This is a block-pixelation
contract, not exact sampling-grid or After Effects pixel parity.

Legacy `BCCSafeColors.aex` likewise requires inspected slot 6 `Host Layer`.
On a fully saturated horizontal hue ramp, inspected slots 8/9 `Saturation
Soft Clip/Hard Clip = 100/100` returned the input byte-for-byte, while `0/50`
changed all 36,864 pixels and reduced mean RGB chroma from 255.0 to 127.5.
The validator requires an opaque, row-preserving result with at least 200
distinct colors, per-pixel chroma between 120 and 135, and overlap between the
source and result maximum/minimum channel sets so a hue rotation cannot satisfy
the saturation contract. Mean Rec.709 luminance was preserved at 127.5008, and
the validator requires mean luminance drift at most 1 plus mean absolute
per-pixel drift at most 15 so simple dimming cannot masquerade as lower
saturation. Ten mutations cover a copied source, grayscale or uniform output,
missing saturation reduction, simple dimming, hue shift, lost alpha, spatial
corruption, a wrong neutral frame, and truncation. The installed AEX plus those
mutations passed
11/11 in 5.81 seconds in
`test_bcc_obsolete_safe_colors_response.py`, reducing the unresolved
input-equal `BCC Obsolete` set from 17 to 16. This establishes saturation-limit
behavior, not exact After Effects color-management or pixel parity.

Legacy `BCCSpillRemover.aex` requires inspected slot 6 `Host Layer`. A four-band
green/red/blue/gray input rendered byte-for-byte at slot 17 `Amount = 0`. With
slot 10 `Screen Type = Green`, slot 14 `Spill Ratio = 50`, and `Amount = 100`,
only the green-dominant band changed, from `(80, 200, 80)` to `(80, 80, 80)`;
the red, blue, and gray bands and all alpha values remained unchanged. The
validator requires removal of green dominance across exactly the green quarter,
preservation of its red/blue channels, byte-exact preservation of all non-green
bands, and row/spatial stability. Eleven mutations cover a copied or uniform
result, retained or over-removed green, red/blue collateral damage, a changed
non-green band, lost alpha, spatial corruption, a wrong neutral frame, and
truncation. The installed AEX plus those mutations passed 12/12 in 5.57 seconds
in `test_bcc_obsolete_spill_remover_response.py`, reducing the unresolved
input-equal `BCC Obsolete` set from 16 to 15. This establishes selective green
spill suppression for the tested controls, not exact After Effects pixel parity.

Legacy `BCCMagicSharp.aex` requires inspected slot 6 `Host Layer`, and its main
slot 8 `Sharpen Amount` only produced a response after the inspected slot 13
`Fine Pass` was enabled. With the other detail passes, range tuning, and grit
disabled, a grayscale soft edge returned byte-for-byte at `Sharpen Amount = 0`.
At slots 8/14 `Sharpen Amount/Fine Sharpen = 3000`, the result changed 14 pixels
per row only near the edge, increased maximum adjacent gradient from 8 to 255,
and produced local 0/255 undershoot/overshoot while preserving flat fields,
grayscale channels, alpha, and identical rows. The validator requires that
localized two-sided edge response rather than exact filter samples. Ten
mutations cover a copied or weak response, nonlocal damage, color or alpha
damage, row/spatial corruption, a shifted response, one-sided clipping, a wrong
neutral frame, and truncation. The installed AEX plus those mutations passed
11/11 in 5.75 seconds in `test_bcc_obsolete_magic_sharp_response.py`, reducing
the unresolved input-equal `BCC Obsolete` set from 15 to 14. This establishes a
strong fine-pass sharpening response, not exact After Effects pixel parity.

Legacy `BCCEdgeCleaner.aex` requires inspected slot 6 `Host Layer`. On a
four-step jagged 0/255 alpha edge, slot 8 `Cleaning Radius = 0` preserved alpha
byte-for-byte. At radius 20 with temporal smoothing and alpha contrast disabled,
the result changed 7,716 edge-local pixels to intermediate alpha values across
x=70..133 while retaining transparent and opaque far fields. Each row changed
52..55 pixels, total alpha changed by only 0.2%, and the 50% edge-position
standard deviation fell from 4.47 to 3.52 while four input positions were
interpolated into 13 output positions. Adjacent-row midpoint differences fell
from mean/max 5.96/12 to 0.084/1. The validator checks local feathering,
alpha-mass preservation, per-row extent, and both distributional and spatial
reduction of boundary jaggedness. Eleven mutations cover a copied or uniform
alpha, nonlocal transparent/opaque damage, row corruption, an unsmoothed
midpoint, alternating low/high midpoint row reordering, alpha-mass shift, a
wrong neutral frame, and truncation. The installed AEX plus those mutations
passed 12/12 in 5.66
seconds in `test_bcc_obsolete_edge_cleaner_response.py`, reducing the unresolved
input-equal `BCC Obsolete` set from 14 to 13. This establishes alpha-edge
cleaning for the tested controls, not exact After Effects pixel parity.

`BCCDegrain.aex` remains semantically unresolved and is not counted as a
successful effect response. A deterministic grayscale-noise probe with both
inspected slot 6 `Host Layer` and slot 13 `Sample Layer` assigned confirmed that
slot 22 `Mix with Original = 100` is byte-exact identity. With mix 0, slot 18
`Filter Strength = 100`, and slot 19 `HiPass Filter = 10`, thresholds 20 and 40
were also identity, while threshold 60 changed 20,081 pixels but shifted mean
from 127.95 to 112.03 and increased variance from 351.81 to 1324.11. Enabling
slot 11 `Lock Sample` worsened that result to mean 76.91 and variance 1611.99.
An earlier threshold-100/HiPass-0 probe collapsed the image to mean 0.54 rather
than producing a valid denoise response. The shipping harness command contract
offers one parameterized render per process; it does not currently expose a
same-session parameter-changing `Setup: Select Sample` then `Setup: Normal`
sequence. The temporary probe was removed after the bounded replan. Resolving
this item requires either a resident preparation/render sequence or evidence
that the plug-in can initialize its sample model during a normal shipping
render; identity, near-black, and increased-noise outputs remain failures. The
unresolved input-equal `BCC Obsolete` count therefore remains 13.

Legacy `BCCDVFixer.aex` requires inspected slot 6 `Host Layer`. On repeating
two-pixel red/blue chroma blocks, slot 14 `Mix with Original = 100` returned the
input byte-for-byte. With slot 9 `Threshold = 0`, slot 11 `Iterations = 20`, and
mix 0, all 36,864 pixels changed while red and blue horizontal adjacent-pixel
variation within a row fell from 19,812 to 32. Red/blue channel means remained
142, green remained exactly
64, alpha and identical rows were preserved, and 15 output colors retained the
red-dominant first edge and blue-dominant last edge instead of collapsing to a
constant. Preview probes independently produced all-black at threshold 100 and
all-white at threshold 0, confirming the threshold direction used by the
normal-output test. Eleven mutations cover a copied, constant, or high-variation
result, grayscale/brightness/color-direction corruption, lost alpha, spatial or
green-channel damage, a wrong neutral frame, and truncation. The installed AEX
plus those mutations passed 12/12 in 5.56 seconds in
`test_bcc_obsolete_dv_fixer_response.py`, reducing the unresolved input-equal
`BCC Obsolete` set from 13 to 12. This establishes strong chroma-block smoothing
for the tested controls, not exact DV reconstruction or After Effects parity.

Legacy `BCCPrism.aex` operates directly on the primary input rather than an
inspected Host Layer parameter. On vertical grayscale bars, slot 17 `Mix with
Original = 100` returned the input byte-for-byte. With inspected start/end points
`(30,50)`/`(70,50)`, depths `0.5`/`2`, slot 10 `Prism Amount = 100`, reflect
outside pattern, and mix 0, 35,072 pixels changed and 33,280 became chromatic.
At the center column the red/green/blue profiles had 15/7/5 threshold crossings,
each retained range 20..230 and mean approximately 125, and pairwise channel
differences covered at least 128 of 144 rows. Horizontal variation within each
row remained at most one level; the top color ordered red > green > blue and the
bottom ordered red < green < blue, establishing directed spectral separation.
Eleven mutations cover copied, grayscale, uniform, row-damaged, transparent,
channel-swapped, vertically reversed, low-dynamic, mean-shifted, wrong-neutral,
and truncated outputs. The installed AEX plus those mutations passed 12/12 in
5.75 seconds in `test_bcc_obsolete_prism_response.py`, reducing the unresolved
input-equal `BCC Obsolete` set from 12 to 11. This establishes the tested prism
dispersion response, not exact optical or After Effects pixel parity.

The first attempt to extend clustering to effects with a secondary layer used
equal first-layer slots as the boundary. A 39-row interrupted milestone exposed
two transient `PF_Err_INTERNAL_STRUCT_DAMAGED` results on the second member of
two clusters even though focused single runs rendered. That unsafeguarded form
was rejected. The accepted form abandons a cluster on every non-`rendered`
member and replays all of its members through the established one-plug-in path.
Pixel-determinism mode remains available across the optimized path, so a
clustered result can be compared with a fresh single session.
A compiled SmartFX fixture now renders A with the shipping dynamic-layer
transport, updates that layer, swaps to B, and compares every output byte with
a fresh B session using the updated image. It also asserts the exact
layer-derived pixels before and after the update. Verification elapsed time is
included in each clustered row's `elapsed_ms`.

The final report is
`%TEMP%/aexcompat-boris-render-all-safe-layer-cluster-2026-09-20.json`:

| result | value |
|---|---:|
| rendered / total | 496 / 496 |
| invalid or empty | 0 |
| distinct pixel SHA-256 values | 192 |
| accepted clustered rows | 211 |
| single or safe-fallback rows | 285 |
| discovery | 12,828 ms |
| total | 680,757 ms |

The run used the saved 256x144 ARGB8, time 0, one-frame conditions with the
first declared secondary layer when present. The report has a complete boundary
fingerprint and the accepted worker SHA-256 `f9494e5163cb3fd1e993617cc648b70f`
`b817c14cf928141c90e6511b6fd36602`. No After Effects process was used. The
post-change PSOFT regression rendered 19/19 in 3,427 ms with zero bucket or
pixel-SHA differences from the known-worker control.

## MediaCore root cohort

The files directly under the MediaCore scan root were measured separately from
vendor subdirectories. `RGSGrowBounds.aex` remains external-blocked with the
Maxon/Red Giant family and was not loaded. Shipping discovery classified the
remaining 27 records as 25 image effects and two AEGPs
(`AeTimelineSyncAEGP.aex` and `nexpression.aex`).

The final report is
`%TEMP%/aexcompat-mediacore-root-render-2026-09-20.json`. Under the saved
256x144 ARGB8, time 0, one-frame conditions it records:

| result | value |
|---|---:|
| rendered image effects | 25 / 25 |
| non-image AEGP | 2 |
| invalid or empty images | 0 |
| expanded-output images | 2 |
| output SHA equal to the solid input | 14 |
| total | 27 |
| elapsed | 23,597 ms |

The two expanded frames are `ONMK_ParticleLab.aex` (2054x1942, origin
-899/-899) and `ParticleKit.aex` (1878x1766, origin -811/-811); both carry
non-empty pixel SHA-256 evidence. The 14 unchanged default renders remain
explicitly **semantically unverified**. Several are plausibly correct no-op
defaults (zero-strength transforms, time effects at time zero, or analysis
effects), but this execution probe does not prove that. They must receive a
parameter/input response test or an AE reference before counting as verified
effect semantics. The report's build fingerprint is complete and uses the
accepted `f9494e...` worker.

## OLM cohort and visible-image classification

The first OLM milestone reported 10/10 `rendered` in 2,631 ms, but that bucket
only proved positive geometry and a non-empty byte buffer. Raw-frame inspection
showed that `ColorKeep.aex` and `DistanceGradation.aex` had zero alpha in every
pixel; counting either as a usable image violated the sweep's acceptance
condition. `ColorKeep` retained RGB `(32,64,128)` under zero alpha, while
`DistanceGradation` was all-zero. `OLMKiraKira.aex` produced a visible 63-color
gradient. The other seven effects returned the opaque solid input under their
defaults and remain semantically unverified no-op candidates.

The sweep now records `nonzero_alpha_pixels` and `invalid_alpha_pixels` for
every rendered frame and classifies a positive-size frame with no visible alpha
as `rendered_transparent`, not `rendered`. Float alpha must be finite and
non-negative; invalid values receive `rendered_invalid_alpha`. A clean SmartFX
transparent result is replayed once through Classic only as comparison evidence
when close evidence proves the Smart selector ran without an error, the session
was not invalidated, and the worker exited normally. The Classic comparison
never replaces the Smart result: transparency alone cannot prove which route
matches AE semantics. `DistanceGradation` has a visible Classic comparison
(36,864 nonzero-alpha pixels, white with alpha 50), while `ColorKeep` is fully
transparent on both routes. At that point both remained explicitly unresolved.

The corrected milestone report is
`%TEMP%/aexcompat-olm-render-visible-final-2026-09-20.json`:

| result | value |
|---|---:|
| visible rendered | 8 / 10 |
| fully transparent | 2 |
| visible Classic comparison (not counted as success) | 1 |
| default outputs equal to the opaque solid input | 7 |
| elapsed | 3,936 ms |

Focused before/after evidence is in
`%TEMP%/aexcompat-olm-DistanceGradation-smart-close-2026-09-20.json`,
`%TEMP%/aexcompat-olm-DistanceGradation-classic-2026-09-20.json`, and
`%TEMP%/aexcompat-olm-DistanceGradation-transparent-comparison-2026-09-20.json`.
The corrected report has a complete fingerprint: CLI SHA-256
`73c42eb09624819ba009ce36a859ea5c43d764e26a01dcef88a7762bbbcf5404`
and the unchanged accepted worker SHA-256
`f9494e5163cb3fd1e993617cc648b70fb817c14cf928141c90e6511b6fd36602`.

`DistanceGradation` is now resolved by a semantic-input probe rather than a
host change. The vendor describes the effect as generating a gradation from an
alpha-channel border
(`https://www.olm.co.jp/post/distance-gradation`); the original fully opaque
solid has no internal alpha border and was therefore not a valid success
oracle. A 256x144 input with a transparent left half and opaque right half
(PNG SHA-256
`d115d69549310799f0b08097a72fc0f98759c056f21def8fb1a3cc85d375f2b5`)
produced a clean Smart render in 271 ms. The output SHA-256 is
`4aef35ff5d76d27c8f1473f7e458ad869c1b68ef9278b32e072b641505ca7d58`;
18,288 pixels have nonzero alpha, invalid alpha is zero, and the output carries
128 total alpha values; each of the 127 nonzero values occupies exactly one
144-pixel column in a gradient away from the vertical boundary. The report is
`%TEMP%/aexcompat-olm-DistanceGradation-alpha-edge-smart-2026-09-20.json`,
with CLI fingerprint
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`
and worker fingerprint
`94071433e24859e52a340bd94a758deaec9c7a442d59d05c9d6ed6c717d6f88d`.
The Classic comparison also produced visible pixels but still crashed during
session close; that diagnostic-only fallback issue does not invalidate the
clean shipping Smart route and remains recorded separately. No After Effects
process was used.

`ColorKeep` is also resolved by a semantic-input probe. The vendor describes
the effect as retaining only selected flat colors
(`https://www.olm.co.jp/post/color-keep`). Shipping discovery records one
enabled default keep color, opaque black; the original `(32,64,128)` solid did
not contain it, so full transparency was the expected response. A 256x144
fully opaque input with a black left half and red right half (PNG SHA-256
`88b5713342cdfcb391d842140fd2dbf426de3e8d00b229cf424b407a02345040`)
produced a clean Smart render in 191 ms. The 18,432 left-half pixels are exactly
`RGBA(0,0,0,255)` and the 18,432 right-half pixels are exactly
`RGBA(255,0,0,0)`: the selected black is retained and the unselected red is
made transparent without discarding its RGB. The output SHA-256 is
`2d77561232b362877b496d1e76a32c0bd7caf53da80fbcd35e8ff321aee26e6f`;
invalid alpha is zero and the worker/session are clean. The report is
`%TEMP%/aexcompat-olm-ColorKeep-flat-colors-smart-2026-09-20.json`, with the
same CLI `cc96567e...` and worker `94071433...` fingerprints as the
DistanceGradation semantic probe. The two previously transparent records are
therefore valid effect responses under effect-appropriate inputs, not image
render failures. The two opaque input-equal OLM defaults (`OLMSmoother2` and
`OLMToonDilate`) are also resolved below with effect-appropriate inputs. On the verified shipping Smart route and these
effect-appropriate inputs, no OLM native execution or visibility failure
remains. The separate `DistanceGradation` Classic close crash described above
is still unresolved; Classic is diagnostic-only for this result. No After
Effects process was used.

`OLMBlur` is resolved by the opted-in real-AEX behavioral fixture in
`tests/test_render_fixture_semantic_response.py`. The fixture rendered the
same structured 256x144 ARGB8 image through the shipping Smart path at time 0
with `Blur Amount` 1 and 20. Both outputs remained fully opaque and nonconstant,
all 36,864 pixels changed between the two parameter values, and horizontal red
edge energy fell from 22.4666 to 0.0635 (ratio 0.00283). The low/high raw-frame
SHA-256 values are respectively
`efb5279c9a452932f8b8cfb46e381066b3db33e9b9046daa59005c407d7f6c22`
and `e62d497d49bde705c14a04fe3b02c1abbf502ebaf78bb948cf1bade6139389e9`.
The input PNG SHA-256 is
`018b20de0910a6327933dbd3cd27d2a832e7ad0587467ad48ed627a430b9082c`;
the installed AEX SHA-256 is
`f0611785e7b14ac4fcfc75f23b8862beb4539eee52d25d472556849535e96e5b`.
The Release harness and worker SHA-256 values are respectively
`b4b264a11629e7542231ebae73477b31cbbe62e09b2fba7ddf9bdfa47a36a26b`
and `94071433e24859e52a340bd94a758deaec9c7a442d59d05c9d6ed6c717d6f88d`.
The focused real-AEX test passed in 1.08 s; its negative behavioral cases reject
no-op, constant, truncated, and transparent false positives. No After Effects
process was used.

`OLMColorKey` is resolved by another opted-in real-AEX behavioral fixture in
the same test module. This matches the vendor's multi-color keying description
(`https://www.olm.co.jp/post/olm-color-key`). The fixture used an opaque
two-color 256x144 ARGB8 image, selected the left-half color in `Color 1`, and
compared `Use Color 1` off and on through the shipping Smart path at time 0.
The disabled output exactly matched the input; enabling the key changed exactly
18,432 pixels, making the selected half transparent while preserving its RGB
and leaving all 18,432 unselected pixels byte-identical and opaque. The input
PNG SHA-256 is
`1878f55a9bc0f9eabefb8c3b86ce7c8f181157fc6e173a3bda3c3c466b11108b`;
the disabled/keyed raw-frame SHA-256 values are respectively
`b8d4180a9208ba2950b9409b29808861da0735b1815f706181057d2330477aad`
and `12775b7253f65ea10217f0fccd2df29b34181daee78b211e31182dd4935680c6`.
The installed AEX SHA-256 is
`9c6cca226a52d35ce7833fcc4c0f914f6b15b3abe0202e0957ba97ba3bb2cf2c`;
the Release harness/worker fingerprints remain `b4b264a1...` / `94071433...`.
The real-AEX case passed in 1.36 s and the full focused module passed 14 tests
with two unrelated opt-in cases skipped in 1.42 s. Its negative behavioral
cases reject no-op, all-transparent, wrong-color, and truncated false positives.
No After Effects process was used.

`OLMDirectionalBlur` is resolved by a shipping Smart fixture that changes the
front `Blur Strength` from its no-op default 0 to 20. This matches the vendor's
directional, detail-preserving description
(`https://www.olm.co.jp/post/olm-directional-blur`). On the same structured
256x144 ARGB8 input at time 0, strength 0 was byte-identical to the input while
strength 20 changed 36,387 pixels and reduced horizontal red edge energy from
27.2314 to 8.9952 (ratio 0.3303), without collapsing the nonconstant image.
The input PNG SHA-256 is
`018b20de0910a6327933dbd3cd27d2a832e7ad0587467ad48ed627a430b9082c`;
the strength 0/20 raw-frame SHA-256 values are respectively
`a7e790d7bb7cc3a220d96bea96c112dca0bc284a256333221f44e9ba243a5379`
and `d406eb0e81c1b03628dbdd5ce4d25a878bbdc265254da3c80ac0ca666b7770e8`.
The installed AEX SHA-256 is
`d3e5e4079a759d521dc7457ebf998487fe43b00f182a1e2f910b187936b6c06e`;
the Release harness/worker fingerprints remain `b4b264a1...` / `94071433...`.
An initial exact-opaque assertion was rejected because 81 processed pixels
round alpha from 255 to 254; the accepted check requires valid near-opaque
alpha (254-255), a nonconstant image, and the measured directional edge-energy
reduction. Its four negative cases reject no-op, constant, transparent, and
truncated false positives. The real-AEX case passed in 1.10 s and the focused
module passed 18 tests with three unrelated opt-in cases skipped in 1.17 s. No
After Effects process was used.

`OLMRadialBlur` is resolved by the corresponding real-AEX Smart fixture. This
matches the vendor's radial zoom/rotation blur description
(`https://www.olm.co.jp/post/olm-radial-blur`). With the default Zoom mode and
center, changing outer `Strength` from 0 to 20 on a 256x144 ARGB8 image with a
16x16 colored marker on an otherwise identical opaque-black field changed
1,472 pixels. Strength 0 was byte-identical to the input. At strength 20, 416
source-black background pixels became colored inside bounds x=30..64,
y=62..81, while 36,192 other source-black pixels remained exactly black. Thus
identical input pixels produce both affected and unaffected outputs according
to their neighborhood; a spatially independent pixel transform cannot satisfy
the fixture. Alpha remains valid and near-opaque (810 pixels at 254, 36,054 at
255). The input PNG SHA-256 is
`1ebfb9fd330b0f079e06996128dc6b0bb64aac21fe5fc14efbaea3ad4a9272fd`;
the strength 0/20 raw-frame SHA-256 values are respectively
`7b511db8308ba3b98466c5bc6ef560beab1082459343eb4a16e6b7764ab2d828`
and `452a6d78bfe9d464f83aa89c4ad277218b5ee1f2aed75f85f4ca4aaababe74fa`.
The installed AEX SHA-256 is
`ffbb1d0109671e3ea9b1a12cd1126f2c72f965197577a57cc602fb096414ccdb`;
the Release harness/worker fingerprints remain `b4b264a1...` / `94071433...`.
The fixture rejects no-op, constant, transparent, uniform per-pixel
color-transform, and truncated false positives. The marker-based real-AEX case
passed in 1.46 s. No After Effects process was used.

`OLMSmoother` is resolved by a Classic real-AEX fixture using a binary stair-step
edge, matching the vendor's MLAA-like jagged-line smoothing description
(`https://www.olm.co.jp/post/olm-smoother`). At the discovered default `Do Smooth
Range` value 6, the effect changed only 287 of 36,864 pixels to intermediate
values along the diagonal boundary while preserving 18,217 source-black pixels
and 18,360 source-white pixels exactly; the other 143 black and 144 white
boundary pixels became intermediate values. The input PNG
SHA-256 is
`b3e64f490d115a24544ff7785265ad5e50d1d78d449c91551a9b4843c61de417`;
the raw ARGB8 output SHA-256 is
`e5aafb3817ae32f40cf27e407127d6cff94a1451438e270a28ade6972ed274a7`.
The fixture's six mutations reject no-op, constant, transparent, whole-region
gray, sparse single-pixel, and truncated false positives. Requested range values 0 and 6 were both
recorded correctly but produced the same output on this binary edge, so a range
difference is not claimed as semantic evidence.

The same plug-in originally produced a valid frame but exited with
`0xC0000005` after `GLOBAL_SETDOWN`, terminal quiescence, and module-audit
completion, before the final close report. The worker now restores the protocol
fd and emits the Classic/Smart completion JSON without touching the global
`std::cout` state after plug-in execution. The final shipping sweep report
`%TEMP%/aexcompat-olm-smoother-final-review-clean-20260920.json` records
`rendered` in 170 ms (363 ms including discovery), a clean session, worker exit
0, 147,456 decoded
bytes, 36,864 nonzero-alpha pixels, and zero invalid-alpha pixels. Its output
pixel SHA-256 is
`64a2111a6142d4793d4a43310155483c9e145ae65678433924341faa77d64df1`;
the installed AEX, CLI, and worker SHA-256 values are respectively
`6206f601b645dc915b78269ae403e5cbee642ac2812e320d85838ec72135fe82`,
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`,
and `68816d7a5963f4864a1ed0f0787592d6afcd2241d3c2bbb46f9b0e9e5bd3308f`.
No After Effects process was used.

`OLMSmoother2` is resolved by the same binary stair-step semantic fixture on
the shipping Smart route. Its discovered defaults are `Smoothness=100`,
`Smooth Range=2`, and `Smoother Version=v2`. The effect changed 286 of 36,864
pixels, kept alpha opaque, produced 286 grayscale intermediate pixels (858 RGB
components), and preserved the remaining flat interiors. The decoded input and output SHA-256
values are respectively
`ebc35a2f579a4653e01fa6ba3d6170331cb83eb1e2d69bf52ace5c8146bd9f6e`
and `4fa3870911332403f6c9866b8fe1fae1ef5808ef8a42934d7d1f413064c55c7d`;
the final artifact exactly matches the output checkpoint. The focused real-AEX
behavioral test passed in 0.77 s. The shipping report
`%TEMP%/aexcompat-olm-smoother2-semantic-20260920.json` records `rendered` in
172 ms (364 ms including discovery), a clean session, worker exit 0, 147,456
decoded bytes, 36,864 nonzero-alpha pixels, and zero invalid-alpha pixels. Its
shipping-boundary pixel SHA-256 is
`5976f5ba06d46bf4c640730acc825278cb3d2ae1dfd67c82fac231d5b5b2a725`.
The installed AEX, CLI, and worker SHA-256 values are respectively
`7d42c00fe382304ea8a2b9d72af4f3a55f18b6fc03f6174786c97d7618b744c7`,
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`,
and `68816d7a5963f4864a1ed0f0787592d6afcd2241d3c2bbb46f9b0e9e5bd3308f`.
No After Effects process was used.

`OLMToonDilate` is resolved with a transparent-background fixture containing a
64x32 opaque rectangle, matching the vendor's closest-color dilation semantics
(`https://www.olm.co.jp/post/olm-toon-dilate`). At the discovered default
`Search Radius=2`, the output preserves the original rectangle and expands it
exactly two pixels on every side. All 400 changed pixels copy the source ARGB
value `[255, 32, 64, 128]` without blending; distant transparent pixels remain
unchanged. The decoded input and output SHA-256 values are respectively
`cf11f06bfecfbc5231926b9559558d69c13705403fe2fe7ac0f7233be91c912e`
and `415ef2679e8cd5138ce50d1a8006079e9f9d88cc14c38c938aa11d7d00529c53`;
the final artifact exactly matches the output checkpoint. Four negative
mutations plus the focused real-AEX case passed in 0.86 s. The shipping report
`%TEMP%/aexcompat-olm-toon-dilate-semantic-20260920.json` records `rendered` in
170 ms (644 ms including discovery), a clean session, worker exit 0, 147,456
decoded bytes, 2,448 nonzero-alpha pixels, and zero invalid-alpha pixels. Its
shipping-boundary pixel SHA-256 is
`afc3d5b6f6e126c8e35b2d7239320e923e9c878aa9600b136264cae59426e005`.
The installed AEX, CLI, and worker SHA-256 values are respectively
`c05db8c118029ff3216d3cae8e6423e2eb41ca8f56de2fb3668db81b9b8c32b3`,
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`,
and `68816d7a5963f4864a1ed0f0787592d6afcd2241d3c2bbb46f9b0e9e5bd3308f`.
The first combined semantic-module run saw one transient parameter-inspection
error for this AEX; an immediate focused retry passed, followed by a clean full
module retry (35 passed, 5 skipped in 2.90 s). The failure did not reproduce and
is retained here rather than counted as a render failure. No After Effects
process was used.

## ONMK subdirectory cohort

The eight AEX files in the `onmk` MediaCore subdirectory were measured under
the saved 256x144 ARGB8, time 0, one-frame conditions. The final report is
`%TEMP%/aexcompat-onmk-render-all-2026-09-20.json`:

| result | value |
|---|---:|
| visible rendered | 8 / 8 |
| transparent / invalid alpha | 0 |
| default outputs equal to the opaque solid input | 5 |
| elapsed | 2,589 ms |

All eight rows have 36,864 nonzero-alpha pixels and a complete build
fingerprint (CLI `73c42eb09624819ba009ce36a859ea5c43d764e26a01dcef88a7762bbbcf5404`,
worker `f9494e5163cb3fd1e993617cc648b70fb817c14cf928141c90e6511b6fd36602`).
Focused raw-frame inspection found no common warning or license image:
`DistortChroma.aex` returned opaque `(32,63,128)` instead of the input
`(32,64,128)`; `RioGradeRust.aex` returned a 500-color opaque gradient;
`UltraGlow.aex` returned opaque black. These are observed output differences,
not proof that the intended effects were applied. All eight defaults remain
semantically unverified without a parameter-response test or AE reference. No
After Effects process was used.

## Rowbyte cohort

The six AEX files in the Rowbyte MediaCore subdirectory all completed native
render and RGBA decode under the saved 256x144 ARGB8, time 0, one-frame
conditions. The report is
`%TEMP%/aexcompat-rowbyte-render-all-2026-09-20.json`:

| execution result | value |
|---|---:|
| rendered buffers | 6 / 6 |
| transparent / invalid alpha | 0 |
| explicit DEMO-watermarked outputs | 4 |
| elapsed | 1,372 ms |

Raw-frame inspection is required for the final classification. `BadTV_x64`,
`DataGlitch_x64`, `DotPixels64`, and `SepRGB_x64` visibly contain a red diagonal
cross and `DEMO`; they are therefore recorded as external license-blocked, not
as successful production images. `SepRGB_x64` also expands to 258x146 at origin
(-1,-1), with 36,864 nonzero-alpha pixels inside that larger frame.
`TVPixel64` produces an opaque cyan pixel-grid pattern without the watermark;
`FastBokeh` returns the opaque input unchanged at its default. Those two remain
semantically unverified, as do the intended effects beneath the four demo
overlays. The report fingerprint is complete and matches CLI
`73c42eb09624819ba009ce36a859ea5c43d764e26a01dcef88a7762bbbcf5404`
and worker
`f9494e5163cb3fd1e993617cc648b70fb817c14cf928141c90e6511b6fd36602`.
No After Effects process was used.

## Zaebects cohort

The Zaebects subdirectory contains one installed image effect, `signal.aex`.
Its single focused run is also the complete cohort milestone, so the same AEX
was not swept twice. `%TEMP%/aexcompat-zaebects-render-all-2026-09-20.json`
records 1/1 visible render in 461 ms, with 36,864 nonzero-alpha pixels, no
transparent or invalid-alpha result, and the complete accepted CLI/worker
fingerprint. Raw-frame inspection shows an opaque scanline/waveform pattern
instead of a warning or license image. This proves native execution and decoded
image output, but not effect semantics without an AE reference. No After
Effects process was used.

## DepthAnythingV2 cohort

The DepthAnythingV2 subdirectory contains one installed image effect. Its
single focused run is also the complete cohort milestone. The report
`%TEMP%/aexcompat-depthanythingv2-render-all-2026-09-20.json` records 1/1
visible render in 1,465 ms with 36,864 nonzero-alpha pixels, no frame/session/
worker failure, and the complete accepted fingerprint. It rendered without a
manual runtime-folder choice. Raw-frame inspection shows an opaque 255-level
grayscale image, dark through the center and bright at the top and bottom,
rather than a warning or license image. That is inference-shaped output, but
the model selection/load path was not independently verified and effect
semantics remain unverified without an AE reference for the same solid input.
No After Effects process was used.

## DepthONNX cohort

The DepthONNX subdirectory contains one installed image effect,
`DepthONNX.aex` (SHA-256
`a6a2e3e60797e08af1f0db177f4ab8df9a58b6c445329412b8627ea52f98408c`).
Its initial Smart run returned `frame_error:-6`. The old forced-Classic path
instead reported `rendered`, but byte inspection found that the output was the
allocation's unchanged `0xCC` fill, not a frame produced by the plug-in. That
was a host false positive.

Classic output storage is now seeded immediately before `RENDER` with a
non-uniform canary and compared with its packed pre-render snapshot during
finalization. Exact equality is reported as `frame_error:-6` at
`classic_finalize`; a legitimate uniform frame, including all `0xCC`, remains
valid. Host-populated `PF_OutFlag_NOP_RENDER` passthrough is explicitly exempt.
The compiled behavioral self-test covers all three cases.

The post-fix focused report is
`%TEMP%/aexcompat-depthonnx-classic-canary-final-2026-09-20.json`:

| result | value |
|---|---:|
| installed image effects | 1 |
| valid rendered images | 0 |
| explicit frame errors | 1 (`-6`, `classic_finalize`) |
| worker/session termination | clean |
| plug-in row elapsed | 273 ms |
| complete report elapsed | 461 ms |

The plug-in's Classic `RENDER` selector returned zero, but the final payload
still exactly matched the canary (SHA-256
`626e48b2a4b79fde239a0f7045333dfe6e294bee8ef4a42cd9efb195d987f8f1`).
The report records the saved 256x144 ARGB8, time 0, one-frame conditions and a
complete fingerprint: CLI
`73c42eb09624819ba009ce36a859ea5c43d764e26a01dcef88a7762bbbcf5404`
and worker
`a243fb31ec9103cad8485082724a5693c12929f948580ef57d8e5d771c82dba7`.

The installed directory, `C:/Program Files/Adobe/Adobe After Effects
2026/Support Files/Plug-ins/Effects/DepthONNX`, contains both required ONNX
Runtime DLLs and a `models/depth_anything_v2_small/manifest.json`, but none of
the three ONNX files named by that manifest. An exact-filename search under the
user profile also found no copy. Strings embedded in the installed AEX include
`no models found; add packs under MediaCore/DepthONNX/models or use Browse
Model Folder`, consistent with a separately supplied model pack. A cached
public project listing identified three export commands for these same manifest
filenames, but its source URL returned 404 during the final evidence check and
is not treated as durable proof.

A compatible 99,060,839-byte ONNX graph was then obtained for a bounded local
probe (SHA-256
`afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c`).
Its inspected contract is IR 9 / opset 14, input `pixel_values` float32
`[batch,3,height,width]`, output `predicted_depth` float32 with dynamic spatial
dimensions, and no external tensor data. Parameter discovery found the model
pack and all three resolution choices. Nevertheless, both Smart and Classic
continued to return `-6`, and the worker never observed ONNX Runtime loaded.
The earlier `effect_sequence_data` result 516 was isolated to `PARAMS_SETUP`;
temporarily returning success with a null value removed that diagnostic without
changing the render failure, so it is not the render blocker.

The discriminating experiment set `ORT_DYLIB_PATH` to the already-admitted
`onnxruntime.dll` beside the AEX. The same Smart render immediately changed
from `frame_error:-6` to a valid rendered frame. The cause was therefore the
dynamic `ort` loader's path selection, not the graph, model discovery, or the
host's DLL search-directory admission. Shipping dispatch now checks only the
already-resolved dependency roots (maximum 16, no recursive tree scan). If
exactly one `onnxruntime.dll` is present and the caller/inherited environment
has not made an explicit choice, its canonical path is passed to that worker
as `ORT_DYLIB_PATH`. Distinct candidates remain ambiguous and are not guessed.
The single-plugin, clustered, one-shot, and resident-session dispatches share
this behavior.

The post-fix installed-AEX report is
`%TEMP%/aexcompat-depthonnx-installed-auto-ort-final-2026-09-20.json`. It was
run with no `ORT_DYLIB_PATH`; only the temporary model-pack root was supplied
because the installed package lacks weights:

| result | value |
|---|---:|
| installed image effects | 1 |
| valid rendered images | 1 |
| plug-in row elapsed | 1,177 ms |
| decoded bytes | 147,456 (256x144 RGBA8) |
| nonzero / invalid alpha pixels | 36,864 / 0 |
| output SHA-256 | `253db088889f5edc2e915f7b73734f5aac6921dc1a9011946f55bb8ff341d821` |
| worker / session | `ok` / clean |

The raw frame is not the solid input or a warning/canary image: it is an opaque
grayscale depth-shaped image with a dark horizontal center and smooth brighter
upper/lower regions. The accepted report fingerprints are CLI
`cc96567e93c05c633d5a94c67d1ee5444b0032656e31d5ba3687691e85f8ac3b`
and worker
`94071433e24859e52a340bd94a758deaec9c7a442d59d05c9d6ed6c717d6f88d`.
The automatic runtime-DLL compatibility failure is resolved. The installed
package's absent model weights remain
`external_blocked:missing_model_assets`; the local probe model is evidence,
not a claim that AEXCompat ships that third-party asset. No After Effects
process was used.

The current-corpus reconciliation supersedes that external state. The two
registered shipping roots now contain six matching weights: three under
`MediaCore/DepthAnythingV2/model` and three under
`MediaCore/models/depth_anything_v2_small`. No model-path environment override
was present. The current full shipping run resolved those roots without a
manual folder choice and rendered the installed `DepthONNX.aex` in 1,076 ms:
147,456 decoded bytes, 36,864 nonzero-alpha pixels, zero invalid-alpha pixels,
clean session, worker exit 0, and output SHA-256
`0a2790a19134fba12c809edf952e4831002416f5c044b39c19fb4bad0f30f377`.
The missing-model external block is therefore no longer current. This proves
automatic model/runtime resolution and nonempty native output, but the solid
input result remains semantically unverified against an AE reference or an
effect-appropriate depth fixture.
