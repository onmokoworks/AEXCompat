param(
    [string]$AfterEffectsSdk = "C:\Program Files\Adobe\AfterEffectsSDK",
    [string]$Generator = "Visual Studio 18 2026",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")][string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\aegp-render-options-probe"
$build = Join-Path $repository "target\aegp-render-options-probe-build"
$tmp = Join-Path $repository "target\tmp"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$env:TEMP = $tmp; $env:TMP = $tmp; $env:AE_SDK_ROOT = $AfterEffectsSdk
if (-not (Test-Path (Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"))) { throw "After Effects SDK headers were not found" }
if (-not $CMake) { $CMake = @("C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe", "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe", "C:\Program Files\CMake\bin\cmake.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1 }
if (-not $CMake) { throw "cmake.exe was not found" }
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE) { throw "configure failed" }
& $CMake --build $build --config $Configuration --target aegp_render_options_probe
if ($LASTEXITCODE) { throw "build failed" }
$exe = Join-Path $build "$Configuration\aegp_render_options_probe.exe"
$json = & $exe
if ($LASTEXITCODE) { throw "fixture failed" }
$parsed = $json | ConvertFrom-Json
if (-not $parsed.passed) { throw "fixture reported failure" }
$result = Join-Path $build "aegp-render-options-result.json"
[IO.File]::WriteAllText($result, (($json -join [Environment]::NewLine) + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
$file = Get-Item $exe
[pscustomobject]@{ executable=$file.FullName; result=$result; size=$file.Length; sha256=(Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant() }
