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
$source = Join-Path $repository "instruments\pf-suite-abi-probe"
$build = Join-Path $repository "target\pf-suite-abi-probe-build"
$header = Join-Path $AfterEffectsSdk "Examples\Headers\AE_GeneralPlug.h"

if (-not (Test-Path -LiteralPath $header)) {
    throw "After Effects SDK headers were not found: $header"
}

$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF suite ABI probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_suite_abi_probe
if ($LASTEXITCODE -ne 0) { throw "PF suite ABI probe build failed" }

$executable = Join-Path $build "$Configuration\pf_suite_abi_probe.exe"
$result = Join-Path $build "pf-suite-abi.json"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "PF suite ABI probe executable was not produced: $executable"
}

$json = & $executable
if ($LASTEXITCODE -ne 0) { throw "PF suite ABI probe execution failed" }
$parsed = $json | ConvertFrom-Json
if ($parsed.schema_version -ne 1) { throw "PF suite ABI probe emitted an unexpected schema" }
[System.IO.File]::WriteAllText($result, (($json -join [Environment]::NewLine) + [Environment]::NewLine), [System.Text.UTF8Encoding]::new($false))

$file = Get-Item -LiteralPath $executable
$hash = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{
    executable = $file.FullName
    result = $result
    size = $file.Length
    sha256 = $hash
}
