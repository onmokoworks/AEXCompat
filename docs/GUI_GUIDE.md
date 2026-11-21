# GUI guide

[日本語（primary text）](GUI_GUIDE_JA.md) · [README](../README.en.md)

Load a Windows x64 AEX, apply it to an image, and inspect its parameters and output. The GUI does not replace After Effects or edit AEP projects. Producing an image does not establish pixel equality with After Effects.

This guide covers `olm/X01-platform-parity` at `13bda6be`. The Japanese guide is the primary text; this is its English translation. Button names use the English UI. In the Windows Japanese UI, select the corresponding labels.

## Start the GUI

For source builds, run commands from the AEXCompat repository root. See [Build Requirements](BUILD_REQUIREMENTS.md) for the build environment.

### Windows x64

Install Rust/Cargo, the MSVC C++ toolchain, Windows SDK, and CMake. Build the worker in PowerShell, then start the GUI.

```powershell
pwsh -File tools/build-native.ps1
cargo run --manifest-path broker/Cargo.toml -p aexcompat-harness --jobs 1
```

The Adobe SDK is required when building SDK probes or sample plug-ins.

### Apple Silicon Mac

Install Rust/Cargo, Xcode Command Line Tools, and CMake. Use a repository path without spaces or colons for the Unicorn build. Keep the temporary directory path free of spaces too.

```sh
mkdir -p "$HOME/.cache/aexcompat-build-tmp"
export TMPDIR="$HOME/.cache/aexcompat-build-tmp"
cargo build --release --manifest-path guest/Cargo.toml -p aex-guest-worker --jobs 1
export AEXCOMPAT_GUEST_WORKER="$PWD/guest/target/release/aex-guest-worker"
cargo run --manifest-path broker/Cargo.toml -p aexcompat-harness --jobs 1
```

If you already have an arm64 worker, skip its build and set `AEXCOMPAT_GUEST_WORKER` to its absolute path. If you set `CARGO_TARGET_DIR`, adjust the worker path to the actual build output. The normal Unicorn route needs neither Windows nor Rosetta.

## Choose an AEX and image

Windows and Mac use the same GUI.

1. Select a Windows x64 `.aex` with Add AEX.... After inspection, editable parameters appear in Effect Controls.
2. Select the input with Choose image.... You can also drag and drop an image.
3. Click Render preview. The detailed panel also offers Render current frame.

Inputs can be PNG, JPEG, BMP, TIFF, or WebP. On Mac, keep the width at most 3840 and the height at most 2160 pixels.

## Edit Effect Controls

Change numbers, sliders, checkboxes, choices, colors, and coordinates. For effects with layer inputs, choose another image in that control. Available controls and ranges depend on the AEX.

Use each control's Reset to restore its default. To restore all parameters, use Reset in the Effect Controls header. In a chain, these apply to the selected stage.

Enable Auto Update to render automatically after edits. For slow effects, turn it off, make your edits, then click the render button. Some controls are disabled while work is running. Check the diagnostics for disabled or unsupported operations.

## View input and output

INPUT shows the source, AEX OUTPUT shows the rendered result, and COMPARE shows both side by side. Scroll over an image to zoom and drag to pan. Fit restores the fitted view. The zoom percentage is relative to the fitted image.

Mac 16 bpc and 32 bpc float renders write a display PNG plus a value-preserving raw or EXR artifact (see Save output).

## Select pixel depth

Use Analysis & Logs → Render Settings → Pixel depth.

| Depth | Use and limits |
|---|---|
| 8 bpc / ARGB8 | Inspect images through PNG previews |
| 16 bpc / ARGB16 | Work with AE internal integer samples; raw white is 32768 |
| 32 bpc float / ARGB32f | Work with floating-point samples; a display PNG cannot preserve HDR or negative values |

Choose a depth supported by the AEX. Selecting a higher depth does not add precision to an 8-bit source. For numerical comparisons, use the corresponding raw or EXR artifact and metadata.

## Apply effects in sequence

Chains are available on Windows and Mac. Mac chains currently support 8 bpc only.

1. Choose an input image and the first AEX.
2. Under Effect Controls → Effect chain, use Add... to add another AEX, up to eight stages.
3. Select a row to edit that stage's parameters. Each stage retains its own settings, even when the same AEX appears more than once.
4. Use Up / Down to reorder, clear the checkbox to bypass a stage, or use Remove to delete the selected stage.
5. Run Render preview or PNG export. Enabled stages run from top to bottom.

On Windows, use the Auto or CPU backend. Every enabled stage must support the selected depth. If a stage fails, later stages stop and the report identifies the failed stage.

