param(
    [string]$AfterEffectsSdk = "C:\Program Files\Adobe\AfterEffectsSDK",
    [string]$Generator = "Visual Studio 18 2026",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")][string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-effect-sequence-data-abi-probe"
$build = Join-Path $repository "target\pf-effect-sequence-data-abi-probe-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"
if (-not (Test-Path -LiteralPath $header)) { throw "After Effects SDK headers were not found: $header" }
if (-not $CMake) {
    $CMake = @(
        "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
        "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
        "C:\Program Files\CMake\bin\cmake.exe"
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $CMake) { throw "cmake.exe was not found; pass -CMake" }
$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "configure failed" }
& $CMake --build $build --config $Configuration --target pf_effect_sequence_data_abi_probe
if ($LASTEXITCODE -ne 0) { throw "build failed" }
$executable = Join-Path $build "$Configuration\pf_effect_sequence_data_abi_probe.exe"
$result = Join-Path $build "pf-effect-sequence-data-abi.json"
$json = & $executable
if ($LASTEXITCODE -ne 0) { throw "execution failed" }
$parsed = $json | ConvertFrom-Json
if ($parsed.suite.slot_count -ne 1) { throw "unexpected ABI report" }
[IO.File]::WriteAllText($result, (($json -join [Environment]::NewLine) + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
[pscustomobject]@{ executable=$executable; result=$result; size=(Get-Item $executable).Length; sha256=(Get-FileHash $executable -Algorithm SHA256).Hash.ToLowerInvariant() }
