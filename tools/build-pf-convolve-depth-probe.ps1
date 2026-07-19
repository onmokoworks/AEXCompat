param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = "",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-convolve-depth-probe"
$build = Join-Path $repository "target\pf-convolve-depth-probe-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_EffectCBSuites.h"
if (-not (Test-Path -LiteralPath $header)) { throw "After Effects SDK headers were not found: $header" }
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF convolve depth probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_convolve_depth_probe
if ($LASTEXITCODE -ne 0) { throw "PF convolve depth probe build failed" }
$artifact = Join-Path $build "$Configuration\pf_convolve_depth_probe.aex"
if (-not (Test-Path -LiteralPath $artifact)) { throw "Probe artifact was not produced: $artifact" }
$file = Get-Item -LiteralPath $artifact
[pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant() }
