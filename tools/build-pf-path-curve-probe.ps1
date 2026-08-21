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
$source = Join-Path $repository "instruments\pf-path-curve-probe"
$build = Join-Path $repository "target\pf-path-curve-probe-build"
$headers = Join-Path $AfterEffectsSdk "Examples\Headers\AE_EffectSuites.h"

if (-not (Test-Path -LiteralPath $headers)) {
    throw "After Effects SDK headers were not found: $headers"
}
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator @ConfigureArgs
if ($LASTEXITCODE -ne 0) { throw "PF path curve probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_path_curve_probe
if ($LASTEXITCODE -ne 0) { throw "PF path curve probe build failed" }

$artifact = Join-Path $build "$Configuration\pf_path_curve_probe.aex"
if (-not (Test-Path -LiteralPath $artifact)) {
    throw "PF path curve probe artifact was not produced: $artifact"
}
$file = Get-Item -LiteralPath $artifact
$hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = $hash }
