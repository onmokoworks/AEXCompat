# Platform parity audit — 2026-10-26

Status: partial. Source inventory at `olm/F54c-resident-watches` (`3ab60261`),
with X01 changes recorded below. This is not a claim of full parity or AE exactness.
`yes` means an implementation/entry point exists, not that all plug-ins work.
`partial` means a bounded subset, different reachability, or unverified equivalent.
Windows runtime behavior has not been tested in this audit. Source observations are
FACT; gap classification and proposed work are INFERENCE. Paths are repo-relative.

## GUI and image transport

| Capability | Windows | macOS | Evidence / gap disposition |
|---|---|---|---|
| AEX selection / parameter discovery | yes | yes | `harness/src/windows/app.rs`; `macos.rs::choose_aex`, `discover_parameters` (paths below `broker/crates/`) |
| Scalar, popup, checkbox, point, color, layer controls | yes | yes | `harness/src/shared_ui.rs`, `shared_descriptor.rs`, `macos.rs::fixture_parameter_payload`; specific unsupported descriptor kinds remain explicit |
| Parameter reset / debounce / viewers / zoom / pan | yes | yes | `harness/src/gui_state.rs`; Windows `cli.rs`; `macos.rs::show_viewer_texture` |
| Supervised parameter change | yes | partial | Native UI dispatcher vs `macos.rs::dispatch_user_changed`; Mac applies visibility/enabled flags, not all mutable descriptor fields. (a) |
| Effect chain: independent stage state, order, bypass, deep intermediates | yes | no | `harness/src/windows/effect_chain.rs`; no Mac chain renderer. (a) Requires shared stage model and raw-world transport, not repeated preview PNG conversion |
| Image formats: PNG/JPEG/BMP/TIFF/WebP | yes | partial | Windows `cli.rs::is_supported_input_image`; Mac chooser/drop admits PNG only although decoder supports more. (a) |
| 8-bpc GUI render and preview | yes | yes | Windows image_render; Mac resident ARGB8 adapter |
| 16-bpc GUI render | yes | no (base) | Mac enum/fixture exists but selector omits it and render returns fixture-only error. (a) X01 closes raw export route |
| 32-bpc GUI render | yes | yes | Mac `save_argb32f_exr_slot`; Windows native frames |
| Deep source precision | yes | partial (base) | Windows `image_render/native_input.rs::from_decoded` retains depth; Mac `encode_resident_image` reduced deep source to RGBA8. (a) X01 shares the depth conversion |
| Deep preview / PNG and raw sidecar export | yes | partial | Windows `effect_chain.rs` exports 16-bit PNG/raw and float display PNG/raw; Mac float EXR/raw fixtures, no deep preview. (a) |
| EXR float artifact | yes | yes | Shared `broker/src/image_render/artifacts.rs`, native fixture and Mac fixture/GUI |
| Debug request save/load, host context editor | yes | partial | Windows `render_contract.rs` / `cli.rs`; Mac `--render-fixture` consumes declarative fixtures, no equivalent general request editor. (a) |
| Audio / timeline / animated parameter workflow | yes | no | Windows app/CLI and broker `parameter_animation.rs`; no equivalent Mac GUI transport. (a), not inherently Windows-only |
| Custom UI draw/click/drag/key probes | yes | no (base GUI) | Native `worker_ui_event_execution.cpp`, Drawbot runtime; U03 adds guest `ui-session` in separate branch `olm/U03-ui-session` (`888e4381`), not this base. (a) Integrate/review U03 before building GUI adapter |
| Compatibility matrix / GPU / performance diagnostics UI | yes | partial | Windows `app.rs`, `ui.rs`, `cli.rs`; Mac worker report + analysis pane, no equivalent matrix/probe controls. (a) |

## Worker commands, worlds and diagnostics

