# AEXCompat

Project direction, achieved milestones, current limitations, and roadmap are documented bilingually in [docs/PROJECT_DIRECTION.md](docs/PROJECT_DIRECTION.md). The Japanese text is authoritative.

AEXCompat is a cleanroom compatibility lab and staged isolated host for After
Effects `.aex` plug-in behavior.

The project is working toward faithful observable host compatibility rather
than emulating the whole After Effects application. It currently includes
static analysis, safety gates, an isolated native broker, cleanroom ABI workers,
parameter discovery, classic rendering, and SmartFX rendering.

## Start Here

- [Project Design](docs/PROJECT_DESIGN_2026-07-03.md) explains the target
  architecture, migration plan, safety gate, and implementation roadmap.
- [Host Core Boundary](docs/HOST_CORE_BOUNDARY_2026-07-13.md) separates generic
  AEX host behavior from fixture-specific conformance profiles and oracles.
- [Descriptor Manifest Promotion](docs/DESCRIPTOR_MANIFEST_PROMOTION_2026-07-13.md)
  regenerates reviewed parameter manifests from isolated L2 observations.
- [Docs Index](docs/README.md) describes the documentation layout.
- [Imported AviUtlas Contracts](imports/aviutlas-rust-contracts/README.md)
  preserves AEX/AEPX/OFX planning assets copied from AviUtlas as provenance.
- [Contract Provenance](contracts/PROVENANCE.md) tracks promoted contract
  documents and their source files.

## Safety Boundary

Unapproved inputs must not:

- load arbitrary `.aex` files or accept caller-controlled plug-in paths;
- call native entry points outside a fixed, hash-checked, stage-approved broker route;
- write `.aepx` or `.aep` projects;
- overwrite existing `target/` artifacts;
- publish private paths, binary payloads, raw payloads, or private image data.

The self-authored ScatterMap fixture has passed the staged gate through classic
and SmartFX ARGB8 rendering. MaskOffset independently passes load, lifecycle,
parameter discovery, and its SmartFX no-mask fallback. Every native run remains
local-only, hash- and size-bound, timeout-limited, Job Object-isolated, and
create-new for evidence.

## Repository Layout

- `tools/`: Python analysis, gate, fixture, oracle, and audit tools.
- `tests/`: unittest coverage for contracts, boundaries, and fail-closed behavior.
- `broker/`: Rust allowlist enforcement and Windows process isolation.
- `minihost/`: cleanroom C++ ABI workers for load, setup, classic, and SmartFX calls.
- `analysis/`: historical run logs and planning documents.
- `contracts/`: promoted local contract documents used by validators and future
  broker/worker reports.
- `imports/`: frozen provenance imports from AviUtlas. Treat these as read-only.
- `target/`: ignored local artifacts. Tools should create new files only.

## Common Commands

```powershell
python -m unittest discover -s tests
python tools/contract_schema_validator.py contracts
```

## Desktop Harness

The provisional Rust desktop harness identifies a selected `.aex`, runs the
no-load static probe, hashes the selected binary, and immediately exposes native
operations for registered and previously unknown AEX files. Native operations
are delegated to the existing isolated broker; the
UI never loads plug-in code.
For the registered ScatterMap profile, the harness also accepts PNG, JPEG, BMP,
TIFF, or WebP input, renders it through the Classic ARGB8 AEX path in the
isolated worker, previews the result, and writes a create-new PNG. Interactive
images are limited to 4096x4096 and 16,777,216 pixels.

An unregistered AEX is enabled automatically after selection. This experimental
route revalidates the file
immediately before launch and retains the worker timeout and pixel guards. It
uses the descriptors observed from that binary's own `PF_PARAMS_SETUP` rather
than a registered fixture schema; incompatible plug-ins fail rather than
receiving fabricated parameters. Process
isolation reduces accidental crash impact but is not a complete security sandbox.

For rebuild-driven development, the UI checks the selected file's size and
last-write identity every 500 ms. Any change marks the selection stale and
disables all native actions. `Reload rebuilt AEX` computes and displays the new
SHA-256, clears parameters and preview state, and requires another inspection.
Reload automatically adopts the rebuilt binary identity at the same path. It
never skips rehashing, and execution still stops if the file changes between
identity refresh and worker launch.

