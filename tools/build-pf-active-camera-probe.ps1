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
$source = Join-Path $repository "instruments\pf-active-camera-probe"
$build = Join-Path $repository "target\pf-active-camera-probe-managed-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"
if (-not (Test-Path -LiteralPath $header)) { throw "After Effects SDK headers were not found: $header" }
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator @ConfigureArgs
if ($LASTEXITCODE -ne 0) { throw "PF active camera probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_active_camera_probe
if ($LASTEXITCODE -ne 0) { throw "PF active camera probe build failed" }
$artifact = Join-Path $build "$Configuration\pf_active_camera_probe.aex"
if (-not (Test-Path -LiteralPath $artifact)) {
    $artifact = Join-Path $build "pf_active_camera_probe.aex"
}
if (-not (Test-Path -LiteralPath $artifact)) { throw "Probe artifact was not produced" }
$file = Get-Item -LiteralPath $artifact
$sha256 = [Security.Cryptography.SHA256]::Create()
try {
    $stream = [IO.File]::OpenRead($artifact)
    try { $digest = $sha256.ComputeHash($stream) }
    finally { $stream.Dispose() }
} finally { $sha256.Dispose() }
[pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = ([BitConverter]::ToString($digest) -replace '-', '').ToLowerInvariant() }
