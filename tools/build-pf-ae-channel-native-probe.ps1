param(
    [string]$AfterEffectsSdk = "C:\Program Files\Adobe\AfterEffectsSDK",
    [string]$Generator = "Visual Studio 18 2026",
    [string]$Architecture = "x64",
    [string]$CMake = "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
    [ValidateSet("Debug", "Release")][string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-ae-channel-native-probe"
$build = Join-Path $repository "target\pf-ae-channel-native-probe-build"
$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "Channel native probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_ae_channel_native_probe --clean-first
if ($LASTEXITCODE -ne 0) { throw "Channel native probe build failed" }
$artifact = Join-Path $build "$Configuration\pf_ae_channel_native_probe.aex"
$file = Get-Item -LiteralPath $artifact
[pscustomobject]@{ path=$file.FullName; size=$file.Length; sha256=(Get-FileHash $artifact -Algorithm SHA256).Hash.ToLowerInvariant() }
