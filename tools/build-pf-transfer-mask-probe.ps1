param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = "Visual Studio 18 2026",
    [string]$Architecture = "x64",
    [string]$CMake = "",
    [ValidateSet("Debug", "Release")][string]$Configuration = "Release"
)
$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-transfer-mask-probe"
$build = Join-Path $repository "target\pf-transfer-mask-probe-build"
if (-not $CMake) {
    $CMake = @(
        "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe",
        "C:\Program Files\CMake\bin\cmake.exe"
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $CMake) { throw "cmake.exe was not found" }
$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF transfer mask probe configure failed" }
& $CMake --build $build --config $Configuration --target pf_transfer_mask_probe
if ($LASTEXITCODE -ne 0) { throw "PF transfer mask probe build failed" }
$artifact = Join-Path $build "$Configuration\pf_transfer_mask_probe.aex"
if (-not (Test-Path -LiteralPath $artifact)) { throw "Probe artifact was not produced" }
$file = Get-Item -LiteralPath $artifact
[pscustomobject]@{ path=$file.FullName; size=$file.Length; sha256=(Get-FileHash $artifact -Algorithm SHA256).Hash.ToLowerInvariant() }