| Capability | Windows native | macOS Unicorn | Evidence / gap disposition |
|---|---|---|---|
| Inspect/setup/discovery, Classic and SmartFX | yes | yes | `minihost/src/l2_main_entry.inc`; guest `main.rs` inspect/setup/render/session routes |
| Resident session / user_changed_param | yes | yes | Native session dispatcher; `guest/.../resident.rs` and harness resident adapter. X01 accepts and validates F54 `rect` responses; base GUI rejected them |
| Direct CLI vocabulary | partial | partial | Native leading `--kind discovery|classic|smart`, broker selection/request transport; guest inspect/setup/trace-selector/render/render-png/render-trace-png/render-region-png/census-png/session. (c) Existing vocabulary retained for compatibility; (a) shared capability adapter still needed |
| 8/16/32-bpc input and output | yes | yes | Native `render_subsystem.h`; guest `FramePixelFormat` and resident session; support per plug-in still negotiated |
| Mixed-depth primary/layers and captured-world fixtures | yes | yes | `broker/src/render_fixture.rs`, native `image_render/declarative_fixture.rs`, Mac `render_single_fixture_headless` |
| Downsample / pixel aspect | yes | yes | Native `render_subsystem.h::SpatialRatio`, `l2_main_entry.inc` classic/smart inputs; guest `geometry.rs`, session `--downsample-x/y`, `--pixel-aspect`. CLI schemas differ; equivalence untested |
| Output origin / negotiated output | yes | partial | Native `render_subsystem.h::validate_output_origin`; guest `--negotiate-output`. Mac GUI validates output against original input dimensions, so negotiated GUI output is not wired. (a) |
| Cropped checkout / request rect / extent / stride | partial | yes | Native request/context/render subsystems and fixture world layout; guest session `--checkout-rect`, `--request-rect`, `--extent-hint`, `--rowbytes-pad`. Matching native public contract for each F54 option remains unverified. (a) differential fixture needed before additions |
| 4K worlds | yes | partial | Native `native_input_tests::native_decode_admits_4k_deep_images_beyond_legacy_allocation_limit`; F54 guest enlarged arena, but Mac GUI `MAX_WIDTH=1920`, `MAX_HEIGHT=1080`. (a) admission + resource budget + 4K GUI regression |
| Host callback/selector trace | yes | yes | Native `TraceWriter` in `l2_main_entry.inc`; guest trace-selector / render-trace-png; report schemas differ. (a) normalize observations, not backend implementation |
| Instruction census / RVA pointer-chain and full-plane watches | no equivalent public route | yes | Guest `main.rs::parse_trace_watches`, x64 trace machinery and F54c resident watches. (a) native instrumentation is separate engineering, not a flag alias |
| Controlled numeric patches | partial | yes | F54b guest math/ISA experiments do not establish Windows UCRT equivalence. (c) intentional oracle experimental policy; do not apply emulation replacements to native DLL math to create artificial parity |
| Loaded byte identity / diagnostics | yes | yes | Native build/module reports; guest PE diagnostics and harness hashes. Recorded provenance, not approval gate |
| GPU backends | partial | partial | Native CUDA/OpenCL/DirectX; guest `--render-backend cpu|opencl|wgpu-metal`. (b) backend API availability; (a) equivalent capability/error reporting and shared software fallback UX |
| Native x86_64 carrier on Mac | n/a | yes (opt-in) | `macos.rs::guest_worker_candidates`, trusted/native flags. (b) execution technology differs; native Windows route is the counterpart |

## Host-suite coverage

These rows compare families, not every ABI slot. Full per-slot behavioral parity
remains open; a suite's presence must never be treated as proof all slots work.