`Inspect parameters` runs `PF_PARAMS_SETUP` in a fresh isolated
worker without implicitly dispatching the optional `ABOUT` selector. This keeps
an ABOUT-only plug-in crash from suppressing parameter discovery. Supported
integer, checkbox, popup-choice, floating-point, color, angle,
2D point, and 3D point descriptors become typed controls using the observed
defaults and valid ranges.
The selected values are encoded by observed slot and revalidated by the render
worker against a fresh parameter setup before any render selector is dispatched.
Layer/input parameters and currently unsupported descriptor types remain on their
plug-in defaults and are not fabricated by the harness.
Arbitrary-data descriptors expose their bounded `PRINT` summary as editable text. The
worker now probes `SCAN` by requiring a new host-live handle, comparing it with
the printed source, and disposing it on every owned path. A plug-in that returns
success with a null handle, as the shipped SDK ColorGrid sample currently does,
rejects an edited value without failing an otherwise unedited render. Assignment
uses a bounded hex transport, requires a distinct host-owned SCAN result, and
disposes both replaced and rejected values. The cleanroom arbitrary scan fixture
passes assignment, PRINT/SCAN comparison, rendering, and balanced cleanup.
Path parameters are assignable as `0=None` or a 1-based index into the explicit
bounded mask scene attached to the request. The worker resolves the index to the
host-owned `PF_PathID` after parameter discovery and rejects missing masks before
dispatching the render selector. The UI and saved typed requests preserve this
selection without implicitly creating a mask.
Attaching a mask scene does not implicitly select its first mask. Descriptor
default `0` remains NONE, a valid positive default resolves by 1-based index,
and a default referring to a missing/deleted mask resolves to NONE. An explicit
typed assignment takes precedence over the descriptor default.

Audio-only Effects have a separate experimental path for 44.1 kHz mono signed
float32 samples. The broker validates finite input samples, revalidates the AEX
identity, confines worker output to broker-owned transport, and publishes the
result with create-new semantics only after AUDIO_SETUP, AUDIO_RENDER,
AUDIO_SETDOWN, guard, and checkout-lifetime checks succeed:

```powershell
cargo run -p aexcompat-harness -- --render-experimental-audio-request plugin.aex input.f32 output.f32 assignments.json
```

Layer-audio checkout uses the SDK request's `start_time`, `duration`, and
`time_scale` rather than exposing the whole sidecar. The host converts the
requested interval to a bounded 44.1 kHz sample window, zero-fills portions
before or after the supplied source, and owns the returned buffer until the
matching checkin. Empty windows are valid; arithmetic overflow and windows over
10,000,000 samples fail closed. The worker and broker reports expose the last
request, mapped sample range, silence count, and balanced lifetime counters.
The checkout callback can resample that transport to any unsigned 16.16 rate
from 1 kHz through 65.535 kHz, duplicate mono into stereo, and return unsigned
PCM, signed PCM, or signed float in the SDK-supported sample widths. Conversion
buffers remain bounded and unsupported format/width combinations fail before a
handle is published.
Up to 16 layer-audio handles may be live simultaneously. Each handle owns its
converted window independently and may be queried or checked in out of checkout
order. The worker rejects unknown and already-checked-in handles, records peak
occupancy and exhaustion, and requires the complete table to be empty before a
render can pass its lifetime contract.
`PF_GET_AUDIO_DATA` includes AE's observable trailing silent frame: the returned
sample count is the requested window length plus one. The sentinel is zero for
float and signed PCM, the midpoint for unsigned PCM, and is present in every
requested channel. Window telemetry excludes this sentinel while returned-frame
telemetry includes it.

Group start/end descriptors are rendered as section boundaries, and supervised
momentary-button descriptors are exposed as explicit UI actions. A button click
dispatches `PF_Cmd_USER_CHANGED_PARAM` with the observed slot in a fresh
five-second isolated worker; non-button, out-of-range, and non-supervised slots
are rejected before selector dispatch.

AEGP plug-ins use a distinct `EntryPointFunc` initialization ABI and are never
called through the effect-selector ABI. The experimental AEGP action currently
hosts only the observed Command Suite v1 and Register Suite v6 surface needed by
the owner-authored timeline fixtures. It records bounded menu/hook registrations,
does not invoke registered hooks, requires all suite leases to be released, and
runs in the same identity-revalidated five-second isolated worker.
The update-menu action additionally owns the registered callback/refcon pairs in
a 64-hook bounded table, invokes them with a `NONE` active-window context, and
hosts their Command Suite enable/check-mark calls. The idle action invokes one
registered idle tick, validates the returned sleep interval within 0..3600, and
retains default-deny suite acquisition. Registered death hooks are owned in a
separate 64-hook bounded table and invoked after the requested event but before
module unload; any death error or leaked suite lease fails the worker contract.
The command action dispatches the first registered menu command twice in one
worker (ON then OFF), preserving registered priority, command filters, refcons,
and `already_handled` propagation. Both passes must report handled before the
module can unload, preventing owner fixtures from leaving their sync thread live.
An active-idle roundtrip additionally toggles ON, performs one idle tick against
an observed empty-project Item Suite9 scene (`GetActiveItem` returns null), and
toggles OFF even if idle fails. The Item Suite layout and version 14 contract are
SDK-instrumented rather than inferred in the cleanroom worker.

The UI also offers `Classic` and `SmartFX` render paths. SmartFX transports the
same bounded RGBA8 input through `PF_Cmd_SMART_PRE_RENDER` checkout and
`PF_Cmd_SMART_RENDER`, requires valid result rectangles and intact guard bytes,
and writes the checked output world to PNG. A plug-in that supports only one path
can therefore fail that path explicitly without the harness silently substituting
the other selector family.

