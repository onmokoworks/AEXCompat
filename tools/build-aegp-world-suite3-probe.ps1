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
$source = Join-Path $repository "instruments\aegp-world-suite3-probe"
$build = Join-Path $repository "target\aegp-world-suite3-probe-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"
if (-not (Test-Path -LiteralPath $header)) { throw "After Effects SDK headers were not found: $header" }

$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator @ConfigureArgs
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
