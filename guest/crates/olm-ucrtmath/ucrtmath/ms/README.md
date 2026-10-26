# OLM-internal UCRT math

The double sin/cos/exp implementation is documented in [README-double.md](README-double.md).

This directory is **OLM-internal**. It is derived from the behavior and
read-only disassembly of ucrtbase 10.0.28000.2705 for OLM interoperability.
It **must not be published standalone**, including as part of a standalone
ucrtmath distribution. The separately attributed AMD files remain in `../src`.
No Microsoft executable or instruction listing is distributed here.

The implementation is independently written C organized around range reduction,
polynomial evaluation, and IEEE special-value handling. It exports `ucrt_sinf`,
`ucrt_cosf`, and `ucrt_log2f`. The build uses native arm64, `-ffp-contract=off`,
`-fno-fast-math`, and explicit double `fma` where the reference fuses operations.
The contract is raw binary32 return bits in round-to-nearest, gradual underflow,
with mode `UCRT_NOFMA` or `UCRT_FMA`. errno, floating-point exception flags,
alternate rounding modes, and host/plugin integration are outside this ticket.

Validation completed: each of the three functions matches all **4294967296**
input words in **both modes**, with **zero mismatches**. The 106167-input
targeted corpus, stream rejection checks, and existing F28 targeted regressions
also pass. See `results/` and the F42 report for identities and exact counts.

## Algorithm observations (facts from the pinned image)

Sine and cosine use double intermediates. Direct sine has tiny identity, cubic,
and degree-nine branches; non-FMA cosine uses its degree-ten polynomial even
for tiny inputs, while FMA cosine has identity/quadratic branches. The non-FMA
polynomials group pairs of coefficients using x² and x⁴; FMA evaluates the
coefficient polynomial with fused Horner steps and a fused final addition.
The cosine leading term is 1-x²/2. The sine input sign and the reduction
quadrant determine the output sign; cosine uses the quadrant alone.

Moderate positive angles use nearest-integer quadrant selection and split π/2
subtraction. The non-FMA path refines cancellation exceeding 15 exponent bits
and selects a small residual approximation using the exponent of the first
residual. The FMA path fuses quadrant selection and the leading subtraction.
Its sine and cosine thresholds differ, as do the two non-FMA thresholds.

Large angles multiply a 53-bit significand by a three-limb window of 2/π,
select a quadrant and signed fractional remainder, and **truncate** the
fraction to double before multiplying by π/2. A rounding-up quadrant uses
the one's complement of the fractional product, as observed in the reference.
The independently structured limb extractor implements this fixed-point
operation without reproducing register-by-register instruction sequencing.
Cosine FMA also retains a second truncated double and uses a compensated
split-π/2 product. Sine FMA and the non-FMA paths use the single-double product.

Log2 uses 16 intervals, normalizes the mantissa around a bit-defined offset,
and evaluates a fourth-degree correction in double. Its AVX path contains
**no fused multiply-adds**; both modes consequently share the same C code.
Positive subnormals are scaled by 2²³ with a corresponding exponent correction.
Zero returns negative infinity; negative domain inputs return negative quiet
NaN; input NaNs preserve sign/payload and are quieted. Trig infinities return
negative quiet NaN, and trig input NaNs also preserve sign/payload and quiet.

## Constants and operation provenance

All addresses below are RVAs in the pinned DLL, SHA256
`385a6d73a50063a4c760bdb2ee59e575de7ff350a2e3b214ea491d08358484f3`.
Hex float literals retain the exact observed double values. Trivial powers of
two, integer masks, and IEEE representation constants are written directly.

| Constants / decision | Source RVA |
| --- | --- |
| Sine coefficients, x³ through x⁹ | `10f910`, `10f918`, `10f920`, `10f928` |
| Cosine coefficients, x² through x¹⁰ | `10f8e0`, `10f8e8`, `10f8f0`, `10f8f8`, `10f900` |
| Cubic 1/6 | `1108c0` (also `10ba90`) |
| 2/π, leading π/2, trailing π/2 | `1108a0`, `1108b0`, `1108d0` (cosine duplicates `10ba80`, `10bab0`, `10bac0`) |
| Refined π/2 tail parts | `1108e0`, `1108f0` (cosine duplicates `10bad0`, `10bae0`) |
| Fixed-point 2/π window | `10bde0 + 119` through `+157`; identical bytes in `1103b0` and `110570`; last two zero bytes are padding |
| Full π/2 multiplier | `110920` (also `110390`, `1103a0`, `110610`) |
| Compensated product high-part mask | `110620`: `fffffffff8000000` |
| Compensated π/2 split | `110630`, `110640`, `110650` |
| Direct trig boundary, identity/cubic thresholds | `bbc98`, `bbca3`, `bbcaa`; FMA cosine `b7be2`, `b7bed`, `b7bf4` |
| Moderate-reduction upper bounds | sine SSE `bbd79`, FMA `bc095`; cosine SSE `b7965`, FMA `b7c95` |
| Cancellation and residual exponent decisions | `bbdfa`, `bbe28`, `bbe35` (cosine `b79e6`, `b7a14`, `b7a21`) |
| Fixed-point window/rounding/truncation | sine `bafc0` and `bc100`; cosine `bae60` and `bb380` |
| Log2 offset `3f330000`, interval width, exponent adjustment | `6f01e`–`6f061`, SSE `6f0e0`–`6f10c` |
| Log2 reciprocal/log pairs, 16 rows | `107c40 + 16*i`, `107c48 + 16*i`, i=0..15 |
| Log2 linear, quadratic, quartic magnitudes | `107c28`, `107c30`, `107c38` |
| Log2 cubic coefficient | `107d60` |
| Subnormal scale 2²³ and exponent correction | `118f60`, `f9165`, `f9199` |
| Log2 negative-domain/NaN arithmetic | `e82dc` |

`check.py` reuses the F28 arm64 producer and qualified Rosetta PE comparator.
It runs at most two mathematical processes and streams 4096 records at a time.
The comparator checks both mode masks, full raw return words, and the exact
input count; the driver checks both process exits. Full runs enumerate every
unsigned 32-bit input, including signed zeros, infinities, and all NaN payloads.
No tolerance, output substitution, per-input patch, or plugin integration is used.

```sh
mkdir -p orchestra/state/F42-ucrtmath-ms-float
python3 ucrtmath/ms/check.py > orchestra/state/F42-ucrtmath-ms-float/targeted-summary.log 2>&1
python3 ucrtmath/ms/check.py --full > orchestra/state/F42-ucrtmath-ms-float/full-summary.log 2>&1
```

After the comparisons, run `python3 ucrtmath/ms/check_harness.py` to check
stream rejection and the existing F28 targeted regressions, then
`python3 ucrtmath/ms/retain.py` to retain the numerical evidence and identities.

The default target corpus is 100,000 seeded random words plus ±128 words around
12 branch/special-value boundaries, both signs, deduplicated. Exhaustive results
and build/source identity are retained in `results/`; see the F42 report for
actual counts and validation status. The exhaustive result, not the algorithm
observations alone, is the numerical justification for the retained constants.

Follow-up tickets must add the worker import shim (F30-style) and then the Mac
call sites (F31-style), with AEX boundary and plugin regression evidence.

Double `ucrt_pow` is documented separately in [pow.md](pow.md), including both
mode algorithms, table provenance, boundary corpus, and qualification commands.
