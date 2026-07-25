# Apple Silicon x64 native carrier result

Issue: #492

Predecessors: #487, #489
Benchmark fixture: unchanged local `OLMBlur.aex`

## Decision

Use a separate x86_64 macOS process as an opt-in native-speed carrier while
Rosetta is available. Keep the existing arm64 Unicorn worker as the
compatibility and failure fallback.

This is a generic PE/Win64/AE Host boundary, not an OLM implementation:

- no plug-in hash, effect name, RVA, parameter name, or blur algorithm is
  present in the carrier;
- the same `PeImage`, generated ABI, `ClassicHost`, parameter materialization,
  Suite callbacks, Smart Render callbacks, and pixel worlds are used by both
  backends;
- runtime selection depends only on an explicit host opt-in and whether the
  carrier succeeds before its deadline.

The earlier proposal to promote one observed OLM hot extent directly was not
implemented. Doing so by writing the blur loop in Rust would have optimized one
effect rather than AEXCompat. The native carrier instead accelerates arbitrary
supported guest code and establishes the speed target for a future durable
arm64 DBT.

## Boundary

The native carrier:

- maps an AMD64 PE image at its preferred base;
- patches its bounded IAT to typed Win64 host callbacks;
- protects PE sections according to execute/write characteristics before
  entering the image;
- invokes Effect entry points with Rust's Win64 ABI;
- keeps callback state thread-local for the synchronous selector call;
- uses a bounded 256 MiB host arena for AE structures, pixels, and handles.

Windows CRT `DllMain` is not entered by default. It requires typed Windows
runtime/SEH implementations and previously stopped inside an unimplemented
runtime boundary. `AEXCOMPAT_NATIVE_RUN_DLLMAIN=1` exists only for diagnostics.
The effect entry and complete tested selector lifecycle work without it.

The harness uses the carrier only when `AEXCOMPAT_NATIVE_CARRIER=1`. Native
setup has a 2-second deadline and render has a 5-second deadline. A nonzero
exit, signal, launch failure, or deadline failure advances to the existing
Unicorn worker. Explicit `AEXCOMPAT_GUEST_WORKER` selection remains available.

The carrier executes untrusted x64 instructions natively in its process. It is
not equivalent to Unicorn's guest-memory isolation or the Windows restricted
worker. Therefore native execution remains explicit opt-in.

## Feasibility probe

`tools/macos-x64-native-carrier-kill-test.c` verifies, in an x86_64 process on
Apple Silicon:

- dynamically mapped RX x64 code execution;
- Win64 register arguments and return value;
- a nested Win64 host callback roundtrip.

The probe emits:

```json
{"schema":"aexcompat.x64-native-carrier-kill-test","version":1,"dynamic_rx":true,"win64_abi":true,"host_callback_roundtrip":true}
```

## Full HD measurements

Environment: the same Apple Silicon Mac and unchanged private fixture used by
#487/#489.

- AEX SHA-256:
  `f0611785e7b14ac4fcfc75f23b8862beb4539eee52d25d472556849535e96e5b`
- 1920x1080 input SHA-256:
  `9cb64466d3e0891df1b4885cf58c80082afa35794b9a9832d3f884d93c1d0c95`
- release native worker SHA-256:
  `e31abe20c7d0e8eabe5ab3da7b6c4db3149ec184eb07e6d3f737b1f6abf54c40`
- ten fresh worker processes per case;
- P95 is nearest-rank, therefore the maximum observation at N=10;
- wall time includes process launch, PNG decode/encode, PE mapping, setup,
  parameter materialization, Smart Render, report serialization, and exit.

| Case | Median | P95 | Native output SHA-256 | Unicorn output |
| --- | ---: | ---: | --- | --- |
| Default | 0.303 s | **0.853 s** | `9bfc672788beb677ad364b8d74c2be263a5e046a16cb4c7e708f03fc91d07237` | exact match |
| Blur Amount=50 | 0.638 s | **0.656 s** | `ea1594ad99258d56360737dfdfefab89c3513d8640dc2c56d073b5e388d49a9c` | exact match |

The default P95 includes a 0.853-second cold first process; its other nine
observations were 0.294–0.422 seconds. Amount=50 observations were
0.635–0.656 seconds.

Historical Unicorn P95 from #487 was 13.707 seconds at default and 60.287
seconds at Amount=50. The native carrier improves the measured P95 by about
16.7x and 84.9x respectively while preserving the exact output hashes.

An additional 256x144 default run completed in 0.03 seconds and exactly matched
the prior Unicorn PNG SHA-256
`a4eba9e2c842679b6c3821c8cfcc56648e69c404d9ff19ff4de5cd31bf6ae394`.

Local ignored benchmark reports:

- `target/issue492/audit-default/benchmark.json`
- `target/issue492/audit-amount50/benchmark.json`

## Reproduction

```sh
tools/build-macos-aex-carriers.sh

python3 tools/benchmark-macos-aex-guest.py \
  --worker guest/target/x86_64-apple-darwin/release/aex-guest-worker \
  --aex "/path/to/OLMBlur.aex" \
  --input fullhd="/path/to/input-1920x1080.png" \
  --runs 10 \
  --output-directory target/issue492/final-default

python3 tools/benchmark-macos-aex-guest.py \
  --worker guest/target/x86_64-apple-darwin/release/aex-guest-worker \
  --aex "/path/to/OLMBlur.aex" \
  --input fullhd="/path/to/input-1920x1080.png" \
  --parameter "Blur Amount=50" \
  --runs 10 \
  --output-directory target/issue492/final-amount50
```

For the GUI, build both carriers and opt in:

```sh
AEXCOMPAT_NATIVE_CARRIER=1 \
  cargo run --release --manifest-path broker/Cargo.toml -p aexcompat-harness
```

## Remaining limitations

- General-purpose Rosetta is not the durable macOS 28+ backend selected by the
  project. This result is the native-speed implementation and oracle for the
  currently supported environment, not a reason to abandon future arm64 DBT.
- Only the imports and Host callbacks already required by the current vertical
  slice are typed. Unknown Windows runtime calls retain the existing temporary
  zero-return behavior, so broader AEX admission must remain conservative.
- Native worker sandboxing/hardening is not yet equivalent to the Windows
  restricted worker.
- A native process that becomes uninterruptible inside the OS translation
  runtime may outlive the harness deadline. The known `DllMain` path is disabled
  by default; process-level containment remains follow-up work.