| Family | Windows | macOS guest | Code evidence / gap disposition |
|---|---|---|---|
| Handle ownership, World, Iterate8/16/float | yes | yes | `worker_pf_world_suite.cpp`, `worker_pf_suites.cpp`; guest `x64/iterate_and_suites.rs` |
| Param utilities / ColorParam / PointParam / ANSI / Effect UI | yes | partial | Native `worker_host_suite_wiring.cpp`; guest acquire dispatch in `iterate_and_suites.rs` supports specific versions. (a) version/slot census needed |
| Drawbot supplier/surface/path/custom UI | yes | no (base) | Native `worker_drawbot_runtime.cpp`, catalog; absent guest acquisition in base; U03 bounded event support must not be read as full Drawbot. (a) |
| Arbitrary data / stream values | partial | no demonstrated equivalent | Native `worker_aegp_compat_selftests.cpp` explicitly tests unsupported arbitrary stream values; no guest equivalent identified. (a) inspect lifecycle selectors before claiming native completeness |
| Color settings / ACE / color conversion | partial | partial | Native `worker_color_settings_runtime.cpp`, `worker_aefx_ace_suite.hpp`, color selftests; guest ColorParam only does not imply ACE/profile support. (a) |
| AEGP Memory / Utility / Persistent Data | yes | partial | Native host suite catalog; guest `iterate_and_suites.rs` publishes bounded versions, explicit unsupported callbacks. (a) |
| Compute cache | partial | partial | Native `worker_compute_cache_suite.hpp`; guest table implements class registration, unsupported callbacks remain. (a) |
| AEGP composition/scene/stream/item/layer rendering | partial | no equivalent family | Native `worker_aegp_scene_model.hpp`, item/layer runtimes; guest limited suites. (a), not Windows-only by nature |
| GPU suite and native graphics interop | partial | partial | Native GPU suite providers; guest `x64/gpu_suite.rs`. (b) graphics handles differ; (a) semantic coverage remains |

## Platform-specific mechanisms and intentionally different contracts

(b) Windows DLL loading/staging, Job Object, private desktop/modal sweep and
inherited HANDLE transport have no literal Unix API twin. Mac counterpart is a
separate controlled worker, process-group termination, resource limits and
bounded IPC (`harness/src/macos_worker_controller.rs`, `process_group_cleanup.rs`).
Neither is a confidentiality sandbox. Old parity text describing dependency
approval/sealing as a necessary security boundary is superseded by CLAUDE's
record-not-enforce policy. Do not recreate removed receipt/hash launch gates.

(c) Existing native and guest CLI/protocol spellings, raw fixture ARGB vs native
chain RGBA sidecars, and controlled oracle math remain explicit contracts. A
shared UX should adapt these contracts with byte-preserving conversions and
provenance rather than silently changing existing defaults.

## Closure sequence and acceptance

1. X01: shared deep decoding; compare all 256 legacy 8-bit samples, fine 16-bit
   words and float HDR/signed-zero bytes; verify Mac PNG-to-slot behavior.
2. X01: expose resident ARGB16 raw export; validate size/hash before publishing,
   protect existing output, test real AEX resident rendering with registered worker.
3. Remaining implementable work: Mac chains with raw intermediates and stage-local
   parameters/layers; image format chooser; deep preview; 4K admission; U03 UI
   integration; descriptor updates; audio/timeline and diagnostic UX.
4. Native F54 counterpart work requires a paired typed fixture matrix establishing
   which geometry/checkout cases already work, then explicit native watch transport.
5. Per-slot host-suite audit and behavioral probes are still required. This source
   inventory is a starting map, not an exhaustive audited slot list.

No registered reference worker or release gate is changed. No Windows runtime,
AE equivalence, full corpus replay, or full GUI parity claim is made.

## Implemented and measured in X01

- `1cc68cc2`: common depth conversion used by native Windows input and Mac
  input slots. Mac no longer truncates 16-bit input to eight bits. Nonfinite
  float input is rejected before slot publication; finite HDR is retained.
- `8238543c`: GUI selector and resident ARGB16 raw artifact publication, retaining
  exact ARGB words plus metadata. Deep preview and PNG export remain open.
- `25e907c0`: optional F54 `rect` response is accepted only when it equals the
  requested zero-origin full frame; legacy replies without it remain accepted.
- FACT: harness unit suite: 82 passed, 0 failed, 3 ignored; shared conversion
  tests: 2 passed; explicit real resident ARGB16 test: 1 passed.
