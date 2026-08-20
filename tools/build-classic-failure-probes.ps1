param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = '',
    [string]$Architecture = 'x64',
    [string]$CMake = '',
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository 'instruments'
$build = Join-Path $repository 'target\classic-failure-probes-build'
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator

$env:AE_SDK_ROOT = $AfterEffectsSdk
& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw 'classic failure probes configure failed' }
& $CMake --build $build --config $Configuration --target `
    pf_crashkit pf_input_write_denied_probe
if ($LASTEXITCODE -ne 0) { throw 'classic failure probes build failed' }

$artifacts = @(
    (Join-Path $build "pf-crashkit\$Configuration\pf_crashkit.aex"),
    (Join-Path $build "pf-input-write-probe\$Configuration\pf_input_write_denied_probe.aex")
)
foreach ($artifact in $artifacts) {
    if (-not (Test-Path -LiteralPath $artifact -PathType Leaf)) {
        throw "classic failure probe artifact was not produced: $artifact"
    }
    $file = Get-Item -LiteralPath $artifact
    [pscustomobject]@{
        path = $file.FullName
        size = $file.Length
        sha256 = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
