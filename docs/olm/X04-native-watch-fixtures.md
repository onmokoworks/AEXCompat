# X04 paired geometry and native instruction watches

The geometry runner executes original AEX bytes in the registered guest. Its
native column is a reviewed structural mapping, **not Windows execution**.
Compile-checking Rust for Windows does not compile the C++ minihost or execute
Windows. `tests/portable/geometry_contract_shape.cpp` separately checks the native
header's typed shape, not its runtime behavior.

Run `python tools/olm/geometry_matrix.py --worker <F54-worker> --worker
<F54b-worker> --worker <F54c-worker> --aex <ColorKeep.aex> --output <new-directory>`.
It runs one process at a time, writes packed typed input by row, streams process
output to files, records output hashes and world headers, and removes pixel dumps.
The matrix covers identity, non-divisible per-axis downsample, pixel aspect,
negotiated output, inset/negative requests, extent, explicit/automatic checkout,
odd/max row padding and UHD at 8/16/32 bpc, plus three admission negatives.
No pixel tolerance or plugin implementation changes are involved.

Structural findings at cb3bb768:

- `render_subsystem.h::SpatialRatio` and `RenderContextState` carry both ratios,
  aspect, full dimensions and pre-effect source origin. Guest and native require
  already sampled input; native flags are not aliases for guest flags.
- `SmartOutputBounds` derives dimensions/origin from result_rect; Classic resize
  uses `ClassicFrameOutput` with the opposite origin convention. This matrix uses
  SmartFX and does not establish Classic resize parity.
- `DiagnosticWorldLayout` supplies input/output padding and extent/request rects.
  Although the parser admits 0..256 bytes, `worker_smart_setup::prepare` requires
  padding divisible by pixel size. Guest odd padding (3 bytes) is a second
  structural gap at every depth; 256-byte padding is structurally comparable.
  Its parser rejects negative requests; the separate smart_output_request path
  accepts signed coordinates. Guest accepts negative negotiated requests, but
  ColorKeep clips the tested request to zero: negative *output* origin remains
  unobserved with this AEX, even though positive inset output is observed.
- `worker_smart_runtime.cpp::pre_checkout_layer` intersects requested rectangles
  with the source bounds, while `checkout_pixels` returns a view of the original
  world. It does not implement F54's cropped allocation, dimensions, stride and
  rebased data pointer. Explicit and automatic checkout rows are a structural gap.
- Native source admits 4096-edge / 16777216-pixel Smart worlds. UHD acceptance is
  structurally comparable; resource budgets remain separate and unchanged.

Existing OLM refs/conformance contains real Windows AE renders and old debugger
captures. Those records do not bind these F54 cases to this minihost revision;
there is no observed Windows execution of this matrix. Do not relabel historical
AE captures as native-minihost fixture runs.

## Observed matrix and verification boundary

FACT, 2026-10-28 macOS: all 117 rows (39 cases × registered F54/F54b/F54c)
met the declared outcome: 108 clean SmartFX renders and 9 precise admission
rejections. All 39 case groups agreed across workers on frame reports, output
hashes and materialized input-world records. No first mismatch was found in
those recorded fields. This is not a comparison against native Windows output.
The input-world dump describes the host's materialized primary input; for
`checkout=auto` it does not by itself prove the callback's cropped view.

ColorKeep AEX SHA-256:
`6d3718868c6c876c3bb370b19cb2bb3c4f89a3a479c29f03ae0d032a5d043b86`.
The three workers are identified by the generated matrix's `guest_worker.sha256`.
Their binaries and the AEX are external, unchanged inputs; this ticket does not
install or rebuild a reference worker. Full recorded matrix/logs accompany the
OLM ticket report, outside this repository. Reproduction needs the command above,
these original inputs, and Python 3.11+.

## Native observer contract (Windows runtime not yet observed)

`native-watch` is a separate, opt-in harness binary. Existing commands, guest
trace machinery and minihost host-callback `TraceWriter` are unchanged. It attaches
to an **already isolated, idle resident `aex_worker.exe`**. It neither launches
AEX files nor substitutes for the broker's Job Object/private desktop floor.
The filename check is a process-role check, not a sandbox or identity guarantee.

