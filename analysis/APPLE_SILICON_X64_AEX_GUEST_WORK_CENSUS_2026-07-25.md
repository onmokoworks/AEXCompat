# Apple Silicon x64 AEX guest-work census

Issue: #489  
Parent: #476  
Predecessor: #487 / #488  
Fixture: unchanged local `OLMBlur.aex`

## Decision question

Determine whether a small and stable set of guest-owned pixel-loop blocks is
concentrated enough that promoting only those blocks out of Unicorn's scalar
SSE helper path plausibly opens an order-of-magnitude route.

This spike does not implement a native, NEON, Metal, AOT, or second emulator
backend. It measures whether one such implementation is licensed.

## Why this was selected

A deliberately context-light Claude Code pass generated twelve moonshots.
The highest-value family was to stop paying x64 dispatch/helper overhead for
every pixel: identify the repeated pixel loop once, then widen, specialize, or
promote it. A second repository-grounded pass found:

- OLMBlur completes without a host-provided `PF_Iterate*` callback, so
  iterate-suite interception is not an entry point for this fixture;
- Unicorn/QEMU lowers the observed scalar SSE operations through C helpers;
- the #487 host-float experiment changed helper arithmetic but retained the
  helper-call boundary, explaining why it measured only a 20–24% gain.

Claude also proposed parameter specialization and black-box operator
classification. The former needs a destination code generator first. The
latter remains the fallback if the guest work is not sufficiently
concentrated.

## Instrumentation

`census-png` installs a Unicorn block hook only around `CMD_SMART_RENDER`.
The hook range is restricted to the mapped PE image, excluding host callback
stubs. Normal `render-png` installs no census hook.

For each distinct `(address, translated block size)` the worker records the
execution count. After the hook is removed, iced-x86 decodes the mapped block
bytes. The report derives:

- estimated dynamic guest instructions and instructions per output pixel;
- scalar single/double SSE FP instructions;
- dynamic work in blocks containing at least one scalar SSE FP instruction;
- cumulative top-1, top-5, and top-20 block concentration;
- the number of blocks needed to reach 80%.

QEMU translation blocks are execution artifacts, not necessarily native
promotion units. Several variants can overlap the same guest bytes depending
on their incoming branch. The report therefore also merges adjacent or
overlapping block ranges into contiguous guest-code extents and ranks those
extents by the dynamic instructions attributed to their member variants.

This is a dynamic instruction census, not a cycle attribution. One scalar SSE
instruction can be much more expensive than one integer instruction in the
current helper-based implementation.

The reproducible command is:

```sh
python3 tools/census-macos-aex-guest.py \
  --worker guest/target/release/aex-guest-worker \
  --aex "/path/to/OLMBlur.aex" \
  --input-small target/issue489/input-64x48.png \
  --input-large target/issue489/input-256x144.png \
  --high-parameter "Blur Amount=50" \
  --runs 10 \
  --output-directory target/issue489/repro-10
```

The PNG inputs and result directory are ignored local artifacts. The tool
requires zero render errors, stable census summaries across runs, and identical
normal/census output hashes.

## Results

All ten census runs per case produced identical census summaries. Normal and
census output SHA-256 values matched in every run.

| Case | Guest instructions / pixel | Scalar SSE FP instruction share | Work in scalar-SSE-containing blocks | Top-20 share | Blocks for 80% |
| --- | ---: | ---: | ---: | ---: | ---: |
| 64x48 default | 875.91 | 30.52% | 70.03% | 68.20% | 29 |
| 256x144 default | 887.11 | 30.98% | 70.93% | 68.65% | 28 |
| 64x48 Amount=50 | 2,193.69 | 40.35% | 86.16% | 82.23% | 19 |
| 256x144 Amount=50 | 3,192.05 | 42.09% | 89.43% | 86.15% | 17 |

The top-20 address sets were:

- identical between the two default sizes;
- 19/20 identical between the two Amount=50 sizes;
- 14/20 shared between default and Amount=50 at 256x144.

The loop family is therefore stable across resolution and substantially, but
not completely, stable across the parameter change.

At the promotable code-extent level, the apparent fragmentation disappears:

| Case | Distinct extents | Top-2 extent share |
| --- | ---: | ---: |
| 64x48 default | 45 | 82.59% |
| 256x144 default | 45 | 83.56% |
| 64x48 Amount=50 | 45 | 93.04% |
| 256x144 Amount=50 | 45 | 95.43% |

The same two extents lead all four cases:

