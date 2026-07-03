# AE / AEX Interop Research Plan - 2026-05-31

## Policy

Superseded direction: the user now wants direct `.aex` hosting where feasible.
Use `analysis/AEX_DIRECT_HOST_AND_AEP_EDIT_STRATEGY_2026-05-31.md` as the newer
canonical plan when it conflicts with this older bridge/export policy.

The earlier AE-mediated route remains useful as a fallback and validation lane:
snapshot JSON plus generated JSX can still let After Effects create and save
projects without AviUtlas writing proprietary binary project files directly.

Facts:

- Observed: local `AviUtlas_exedit_for_ae` experiments already contain AE JSX,
  AEGP, Electron, and named-pipe sync prototypes.
- Observed: local `.ffx`, `.aep`, and `.aex` assets exist, but user presets and
  projects are private metadata-only fixtures.
- Inferred: the safest public path is to export a neutral snapshot plus JSX that
  AE executes to build a comp.
- Unverified: exact mapping fidelity for every ExEdit effect in AE, especially
  presets and third-party plugins.

## Format Boundaries

| Format | Initial AviUtlas Treatment | Publication Risk |
| --- | --- | --- |
| `.jsx` | Generate clean ExtendScript from AviUtlas snapshot data | Low if generated independently |
| `.ffx` | Store/apply as opaque preset path inside AE | Medium; no direct parser |
| `.aep` | Prefer `.aepx` patches or generated JSX transactions first; binary `.aep` write support deferred | Medium-high; project files remain private/local fixtures |
| `.aex` | Direct-host target through an out-of-process sandbox worker; AE-hosted AEGP remains fallback | High; SDK/runtime crash and licensing care |
| `exedit.auf` | External oracle/host only, never loaded into AE | High if ABI mixed incorrectly |

## Proposed Milestones

1. `AE_SNAPSHOT_SCHEMA`: comp, layer, timing, transform, text, shape, media, and
   placeholder effect records.
2. `export-ae-jsx`: generate a synthetic AE comp from a small `.exo/.exa` subset.
3. `OpaquePresetRef`: carry `.ffx` paths and let AE call `applyPreset` when the
   user supplies a preset.
4. `AEX static catalog`: classify local `.aex` files without executing code and
   separate self-built fixtures from third-party/user assets.
5. `AEX sandbox host`: design and prototype an out-of-process worker for
   allowlisted legacy CPU effect plug-ins only.
6. `AEPX/JSX patch editor`: support structural `.aepx` edits and generated JSX
   transactions before any direct binary `.aep` writer.

## Guardrails

- Do not write local `.aep` binaries directly in the first iteration.
- Do not parse local `.ffx` binaries for public implementation.
- Do not copy user presets/projects into tests.
- Require explicit user opt-in for JSX paths that call shell/system commands.
- Keep all AE SDK `.aex` builds as local experiments until license/runtime
  requirements are documented.
- Do not load third-party `.aex` binaries into the AviUtlas process. Direct
  hosting starts with static cataloging and a sandbox worker.
