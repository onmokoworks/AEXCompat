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
$source = Join-Path $repository "instruments\pf-aegp-async-layer-receipt-probe"
$build = Join-Path $repository "target\pf-aegp-async-layer-receipt-probe-build"
$headers = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"
if (-not (Test-Path -LiteralPath $headers)) { throw "After Effects SDK headers were not found: $headers" }
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF AEGP async layer receipt probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_aegp_async_layer_receipt_probe
if ($LASTEXITCODE -ne 0) { throw "PF AEGP async layer receipt probe build failed" }
$artifact = Join-Path $build "$Configuration\pf_aegp_async_layer_receipt_probe.aex"
if (-not (Test-Path -LiteralPath $artifact)) { throw "Probe artifact was not produced: $artifact" }
$file = Get-Item -LiteralPath $artifact
$hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = $hash }
