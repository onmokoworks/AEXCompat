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
if (-not $CMake -or -not (Test-Path -LiteralPath $CMake)) { throw "cmake.exe was not found; pass -CMake with its absolute path" }

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
