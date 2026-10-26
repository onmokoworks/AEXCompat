# OLM-internal double pow

`pow.c` is an independently structured C interoperability reimplementation of
ucrtbase 10.0.28000.2705's double `pow`. The F42 policy applies: OLM-internal,
derived from behavior and read-only disassembly, **never publish standalone**.
No Microsoft executable or instruction listing is included. This is separate
from the AMD-derived float implementation and from the rejected F43 experiments.

Contract: binary64 return bits, round-to-nearest/ties-to-even, gradual underflow,
`UCRT_NOFMA` / `UCRT_FMA`; compile with `-ffp-contract=off -fno-fast-math`.
Explicit `fma` is used only for the reference's fused operations. errno, exception
flags, alternate rounding modes, and plugin/worker integration are not claimed.

## Algorithm and observed boundaries

Both paths normalize the input around `0x1.69555p-1` and use 128 log intervals.
The used fields are reciprocal center, leading log, and log tail. A fourth field
between reciprocal and log is unused and is deliberately omitted. Positive
subnormals are scaled by 2^52 and their encoded exponent is adjusted before
normalization. The high/low log representation retains the addition errors and
a polynomial correction through degree eight.

The FMA path computes the reduced argument with a fused subtraction and retains
the quadratic's product error with another FMA. The non-FMA path splits the
normalized input at 32 trailing bits, producing separate high and low reduced
arguments. Its quadratic error uses those parts. The exponent multiplication is
also mode-specific: fused product residual versus a 27-bit-cleared high-part
split of both operands. The C helpers express these mathematical stages, not a
register-by-register transcription.

The exp stage is shared: 256 intervals, a shifted rounding operation for the
integer index, split ln(2)/256 subtraction, and a degree-five correction. The
table holds a relative tail and an encoded scale with the index contribution
pre-subtracted. Its arithmetic is deliberately unfused even in FMA mode.
Products below 2^-54 use 1+high. Products of magnitude at least 1024 immediately
overflow/underflow. Magnitudes at least 512 enter scale-safe reconstruction.
Positive large results use an intermediate scale reduced by 1009 exponent bits.
Negative large results use a scale increased by 1022 bits. Before subnormal
scaling, a signed unit addition and recovered sum residuals round directly on
the eventual output grid, avoiding a rounded normal intermediate's loss of tail.
This is the causal mechanism addressing F43 W1, not a witness-specific patch.

Special values are handled before reduction. A negative finite base requires an
integer exponent and odd integers supply the sign. Quiet NaNs with zero exponent
and base +1 with quiet NaN exponent return 1; signaling NaNs are quieted. With a
finite nonzero exponent, a NaN base follows the square-then-odd-sign rule: a
negative NaN with odd integer exponent becomes positive. NaN exponent paths use
the first-operand NaN priority of the x86 addition. These payload/sign decisions
are explicit bit operations because ARM's arithmetic NaN priority differs.

## Constant provenance

All addresses are RVAs in the local pinned DLL, SHA256
`385a6d73a50063a4c760bdb2ee59e575de7ff350a2e3b214ea491d08358484f3`.
Only the used 128 x 3 log fields and 256 x 2 exp fields are retained;
`pow_extract.py` regenerates them and asserts the DLL pin. Numeric coefficients
are exact hexadecimal doubles; integer masks and powers of two are written
as representation operations. Validation justifies them only within the tested
corpora, not as an exhaustive proof over 2^128 input pairs.

| Field or decision | RVA |
| --- | --- |
| Export / FMA / non-FMA entry | `78440` / `65160` / `a5310` |
| Log normalization offset, 7-bit index | `654f5`–`65523`, `a569e`–`a56f5` |
| Log reciprocal / head / tail, i=0..127 | `105b18+32*i`, `105b28+32*i`, `105b30+32*i` |
| ln(2) head / tail | `105ac8`, `106b20` |
| Log polynomial constants | `105ab8`, `106b18`, `106b50`, `105ab0`, `106b58`, `105ac0` |
| -1/2 | `106b48` |
| FMA reduced argument / quadratic residual | `65543`, `6556d` |
| Non-FMA input split / product split masks | `a56db`–`a5705`, `a5840` |
| Product magnitude thresholds | `6562a`–`65694`, `a5884`–`a58e8` |
| 256/ln(2), shifted rounding constant | `107bf8`, `107c00` |
| ln(2)/256 head, negated tail | `106b68`, `107be8` |
| Exp polynomial constants | `107be0`, `106b60`, `107c08`, `107bf0` |
| Exp tail / encoded scale, j=0..255 | `106be0+16*j`, `106be8+16*j` |
| Overflow/subnormal scale adjustment | `6573c`, `6577c`, `a599e`, `a59d0` |
| Subnormal compensated rounding | `6578e`–`65803`, `a59e2`–`a5a76` |
| Exponent shortcut bounds | `653cf`, `6542d`, `a557d`, `a55eb` |
| NaN/zero/infinity and odd-sign decisions | `651d3`–`65353`, `a538e`–`a550c` |

## Reproduction

From the worktree, run sequentially (no jobs pool). The driver runs one native
arm64 producer and one Rosetta comparator, with fixed 4096-row buffers, checks
both exit statuses, and records input counts and source/binary hashes. The
comparator uses the unchanged F27c pinned PE loader and two relocated images,
asserts mode masks 0 and 3, and sets MXCSR to 0x1f80. Exact 64-bit words are
compared, including all NaN bits. It returns nonzero on any mismatch.

```sh
state="$PWD/orchestra/state/F43c-ucrtmath-ms-pow"
mkdir -p "$state"
export TMPDIR="$state" PYTHONDONTWRITEBYTECODE=1
python3 ucrtmath/ms/pow_targets.py "$state"
python3 ucrtmath/ms/pow_check.py file --count 2243557 --inputs "$state/targets.txt" --tag targeted
python3 ucrtmath/ms/pow_check.py random --count 1000000000
python3 ucrtmath/ms/pow_check.py ranges --count 100000000
python3 ucrtmath/ms/pow_harness_check.py
python3 ucrtmath/ms/pow_retain.py --check
```

Random inputs use F28's two independent SplitMix64-derived raw words with its
original seed, so the first 100 million pairs contain F43's campaign inputs.
Ranges use independent uniformly spaced doubles in [0,4) and [-8,8); targeted
inputs separately include closed endpoints and adjacent integer/half-integer
exponents. The 2,243,557-row targeted corpus includes all three distinct finite
F43 first witnesses, 8,193 consecutive near-one bases, special-value cross
products, dense exponent neighborhoods, 4,097 exponent ulps around each selected
output transition, and log-table boundaries at seven exponent scales. Python
libm only selects boundary inputs; it never supplies expected outputs.
`pow_targets.py` records per-family counts. Some family overlaps are intentional.

Qualification results and source identities are retained in `results/pow/`;
see `orchestra/reports/F43c-ucrtmath-ms-pow.md` for actual results and limitations.
No test result here establishes plugin AE exactness or updates a release gate.