- FACT: the first real F54c run failed at JSON decode (`unknown field rect`),
  before output publication. After the third change the test passed, matching
  exported raw bytes' SHA-256 to the worker's reported world hash.
- FACT: `cargo check --manifest-path broker/Cargo.toml -p aexcompat-broker --lib
  --target x86_64-pc-windows-msvc --locked --jobs 1` passed. The shared Windows
  call-site change is **untested on Windows**; cross-check is not runtime evidence.
- Worker SHA-256: `3d5eec0c6792f176a9cfbc38093c53b852f4ea1baae2cfd313345c02f55d6fba`
  (matches registered worker-F54c). ColorKeep 2025 AEX SHA-256:
  `6d3718868c6c876c3bb370b19cb2bb3c4f89a3a479c29f03ae0d032a5d043b86`.
- The real AEX test validates GUI adapter/worker/export transport; it is not
  native AE numerical equivalence or exhaustive corpus replay.

## Draft claim comment (not posted)

X01 platform parity audit and initial macOS transport fixes. Scope: audit GUI,
worker and suite asymmetries against F54c, share deep input conversion, enable
ARGB16 resident GUI raw export, and validate F54 output rectangles. Acceptance:
behavioral byte-preservation/error tests, registered-worker integration,
macOS build, Windows compile check, and independent local diff review. Remaining
chain, UI, suite and geometry work stays explicitly open. No issue number has
been invented and no claim has been posted.

## Draft PR text (not posted)

Title: Audit platform parity and close macOS deep-image transport gaps

The macOS GUI rejected resident ARGB16 exports and decoded deep inputs through
8-bit pixels. It also rejected F54 resident replies containing `rect`. Share
Windows' existing depth conversion, publish validated ARGB16 raw artifacts, and
accept only full-frame zero-origin rectangles. Existing eight-bit conversions
remain byte-identical. The audit separates missing UX/host behavior from native
platform mechanisms and documents remaining work without declaring full parity.

Validation: 82 harness tests, 2 shared conversion tests, and 1 real ColorKeep/F54c
resident export test passed. Windows broker target check passed; Windows runtime
is untested. No registered worker, plugin kernel, or release gate changed.

Independent local review found nonfinite float admission and a wrong raw-export
completion message; both were fixed and re-reviewed with no remaining findings.
Reviewed code boundary: `25e907c0`. No rejected findings. This is a partial X01
implementation; effect chains, full per-suite parity, audio/timeline, 4K GUI and
native watch tooling are not completed. Native runtime tests are required before
claiming Windows behavior.

Whole tracked Rust formatting check exits 1 on inherited guest `x64/engine.rs`,
`x64/tests_cases.rs`, and `x64/trace.rs`. All three are byte-unchanged from
`3ab60261` (`git diff 3ab60261 --exit-code -- <paths>` exits 0). Format check
of all four changed Rust files passes. No unrelated formatting rewrite applied.

FACT: macOS `cargo build --release --manifest-path broker/Cargo.toml -p
 aexcompat-harness --locked --jobs 1` passed. The resulting Release executable's
`--describe-aex` with registered F54c and ColorKeep produced a valid descriptor
packet (defaults, parameters, plugin_identity, schema, schema_version). No GUI
visual QA or Windows execution is implied by this headless smoke check.

## X01b continuation — implemented deltas

FACT: the earlier table describes the X01 starting boundary. The following rows
supersede its affected cells; unlisted gaps remain open. Full parity is **not**
achieved. No native AE equivalence is claimed.