`Run 6-case compatibility matrix` executes Classic and SmartFX independently
at 8, 16, and 32 bpc. Every case receives a fresh isolated worker and a distinct
output path; crashes, timeouts, selector failures, and invalid SmartFX result
rectangles are recorded per row without stopping later cases. The same route is
available as `--render-experimental-matrix AEX INPUT OUTPUT_DIRECTORY`.
The worker follows AE's depth negotiation: 16 bpc requires
`PF_OutFlag_DEEP_COLOR_AWARE`, and 32-bpc float requires
`PF_OutFlag2_FLOAT_COLOR_AWARE`. A missing declaration is reported as
`unsupported_pixel_depth` without dispatching a render selector or creating an
output, rather than forcing an ABI world the plug-in did not opt into. Matrix
totals keep these rows in `unsupported_count`; they are excluded from
`applicable_count` and `failed_count`.

For pixel-fidelity debugging, the UI accepts an optional image rendered by
After Effects and compares it with the latest AEXCompat output. It reports an
exact-match result, differing-pixel count, maximum RGBA8 channel error, and mean
absolute channel error; dimension mismatches fail explicitly. The same bounded
comparison is available to automated conformance runs. It exits with code 0
only for a pixel-exact match, 2 for a same-size pixel difference, and 1 for an
invalid or differently-sized input:

```powershell
cargo run -p aexcompat-harness -- --compare-images ae-reference.png aex-output.png
```

Secondary Layer parameters can be bound by their observed slot for automated
Classic or SmartFX tests. Multiple `SLOT IMAGE` pairs are accepted; duplicate,
unknown, and non-Layer slots fail after isolated parameter inspection but before
any render selector is dispatched:

```powershell
cargo run -p aexcompat-harness -- --render-experimental-layer-slots plugin.aex input.png output.png 3 map.png 7 background.png
cargo run -p aexcompat-harness -- --render-experimental-smart-layer-slots plugin.aex input.png output.png 3 map.png 7 background.png
```

For mixed typed parameters, a strict JSON assignment document can set numeric,
ARGB8 Color, Angle/Point/Point3D components, and Layer slots atomically. The
document is limited to 64 KiB, rejects duplicate slots and unknown fields, and
is revalidated against a fresh parameter inspection before rendering. An
optional strict `timing` object selects frame `0..10000000`, FPS `1..1000`,
and an independent composition duration greater than the selected frame:

```json
{
  "schema_version": 1,
  "timing": { "frame": 30, "fps": 30, "duration_frames": 300 },
  "assignments": [
    { "slot": 1, "value": 25.0 },
    { "slot": 2, "color": [255, 220, 80, 30] },
    { "slot": 3, "components": [320.0, 180.0] },
    { "slot": 4, "layer": "map.png" }
  ]
}
```

Fractional frame rates use AE's exact integer timebase instead of a rounded
decimal FPS. For example, 29.97 fps is represented by `"time_scale": 30000`
and `"time_step": 1001` in place of `fps`. The UI provides 23.976, 29.97,
and 59.94 presets and displays the effective rate.

The same request may include a bounded `host_context.mask_scene` containing
explicit open or closed Bezier masks. Masks are never invented implicitly.
Connected contexts are available to AEGP Mask/Stream/Mask Outline suites in
both Classic and SmartFX workers, allowing effects such as PathArray to inspect
the effect layer's masks. Loading and saving a debug request preserves this
context; the UI shows the attached mask count and offers an explicit clear.

For spatially dependent effects, `host_context.spatial` supplies the exact
`PF_InData` rational values for `downsample_x`, `downsample_y`, and
`pixel_aspect_ratio`. Each value is a bounded `numerator`/`denominator` pair;
optional full-resolution width and height keep `PF_InData.width/height` distinct
from the downsampled image world. The desktop harness provides Full, Half,
Quarter, and D1/DV NTSC presets and derives the logical size from the input.
An optional paired pre-effect source origin reproduces trimmed input buffers
produced by earlier resizing effects without changing the physical image data.

`host_context.render_environment` can reproduce Low/High quality, full-frame
or upper/lower field rendering, and shutter angle/phase. Shutter values use
human-readable ratios in JSON and are transported as AE 16.16 fixed-point
values after bounded validation in both broker and isolated worker.

The desktop harness uses this same document format. After inspecting an AEX,
`Save debug request...` captures the current editable parameters, connected
Layer images, frame, and FPS. `Load debug request...` validates the complete
document before changing any control, making a failing Effect render directly
reproducible from both the UI and CLI.

Parameters carrying `PF_ParamFlag_SUPERVISE` dispatch
`PF_Cmd_USER_CHANGED_PARAM` when edited. The isolated worker receives the
current typed values for the complete parameter set before the selector runs;
returned `UPDATE_PARAMS_UI` disabled/hidden flags are then applied atomically
to the controls. The same path is reproducible without the UI:

