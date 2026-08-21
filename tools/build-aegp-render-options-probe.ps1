param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = "",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")][string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\aegp-render-options-probe"
$build = Join-Path $repository "target\aegp-render-options-probe-build"
$tmp = Join-Path $repository "target\tmp"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$env:TEMP = $tmp; $env:TMP = $tmp; $env:AE_SDK_ROOT = $AfterEffectsSdk
if (-not (Test-Path (Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"))) { throw "After Effects SDK headers were not found" }
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
& $CMake -S $source -B $build -G $Generator @ConfigureArgs
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
