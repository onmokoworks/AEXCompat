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
$source = Join-Path $repository "instruments"
$build = Join-Path $repository "target\abi-layout-probe-build"
$evidence = Join-Path $repository "analysis\AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json"

$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$env:AE_SDK_ROOT = $AfterEffectsSdk

$multiConfig = $Generator.StartsWith("Visual Studio ", [System.StringComparison]::Ordinal)
$configureArguments = @("-S", $source, "-B", $build, "-G", $Generator)
if ($multiConfig) {
    $configureArguments += $ConfigureArgs
} else {
    $configureArguments += "-DCMAKE_BUILD_TYPE=$Configuration"
}
& $CMake @configureArguments
if ($LASTEXITCODE -ne 0) { throw "ABI layout probe configure failed" }
& $CMake --build $build --config $Configuration --target abi_layout_probe
if ($LASTEXITCODE -ne 0) { throw "ABI layout probe build failed" }

$executable = if ($multiConfig) {
    Join-Path $build "$Configuration\abi_layout_probe.exe"
} else {
    Join-Path $build "abi_layout_probe.exe"
}
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "ABI layout probe executable was not produced: $executable"
}
$json = (& $executable) -join [Environment]::NewLine
if ($LASTEXITCODE -ne 0) { throw "ABI layout probe execution failed" }
$parsed = $json | ConvertFrom-Json
if ($parsed.schema_version -ne 1 -or $parsed.sdk_boundary -ne "instrument_observation") {
    throw "ABI layout probe emitted an unexpected schema"
}
[System.IO.File]::WriteAllText(
    $evidence,
    $json + [Environment]::NewLine,
    [System.Text.UTF8Encoding]::new($false)
)

[pscustomobject]@{
    executable = $executable
    evidence = $evidence
    sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
}
