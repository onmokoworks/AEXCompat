param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = "Visual Studio 18 2026",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-color-settings-abi-probe"
$build = Join-Path $repository "target\pf-color-settings-abi-probe-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"
if (-not (Test-Path -LiteralPath $header)) { throw "After Effects SDK headers were not found: $header" }

$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF Color Settings ABI probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_color_settings_abi_probe
if ($LASTEXITCODE -ne 0) { throw "PF Color Settings ABI probe build failed" }

$executable = Join-Path $build "$Configuration\pf_color_settings_abi_probe.exe"
$result = Join-Path $build "pf-color-settings-abi.json"
if (-not (Test-Path -LiteralPath $executable)) { throw "PF Color Settings ABI probe executable was not produced: $executable" }
$json = & $executable
if ($LASTEXITCODE -ne 0) { throw "PF Color Settings ABI probe execution failed" }
$parsed = $json | ConvertFrom-Json
if ($parsed.schema_version -ne 1 -or $parsed.suite.slot_count -ne 20) { throw "PF Color Settings ABI probe emitted an unexpected schema" }
[System.IO.File]::WriteAllText($result, (($json -join [Environment]::NewLine) + [Environment]::NewLine), [System.Text.UTF8Encoding]::new($false))

$file = Get-Item -LiteralPath $executable
[pscustomobject]@{
    executable = $file.FullName
    result = $result
    size = $file.Length
    sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
}
