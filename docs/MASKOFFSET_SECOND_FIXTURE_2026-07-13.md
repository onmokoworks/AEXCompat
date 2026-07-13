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
`2D498526F705AE749542C7D9D0C8BE5E000243AFF8C64D37AEF36F12F60C4DC2`.
Regeneration from the real L2 report matched all nine observed descriptors.
Eight numeric descriptors are assignable. `Fill Color` remains fail-closed
until the generic request and worker ABIs support color values.

## SmartFX Mask Evidence

The `maskoffset` render profile advertises SmartFX only. A classic render request
fails before allowlist access, worker launch, or report creation. The isolated
SmartFX request applied all eight numeric descriptors twice. Both runs completed
Smart PreRender and Smart Render with zero errors, preserved guard bytes, echoed
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

The older `--smart-request` mode remains mask-free and still exercises the
empty-polygon fallback, allowing the two host capability states to be compared.

## Remaining Work

This evidence does not claim complete MaskOffset render compatibility. Completion
still requires generic color payloads, configurable/multiple/open/Bezier mask
scenes, expansion/rounding/feather/invert oracle coverage, and dedicated
mask-suite error/crash isolation cases. Those additions must not introduce
MaskOffset identity or algorithms into `host_core`.