| Capability | Windows | macOS after X01b | Disposition / code |
|---|---|---|---|
| GUI effect chains, order/bypass, independent scalar/default/layer state | yes | yes | `harness/src/macos_chain.rs`; common `gui_state::EffectChainSlot`. At most eight stages, sequential workers; exact typed ARGB intermediates, per-stage checksum and provenance, final PNG/raw/EXR. Supervised warmup never publishes a single stage as chain output. Deep preview remains a separate gap. |
| 4K UHD GUI | yes | yes | `macos.rs::validate_resident_image_dimensions`: 3840x2160, matching guest bounds. Check before decode and after decode. Existing session byte/file/process/deadline bounds retained. Large multi-layer requests can still exceed those bounds explicitly. |
| Explicit UPDATE/EVENT guest CLI | yes (native dispatcher) | yes (bounded U03 recorder) | Merge `olm/U03-ui-session` at `92c5b4b3`. `classic_ui.rs`, `x64/ui_session.rs`, `ui_draw.rs`; not an interactive Mac custom-UI canvas. |
| Click/drag/keydown point ABI | corrected, **untested on Windows** | yes for supported U03 events | `worker_ui_event_execution.cpp`: h/x precedes v/y. Production dispatcher linked into `tests/portable/ui_event_coordinates.cpp`; asymmetric signed-coordinate test fails before and passes after correction. Existing default symmetric coordinates remain the same. |
| Instruction/RVA/pointer-chain/plane watches in native worker | no public counterpart | yes in guest | Still (a), implementable but not implemented here. Existing native `instruments/common/trace_writer.*` and `worker_host_suite_router.cpp` record host calls, not instruction-site observations. Mapping flags onto that logger would not implement F54c semantics. |

### UI suite boundary after the U03 merge

FACT: these are implemented slot subsets in the explicit UI session, not blanket
suite support, drawing rasterization, nor ordinary render-session suite upgrades.
Unsupported calls remain explicit errors. Version mapping is in
`x64/ui_session.rs::ui_suite_address` (paths under `guest/crates/aex-guest-worker/src`).

| UI family / versions | Implemented guest boundary | Remaining parity work (a) |
|---|---|---|
| Param Utils 3 | UI update recording / parameter UI flags, `emulate_ui_callback` | Compare every slot/version against native runtime |
| PF Interface 1, Layer 11, Effect 3/4 | Interface slots 0/1; Layer slot 14 current time; Effect slot 8 disposal | Other scene/effect/layer operations |
| Stream 7/10; Dynamic Stream 2/3/5 | Stream slots 5,7,13,14; DynamicStream slots 4/5 (v5 shifts one slot); checked handle lifetimes and HIDDEN | Complete stream types and arbitrary stream values |
| Custom UI 1, Draw 1 | Context-bound drawing reference, supplier/surface reference | Native GUI interaction adapter, actual canvas |
| Supplier 1 | Slots 0/1/4/6 object creation, 11/12 retain/release | Remaining supplier operations |
| Surface 2 | Slots 2/3/4/9/13 (recorded bit-exact drawing/text/antialias calls) | Rasterization/font metrics, remaining operations |
| Path 1 | Slots 0/1/3/5 | Other path operations |
| App 1/4 | Slots 1/7/9 (explicit theme, picker and invalidation) | Other app services; no approximate color fallback |
| Arbitrary parameter lifecycle | Original callbacks perform COPY, FLAT_SIZE and FLATTEN in `classic_ui.rs` | This does not establish native/guest arbitrary *stream* support or raw equality of uninitialized serializer tails |
| ACE / color management | No X01b addition | Profiles/transforms and per-slot paired witnesses remain open |

FACT: U03 replay on the combined worker: **17 sessions, 174 operations, zero
semantic mismatches**, using the existing versioned active-arbitrary-record
comparator. Ten KiraKira sessions have raw inactive serializer-tail differences;
full raw-buffer equality is explicitly **not** claimed. The replay retains both
worker identities; it does not rewrite U03's frozen manifest or expected traces.
The ten U03 focused tests pass after integration.

FACT: chain tests pass with original ColorKeep/F54c at all three depths; the
first stage output hash equals the second stage input hash. Final selected
artifacts are checked, and all-bypassed export uses no worker. Unit tests cover
noncommutative ordering, bypass, failure short-circuit, deep bit preservation
(including signed zero/NaN payload), invalid layout, independent layer state and
supervised warmup with Auto Update off. These are transport/host tests, not AE
arithmetic conformance. Real 4K ColorKeep renders pass at 8/16/32 bpc; no deadline
or resource limit was raised.

