# OLM-internal double sin, cos, exp

`double.c` independently implements the algorithms observed in pinned
ucrtbase 10.0.28000.2705, SHA256
`385a6d73a50063a4c760bdb2ee59e575de7ff350a2e3b214ea491d08358484f3`.
Like the F42 float implementation, this code is **OLM-internal and must not be
published standalone**. No Microsoft binary or instruction listing is included.
The exported functions are `ucrt_sin`, `ucrt_cos`, and `ucrt_exp`, each taking
`UCRT_NOFMA` or `UCRT_FMA`. The contract is binary64 return bits with
round-to-nearest and gradual underflow. errno, exception flags, other rounding
modes, and plugin/worker integration are outside this ticket.

The native build uses `-arch arm64 -ffp-contract=off -fno-fast-math`; fused
operations are explicit `fma` calls. The implementation does not call host
sin/cos/exp or use expected-result patches.

## Algorithm observations

Trig uses degree-13 sine and degree-14 cosine polynomials. Direct evaluation and
reduced evaluation differ, with reduced evaluation retaining the low part of
the residual. Non-FMA groups polynomial terms; FMA uses Horner evaluation and
explicit compensation. Small inputs have identity, cubic, or quadratic branches
with function/mode-dependent strictness at their endpoints.

Moderate non-FMA reduction uses nearest-quadrant selection and split pi/2,
refining when cancellation removes more than 15 exponent bits. Moderate FMA
reduction uses a fused rounding-bias operation to select the nearest quadrant,
a full-double pi/2 product and two trailing parts. Both return a high/low pair.

Large reduction selects three 64-bit limbs of 2/pi by exponent and multiplies
by the input's 53-bit significand. Its fractional product is complemented when
the quadrant rounds up. Two successively truncated significands are multiplied
by split pi/2, then normalized to a compensated high/low pair. The implementation
uses a generic fixed-point extractor, not a register-by-register translation.

Exp truncates x*(64/ln(2)) to an integer. A split negative ln(2)/64 reduction,
degree-six correction polynomial, and 64-entry high/low/full 2^(j/64) table
reconstruct the result. The FMA path fuses the leading reduction and polynomial,
but table reconstruction remains separate multiply/add operations. Normal
scaling adjusts the exponent field; subnormal scaling multiplies by an exactly
represented power of two. Dedicated tiny, overflow, and underflow endpoints
match the reference's branch comparisons. NaNs preserve payload/sign and quiet;
trig infinities return negative quiet NaN; exp(-infinity) returns positive zero.

## Minimal constant provenance

All addresses are RVAs in the pinned DLL. Only functional numeric data is
retained in `double.c` and `double_tables.h`; duplicated constants are shared.
Powers of two, IEEE masks, and integer arithmetic constants are written directly.

| Data or decision | Source RVA |
| --- | --- |
| Six sine coefficients | `10f990 + 16*i`, i=0..5 |
| Six cosine coefficients | `10f930 + 16*i`, i=0..5 |
| Direct trig limits and tiny branches | sine `bb708`, `bb960`, `bba4d`–`bba98`; cosine `b7392`, `b75d0`, `b7698`, `b770f`–`b7757` |
| 2/pi and non-FMA pi/2 splits | `110800`, `110770`, `110780`, `110790`, `1107a0` (cosine duplicates `10ba00`, `10b980`–`10b9b0`) |
| Reduction cutoffs | `110830` / `10ba30` (500000); `110838` / `10ba38` (20000000) |
| FMA moderate reduction constants | `110720`, `110730`, `110740`, `110750`, `110760`; algorithm `bb580`–`bb5f6` |
| Large-reduction 2/pi window, 149 bytes | `10bde0 + 7` through `+155`; identical used window at `110570` |
| Large-reduction pi/2 and split parts | `110450`–`110478` (SSE); `110610`, `110630`, `110640`, `110650` (FMA) |
| Fraction splitting mask | `110620`, equivalent SSE truncation at `bb2c2`–`bb2c7` |
| Fixed-point extraction and compensation | `bb160`–`bb366`, `bb380`–`bb56b` |
| Exp degree-six coefficients | `10bb90`, `10bba0`, `10bbb0`, `10bbc0`, `10bbd0` |
| Exp upper/lower/min-subnormal cutoffs | `10bc00`, `10bc08`, `10bc10` |
| Exp 64/ln(2), negative ln(2)/64 high and tail | `10bc18`, `10bc40`, `10bc48` |
| Exp tiny cutoff | `10bc60` |
| 64-entry exp head/tail/full tables | `110c00 + 8*i`, `110e00 + 8*i`, `111000 + 8*i`, i=0..63 |
| Exp scaling and endpoint decisions | `b7de0`–`b8034`, `b8040`–`b81f4` |

Read-only evidence can be regenerated with MAIN's existing Python environment:

```sh
state="$PWD/orchestra/state/F43b-ucrtmath-ms-double-trig-exp"
mkdir -p "$state"
export TMPDIR="$state" PYTHONDONTWRITEBYTECODE=1
main_python='/Users/onmk/Documents/Projects/Personal/OLM as/tools/emulation/.venv/bin/python'
"$main_python" ucrtmath/ms/inspect_double.py reduce > "$state/reduce.txt"
```

## Validation

`check_double.py` runs a native arm64 producer and the qualified Rosetta PE
loader/comparator, never more than two math processes. Records contain the
input and both native outputs; each raw word is compared with its corresponding
mode in a separate relocated DLL. The comparator checks the pinned DLL hash,
mode masks 0/3, MXCSR 0x1f80, stream completeness, and exact count. The Python
driver checks both process exits and fails on any mismatch. Streams are bounded
to 4096 records and are never retained in full.

Random inputs are `SplitMix64(index XOR 0xf43b)`, using all 64 bits. Range inputs
map the top 53 bits to [-64,64) or [-745,710). Endpoints are covered separately.
Both modes receive identical inputs. These are sampled binary64 comparisons,
not an exhaustive binary64 proof or new native Windows/AE measurements.

```sh
python3 ucrtmath/ms/check_double.py > "$state/pilot.log" 2>&1
python3 ucrtmath/ms/check_double.py --full > "$state/full.log" 2>&1
```

The full run requests 1,000,000,000 random inputs plus 100,000,000 range inputs
per function and mode. `--function`, `--kind`, and `--count` permit bounded replay.
Boundary corpora are generated deterministically; the generator and retained
result files define their exact scope. See the F43b report for measured results,
first mismatches (if any), and remaining qualification work.

Measured qualification completed with zero mismatches in both modes for each
function: 1,000,000,000 random inputs, 100,000,000 range inputs, and 2,651,476
boundary inputs for each trig function / 1,316,570 for exp. Boundary windows
cover both signs, pi/2 multiples 1..4096 and powers through exponent 1023,
branch thresholds, powers throughout the subnormal range, every exp integer
reduction boundary for n=-68800..65536, and ±4096 raw words around exp's three
saturation cutoffs. Each mode compares all raw result bits, including NaNs.

```sh
python3 ucrtmath/ms/check_double_harness.py > "$state/harness.log" 2>&1
python3 ucrtmath/ms/retain_double.py --check > "$state/recheck.log" 2>&1
```

Expected: 15 stream cases and six existing float regressions pass; the recheck
confirms all nine double campaigns, all six float campaigns and source hashes
against `results-double/`. Timings and binary hashes are recorded provenance,
not portable rebuild acceptance criteria. `retain_double.py` without `--check`
packages newly measured results; it is not necessary for ordinary verification.