The observer uses Windows x64 hardware **execution** breakpoints, not data
watchpoints or instruction patching. It resolves one case-insensitive module
basename to its current load base and image size, rejects duplicate loaded names,
checks every RVA is inside executable committed image memory, and arms existing
and subsequently created threads. Captures occur immediately **before** the named
instruction. The public CLI emits `native_watch_armed` only after the attach
breakpoint has been handled and all known threads armed; send the desired frame
only after that marker. Earlier execution cannot be recovered.

Specification (decimal integer RVAs; set values from the actual binary):

```json
{
  "module": "ColorKeep.aex",
  "watches": [
    {
      "id": "context",
      "instruction_rva": 4660,
      "register": "rdx",
      "dereference_offsets": [],
      "size": 128,
      "occurrence": 2
    }
  ]
}
```

`4660` above is a **schema example, not a measured ColorKeep address**. Replace it
with an instruction boundary from `dumpbin /exports` or the retained disassembly.
Use an instruction inside the desired selector if selector-local alignment is
needed. Counting starts at arming, not at selector entry or frame start.

Limits and semantics:

- One module, 1–4 distinct instruction RVAs, unique watch IDs. No absolute-address
  spec, automatic symbol resolution, function-return pairing, selector filter,
  XMM capture, instruction census, GUI controls, or early DLL-load observation.
- `occurrence` is 1-based **per RVA across threads in debugger event order**.
  `sequence` orders all watched-site hits. Thread ID is recorded. Concurrent
  scheduling is perturbed by debugging; guest/native ordering is not presumed equal.
- A general-purpose register supplies the root pointer. Up to 8 declared offsets,
  each 0..4096, read exactly one little-endian 64-bit pointer each; then exactly
  `size` bytes are read. Checked arithmetic and all-or-nothing `ReadProcessMemory`
  prevent overflow/partial-read success. No unbounded walk or implicit plane size.
- 1..16 MiB per capture; 32 MiB aggregate declared payload. A contiguous plane
  within that cap can be observed (portable test exercises 8192 bytes). Larger
  planes need deliberate bounded slices in future work; 4K admission does not
  imply full 4K-plane watch capacity.
- Maximum 1,000,000 debug events and an explicit 1..300000 ms observation window.
  Success and event-boundary completion restore original debug registers before
  detach. Existing occupied debug registers are refused. An idle timeout, an
  incomplete initial attach, or failed cleanup **terminates the diagnostic worker**
  rather than detaching with an outstanding breakpoint. Debugger crash also keeps
  Windows' default kill-on-exit behavior. This affects only an explicitly watched
  worker; normal rendering defaults are unchanged.
- The observer opens its own report with create-new semantics. No worker-side
  report path or reopened inherited output handle is introduced. Module provenance
  records basename, load base, image size, and SHA-256 from the debug event's file
  handle (`file_sha256` is a file identity, not a hash of relocated image memory).

Schema `aexcompat.native-instruction-watch/1` includes config, module identity,
per-watch hit totals, completion/cleanup status and `memory_witnesses`:
`watch_id`, `instruction_rva`, `occurrence`, global `sequence`, `thread_id`,
`phase=instruction_before`, and `before.{status,address,size,hex,pointer_chain}`.
Missing occurrences remain missing witnesses with a failed report. Pointer failure,
range overflow and unreadable payload are explicit non-captured statuses; they
never publish partial hex. A successful attach is not a successful capture.

For a future guest diff, join on ID/RVA/occurrence, compare phase first, then
captured bytes and declared sizes. Guest call-site entry/return snapshots are not
automatically identical to a native pre-instruction snapshot (notably RSP at CALL).
Heap/stack addresses and thread IDs are run-local; preserve them as provenance,
not byte-equivalence expectations. No `after` record is fabricated.

