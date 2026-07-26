[CmdletBinding()]
param(
    [string]$SdkRoot = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$BuildRoot
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$SdkRoot = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $SdkRoot
if (-not $BuildRoot) {
    $BuildRoot = Join-Path $repoRoot 'target\issue26-scene-probe-build'
}
$source = Join-Path $repoRoot 'instruments\aex\issue26-scene-probe'
$artifact = Join-Path $BuildRoot 'Release\issue26_scene_probe.aex'
$manifest = Join-Path $BuildRoot 'build-result.json'
$env:AE_SDK_ROOT = $SdkRoot

& cmake -S $source -B $BuildRoot -A x64
if ($LASTEXITCODE -ne 0) {
    throw "Issue #26 scene probe configure failed with exit code $LASTEXITCODE"
}
& cmake --build $BuildRoot --config Release --parallel
if ($LASTEXITCODE -ne 0) {
    throw "Issue #26 scene probe build failed with exit code $LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $artifact -PathType Leaf)) {
    throw "Probe build did not produce $artifact"
}

$result = [ordered]@{
    schema_version = 1
    status = 'built'
    configuration = 'Release|x64'
    sdk_root = $SdkRoot
    artifact = $artifact
    artifact_size = (Get-Item -LiteralPath $artifact).Length
    artifact_sha256 = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
}
$result | ConvertTo-Json | Set-Content -LiteralPath $manifest -Encoding utf8
Write-Output ($result | ConvertTo-Json -Compress)
