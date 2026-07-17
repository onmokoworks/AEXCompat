param(
    [string]$OutputRoot = "",
    [switch]$Build
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if (-not $OutputRoot) {
    $OutputRoot = Join-Path $repo 'target\ae-oracle-bundle'
}

$probes = @(
    [pscustomobject]@{
        id = 'transform-multimatrix'
        artifact = Join-Path $repo 'target\pf-transform-multimatrix-oracle-build\Release\pf_transform_multimatrix_oracle.aex'
        buildScript = Join-Path $PSScriptRoot 'build-pf-transform-multimatrix-oracle.ps1'
        captureScript = Join-Path $PSScriptRoot 'ae-transform-multimatrix-oracle-run.jsx'
        effectName = 'AEXCompat PF MultiMatrix Oracle'
    },
    [pscustomobject]@{
        id = 'path-curve'
        artifact = Join-Path $repo 'target\pf-path-curve-probe-build\Release\pf_path_curve_probe.aex'
        buildScript = Join-Path $PSScriptRoot 'build-pf-path-curve-probe.ps1'
        captureScript = Join-Path $PSScriptRoot 'ae-path-curve-oracle-run.jsx'
        effectName = 'AEXCompat PF Path Curve'
    },
    [pscustomobject]@{
        id = 'color'
        artifact = Join-Path $repo 'target\pf-color-oracle-build\Release\pf_color_oracle.aex'
        buildScript = Join-Path $PSScriptRoot 'build-pf-color-oracle.ps1'
        captureScript = Join-Path $PSScriptRoot 'ae-color-oracle-run.jsx'
        effectName = 'AEXCompat PF Color Oracle'
    }
)

if ($Build) {
    foreach ($probe in $probes) { & $probe.buildScript }
}

$resolvedOutput = [System.IO.Path]::GetFullPath($OutputRoot)
$targetRoot = [System.IO.Path]::GetFullPath((Join-Path $repo 'target'))
if (-not $resolvedOutput.StartsWith($targetRoot + [System.IO.Path]::DirectorySeparatorChar, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "OutputRoot must be below the repository target directory: $resolvedOutput"
}
if (Test-Path -LiteralPath $resolvedOutput) {
    Remove-Item -LiteralPath $resolvedOutput -Recurse -Force
}
$payloadRoot = New-Item -ItemType Directory -Path (Join-Path $resolvedOutput 'payload') -Force
$runnerRoot = New-Item -ItemType Directory -Path (Join-Path $resolvedOutput 'runners') -Force

$entries = foreach ($probe in $probes) {
    if (-not (Test-Path -LiteralPath $probe.artifact -PathType Leaf)) {
        throw "Oracle artifact is missing; rerun with -Build: $($probe.artifact)"
    }
    if (-not (Test-Path -LiteralPath $probe.captureScript -PathType Leaf)) {
        throw "Oracle capture runner is missing: $($probe.captureScript)"
    }
    $destination = Join-Path $payloadRoot.FullName ([System.IO.Path]::GetFileName($probe.artifact))
    Copy-Item -LiteralPath $probe.artifact -Destination $destination
    $runnerDestination = Join-Path $runnerRoot.FullName ([System.IO.Path]::GetFileName($probe.captureScript))
    Copy-Item -LiteralPath $probe.captureScript -Destination $runnerDestination
    [ordered]@{
        id = $probe.id
        file = "payload/$([System.IO.Path]::GetFileName($destination))"
        size = (Get-Item -LiteralPath $destination).Length
        sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        runner = "runners/$([System.IO.Path]::GetFileName($runnerDestination))"
        runner_sha256 = (Get-FileHash -LiteralPath $runnerDestination -Algorithm SHA256).Hash.ToLowerInvariant()
        effect_name = $probe.effectName
    }
}

$manifest = [ordered]@{
    schema = 'aexcompat-ae-oracle-bundle-v1'
    install_directory = 'AEXCompatOracleBundle'
    probes = @($entries)
}
$manifestPath = Join-Path $resolvedOutput 'manifest.json'
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8
[pscustomobject]@{
    bundle = $resolvedOutput
    manifest = $manifestPath
    manifest_sha256 = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
    probes = $entries.Count
}