Implementation follows Microsoft's [debug event suspension model](https://learn.microsoft.com/en-us/windows/win32/debug/writing-the-debugger-s-main-loop),
[attach breakpoint lifecycle](https://learn.microsoft.com/en-us/windows/win32/api/debugapi/nf-debugapi-debugactiveprocess),
and [event handle ownership](https://learn.microsoft.com/en-us/windows/win32/api/debugapi/nf-debugapi-waitfordebugevent).
These document the API contract; they are not evidence this adapter ran on Windows.

## macOS checks

Use a space-free checkout and an external disposable cargo target/TMPDIR. Limit
jobs to 1. Redirect command output to the ticket's scratch logs.

```sh
python3 -m unittest discover -s tools/olm -p 'test_*.py'
# 3 matrix runner tests, including wrong-reason/crash/timeout negative mutations
clang++ -std=c++20 -Iminihost/src -fsyntax-only tests/portable/geometry_contract_shape.cpp
cargo test --manifest-path broker/Cargo.toml -p aexcompat-harness --bin native-watch --locked --jobs 1 -- --test-threads=1
# 7 portable tests; no Windows adapter execution
cargo check --manifest-path broker/Cargo.toml -p aexcompat-harness --all-targets --target x86_64-pc-windows-msvc --locked --jobs 1
# Includes the Windows adapter and Windows-only tests; no linking/execution
```

## Windows-side Claude handoff

Windows execution is **pending**. The paired machine was offline (SSH timeout,
Tailscale offline); neither the native C++ Release worker nor debugger tests were
run there by this ticket. Do not turn the commands below into past-tense results.

1. Make an isolated Windows checkout of the delivered AEXCompat branch/commits.
   Preserve the existing Windows checkout. Set a private scratch path and
   `TEMP`, `TMP`, `CARGO_TARGET_DIR` there; run one build/test process at a time.
   Read `AGENTS.md`/`CLAUDE.md`, record `git rev-parse HEAD`, OS/CPU and tool versions.
2. In PowerShell, from that checkout, run:

```powershell
$env:PYTHONUTF8 = '1'
cargo test --manifest-path broker\Cargo.toml -p aexcompat-harness --bin native-watch --locked --jobs 1 -- --test-threads=1
# Seven portable tests pass; five Windows tests (including helper) are ignored.
cargo test --manifest-path broker\Cargo.toml -p aexcompat-harness --bin native-watch --locked --jobs 1 native_hardware_watch -- --ignored --nocapture --test-threads=1
# Expected: four runtime tests pass. They exercise real Windows debug APIs:
# second thread / Nth touch; unreadable pointer; missing occurrence; idle timeout.
# These expectations have NOT been observed yet.
cargo build --release --manifest-path broker\Cargo.toml -p aexcompat-harness --bins --locked --jobs 1
pwsh -File tools\build-native.ps1 -Target aex_worker
pwsh -File tools\build-native.ps1 -Target worker_render_lifecycle_selftest
.\target\minihost-build\worker_render_lifecycle_selftest.exe
.\target\minihost-build\aex_worker.exe --kind smart --self-test-pf-smart-geometry-rects
```

3. Retain stdout/stderr and exit codes for each test separately. A failed runtime
   test is a boundary, not permission to weaken a capture or geometry invariant.
   Stop and report the first failure before attempting real AEX observation.
4. For actual AEX capture, use the existing broker/GUI resident-session owner to
   open a CPU Smart session and hold frame dispatch until the observer is armed.
   Obtain **that session's** worker PID (not a discovery PID chosen from a list).
   This observer does not add an automatic launch/wait integration to the existing
   renderer. If the session owner cannot hold dispatch, that integration remains
   follow-up work; do not race an attach and call it a paired fixture.
5. Use `dumpbin /exports <AEX>` and retained disassembly to select the site; for
   exported `EffectMain` entry, RDX is PF_InData under the Windows x64 ABI. Record
   the AEX hash, selector/attachment boundary and exact typed matrix case. Create
   the JSON spec locally and launch in a second terminal:

```powershell
& "$env:CARGO_TARGET_DIR\release\native-watch.exe" $workerPid $specPath $newReportPath 30000
# Wait for native_watch_armed; then dispatch exactly the declared frame from
# the resident owner. Idle timeout can terminate this diagnostic worker.
```

6. For each native matrix case, map typed values to `RenderContextState`,
   `DiagnosticWorldLayout` and `WorldLayout` as recorded in the matrix. Use the
   existing `--headless --render-fixture <aex> <fixture.json> <new-output-directory>`
   route for declarative image/world fixtures and the native request context for
   spatial ratios/Smart output requests. Retain generated native requests plus
   captured world geometry, exact raw output hashes, module identity and watch
   report. The fixture generator currently automates guest runs; it does **not**
   automatically drive every native request. Mark cropped/odd-padding cells as
   structural gaps, not comparable successes, until a separate ticket closes them.
7. Return a machine-readable per-case table with actual Windows execution flag,
   exit/status, hashes, first differing byte/field (if any), and exact commands.
   Include worker/harness/AEX hashes and test reports. Do not reuse this ticket's
   static native column as execution evidence or claim native AE equivalence.
