# MaskOffset Second Conformance Fixture

MaskOffset is the second owner-authored AEX used to prove that AEXCompat is not
architecturally tied to ScatterMap. This promotion is staged: load and selector
observation are proven, while rendering is not yet claimed.

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

## Remaining Work

This evidence does not claim MaskOffset render compatibility. Completion still
requires generic color payloads, the AEGP mask/suite behavior used to obtain mask
paths, Smart PreRender/Render world handling, an independent pixel oracle, and
error/crash isolation cases. Those additions must not introduce MaskOffset
identity or algorithms into `host_core`.
