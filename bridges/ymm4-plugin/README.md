# YMM4 AEXCompat bridge

This is the managed YMM4 VideoEffect half of issues #378 and #379. It is built
against the local YMM4 assemblies and calls `aexcompat_ymm4_native.dll`, which
keeps a resident AEXCompat `RenderSession` on a dedicated Rust thread.

## Build

```powershell
$ymm4 = 'H:\04_software\YukkuriMovieMaker_v4_Lite'
cargo build --release --manifest-path bridges\ymm4-native\Cargo.toml
dotnet build bridges\ymm4-plugin\AEXCompat.Ymm4.csproj -c Release -p:YMM4DirPath="$ymm4\"
```

The plugin is intentionally environment-configured for the first vertical
slice. Set these before starting YMM4:

```powershell
$env:AEXCOMPAT_YMM4_PLUGIN = 'C:\path\to\your\effect.aex'
$env:AEXCOMPAT_YMM4_REPOSITORY = 'D:\Projects\01_Project\04_Tools\AEXCompat'
```

After adding the `AEXCompat` video effect to a YMM4 video item, the effect
property panel discovers the selected AEX and exposes supported visible
parameters as native YMM4 controls:

- bounded float/integer values: slider plus numeric field
- integer popups: combo box
- 0/1 integer values: checkbox
- colors: ARGB hexadecimal field (`#AARRGGBB`)

The selected AEX path can also be edited in the effect's `AEXファイル` field.
Changing it refreshes the parameter panel. The first GUI slice intentionally
leaves unsupported point/layer/path/custom parameters at their discovered
defaults and does not yet provide YMM4 keyframe animation for AEX parameters.

Install both files into YMM4's plugin directory:

```powershell
$pluginDir = Join-Path $ymm4 'user\plugin'
New-Item -ItemType Directory -Force -Path $pluginDir | Out-Null
Copy-Item bridges\ymm4-plugin\bin\Release\net10.0-windows10.0.19041.0\AEXCompat.Ymm4.dll $pluginDir -Force
Copy-Item bridges\ymm4-native\target\release\aexcompat_ymm4_native.dll $pluginDir -Force
```

The bridge intentionally leaves the frame unchanged when the environment is
missing, parameter discovery fails, the bridge cannot open, or a frame fails.
The error is kept in the native/managed bridge for later runtime debugging;
YMM4 debugging and AEX-specific behavior are outside this build acceptance
check.