Chains support still images, not audio. Settings remain in the current GUI session. Windows debug request save/load operates on the selected stage; it does not save the entire chain. See [Windows GUI effect chains](WINDOWS_GUI_EFFECT_CHAIN.md) for details.

## Export

### Windows

Choose a destination with Save PNG... or Render and save PNG... in the detailed panel. Chain exports contain:

| Depth | Files |
|---|---|
| ARGB8 | PNG |
| ARGB16 | 16-bit PNG and `.rgba16le` |
| ARGB32f | 8-bit display PNG and `.rgba32f-le` |

Raw sidecars are tightly packed, little-endian RGBA. Read dimensions and format from the chain report. The 16-bit PNG uses the image format's full range; raw uses AE's white point of 32768. Float raw preserves finite HDR and negative values. Keep PNG and raw files together. Existing files are not overwritten; choose a new name.

### Mac

As on Windows, save with Render preview (into `target/harness-output`), Save PNG..., or Render and save PNG....

| Depth | Files |
|---|---|
| ARGB8 | PNG |
| ARGB16 | A display PNG, plus `output.bin` and `output.json` inside the neighboring `<name>.<number>.artifact/` |
| ARGB32f | A display PNG, plus `output.exr` and `output.json` inside the neighboring `<name>.<number>.artifact/` |

Mac ARGB16 raw is little-endian ARGB. Do not read it as the Windows chain's RGBA sidecar. `output.json` records dimensions, channel order, depth, hashes, and other metadata. Keep the PNG and the `.artifact/` folder together.

## Read diagnostics and reports

Open Analysis & Logs to inspect the selected AEX and input, render settings, and results. Compatibility, GPU, and performance diagnostics run on Windows; on Mac they stop with a stated reason. If an operation is disabled, check its stated capability limit.

For a bug report, record the OS, AEXCompat commit, AEX version, input dimensions and depth, parameters, operation, and error text. Remove personal absolute paths from reports. Do not publish private AEX binaries, SDK files, or source images.

## Common errors

| Symptom | Action |
|---|---|
| Worker not found | Build the native worker on Windows. On Mac, check that `AEXCOMPAT_GUEST_WORKER` points to an existing arm64 executable |
| `Could not inspect AEX parameters` | Check that the AEX is Windows x64, then inspect diagnostics for missing DLLs or unsupported suites. On Windows, also check dependency DLL locations and search paths |
| `Input exceeds the 4K UHD boundary` on Mac | Use an image with width at most 3840 and height at most 2160 |
| Render button disabled | Check that AEX inspection and image loading have completed and no other operation is running |
| `… is not available in the macOS x64 guest` on Mac | That feature is Windows-only. See Remaining Windows / Mac differences |
| `dependency DLLs are not loaded by the macOS x64 guest` on Mac | The guest cannot load DLLs placed next to the AEX. Render that AEX on Windows |
| Chain fails partway through | Inspect the named stage and try it alone. Check depth support in every stage |
| Destination exists or writing fails | Choose a new name in a writable directory. Check the raw sidecar destination too |
| Windows render times out | The default is 30 seconds per frame. If needed, set `$env:AEXCOMPAT_FRAME_DEADLINE_MS="120000"` before launch. The allowed range is 1000–600000 ms |
| Unsupported suite, selector, output size, or checksum error | Record the error and reproduction conditions. Do not disable validation to accept the output |

## Remaining Windows / Mac differences

X01b adds Mac chains and 3840×2160 4K UHD input/rendering, tested with a real AEX at 8/16/32 bpc. Large multilayer requests can still reach existing memory, file-count, or time limits. See the [audit](../analysis/PLATFORM_PARITY_2026-10-26.md).

- The GUI is the same; Windows runs a native worker and Mac runs the Unicorn x64 guest.
- On Mac these stop with a stated reason: GPU (OpenCL, CUDA, DirectX), mask/spatial/render context, custom UI actions, times other than frame 0 at 30 fps, dependency DLLs, 16/32 bpc chains, audio, and compatibility/GPU/performance/AEGP diagnostics.
- On Mac, supervised parameter changes (USER_CHANGED_PARAM) work after the same AEX has rendered once.
- Mac input is limited to 3840×2160.
- 16-bit raw is RGBA for Windows chains and ARGB on Mac. Float exports are PNG plus raw for Windows chains, and PNG plus EXR plus JSON on Mac.
- U03’s CLI records UI events and drawing calls. An interactive custom-UI canvas on Mac and text rasterization remain unimplemented. Full suite versions/slots, ACE, and arbitrary stream parity are unverified.
- RVA, pointer-chain, and full-plane watches exist in the guest. Native counterparts, paired ROI comparisons, and GUI output-region/geometry controls remain open.

These instructions were checked against source. They do not establish Windows runtime verification or complete feature parity between the two operating systems.
