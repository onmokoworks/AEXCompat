# YMM4 AEXCompat bridge

This is the managed YMM4 VideoEffect bridge. It is built against the local
YMM4 assemblies and calls `aexcompat_ymm4_native.dll`, which keeps a resident
AEXCompat `RenderSession` on a dedicated Rust thread.

## Build

```powershell
$ymm4 = 'H:\04_software\YukkuriMovieMaker_v4_Lite'
cargo build --release --manifest-path bridges\ymm4-native\Cargo.toml
dotnet build bridges\ymm4-plugin\AEXCompat.Ymm4.csproj -c Release -p:YMM4DirPath="$ymm4\"
```

The installed plugin resolves the native bridge and bundled render workers
relative to its own assembly directory. Environment variables are optional
development fallbacks only:

```powershell
$env:AEXCOMPAT_YMM4_PLUGIN = 'C:\path\to\your\effect.aex'
$env:AEXCOMPAT_YMM4_REPOSITORY = 'D:\Projects\01_Project\04_Tools\AEXCompat'
```

After adding the `AEXCompat` video effect to a YMM4 video item, use the
`参照...` button to choose an AEX file. The effect panel discovers supported
visible parameters and exposes them as:

- bounded float/integer values: slider plus numeric field
- integer popups: combo box
- 0/1 integer values: checkbox
- colors: ARGB hexadecimal field (`#AARRGGBB`)

The runtime folder is detected beside the managed plugin when the bundled
worker files are installed. It also has a folder browse fallback for local
development. Unsupported point/layer/path/custom parameters remain at their
discovered defaults, and YMM4 keyframe animation for AEX parameters is not yet
provided.

## Install

Install the managed/native plugin and the render workers into YMM4's plugin
directory:

```powershell
$pluginDir = Join-Path $ymm4 'user\plugin'
$workerDir = Join-Path $pluginDir 'target\minihost-build'
New-Item -ItemType Directory -Force -Path $workerDir | Out-Null
Copy-Item bridges\ymm4-plugin\bin\Release\net10.0-windows10.0.19041.0\AEXCompat.Ymm4.dll $pluginDir -Force
Copy-Item bridges\ymm4-native\target\release\aexcompat_ymm4_native.dll $pluginDir -Force
Copy-Item target\minihost-build\aex_render_worker.exe $workerDir -Force
Copy-Item target\minihost-build\aex_smart_worker.exe $workerDir -Force
Copy-Item target\minihost-build\aex_l2_worker.exe $workerDir -Force
```

The bridge leaves the frame unchanged when the selected AEX is missing,
parameter discovery fails, the bridge cannot open, or a frame fails.
