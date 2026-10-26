# GUI guide

[日本語（primary text）](GUI_GUIDE_JA.md) · [README](../README.en.md)

Load a Windows x64 AEX, apply it to an image, and inspect its parameters and output. The GUI does not replace After Effects or edit AEP projects. Producing an image does not establish pixel equality with After Effects.

This guide covers `olm/X01-platform-parity` at `97b596d1`. The Japanese guide is the primary text; this is its English translation. Button names use the English UI. In the Windows Japanese UI, select the corresponding labels.

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

1. Select a Windows x64 `.aex` with Add AEX... on Windows or AEX... on Mac. After inspection, editable parameters appear in Effect Controls.
2. Select the input with Choose image... on Windows or PNG... on Mac. You can also drag and drop an image.
3. Click Render preview on Windows or Render on Mac. Windows also offers Render current frame in the detailed panel.

Windows accepts PNG, JPEG, BMP, TIFF, and WebP. This Mac GUI version accepts PNG, with width at most 1920 and height at most 1080 pixels.

## Edit Effect Controls

Change numbers, sliders, checkboxes, choices, colors, and coordinates. For effects with layer inputs, choose another image in that control. Available controls and ranges depend on the AEX.

Use each control's Reset to restore its default. To restore all parameters, use Reset in the Effect Controls header on Windows or Reset All on Mac. In a Windows chain, these apply to the selected stage.

Enable Auto Update to render automatically after edits. For slow effects, turn it off, make your edits, then click the render button. Some controls are disabled while work is running. Check the diagnostics for disabled or unsupported operations.

## View input and output

INPUT shows the source, AEX OUTPUT shows the rendered result, and COMPARE shows both side by side. Scroll over an image to zoom and drag to pan. Fit restores the fitted view. The zoom percentage is relative to the fitted image.

Mac 16 bpc and 32 bpc float renders write files but do not show an output preview. Check the destination under Output and the Worker report to confirm completion.

## Select pixel depth

Use Analysis & Logs → Render Settings → Pixel depth. Mac also has a format selector in the top bar.

| Depth | Use and limits |
|---|---|
| 8 bpc / ARGB8 | Inspect images through PNG previews |
| 16 bpc / ARGB16 | Work with AE internal integer samples; raw white is 32768 |
| 32 bpc float / ARGB32f | Work with floating-point samples; a display PNG cannot preserve HDR or negative values |

Choose a depth supported by the AEX. Selecting a higher depth does not add precision to an 8-bit source. For numerical comparisons, use the corresponding raw or EXR artifact and metadata.

## Apply effects in sequence

Chains are available in the Windows GUI in this version. The Mac GUI handles one AEX at a time.

1. Choose an input image and the first AEX.
2. Use Effect Controls → Effect chain → Add... to add another AEX, up to eight stages.
3. Select a row to edit that stage's parameters. Each stage retains its own settings, even when the same AEX appears more than once.
4. Use Up / Down to reorder, clear the checkbox to bypass a stage, or use Remove to delete the selected stage.
5. Run Render preview or PNG export. Enabled stages run from top to bottom.

Use the Auto or CPU backend. Every enabled stage must support the selected depth. If a stage fails, later stages stop and the report identifies the failed stage.

Chains support still images, not audio. Settings remain in the current GUI session. Debug request save/load operates on the selected stage; it does not save the entire chain. See [Windows GUI effect chains](WINDOWS_GUI_EFFECT_CHAIN.md) for details.

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

Render saves under `target/harness-output`. Analysis & Logs → Output displays the actual destination.

| Top-bar format | Files |
|---|---|
| ARGB8 PNG | `mac-aex-….png` |
| ARGB16 raw | `output.bin` and `output.json` inside `mac-aex-….raw-artifact/` |
| FLOAT32 EXR | `output.exr` and `output.json` inside `mac-aex-….exr-artifact/` |

Mac ARGB16 raw is little-endian ARGB. Do not read it as the Windows chain's RGBA sidecar. `output.json` records dimensions, channel order, depth, hashes, and other metadata. Keep the whole directory. The Mac GUI does not also export a 16-bit PNG or float raw sidecar.

## Read diagnostics and reports

Open Analysis & Logs to inspect the selected AEX and input, render settings, and results. Mac's Worker report shows worker results and errors. Windows also offers compatibility, GPU, and performance diagnostics. If an operation is disabled, check its stated capability limit.

For a bug report, record the OS, AEXCompat commit, AEX version, input dimensions and depth, parameters, operation, and error text. Remove personal absolute paths from reports. Do not publish private AEX binaries, SDK files, or source images.

## Common errors

| Symptom | Action |
|---|---|
| Worker not found | Build the native worker on Windows. On Mac, check that `AEXCOMPAT_GUEST_WORKER` points to an existing arm64 executable |
| `Could not inspect AEX parameters` | Check that the AEX is Windows x64, then inspect diagnostics for missing DLLs or unsupported suites. On Windows, also check dependency DLL locations and search paths |
| Cannot select a Mac image, or `Input exceeds the initial Full HD boundary` | Use PNG with width at most 1920 and height at most 1080 |
| Render button disabled | Check that AEX inspection and image loading have completed and no other operation is running |
| No Mac output view at 16/32 bpc | Check for `Completed: render artifact` and the Output destination. Deep previews are not implemented |
| Chain fails partway through | Inspect the named stage and try it alone. Check depth support in every stage |
| Destination exists or writing fails | Choose a new name in a writable directory. Check the raw sidecar destination too |
| Windows render times out | The default is 30 seconds per frame. If needed, set `$env:AEXCOMPAT_FRAME_DEADLINE_MS="120000"` before launch. The allowed range is 1000–600000 ms |
| Unsupported suite, selector, output size, or checksum error | Record the error and reproduction conditions. Do not disable validation to accept the output |

## Remaining Windows / Mac differences

This section is scheduled for revision after X01b. See the [audit](../analysis/PLATFORM_PARITY_2026-10-26.md).

- Windows uses a native worker; Mac uses Unicorn. The Mac GUI has no GPU backend selector.
- Windows has chains, audio/timeline workflows, debug request editing, and custom UI operations/diagnostics; the Mac GUI has no equivalent controls.
- Mac image selection is PNG-only, up to 1920×1080. Deep previews and deep PNG export are not implemented.
- 16-bit raw is RGBA for Windows chains and ARGB on Mac. Float GUI exports are PNG plus raw for Windows chains, and EXR plus JSON on Mac.
- Dependency DLL discovery, compatibility/GPU/performance/AEGP diagnostic UI, and UI updates after parameter changes still differ.

These instructions were checked against source. They do not establish Windows runtime verification or complete feature parity between the two operating systems.
