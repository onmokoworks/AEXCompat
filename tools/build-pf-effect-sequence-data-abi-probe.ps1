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
$source = Join-Path $repository "instruments\pf-effect-sequence-data-abi-probe"
$build = Join-Path $repository "target\pf-effect-sequence-data-abi-probe-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"
if (-not (Test-Path -LiteralPath $header)) { throw "After Effects SDK headers were not found: $header" }
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
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
