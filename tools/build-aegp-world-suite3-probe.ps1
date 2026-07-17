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
$source = Join-Path $repository "instruments\aegp-world-suite3-probe"
$build = Join-Path $repository "target\aegp-world-suite3-probe-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"
if (-not (Test-Path -LiteralPath $header)) { throw "After Effects SDK headers were not found: $header" }

if (-not $CMake) {
    $CMake = @(
        "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
        "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
        "C:\Program Files\CMake\bin\cmake.exe"
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $CMake) { throw "cmake.exe was not found; pass -CMake with its absolute path" }

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "AEGP World Suite 3 probe configure failed" }
& $CMake --build $build --config $Configuration --target aegp_world_suite3_probe
if ($LASTEXITCODE -ne 0) { throw "AEGP World Suite 3 probe build failed" }

$executable = Join-Path $build "$Configuration\aegp_world_suite3_probe.exe"
$result = Join-Path $build "aegp-world-suite3-result.json"
$json = & $executable
if ($LASTEXITCODE -ne 0) { throw "AEGP World Suite 3 probe execution failed" }
$parsed = $json | ConvertFrom-Json
if (-not $parsed.passed) { throw "AEGP World Suite 3 probe reported failure" }
[System.IO.File]::WriteAllText($result, (($json -join [Environment]::NewLine) + [Environment]::NewLine), [System.Text.UTF8Encoding]::new($false))

$file = Get-Item -LiteralPath $executable
[pscustomobject]@{
    executable = $file.FullName
    result = $result
    size = $file.Length
    sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
}
