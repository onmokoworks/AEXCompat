# MaskOffset Second Conformance Fixture

MaskOffset is the second owner-authored AEX used to prove that AEXCompat is not
architecturally tied to ScatterMap. Load, selector observation, and a masked
SmartFX render through host-provided AEGP suites are proven.

## Fixed Identity

- Profile id: `maskoffset`
- Local artifact: `Ae_Plugins/AEPluginBuild/MaskOffset.aex`
- Size: 200704 bytes
- SHA-256: `B7C41F4F906FCE74B26BD2F06520F6BFDF75DCD1A2682DBB85D50D1DE833877B`
- Owner-authored source: `Ae_Plugins/MaskOffsetRust`
- Exported entry points: `EffectMain`, `PluginDataEntryFunction2`
- Runtime imports: Windows API/UCRT/VCRUNTIME only

The binary path is present only in ignored local allowlists. Broker callers
select the registered `maskoffset` id and cannot supply a native path.

## Native Evidence

The generic commands below ran inside the existing timeout and Job Object
isolation boundary:

```powershell
broker.exe l1 maskoffset target/l1-results/maskoffset-second-fixture-20260713-001.json
broker.exe l2 maskoffset target/l2-results/maskoffset-second-fixture-20260713-001.json
```

L1 loaded and unloaded the fixed module without dispatching selectors. L2
completed About, GlobalSetup, ParamsSetup, sequence/frame setup and setdown,
and GlobalSetdown with zero errors. It observed nine parameters, out flags `4`,
out flags 2 `525312`, no render, and a normal worker exit. MaskOffset attempts
AEGP Utility suite acquisition during GlobalSetup and safely continues when the
minimal L2 host does not provide it.

The promoted descriptor manifest is
`profiles/maskoffset/parameter_descriptors.json`, bound to L2 receipt
`maskoffset-l2-20260713-001` and canonical SHA-256
`13876295DB0A58D4B401525E88E44071A1C489D649B146F7149FD408B651DB13`.
Regeneration from the real L2 report matched all nine observed descriptors.
All nine descriptors are assignable. Manifest v2 records the observed type 5
default as a strict ARGB8 object. Request v2 remains numeric-only; request and
worker payload v3 add typed `color` / `argb8` values without weakening v2.

## SmartFX Mask Evidence

The `maskoffset` render profile advertises SmartFX only. A classic render request
fails before allowlist access, worker launch, or report creation. The isolated
SmartFX requests applied all numeric descriptors and Fill Color twice. Both
runs completed Smart PreRender and Smart Render with zero errors, preserved guard bytes, echoed
the exact descriptor id/slot/kind/value sequence, and produced the same output.

The `--smart-mask-request` worker mode publishes Utility v13, PF Interface v1,
Layer Mask v7, Stream v11, and Mask Outline v5 through their documented suite
names and ABI slots. The host scene contains one closed rectangle with vertices
`(4,3)`, `(12,3)`, `(12,9)`, and `(4,9)`. Handles are opaque host objects; no
plugin identity or MaskOffset algorithm is consulted by suite dispatch.

Smart PreRender reads the outline through the AEGP call chain. The host then
transfers `pre_render_data` to Smart Render and invokes the plugin-provided
delete callback after rendering. PF World Suite v2 reports the checked-out
world's pixel format. Mode 2 with zero expansion, rounding, feather, and invert
makes pixels outside the rectangle transparent. The independent host oracle is
`D1003EF35A6EA3B037989F00672996FEA59867FBB50283A689AC95EDD9BF2359`.
Both isolated native runs matched it. The fixed SmartFX receipt is
`maskoffset-smartfx-20260713-001`.

A second v3 request selected Fill Inside and ARGB `(255,20,180,70)`. Both
isolated runs matched the independent color-fill oracle
`BF419F44E915901BAC882E9B9E3411B8407DF7F4E3A4A1C2719314BDFBB74B5F`.
The worker revalidated type 5 before native rendering and echoed the structured
Color value, proving that the pixel result came through `PF_ColorDef.value`.

The older `--smart-request` mode remains mask-free and still exercises the
empty-polygon fallback, allowing the two host capability states to be compared.

## Suite Failure Isolation

The `smart-suite-fault` broker route accepts only a registered mask-capable
profile and two fixed fault ids. It reuses the reviewed AEX identity, allowlist,
timeout, Job Object, bounded capture, parameter manifest, and create-new output
boundary; callers cannot provide a native path or arbitrary worker mode.

With `mask_count_error`, the Layer Mask Suite count callback returned an AE
error. MaskOffset completed both selectors with zero errors and followed its
empty-polygon fallback. Both runs preserved guards and matched the independent
input-copy hash `863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7`.

With `mask_count_crash`, the same fixed callback raised an Access Violation.
Both workers were classified `crashed`; no worker report was trusted, and the
broker survived both runs. The fault is compiled into the isolated test worker
and cannot be selected through a normal render request. Worker startup disables
Windows GP-fault UI so unattended conformance cannot stall in an error modal.

## Host-Owned Mask Scenes

The mask ABI no longer stores one hard-coded mask, stream, outline, or vertex
array. Each host-owned mask record now has distinct opaque handles, open state,
and vertices. The fixed `smart-mask-scene` gate exercises empty, translated
rectangle, and two-mask scenes; the two-mask case independently selects both
indices and proves distinct pixel results across deterministic double runs.

Scene ids are broker-enumerated and unknown ids fail before output creation or
native lookup. The worker only echoes host scene identity and count. MaskOffset
pixel expectations remain in its fixture adapter rather than `host_core`, while
the AEGP callbacks dispatch solely by opaque host handles.

Request v4 adds a bounded `host_context.mask_scene` to the normal SmartFX request
path. It accepts up to 8 open or closed masks, 64 vertices per mask, 128 vertices
total, position-relative cubic in/out tangents, all components in
`[-32768, 32768]`, and an 8192-byte worker transport. Broker and worker
independently enforce these limits. A real two-polygon request selected its
second mask and matched an independent ray-cast oracle twice.

Real open and cubic Bezier requests also matched independent 32-sample-per-
segment oracles twice. Worker echoes proved one open mask in the first case and
four non-zero-tangent vertices in the second. The open oracle preserves the
target AEX's observable end-exclusive read of the SDK's `[0..num_segments]`
vertex range while the host callback itself remains SDK-conformant.

## Remaining Work

This evidence does not claim complete MaskOffset render compatibility. Completion
still requires expansion/rounding/feather/invert oracle coverage and broader
mask-suite mutation/lifetime behavior. Bounded open, closed, straight, and cubic
Bezier host scenes are now covered. Those additions must not introduce
MaskOffset identity or algorithms into `host_core`.
