param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = "",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")][string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
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
