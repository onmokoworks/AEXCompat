# Apple Silicon x64 AEX performance decision record

Issue: #487  
Parent: #476  
Initial fixture: unchanged local `OLMBlur.aex`

## Decision to make

Determine whether the existing Unicorn backend has a safe and maintainable
path to sub-second interactive preview, and whether exact Full HD can approach
one second without effect-specific source ports. If the measured ceiling is
insufficient, stop optimizing this backend and select one follow-up backend
spike instead of opening parallel implementations.

The two latency claims are deliberately separate:

- **Draft preview P95:** visible response while changing a parameter.
- **Exact Full HD P95:** a 1920x1080 render at the requested parameters.

Default parameters alone are not sufficient evidence. At minimum the decision
must also include a high-cost setting such as `Blur Amount=50`.

## Established baseline

PR #484 established the first real rendering path:

- the unchanged Windows x64 AEX is mapped by the Rust guest worker;
- Classic setup/sequence/frame lifecycle drives Smart CPU render;
- parameter descriptors and values cross a generic boundary;
- arbitrary PNG input and ARGB8 output work through 1920x1080;
- incomplete emulation is rejected unless execution reaches the return
  sentinel.

Removing an image-wide instruction hook and Unicorn instruction counting
changed the default 1920x1080 render from 241.88 seconds to 13.50 seconds.
The output SHA-256 remained
`9bfc672788beb677ad364b8d74c2be263a5e046a16cb4c7e708f03fc91d07237`.

The input fixture SHA-256 values used in the original run were:

- 64x48: `2772bd0c001312cebb65b7520fe325349b87303a62b4051900cc15f2df2646d7`
- 1920x1080: `9cb64466d3e0891df1b4885cf58c80082afa35794b9a9832d3f884d93c1d0c95`

These PNGs are local ignored artifacts and are not repository fixtures.

## Independent investigation summary

### Claude Code measurements

Claude Code inspected the repository, the vendored Unicorn 2.1.5/QEMU source,
and a macOS `sample` profile. Its measured default Full HD profile attributed
approximately:

- 56% to scalar SSE floating-point/softfloat helpers;
- 29% to Unicorn's per-translation-block exit-request helper;
- 6% to softmmu store paths;
- less than 1% to translated guest code at the top of sampled stacks.

It also measured nearly linear pixel cost and negligible setup/PE-map cost.
The detailed external report is not treated as repository evidence by itself;
this issue must reproduce the important measurements with
`tools/profile-macos-aex-guest.sh`.

Claude's proposed safe decision experiments were:

1. independently measure exit-request spill/reload overhead;
2. independently measure a host-float upper bound for scalar SSE helpers;
3. record Smart pre-render input rectangles and prove serial band stitching
   before adding parallelism;
4. retain timeout and return-sentinel failure gates in every variant.

### Codex sub-agent convergence

Several deliberately different prompts converged on these points:

- process persistence and translation-cache warmth only remove a small fixed
  cost and cannot explain a 13.5x Full HD improvement;
- a 384x216 to 480x270 Draft render is the lowest-risk sub-second interaction
  path;
- Full HD parallelism is only valid after the plug-in's Smart Render input
  request supplies a sufficient apron/halo;
- a Windows x64 remote helper is the most practical non-local fallback;
- AOT lifting, FEXCore integration, and verified Metal-kernel synthesis are
  longer research tracks, not simultaneous implementation tasks for #487.

### Rosetta constraint

An x86_64 native carrier is useful only as a short-lived native-speed baseline.
Apple documents that arm64 and x86_64 code cannot coexist in one process, which
matches AEXCompat's existing out-of-process worker boundary. Apple also states
that general-purpose Rosetta support is planned only through macOS 27.
Therefore #487 must not select Rosetta as the durable backend.

## Experiments and gates

| Experiment | Evidence required | Stop condition |
| --- | --- | --- |
| Baseline profile | external sample, wall time, identities, output hash | results are not reproducible |
| Exit-check upper bound | isolated patch result plus timeout/return failure tests | output or failure semantics change |
| Scalar fast-FP upper bound | isolated patch result plus output comparison and special-value risk note | gain is small or correctness cannot be bounded |
| Serial band/apron | stitched output comparison against the full-frame render | seams/hash mismatch or request rectangle is insufficient |
| Parallel band render | per-band fail-closed report and Draft/Full HD P95 | serial semantics were not first proven |

## Reproduction environment

All timings below were collected on the same Apple Silicon Mac with the
unchanged AEX at
`/Users/onmk/Documents/Projects/Personal/OLM as/plugins_2025/OLMBlur.aex`.
Its SHA-256 was
`f0611785b97b37405444e3298a78edeea36773185d9cf9fed4734f812b782586`.

`tools/benchmark-macos-aex-guest.py` performs sequential runs, requires
`render_error == 0`, and rejects output hashes that vary between runs. P95 is
nearest-rank; with five runs it is the slowest observation. Local result files
are under ignored `target/issue487/`.

## Measured results

### Canonical backend

| Case | Runs | Median | P95 | Output SHA-256 |
| --- | ---: | ---: | ---: | --- |
| Draft 64x48, default | 5 | 0.028 s | 0.035 s | `da8ca4fd...` |
| Draft 384x216, default | 5 | 0.541 s | 0.561 s | `a11aa866...` |
| Draft 480x270, default | 5 | 0.844 s | 0.856 s | `ff5907f3...` |
| Draft 64x48, Amount=50 | 5 | 0.059 s | 0.060 s | `3c081e09...` |
| Draft 256x144, Amount=50 | 5 | 0.867 s | 0.878 s | `c8257607...` |
| Draft 384x216, Amount=50 | 5 | 2.031 s | 2.034 s | `69bbdd40...` |
| Draft 480x270, Amount=50 | 5 | 3.292 s | 3.381 s | `8fc11a78...` |
| Exact 1920x1080, default | 5 | 13.580 s | 13.707 s | `9bfc6727...` |
| Exact 1920x1080, Amount=50 | 5 | 60.183 s | 60.287 s | `ea1594ad...` |