- RVA `0x10f2..0x1425`, 819 bytes;
- RVA `0x1a83..0x1dce`, 843 bytes.

Together they are 1,662 static bytes. The raw top-20 translation-block metric
is retained because it was pre-registered, but it must not be mistaken for the
size of a native promotion target.

### Census overhead

The census is intentionally diagnostic. Ten-run wall P95 was:

| Case | Normal P95 | Census P95 |
| --- | ---: | ---: |
| 64x48 default | 0.033 s | 0.035 s |
| 64x48 Amount=50 | 0.060 s | 0.077 s |
| 256x144 default | 0.247 s | 0.309 s |
| 256x144 Amount=50 | 0.872 s | 1.063 s |

The Amount=50 wall samples remain bimodal near 0.23 and 0.87 seconds on this
machine, while the dynamic census is identical. P95 is retained for latency
claims; the bimodality does not change block concentration.

## Optimistic native floor

Extrapolating the 256x144 instruction rate linearly to 1920x1080 gives:

- default: approximately 1.84 billion guest instructions;
- Amount=50: approximately 6.62 billion guest instructions.

At an assumed 3.5 GHz, an ideal scalar native stream would have these arithmetic
floors before memory, branches, lowering overhead, and synchronization:

| Case | 1 IPC | 2 IPC | 4 IPC |
| --- | ---: | ---: | ---: |
| default | 0.526 s | 0.263 s | 0.131 s |
| Amount=50 | 1.891 s | 0.946 s | 0.473 s |

Amount=50 rises from 2,194 to 3,192 instructions/pixel between the two tested
sizes, likely because a large radius is clipped more aggressively by the small
frame. Its 6.62-billion Full HD estimate is therefore a lower bound, not a
stable linear prediction.

An ideal four-lane widening divides the table values by four, giving 0.131 seconds
at default and 0.473 seconds at Amount=50 even at 1 IPC. These values show that
the physics do not rule out one second. They do not predict an implementation:
aliasing, divergent radius loops, memory traffic, x86 FP semantics, and
promotion boundaries are omitted.

## Pre-registered gate audit

| Gate | Evidence | Result |
| --- | --- | --- |
| Promotable hot guest code covers at least 80% | raw top-20 TBs: default 68–69%, high 82–86%; top-2 contiguous extents: 83–95% | Pass at the intended promotion-unit granularity |
| At least 40% weighted hot work is scalar-SSE/helper-bound | #487 external profile attributed about 56% of samples to scalar SSE/softfloat helpers; scalar-SSE-containing extents cover 70–89% of dynamic instructions | Pass |
| Same dominant family across sizes/settings | the same two 819/843-byte extents lead all four cases | Pass |
| Optimistic default Full HD floor below 1.0 s | 0.526 s at the conservative 1-IPC scalar arithmetic floor | Pass, model only |
| Normal output and failure boundaries unchanged | stable paired hashes; normal mode has no hook; timeout/sentinel tests pass | Pass |

The raw top-20 TB threshold appears to fail at default, but a TB is not the
unit named by the decision question: overlapping TB variants cover the same
guest bytes. Coalescing them does not relax the 80% threshold; it measures the
static region that an inline lowering prototype would actually replace.
Likewise, raw SSE instruction share is not a cycle weight. The independent
external profile from #487 supplies the pre-registered weighted-cost evidence.

## Decision

**Continue to one bounded inline-lowering prototype; do not build a full
backend yet.**

All pre-registered gates pass when concentration and weighted cost are scored
at their intended units. Two stable extents totaling 1,662 bytes account for
83–95% of estimated dynamic guest instructions, and the independent #487
profile places the scalar SSE/helper path above the weighted 40% gate.

The prototype should target only one of the two extents and one parameter case.
Its purpose is to measure the removable helper-call/spill boundary, not to
promise general x64 translation. It must:

- remain opt-in with exact Unicorn as the fallback and oracle;
- preserve timeout, callback errors, and RIP return-sentinel failure behavior;
- compare output bytes for default and Amount=50 on small fixtures;
- report compile/promotion time separately from steady-state render time;
- stop if the promoted extent cannot provide at least a 3x end-to-end gain on
  256x144 Amount=50 or if its memory/branch behavior cannot be bounded.

Amount=50 Full HD sub-second performance remains unproven: its instruction rate
is size-dependent, and the idealized cost model omits memory and divergence.
Passing #489 licenses only this prototype, not adoption.

If the bounded prototype fails, return to the cheap verified-surrogate
classification spike without opening both paths simultaneously.