```powershell
cargo run -p aexcompat-harness -- --trigger-experimental-request plugin.aex 8 assignments.json
```

```powershell
cargo run -p aexcompat-harness -- --render-experimental-request plugin.aex input.png output.png assignments.json
cargo run -p aexcompat-harness -- --render-experimental-smart-request plugin.aex input.png output.png assignments.json
```

An entire six-case matrix can render and compare in one isolated conformance
run. `AE_REFERENCE_DIRECTORY` must contain `classic-argb8.png`,
`classic-argb16.png`, `classic-argb32f.png`, and the equivalent three
`smartfx-*` files. Missing, malformed, differently-sized, or non-exact
references fail their row and produce exit code 2 after all six workers finish:

Every successful row also records whether the output changed pixels, was a
pixel-exact pass-through, or changed dimensions. This observation does not by
itself decide compatibility: generators and effects with an unconnected Path,
Layer, or Mask can legitimately pass through. AE reference comparison remains
the fidelity oracle.

```powershell
cargo run -p aexcompat-harness -- --render-experimental-reference-matrix plugin.aex input.png AE_REFERENCE_DIRECTORY OUTPUT_DIRECTORY
```

`tools/capture-ae-reference.ps1` creates one AE reference PNG through a
temporary unsaved project. It refuses to run while any AfterFX process exists
and requires the installed AEX SHA-256 to exactly match the tested binary, so it
cannot attach to or close an active user project. Example after closing AE:

```powershell
.\tools\capture-ae-reference.ps1 -AfterEffects "C:\Program Files\Adobe\Adobe After Effects 2025\Support Files\AfterFX.exe" -TestedAex plugin.aex -InstalledAex installed-plugin.aex -InputImage input.png -OutputPng reference.png -EffectName "Effect Match Name" -Frame 0 -Fps 30 -DurationFrames 300 -Bpc 8
```

Both interactive paths run inside a balanced effect-instance lifecycle. After
global and parameter setup, the worker performs `PF_Cmd_SEQUENCE_SETUP` and
`PF_Cmd_FRAME_SETUP`, forwards the returned sequence/frame data into the render,
then calls frame and sequence setdown in reverse order. A setup failure suppresses
the pixel selector while still releasing every lifecycle stage that started;
setdown failures also make the operation fail instead of being hidden. The UI's
bounded worker diagnostics identify each of these selector boundaries.

The Effect development UI can create 8-bpc ARGB32, 16-bpc ARGB64, or 32-bpc
floating-point ARGB128 input/output worlds for both Classic and SmartFX. Source
images remain ordinary RGBA files: the isolated worker converts channels to the
After Effects 16-bpc range `0..32768` or float range `0..1`, applies the same
depth to selected secondary layers, and converts the guarded result back to an
RGBA8 PNG. The report must echo the requested pixel format; a worker cannot
silently fall back to 8 bpc. Equivalent command-line smoke routes are
`--render-experimental-16`, `--render-experimental-32`, and their
`--render-experimental-smart-{16,32}` variants.

The complete 15-entry `PF GPU Device Suite` v1 table is hosted at the public
SDK ABI boundary. CUDA devices are enumerated through `cuDeviceGetCount` with a
hard limit of 16; each published device exposes its driver device and retained
primary context. The selected render device owns its GPU worlds and memory
operations, while mismatched or unavailable ordinals fail before dispatch;
exclusive access and allocation registries enforce
256-block and 256 MiB aggregate limits. CUDA framework 3 uses a System32-only
Driver API boundary, a selected primary context, real device allocations, and
explicit ARGB32f/BGRA128 upload and download. Other framework fixtures retain
bounded conformance storage without claiming hardware execution. Unknown
pointers and double frees are rejected.
Purge never invalidates memory still owned by a plug-in and therefore reports
zero bytes purged. A render cannot pass with a live allocation or unreleased
exclusive-access lease. The owner-authored `pf_gpu_memory_probe` fixture calls
every table entry and records balanced allocation and rejection evidence.
GPU setup, pre-render, render, and setdown receive the same negotiated framework.
SmartFX rejects a successful selector when its output remains at the host's
sentinel pattern or contains non-finite ARGB32f channels. The broker records
that attempt as `output_validation` and retries once in a fresh CPU worker;
only the validated fallback may publish an output image.
Adobe's unmodified `SDK_Invert_ProcAmp` source also builds as a CUDA-enabled
fixture through `tools/build-sdk-invert-cuda.ps1`. Its CUDA SmartFX result is
RGBA8-exact against the CPU path on the 37x23 oracle, with three device
allocations created and freed, zero live bytes, and no CPU fallback.
The same unmodified SDK fixture builds for OpenCL through
`tools/build-sdk-invert-opencl.ps1`. The System32 OpenCL loader enumerated one
GPU device and completed setup, SmartFX render, and setdown with three balanced
device allocations. On the 37x23 external-image oracle, BGRA128 upload and
download were 13,616 bytes each; the OpenCL and CPU paths produced identical
3,404-byte public RGBA8 output (SHA-256
`6f24052bf442cc05899fdfe3779514c610652c6ab1d8dcba083dbf36f9ad0617`),
with zero differing bytes and maximum error zero. Their internal float-world
hashes are not byte-identical, so conformance is claimed only for the observed
public RGBA8 output.

