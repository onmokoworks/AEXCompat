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
$source = Join-Path $repository "instruments\pf-frame-origin-probe"
$build = Join-Path $repository "target\pf-frame-origin-probe-build"
$headers = Join-Path $AfterEffectsSdk "Examples\Headers\AE_Effect.h"

if (-not (Test-Path -LiteralPath $headers)) {
    throw "After Effects SDK headers were not found: $headers"
}

$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF frame origin probe configure failed" }
& $CMake --build $build --config $Configuration
if ($LASTEXITCODE -ne 0) { throw "PF frame origin probe build failed" }

$results = @()
foreach ($name in @("pf_frame_origin_probe", "pf_frame_origin_offered_probe")) {
    $artifact = Join-Path $build "$Configuration\$name.aex"
    if (-not (Test-Path -LiteralPath $artifact)) {
        throw "PF frame origin probe artifact was not produced: $artifact"
    }
    $file = Get-Item -LiteralPath $artifact
    $hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
    $results += [pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = $hash }
}
$results
