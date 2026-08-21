param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = "",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release",
    [string]$Target = "pf_visual_audio_sidecar_probe"
)
# `instruments\pf-visual-audio-probe\CMakeLists.txt` is an add_subdirectory
# child (no project()/cmake_minimum_required of its own), so the configure has
# to start at the `instruments` root; only the requested probe target is then
# built out of that tree. The build directory is private to this script rather
# than the shared `target\instruments-build`: CI configures that one as a
# single-config Ninja tree (.github/workflows/ae-sdk-tests.yml) and
# tests/test_instruments_trace_writer.py hardcodes its single-config output
# path, so configuring it with a multi-config generator here would break both.
$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments"
$build = Join-Path $repository "target\pf-visual-audio-probe-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_Effect.h"
if (-not (Test-Path -LiteralPath $header)) { throw "After Effects SDK headers were not found: $header" }
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$env:AE_SDK_ROOT = $AfterEffectsSdk
if (Test-Path -LiteralPath (Join-Path $build "CMakeCache.txt")) {
    & $CMake -S $source -B $build
} else {
    & $CMake -S $source -B $build -G $Generator @ConfigureArgs
}
if ($LASTEXITCODE -ne 0) { throw "PF visual audio probe configure failed" }
& $CMake --build $build --config $Configuration --target $Target
if ($LASTEXITCODE -ne 0) { throw "PF visual audio probe build failed" }
$artifact = Join-Path $build "pf-visual-audio-probe\$Configuration\$Target.aex"
if (-not (Test-Path -LiteralPath $artifact)) { throw "Probe artifact was not produced: $artifact" }
$file = Get-Item -LiteralPath $artifact
[pscustomobject]@{ path = $file.FullName; size = $file.Length; sha256 = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant() }