The image transport has been exercised against owner-built ScatterMap and
CMYKMisreg AEX binaries with a 37x23 variable-alpha RGBA image. Both Classic and
SmartFX produced validated PNG output with intact worker guards; CMYKMisreg also
exercises dynamic 2D point parameters through `PF PointParamSuite`.
AeGpuProbe also completes both CPU Classic and SmartFX paths with identical
input/output hashes. Its optional ABOUT selector currently aborts inside the
plug-in on an oversized message, while the independently isolated parameter
inspection and render operations remain available.
Observed `PF_Param_LAYER` descriptors are shown as optional secondary-image
inputs. The selected image is bound to its observed slot, converted to a guarded
ARGB8 world, and exposed through Classic checkout or the plug-in-assigned
SmartFX checkout ID. The bounded transport accepts up to eight secondary layers,
rejects duplicate slots before rendering, and keeps each image's dimensions and
checkout lifetime independent.

Parameter discovery uses dynamic storage with a 1024-descriptor safety cap.

Legacy `PF_Param_FIX_SLIDER` descriptors are decoded using the SDK's signed
16.16 layout and exposed as bounded floating-point controls. The same transport
converts edited values back to 16.16 with overflow rejection. Adobe's unmodified
SDK `Gamma_Table` example was built as a test-only fixture: its Gamma descriptor
matched range `0..2`, default `1`, precision `1`, and a non-default `1.5` render
completed through the host's bounded ARGB8 `copy`, `iterate`, and ANSI `pow`
callbacks with intact guards and balanced sequence/frame lifecycle.

The host advertises SDK specification version 13.28 rather than a zero/legacy
version. Adobe's `Paramarama` fixture consequently exposes its modern-only 3D
point and button controls. The unmodified SDK fixture now provides a fixed
cross-type oracle for integer, color, float, checkbox, angle, popup, 3D point,
and supervised button parameters. Its amount-zero World Transform copy is
pixel-exact, its amount-93 convolve changes pixels at both advertised Classic
8/16-bpc depths, and its supervised button returns the expected host message.
The digest-bound evidence is recorded in
`analysis/SDK_PARAMARAMA_PARAMETER_MATRIX_RESULT_2026-07-15.json`.
`PF_Param_CUSTOM`, `NO_DATA`, `ARBITRARY_DATA`, and
`PATH` descriptors are retained in inspection results as explicit read-only
controls instead of disappearing or being misrepresented as sliders. Parameter
inspection uses a dedicated `GLOBAL_SETUP -> PARAMS_SETUP -> GLOBAL_SETDOWN`
worker route, so a plug-in such as SDK `PathMaster` can expose its Path descriptor
even when its later sequence lifecycle requires project suites not yet connected.
Modern AE no longer specifies the legacy 127-parameter limit; this bounded cap
supports large owner-built effects such as ParticleLab while preventing
unlimited allocation. SmartFX output is allocated from the validated PreRender
maximum result rectangle, including negative origins and output dimensions that
differ from the input. The same 4096x4096 and 16,777,216-pixel limits apply.

Effects advertising `PF_OutFlag_SEND_UPDATE_PARAMS_UI` receive
`PF_Cmd_UPDATE_PARAMS_UI` with their host-owned parameter array. The bounded
`PF Param Utils Suite` v3 accepts `PF_UpdateParamUI` only during that selector
for an observed slot. Updated disabled and invisible UI flags are reflected by
the Rust controls. `PF_Cmd_QUERY_DYNAMIC_FLAGS` is likewise dispatched only
when its GlobalSetup capability bit is advertised.

The image harness exposes frame and FPS controls and transports validated
`current_time`, `time_step`, `total_time`, and `time_scale` values through both
Classic and SmartFX. Interactive native rendering has a 30-second timeout while
parameter inspection remains capped at 5 seconds. A timeout kills only the
worker and does not replace or overwrite the requested output PNG.

The harness can also probe two Classic frames inside one persistent effect
sequence. This route performs one `SEQUENCE_SETUP`, two bounded
`FRAME_SETUP -> RENDER -> FRAME_SETDOWN` cycles at consecutive times, and one
`SEQUENCE_SETDOWN` in the same isolated worker. Adobe's `Gamma_Table` fixture
proves that its lookup-table `sequence_data` handle survives both frames and is
disposed exactly once; see
`analysis/SDK_GAMMA_PERSISTENT_SEQUENCE_RESULT_2026-07-15.json`.

