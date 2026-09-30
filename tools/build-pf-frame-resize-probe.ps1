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
$source = Join-Path $repository "instruments\pf-frame-resize-probe"
$build = Join-Path $repository "target\pf-frame-resize-probe-build"
$headers = Join-Path $AfterEffectsSdk "Examples\Headers\AE_Effect.h"

if (-not (Test-Path -LiteralPath $headers)) {
    throw "After Effects SDK headers were not found: $headers"
}

$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator @ConfigureArgs
if ($LASTEXITCODE -ne 0) { throw "PF frame resize probe configure failed" }
& $CMake --build $build --config $Configuration
if ($LASTEXITCODE -ne 0) { throw "PF frame resize probe build failed" }

$results = @()
foreach ($name in @(
        "pf_expand_allowed_probe", "pf_expand_denied_probe",
        "pf_shrink_allowed_probe", "pf_shrink_denied_probe")) {
    $artifact = Join-Path $build "$Configuration\$name.aex"
    if (-not (Test-Path -LiteralPath $artifact)) {
        throw "PF frame resize probe artifact was not produced: $artifact"
    }
    $file = Get-Item -LiteralPath $artifact
    $hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
    $results += [pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = $hash }
}
$results
