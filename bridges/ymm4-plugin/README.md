# YMM4 AEXCompat bridge

This is the managed YMM4 VideoEffect half of issue #378. It is built against
the local YMM4 assemblies and calls `aexcompat_ymm4_native.dll`, which keeps a
resident AEXCompat `RenderSession` on a dedicated Rust thread.

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

Install both files into YMM4's plugin directory:

```powershell
$pluginDir = Join-Path $ymm4 'user\plugin'
New-Item -ItemType Directory -Force -Path $pluginDir | Out-Null
Copy-Item bridges\ymm4-plugin\bin\Release\net10.0-windows10.0.19041.0\AEXCompat.Ymm4.dll $pluginDir -Force
Copy-Item bridges\ymm4-native\target\release\aexcompat_ymm4_native.dll $pluginDir -Force
```

The first runtime pass intentionally leaves the frame unchanged when the
environment is missing, the bridge cannot open, or a frame fails. The error is
kept in the native/managed bridge for later runtime debugging; YMM4 debugging
and AEX-specific behavior are outside this build acceptance check.