Sequence save/reload can be probed separately through
`SEQUENCE_SETUP -> SEQUENCE_FLATTEN -> SEQUENCE_RESETUP -> RENDER ->
SEQUENCE_SETDOWN`. The host validates that both plug-in transitions replace the
handle, disposes the flattened host-owned handle after resetup, and rejects any
unbalanced lifetime. Adobe's `PathMaster` proves a three-handle/three-dispose
roundtrip followed by a valid Path render; see
`analysis/SDK_PATHMASTER_SEQUENCE_FLATTEN_RESULT_2026-07-15.json`.

Modern non-destructive saves use `GET_FLATTENED_SEQUENCE_DATA` instead. The
host keeps the running unflattened sequence alive, owns and disposes only the
returned flat copy, then proves the original instance can still render before
`SEQUENCE_SETDOWN`. PathMaster verifies the resulting two-handle/two-dispose
contract in
`analysis/SDK_PATHMASTER_NONDESTRUCTIVE_SEQUENCE_SAVE_RESULT_2026-07-15.json`.

External project dependencies can be queried from the harness in either ALL or
MISSING mode. `GET_EXTERNAL_DEPENDENCIES` runs behind an SEH and five-second
worker boundary; returned strings must use a live, host-owned, NUL-terminated
handle no larger than 64 KiB. Convolutrix proves the returned-handle path and
Resizer proves that a null handle is a valid empty MISSING result. Evidence is
recorded in `analysis/SDK_EXTERNAL_DEPENDENCIES_RESULT_2026-07-15.json`.

An effect options dialog can be probed from the harness only when Global Setup
advertises `PF_OutFlag_I_DO_DIALOG`. The selector runs behind an SEH and
five-second worker boundary; unadvertised effects are rejected without
dispatch. The SDK Checkout fixture verifies its returned message and
`PF_OutFlag_DISPLAY_ERROR_MESSAGE`, while Resizer verifies the refusal path.
Evidence is recorded in
`analysis/SDK_CHECKOUT_OPTIONS_DIALOG_RESULT_2026-07-15.json`.

The separate apply-time dialog path honors `PF_OutFlag_SEND_DO_DIALOG` only
after a successful Sequence Setup and only when Global Setup also advertises
`PF_OutFlag_I_DO_DIALOG`. The owner-authored cleanroom fixture verifies the
complete sequence/dialog/setdown order, while Checkout verifies that capability
advertising alone never triggers an automatic dialog. Evidence is recorded in
`analysis/PF_AUTOMATIC_OPTIONS_DIALOG_RESULT_2026-07-15.json`.

Classic image rendering now consumes `PF_OutFlag_NOP_RENDER`. Such effects keep
their instance Sequence Setup/Setdown lifecycle, but receive no Frame Setup,
Render, or Frame Setdown selector; the host emits an exact guarded copy of the
source image instead. The cleanroom fixture deliberately fails if any forbidden
classic or SmartFX frame selector is dispatched. SmartFX NOP_RENDER also skips
SMART_PRE_RENDER and SMART_RENDER while publishing full-source result rectangles.
Evidence is recorded in
`analysis/PF_NOP_RENDER_PASSTHROUGH_RESULT_2026-07-15.json`.

Classic and SmartFX source worlds now use dedicated virtual-memory storage.
They are read-only during plug-in execution unless Global Setup advertises
`PF_OutFlag_I_WRITE_INPUT_BUFFER`, in which case the private source copy is
writable while remaining separate from the output. A paired cleanroom fixture
proves permitted mutation and verifies that an unadvertised write is contained
as `0xC0000005` inside both classic RENDER and SmartFX SMART_RENDER workers.
The SmartFX fixture also verifies PRE_RENDER checkout rectangles before the
write boundary. Evidence is recorded in
`analysis/PF_INPUT_BUFFER_WRITE_RESULT_2026-07-15.json`. `WORKS_IN_PLACE`
aliasing is intentionally not inferred without an Adobe contract or fixture.

```powershell
cd broker
cargo run -p aexcompat-harness
```

The same image path can be smoke-tested without opening the GUI:

```powershell
cargo run -p aexcompat-harness -- --render-image input.png output.png
```

AEGP validation includes both an empty-project active-idle roundtrip and a
deterministic non-empty scene containing one active comp at frame 1/30 and three
typed layers. The comp scene exposes the observed Item Suite 9, Comp Suite 11,
and Layer Suite 5/9 read boundaries; unsupported calls return an AE error and
unknown suites remain default-deny. Both modes toggle the plug-in ON and OFF in
an isolated worker before unload.

