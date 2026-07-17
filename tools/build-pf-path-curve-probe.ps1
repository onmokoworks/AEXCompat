param(
    [string]$AfterEffectsSdk = "C:\Program Files\Adobe\AfterEffectsSDK",
    [string]$Generator = "Visual Studio 18 2026",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-path-curve-probe"
$build = Join-Path $repository "target\pf-path-curve-probe-build"
$headers = Join-Path $AfterEffectsSdk "Examples\Headers\AE_EffectSuites.h"

if (-not (Test-Path -LiteralPath $headers)) {
    throw "After Effects SDK headers were not found: $headers"
}
if (-not $CMake) {
    $pathCommand = Get-Command cmake -ErrorAction SilentlyContinue
    if ($pathCommand) {
        $CMake = $pathCommand.Source
    } else {
        $CMake = @(
            "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
            "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
            "C:\Program Files\CMake\bin\cmake.exe"
        ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }
}
if (-not $CMake -or -not (Test-Path -LiteralPath $CMake)) {
    throw "cmake.exe was not found; pass -CMake with its absolute path"
}

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
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
