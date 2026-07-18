param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = "Visual Studio 18 2026",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-smart-timed-multilayer-probe"
$build = Join-Path $repository "target\pf-smart-timed-multilayer-probe-build"
$headers = Join-Path $AfterEffectsSdk "Examples\Headers\AE_Effect.h"
if (-not (Test-Path -LiteralPath $headers)) { throw "After Effects SDK headers were not found: $headers" }
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "Smart timed multilayer probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_smart_timed_multilayer_probe --clean-first
if ($LASTEXITCODE -ne 0) { throw "Smart timed multilayer probe build failed" }
$artifact = Join-Path $build "$Configuration\pf_smart_timed_multilayer_probe.aex"
if (-not (Test-Path -LiteralPath $artifact)) { throw "Probe artifact was not produced: $artifact" }
$file = Get-Item -LiteralPath $artifact
[pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant() }
