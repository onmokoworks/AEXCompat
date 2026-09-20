# Windows ARGB8 pixel transport checkpoint

The tested 1920x1080 resident render path took **26.6% less time** after batching
8-bit channel permutation. This is not a threefold application speedup or a
new all-AEX sweep result. No one-shot improvement was demonstrated.

## Change and unchanged contracts

The native worker used an out-of-line, depth-dispatching helper once per pixel
for several RGBA/ARGB boundaries. Packed 8-bit conversion now uses one bulk
call; Classic input copies complete rows instead of copying each pixel into
the strided world. Windows little-endian 32-bit rotations use `memcpy` loads
and stores, without alignment assumptions or a new SIMD instruction-set
requirement. This is less repeated work, not extra process parallelism.

The same helper serves Classic/Smart primary and secondary inputs, resident
output, and Auto8 canonical input hashing. Non-8-bit and null-input paths are
unchanged. Existing geometry, row stride, shared-memory bounds, output guards,
lifecycle, and ownership checks remain in place. Disjoint and exact-alias
buffers are supported; partial overlap is not part of the contract.

These are shared worker paths behind the shipping interactive UI/CLI adapter;
no alternate benchmark-only renderer or UI success override was added.

## Paired measurement

One Release broker test executable drove the existing parameter-echo fixture
with identical nonuniform RGBA input on both worker versions. Each run rendered
12 one-shot frames and 12 resident frames, changing the echo parameter from
0 through 11. Five old/new pairs ran serially in AB/BA/AB/BA/AB order, without
concurrent builds or other tests. Times include the render call and its PNG
write; later PNG decoding, full-frame pixel checks, and SHA comparisons are
outside the timed regions. Each displayed run median is sorted sample 7 of 12,
matching the pre-existing benchmark convention; no sample was discarded.

| Pair | Old resident ms | New resident ms | Old one-shot ms | New one-shot ms |
| --- | ---: | ---: | ---: | ---: |
| 1 | 47.2977 | 35.7352 | 197.0325 | 201.3504 |
| 2 | 49.0293 | 36.5680 | 193.6478 | 202.6777 |
| 3 | 49.0595 | 35.6937 | 197.7768 | 193.9617 |
| 4 | 47.5415 | 35.5443 | 196.3365 | 196.5999 |
| 5 | 48.6645 | 38.0061 | 193.8040 | 198.6482 |

The median of the five run medians is 48.6645 -> 35.7352 ms, a 26.57%
reduction (1.36x throughput). Every pair improved by 21.9–27.3%, exceeding the
predeclared 5% retention threshold. One-shot medians are approximately flat
(median of medians 196.3365 -> 198.6482 ms); startup and other work remain.
Single-frame outliers remain, including a candidate resident sample of about
195.5 ms. This is a local diagnostic, not a statistical or tail-latency guarantee.

All 240 outputs decoded to the exact expected echo pixels. Output hashes,
plugin identity, and input identity matched across modes and versions; resident
sessions closed cleanly. The fixture ignores source color, so this measurement
alone does not establish source-dependent effect correctness.

Measured identities (SHA-256):

- Old worker, source checkpoint `61467117f`:
  `1b86cb69d0dc2f734f6c0bb57459db6ee9b3a7315bb6e8a5a08401bf29a420f0`
- New worker, this checkpoint's native source diff:
  `e3548ff41be2bc8a245c878566c336029e66ec5b9cd48f740a39d699b58429f3`
- Shared Release benchmark executable:
  `15ff22e44b5cb77e79311518959b6cd6db131926dace0b84ffada20402ea9803`
- Synthetic parameter-echo AEX:
  `19585d92cdfe9453043efcb9ba1c5542af9a072433a02971ee4a932fd6b9dd12`
- Input RGBA:
  `a44dbcf878a69ad102c151e31337b00664cde930a0f39adee55b9d274905f4bd`

The strengthened manual benchmark is reproducible with locally built worker
and echo probe artifacts:

```powershell
cargo test --release --manifest-path broker/Cargo.toml -p aexcompat-broker `
  --test resident_session_live resident_session_latency_versus_one_shot `
  -- --ignored --nocapture
```

It prints all frame timings and image/provenance hashes as JSON. Compare frozen
old/new worker artifacts with the same test executable; never replace a live
worker or mistake a rebuilt candidate for a baseline. Private raw measurements
remain under `target/resident-exp28-pair*-*.log`; they are not redistributed.

## Validation and limits

- Release worker and four affected native self-tests built successfully;
  header dependency verification covered 216 translation units.
- Native bulk tests cover zero/null, short/tail/1919-pixel counts, offsets 0–15,
  guards, exact alias, all 256 channel values, packed/padded Classic worlds, and
  unchanged 16/32-bit/null-gradient behavior. The pytest wrapper is registered
  in the built-artifact manifest; the five focused native wrappers passed.
- Worker output-guard self-test passed, including intentionally faulting cleanup
  and overrun detection. No safety invariant was weakened.
- Broker library: 329 passed, 1 ignored. Wrapper integration: 31 reported
  passes, including existing internal missing-fixture skips; the three Auto8
  boundary cases were separately run with their built probe and all passed.
- Resident parameter update and both real synthetic cluster tests: 3 passed.
  Shipping Auto8 synthetic CLI cases: 11 passed.
- Exactly 10 installed regression frames passed at 256x144 ARGB8/time 0:
  OLMSmoother Classic (1), BCC Tritone Smart with secondary input (5), and
  Fast Grain Auto8 GPU (4), using existing pixel/response oracles. This is
  compatibility evidence, not another performance sample or Adobe equivalence.
  The unchanged Release UI/CLI harness hash was
  `c228bd8b55d7fc75dd3ada262c4623fb9244961b09c3c0c79defa8c05faa01a9`.

No AE process, Maxon/Sapphire, forbidden PSOFT installation, or full sweep was
run. The historical inventory remains 984 = 567 decoded/semantics-unverified
+ 414 externally blocked/not executed + 3 non-image; bounded followups do not
silently relabel those historical buckets. This optimization does not establish
new plugin compatibility or change those totals. Full-corpus timing, heavier
effect kernels, other depths, and other machines remain unmeasured here.

The separate-TU component prototype predicted the improvement (1080p Classic
input 10.093 -> 1.5808 ms, output 3.7125 -> 0.7088 ms), but was only a selection
experiment. No temporary instrumentation was added to product code. The
retention decision uses the paired shipping resident measurements above.
