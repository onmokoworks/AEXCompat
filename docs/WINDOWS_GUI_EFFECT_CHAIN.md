# Windows GUI effect chains

An effect chain applies multiple AEX effects **in order**, not in parallel:
input image → first enabled effect → next enabled effect → final image.

## Use

1. Choose an input image and use **Add AEX...** to select an effect.
2. In **Effect Controls → Effect chain**, use **Add...** for another AEX.
3. Select a row to edit that stage's parameters. Each stage retains its own
   inspected plug-in, parameters, dependency folders, typed host context and
   custom-UI render settings, including when the same AEX is used twice.
4. Use **Up / Down** to reorder, the checkbox to bypass a stage, or **Remove**
   to remove the selected stage. At most eight stages can be present.
5. **Render preview** and the existing PNG render/save action apply the complete
   enabled chain. Auto Update also renders the whole chain after an edit.

Only a successful, decodable final image is shown or exported. An error names
the failed stage and stops later stages; it does not display a partial result
as the final output. Existing output files are not overwritten. Intermediate
PNGs are confined to an owned directory under `target/chain-transport` and
removed after the render, including on normal error returns.

## Current limits

- Chains use **ARGB8**, with Auto or CPU backend. The current worker primary
  input transport is RGBA8, so deep intermediate frames are not preserved.
  Multi-stage rendering explicitly rejects ARGB16/ARGB32f instead of silently
  quantizing them. Existing single-effect deep rendering remains available.
- Still images only; no audio-only effects or audio sidecars in chains.
- Stages use the existing isolated native worker and its normal inspection,
  dependency discovery, frame deadline and cleanup. A chain is not an AE
  composition renderer or an AE-equivalence claim.
- Chain edits live in the current GUI session. The debug request load/save
  actions still address the **selected stage**, not a saved chain project.
- This implementation is for the Windows native GUI, not the macOS Unicorn
  backend.

## Verification

Behavioral tests cover per-stage state, ordering with non-commuting image
transforms, bypass/removal, invalid output, first-error short circuit, output
protection, intermediate cleanup and final-preview state transitions:

```powershell
cargo test --manifest-path broker/Cargo.toml -p aexcompat-harness effect_chain --locked
```

With the Release worker and SDK `pf_performance_probe` /
`pf_automatic_wide_time_allowed_probe` built in the documented native build
locations, the opt-in integration test drives GUI selection/inspection,
Classic → SmartFX chain dispatch and final preview, asserting actual pixels:

```powershell
cargo test --manifest-path broker/Cargo.toml -p aexcompat-harness effect_chain_native_gui_preview_applies_both_aex_stages --locked -- --ignored
```
