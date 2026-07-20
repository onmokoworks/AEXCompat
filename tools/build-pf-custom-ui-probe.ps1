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
$source = Join-Path $repository "instruments\pf-custom-ui-probe"
$build = Join-Path $repository "target\pf-custom-ui-probe-build"
$headers = Join-Path $AfterEffectsSdk "Examples\Headers\AE_Effect.h"

if (-not (Test-Path -LiteralPath $headers)) {
    throw "After Effects SDK headers were not found: $headers"
}

$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF custom UI probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_custom_ui_probe
if ($LASTEXITCODE -ne 0) { throw "PF custom UI probe build failed" }

$artifact = Join-Path $build "$Configuration\pf_custom_ui_probe.aex"
if (-not (Test-Path -LiteralPath $artifact)) {
    throw "PF custom UI probe artifact was not produced: $artifact"
}
$file = Get-Item -LiteralPath $artifact
$hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = $hash }