The default profile was collected with macOS `sample` attached directly to the
worker process, without guest instruction hooks. The wall time including
sampling was 14.443 seconds. Its dominant leaf/near-leaf stack counts included:

- `helper_check_exit_request`: 1,808
- `helper_mulss`: 1,804
- `helper_addss`: 1,028
- `float32_add`: 291
- `float32_mul`: 236

This reproduces the broad shape of Claude's report, but not its optimistic
speedup estimates.

### Exit-request upper bounds

Each experiment used a fresh ignored copy of Unicorn and a separate Cargo
target directory. None is a product patch.

| Variant, default Full HD | Runs | Median | P95 | Interpretation |
| --- | ---: | ---: | ---: | --- |
| Canonical | 5 | 13.580 s | 13.707 s | baseline |
| helper marked `TCG_CALL_NO_WG` | 5 | 13.576 s | 13.871 s | no measurable gain |
| helper body no-op | 3 | 13.284 s | 13.458 s | unsafe; timeout request disabled |
| helper call removed | 3 | 12.239 s | 12.374 s | unsafe ceiling, about 10% |

Removing the call entirely also removes the fast stop mechanism. The result is
useful only as an upper bound: even a perfect safe inline exit check cannot
close a 13.7x gap. The canonical backend retains the check.

### Scalar SSE host-float upper bound

A fresh Unicorn copy replaced only scalar `addss`/`mulss` softfloat operations
with native host float operations. This is not semantically safe for all
NaNs, exceptions, rounding modes, or MXCSR states, so it is also an upper-bound
experiment rather than a proposed patch.

| Case | Runs | Median | P95 | Canonical P95 | Gain |
| --- | ---: | ---: | ---: | ---: | ---: |
| Full HD, default | 5 | 10.893 s | 10.909 s | 13.707 s | 20.4% |
| Full HD, Amount=50 | 5 | 45.480 s | 45.576 s | 60.287 s | 24.4% |

The output hashes matched the canonical backend for these fixtures:
`9bfc6727...` at default and `ea1594ad...` at Amount=50. That does not establish
general floating-point equivalence. Even this deliberately optimistic ceiling
misses one second by 10.9x at default and 45.6x at the high-cost setting.

The native-helper experiment still paid TCG helper call/spill overhead. As an
even more generous argument against premature optimization, eliminating the
entire sampled scalar-FP and exit-helper shares would leave roughly 35% of the
run: about 4.8 seconds at default and 21 seconds at Amount=50. Even an idealized
inline lowering therefore remains several whole design changes away from one
second.

### Smart Render region and parallelism gate

The worker now records the `PF_RenderRequest` passed to
`checkout_layer`. A generic `render-region-png` diagnostic can send a
non-full-frame `output_request` without modifying the AEX.

For a 1920x1080 input, requesting only the central band
`[left=0, top=500, right=1920, bottom=600]` produced:

- checkout input request `[0, 0, 1920, 1080]`;
- 13.57 seconds wall time;
- output SHA-256 `9bfc6727...`, exactly equal to the full-frame render.

The same full-input behavior occurred on 64x48 at default and Amount=50.
This experiment cannot separate plug-in behavior from all current host wiring:
the host still exposes full-frame extent/result/output worlds. It does prove
that the current AEXCompat host plus OLMBlur does not propagate a usable
band/apron boundary. Serial stitching therefore cannot be established without
additional host work, so the parallel benchmark is intentionally not run.
Running it now would violate the experiment's own gate and duplicate
full-frame work.

The diagnostic reports `render_mode = smart-cpu-region` and the requested
rectangle explicitly. A partial output is therefore not silently represented
as an ordinary full-frame render.

### Failure boundaries

The tracked worker keeps the canonical Unicorn exit request, 600-second
wall-clock timeout, and explicit RIP return-sentinel validation. Focused tests
now cover:

- a normal Win64 return reaching the sentinel;
- a `hlt` that stops before the sentinel and fails closed;
- an infinite loop stopped by a short test timeout and rejected before return.

The benchmark additionally requires a zero render error and stable image hash.
`cargo test --manifest-path guest/Cargo.toml -p aex-guest-worker` passes all
seven tests.

## Decision

Do **not** pursue a vendored Unicorn fast-FP fork, exit-check surgery, or
OLMBlur band parallelism as the route to sub-second Apple Silicon interaction.
Their independently measured ceilings are too small, and the only large local
parallelism candidate fails its semantic prerequisite.

Keep Unicorn as the compatibility/correctness backend because it already loads
the unchanged AEX, exposes generic parameters, and renders images. For the
current GUI, use a closed-loop generic Draft policy: briefly probe the current
effect and parameter set, estimate its per-pixel cost, and choose a pixel
budget with headroom below one second. Do not key policy to an OLMBlur
parameter name.

For scale, default OLMBlur meets sub-second P95 at 480x270 but has little GUI
headroom there; Amount=50 meets it at the newly confirmed 256x144
(P95 0.878 seconds). Exact Full HD remains an explicit slow final render, not
an interactive promise.

The next performance issue should select one different execution strategy
rather than combining several: the pragmatic baseline is a Windows x64 remote
worker; local AOT/lifting or FEXCore integration remain bounded research
spikes. Rosetta is not a durable selection.