### Remaining work and Windows verification

Native RVA watches require a real instruction observer, module-relative address
resolution, bounded validated pointer reads, hit ordering and capture timing,
plus paired native/guest fixtures. No debugger breakpoint, page-guard or native
pointer-reading implementation has been added. This remains an implementation
gap, not an inherently Mac-only feature or an intentionally different result.

Other implementable gaps remain: interactive custom-UI canvas on Mac (U03 is a
CLI recording session), deep previews, full suite/version coverage, audio/time
editing, shared diagnostics UI, negotiated geometry GUI controls, and the paired
native/guest ROI fixture matrix. The inherited full inventory still records
these; the successful bounded tests do not close them.

Windows verification: build the native worker with `tools/build-native.ps1` and
run the portable UI-coordinate harness with MSVC against the production
dispatcher; then run original-AEX click, drag and keydown probes with distinct
x/y coordinates and native lifecycle self-tests. Run shared artifact tests and
Windows effect-chain tests at all depths, including all-bypass. The native point
fix and shared chain/artifact changes are **untested on Windows**.

### Updated draft claim and PR text (not posted)

Draft claim: Continue X01 platform parity in the existing worktree. Implement
Mac typed effect chains and 4K UHD admission, integrate U03's explicit UI session,
and align the native UI point ABI. Acceptance: behavioral tests, original-AEX
chain and 4K renders, frozen UI witness replay, Release builds, compile checks,
and independent local diff review. Native watches and full host-suite parity
remain explicitly open. No external claim is posted.

Draft PR title: Add macOS effect chains and 4K, integrate Unicorn UI sessions

Draft PR body: Mac GUI chains now preserve typed ARGB values between independently
configured stages and export the selected PNG/raw/EXR artifact. 4K UHD inputs are
admitted with bounded decode and existing process limits. Merge U03's explicit
UI recorder without enabling its suite interception in ordinary render sessions.
Correct reversed native event coordinates using a production-dispatcher test.
This is a partial parity stack; native instruction watches and full UI/suite
coverage remain open. Windows runtime is untested. No push, PR, claim, or release
gate change was made. See the OLM X01b report for final counts, source/build hashes,
commands and review boundary. Local review findings (warmup publication, deep
export, stale FHD label) were accepted and fixed; none were rejected.

Final X01b validation facts: harness **87 passed, 5 ignored**; artifact tests
**8 passed**; combined-worker chain tests **5 passed** (including real AEX and
all-bypass exports); 4K admission/real all-depth tests **2 passed** against the
registered F54c worker. Guest full suite: **719 passed, 3 ignored** in the library,
plus **10 passed** in the CLI. macOS Release harness and guest builds passed.
Windows **harness** target check (`x86_64-pc-windows-msvc`) passed, including its
shared broker dependencies. Native C++ dispatcher portable test passed; the
Windows worker executable was not built or run. Release `--describe-aex` returned
101 ColorKeep parameters through the combined worker. No GUI visual QA claimed.

Combined worker registered separately as `worker-X01b-parity`, SHA-256
`34ce39821a53815dbd2252046cf375222d27a8045685fa6d946cfad211e40c4b`, source
`2c7b29d60962c57a7f376c3fa5c5cc34de51e00d`, with `build.json` and source archive.
Existing F54c remains SHA-256
`3d5eec0c6792f176a9cfbc38093c53b852f4ea1baae2cfd313345c02f55d6fba`.
Full OLM render corpora were not replayed; registered prior workers and OLM
plugin kernels are unchanged. Guest tests cover ordinary render/session routes.

Whole-tree rustfmt still reports the same three inherited files (`engine.rs`,
`tests_cases.rs`, `trace.rs` under guest x64); all are byte-unchanged from X01
`97b596d1`. All changed Rust files pass. Code review boundary: `2c7b29d6`.