The non-empty scene also exposes full-size, SDK-observed Comp Suite 12, Layer
Suite 8, Effect Suite 4, and Stream Suite 6 tables. Their unsupported entries
return an AE error rather than a null function pointer, allowing AeTimelineSync
to complete layer, empty-effects, and empty-transform snapshots. Cached
read-suite leases are accepted only from this allowlist, with per-suite and
aggregate bounds, and are reclaimed when the isolated worker exits.
The scene contains one active synthetic effect (`AEXCompat.Probe`) on its
first layer. The other two layers report empty effect stacks. Layer handles have
stable indices and IDs, share the active parent comp, support ID lookup, and
report no parenting relationship. Effect Suite 4 exposes enumeration, installed metadata, flags,
and an owned effect reference; Stream Suite 6 reports only the required input
pseudo-stream. The worker rejects duplicate acquisition and requires every
effect reference to be disposed before the roundtrip can pass.
Transform snapshots expose seven owned streams for each layer: position, anchor, scale,
rotation Z/X/Y, and opacity. The deterministic 2D values are `(320,180)`,
`(0,0)`, `(100,100)`, zero rotation, and `100` opacity. Stream references and
their sampled values have separate lifetimes; all seven selectors must be
observed and every value must be disposed before its stream can be disposed.
The synthetic effect has four non-keyframed user parameter streams: OneD
`Amount=42.5`, TwoD `Center=(160,90)`, ThreeD `Vector=(1,2,3)`, and Color
`Tint=(0.25,0.5,0.75,1)`. Current AeTimelineSync builds exercise 67
Stream/Value ownership roundtrips across three ticks. All four owned UTF-16
name handles are read and freed through the common Memory Suite.
`Amount` reports two keyframes while every other effect and transform stream is
non-keyframed. AeTimelineSync checks keyframe counts for all 67 streams, and the
conformance contract requires exactly one positive stream report.
The read-only Keyframe Suite surface also exposes `Amount` keys at frames 0 and
60 with values 10 and 90 and Linear/Hold interpolation. Its SDK-observed time,
value, and interpolation slots are exercised only by the explicit keyframe
roundtrip, so ordinary validation never takes over the user's global
timeline-sync pipe. The dedicated worker creates one bounded instance of
`\\.\pipe\ae-timeline-sync`, sends the 32-byte protocol-v2 keyframe request,
and reads the bounded 192-byte Keyframes Snapshot response. The response must
identify comp `1001`, layer `2001`, effect `AEXCompat.Probe`, and property
`Amount`, with frame 0 Linear/value 10 and frame 60 Hold/value 90. It also
requires two successful time, value, and interpolation reads. This adds four
stream acquisitions, two sampled values, and one parameter-name handle to the
normal scene totals: 71/71 streams, 69/69 values, and 26/26 memory handles.

```powershell
cargo run -p aexcompat-harness -- --dispatch-experimental-aegp-keyframe-roundtrip path\to\AeTimelineSyncAEGP.aex
```
The explicit seek roundtrip sends a bounded protocol-v2 host request for frame
75 at 30 fps. It requires an exact type-6 ACK and one main-thread
`AEGP_SetItemCurrentTime` call with native item time `75/30`; the ordinary
comp-idle path never creates or claims the global pipe.

```powershell
cargo run -p aexcompat-harness -- --dispatch-experimental-aegp-seek-roundtrip path\to\AeTimelineSyncAEGP.aex
```
The explicit trim roundtrip sends a protocol-v2 request for layer `2001`, changing
its comp-time span from frames 0–300 to 30–240. The worker requires one
`AEGP_SetLayerInPointAndDuration` call, final in-point `30/30`, duration
`210/30`, and an exact type-16 acknowledgement before disconnecting the pipe.

```powershell
cargo run -p aexcompat-harness -- --dispatch-experimental-aegp-trim-roundtrip path\to\AeTimelineSyncAEGP.aex
```
The explicit switch roundtrip toggles VIDEO_OFF, AUDIO_OFF, LOCKED, and SOLO on
layer `2001`. It validates AE's inverted active-bit semantics: the initial AEGP
flags `0x5` become `0x4026`, while the protocol ACK reports `0x32`. Exactly four
typed `AEGP_SetLayerFlag` calls are required and the other layers remain unchanged.

```powershell
cargo run -p aexcompat-harness -- --dispatch-experimental-aegp-switch-roundtrip path\to\AeTimelineSyncAEGP.aex
```
The layer tree now exposes an AV layer with video and effects active, Normal
transfer mode, an in-point of `0/30`, and a duration of `300/30` (10 seconds).
All five attribute callbacks validate the typed layer handle and comp-time mode;
their invocation is required by the Rust comp-idle conformance contract.
Comp-idle runs three persistent idle ticks between a single ON/OFF command pair.
The host advances the deterministic playhead from frame 1 through frame 3 at
30 fps while keeping time stable within each tick. Current AeTimelineSync
therefore resamples all seven transforms on each playhead change; older builds
that do not publish transforms still exercise the same three-tick lifecycle.
Each active tick is preceded by an update-menu dispatch and must report the
command enabled and checked. A final update-menu dispatch after the OFF command
must report enabled but unchecked, making the visible menu transition part of
the conformance contract rather than telemetry only.
Two of the three layers are selected. Comp Suite selection lookup and Collection
Suite 2 expose a fresh owned collection on every tick; AeTimelineSync enumerates
both layer items and disposes each collection. The host requires three creates,
six item reads, three disposals, and no live collection at unload.
Each layer also exposes distinct UTF-16 layer and source names through two
AEGP Memory Suite handles. AeTimelineSync reads nine name pairs across three
ticks; the conformance contract requires all 18 bounded handles to be unlocked
and freed before unload.
The active comp item is named `AEXCompat Composition` and has a native duration
of `300/30` (10 seconds). Three item-name handles plus the 18 layer/source-name
handles plus four effect parameter names make 25 balanced AEGP memory allocations during the three-tick current
AeTimelineSync roundtrip.

