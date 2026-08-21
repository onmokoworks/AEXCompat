# Sealed PF wgpu/DX12 compute probe

This probe is a bounded hardware observation, not a production GPU policy.
It loads a self-authored PF Effect AEX through the existing restricted sealed
L2 admission path and mandatory module audit. `PF_Cmd_GLOBAL_SETUP` loads the
sealed companion runtime, selects only the DX12 backend in fixed
`wgpu 0.19.4`, executes one inline-WGSL affine compute over 64 `u32` values,
and reads the result back. `PF_Cmd_GLOBAL_SETDOWN` drops every retained wgpu
object before reporting cleanup.

The selected wgpu adapter's exact DXGI LUID is obtained through the DX12 HAL.
The broker requires that LUID plus PCI vendor/device identity to match the
existing Windows GPU platform collector, then records the active INF, catalog
digest, driver version, and OS build without private absolute paths.

## Build

The runtime has an independent committed `Cargo.lock`; release builds use
`--locked`, one codegen unit, thin LTO, and `/Brepro`. The build script records
the AEX, runtime DLL, C++ source, Rust manifest/source, and lockfile sizes and
SHA-256 values in a target-local manifest.

```powershell
.\tools\build-wgpu-dx12-pf-probe.ps1
```

Build the existing `aex_worker.exe` (the probe dispatches it with
`--kind discovery`) in Release configuration and the `wgpu-dx12-pf-probe`
broker binary, then run:

```powershell
.\broker\target\release\wgpu-dx12-pf-probe.exe `
  (Resolve-Path .).Path `
  (Resolve-Path .\target\pf-wgpu-dx12-probe-build\Release\artifacts.json).Path `
  .\target\wgpu-dx12-pf-probe-results\local.json
```

The output path is create-new and must remain below `target`. Both artifacts
and all recorded sources are re-hashed before dispatch; the sealed launch
re-authenticates the AEX and companion DLL again while staging them. Missing,
tampered, traversal, duplicate-key, unknown-field, mismatched readback,
incomplete cleanup, and module-audit failures remain errors.

wgpu's DX12 device initialization exceeds the production worker's default
512 MiB process-commit cap on the observed Windows host. This probe alone uses
a bounded 1 GiB cap through a dedicated internal launch path; the normal
production default remains unchanged. The report records the limit and peak.

`wgpu_compute_ready=true` means only that this fixed compute/readback and
cleanup observation passed. `backend_ready` remains `false`. Pixel rendering,
pixel equivalence, Vulkan, Metal, multi-vendor coverage, automatic fallback,
production promotion, After Effects startup, and other host applications are
outside this probe.
