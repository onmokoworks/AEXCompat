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
$source = Join-Path $repository "instruments\pf-aegp-render-suite5-probe"
$build = Join-Path $repository "target\pf-aegp-render-suite5-probe-build"
$headers = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"
if (-not (Test-Path -LiteralPath $headers)) { throw "After Effects SDK headers were not found: $headers" }
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF AEGP Render Suite5 probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_aegp_render_suite5_probe
if ($LASTEXITCODE -ne 0) { throw "PF AEGP Render Suite5 probe build failed" }
$artifact = Join-Path $build "$Configuration\pf_aegp_render_suite5_probe.aex"
if (-not (Test-Path -LiteralPath $artifact)) { throw "Probe artifact was not produced: $artifact" }
$file = Get-Item -LiteralPath $artifact
$hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = $hash }