Classic `PF_Cmd_FRAME_SETUP` output resizing is gated by the SDK's
`PF_OutFlag_I_EXPAND_BUFFER` and `PF_OutFlag_I_SHRINK_BUFFER` advertisements.
The host reallocates guarded output worlds only for advertised direction changes;
unadvertised expansion or shrink requests fail before `PF_Cmd_RENDER`. Cleanroom
positive and negative fixtures preserve this contract in
`analysis/PF_FRAME_RESIZE_FLAG_RESULT_2026-07-15.json`.

After approving an AEX in the harness, use **Probe FRAME_SETUP expansion** or
**Probe FRAME_SETUP shrink**. The equivalent isolated CLI probes are:

```powershell
cargo run -p aexcompat-harness -- --probe-experimental-expand-buffer path\to\effect.aex
cargo run -p aexcompat-harness -- --probe-experimental-shrink-buffer path\to\effect.aex
```

Temporal parameter and layer checkout is also admission-gated. Classic effects
must advertise `PF_OutFlag_WIDE_TIME_INPUT` before calling `PF_CHECKOUT_PARAM`
at a non-current time. SmartFX may instead advertise
`PF_OutFlag2_AUTOMATIC_WIDE_TIME_INPUT`; unadvertised non-current
`checkout_layer` calls are rejected without creating a checkout. The SDK
Checkout effect and four cleanroom positive/negative paths are recorded in
`analysis/PF_WIDE_TIME_INPUT_RESULT_2026-07-15.json`.

Shutter angle and phase are always transported in `PF_InData`.
`PF_OutFlag_I_USE_SHUTTER_ANGLE` is tracked separately as a render dependency;
it does not gate field visibility. AEXCompat does not yet retain a frame cache,
so the future cache-key requirement and paired cleanroom observation are recorded
in `analysis/PF_SHUTTER_DEPENDENCY_RESULT_2026-07-15.json`.

Visual effects must advertise `PF_OutFlag_I_USE_AUDIO` before using the layer
audio callbacks. Audio-only effects are exempt, matching the SDK Backwards
sample. The admission and source-availability reasons are reported separately;
image-render audio sidecar transport is documented in
`analysis/PF_VISUAL_AUDIO_ADMISSION_RESULT_2026-07-15.json`.

For a classic ARGB8 image render with mono float32 little-endian 44.1kHz audio:

```powershell
cargo run -p aexcompat-harness -- --render-experimental-image-audio-sidecar effect.aex input.png input.f32 output.png
```

Both broker and worker reject empty, oversized, incorrectly sized, or non-finite
sidecars before selector dispatch. Temporary image/audio transports are removed
after the isolated worker completes.

The desktop harness exposes the same path through **Select visual audio sidecar
(.f32, optional)** beside the input image. Quick render and save-to-PNG use the
selected sidecar; incompatible SmartFX, deep-color, host-context, or custom-UI
combinations are rejected before a worker starts.

### DirectX SDK fixture

The unmodified Adobe SDK `SDK_Invert_ProcAmp` fixture can be rebuilt with
DirectX enabled by running `tools/build-sdk-invert-directx.ps1`. The script
compiles both HLSL kernels with DXC, places their `.cso` and `.rs` files in
the `DirectX_Assets` directory adjacent to the AEX, and records build hashes.

On the measured two-hardware-adapter host, adapter index 0 completed device
setup, Smart Pre-Render, Smart Render, and device setdown without an error. The
37x23 DirectX and CPU external image paths produced byte-identical 3404-byte
RGBA8 output, all three device allocations were freed, no live allocation
remained, and no SEH exception occurred. Exact fixture identity, output hashes,
and the internal 16x12 device-world smoke result are recorded in
`analysis/SDK_DIRECTX_DEVICE_WORLD_RESULT_2026-07-16.json`.

### Sampling and fill runtime evidence

The owner-authored Sampling and Fill Matte premultiply probes completed in the
AEXCompat image runtime with exit code 0, render error 0, intact guards, and
balanced suite leases. The measured artifact identities, output hashes, area
sampling callbacks, and frozen suite slots are recorded in
`analysis/PF_SAMPLING_FILL_RUNTIME_RESULT_2026-07-16.json`. This establishes
AEXCompat runtime success only; comparison against an After Effects pixel
oracle is pending.
