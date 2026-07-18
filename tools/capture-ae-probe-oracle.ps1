param(
    [Parameter(Mandatory = $true)][string]$AfterEffects,
    [Parameter(Mandatory = $true)][string]$ProbeAex,
    [Parameter(Mandatory = $true)][string]$InputImage,
    [Parameter(Mandatory = $true)][string]$OutputPng,
    [Parameter(Mandatory = $true)][string]$EffectName,
    [string]$PluginRoot,
    [string]$CaptureScript,
    [string]$ExpectedRaw,
    [int]$Width,
    [int]$Height,
    [ValidateSet('rgba8', 'rgba16le', 'rgba32f-le')][string]$RawFormat = 'rgba8',
    [double]$Tolerance = 0,
    [string]$ComparisonReport,
    [string]$PlanPath,
    [switch]$PlanOnly,
    [ValidateSet(8, 16, 32)][int]$Bpc = 8,
    [ValidateRange(5, 600)][int]$TimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
$probePath = (Resolve-Path -LiteralPath $ProbeAex).Path
$probeHash = (Get-FileHash -LiteralPath $probePath -Algorithm SHA256).Hash
$afterEffectsPath = (Resolve-Path -LiteralPath $AfterEffects).Path
$supportFiles = Split-Path -Parent $afterEffectsPath
$pluginRoot = if ($PluginRoot) {
    (Resolve-Path -LiteralPath $PluginRoot).Path
} else {
    Join-Path $supportFiles 'Plug-ins'
}
$installRoot = Join-Path $pluginRoot "AEXCompatOracle-$($probeHash.Substring(0, 12))"
$installedPath = Join-Path $installRoot ([System.IO.Path]::GetFileName($probePath))
$comparisonValues = @($ExpectedRaw, $ComparisonReport) | Where-Object { $_ }
if ($comparisonValues.Count -gt 0 -and
    (-not $ExpectedRaw -or $Width -le 0 -or $Height -le 0 -or -not $ComparisonReport)) {
    throw 'ExpectedRaw, positive Width/Height, and ComparisonReport must be supplied together.'
}

if ($PlanOnly) {
    if (-not $PlanPath) { throw 'PlanPath is required with PlanOnly.' }
    if (-not $ExpectedRaw -or $Width -le 0 -or $Height -le 0 -or -not $ComparisonReport) {
        throw 'ExpectedRaw, positive Width/Height, and ComparisonReport are required with PlanOnly.'
    }
    $inputPath = (Resolve-Path -LiteralPath $InputImage).Path
    $rawPath = (Resolve-Path -LiteralPath $ExpectedRaw).Path
    $outputPath = [System.IO.Path]::GetFullPath($OutputPng)
    $reportPath = [System.IO.Path]::GetFullPath($ComparisonReport)
    $planFullPath = [System.IO.Path]::GetFullPath($PlanPath)
    foreach ($newPath in @($outputPath, $reportPath, $planFullPath)) {
        if (Test-Path -LiteralPath $newPath) { throw "Refusing to overwrite planned output: $newPath" }
        if (-not (Test-Path -LiteralPath (Split-Path -Parent $newPath) -PathType Container)) {
            throw "Planned output parent does not exist: $newPath"
        }
    }
    $running = @(Get-Process AfterFX,aerender,aerendercore -ErrorAction SilentlyContinue)
    $capture = @(
        '&', (Join-Path $PSScriptRoot 'capture-ae-probe-oracle.ps1'),
        '-AfterEffects', $afterEffectsPath, '-ProbeAex', $probePath,
        '-InputImage', $inputPath, '-OutputPng', $outputPath,
        '-EffectName', $EffectName, '-Bpc', [string]$Bpc
    )
    if ($CaptureScript) { $capture += @('-CaptureScript', (Resolve-Path -LiteralPath $CaptureScript).Path) }
    $compare = @(
        'python', (Join-Path $PSScriptRoot 'compare-pixel-oracles.py'),
        '--raw', $rawPath, '--render', $outputPath,
        '--width', [string]$Width, '--height', [string]$Height,
        '--raw-format', $RawFormat, '--tolerance', [string]$Tolerance,
        '--out', $reportPath
    )
    $plan = [ordered]@{
        schema_version = 1
        status = if ($running.Count) { 'blocked_existing_ae_session' } else { 'ready_to_capture' }
        side_effects_performed = $false
        blocker = if ($running.Count) { 'After Effects is already running; capture was not launched.' } else { $null }
        running_process_ids = @($running | ForEach-Object Id)
        fixture = [ordered]@{ path = $probePath; size_bytes = (Get-Item $probePath).Length; sha256 = $probeHash.ToLowerInvariant() }
        input = [ordered]@{ path = $inputPath; sha256 = (Get-FileHash $inputPath -Algorithm SHA256).Hash.ToLowerInvariant() }
        expected_raw = [ordered]@{ path = $rawPath; size_bytes = (Get-Item $rawPath).Length; sha256 = (Get-FileHash $rawPath -Algorithm SHA256).Hash.ToLowerInvariant(); width = $Width; height = $Height; format = $RawFormat }
        capture_argv = $capture
        compare_argv = $compare
    }
    $plan | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $planFullPath -Encoding utf8
    $plan | ConvertTo-Json -Depth 8
    return
}

if (Get-Process AfterFX,aerender,aerendercore -ErrorAction SilentlyContinue) {
    throw 'After Effects is already running; refusing to install or capture an oracle.'
}
if (Test-Path -LiteralPath $installRoot) {
    throw "Temporary oracle plug-in directory already exists: $installRoot"
}

try {
    New-Item -ItemType Directory -Path $installRoot -Force | Out-Null
    Copy-Item -LiteralPath $probePath -Destination $installedPath
    $installedHash = (Get-FileHash -LiteralPath $installedPath -Algorithm SHA256).Hash
    if ($installedHash -ne $probeHash) {
        throw "Temporary AEX hash mismatch: $installedHash != $probeHash"
    }

    $captureArgs = @{
        AfterEffects = $AfterEffects
        TestedAex = $probePath
        InstalledAex = $installedPath
        InputImage = $InputImage
        OutputPng = $OutputPng
        EffectName = $EffectName
        Bpc = $Bpc
        TimeoutSeconds = $TimeoutSeconds
    }
    if ($CaptureScript) { $captureArgs.ScriptPath = $CaptureScript }
    & (Join-Path $PSScriptRoot 'capture-ae-reference.ps1') @captureArgs

    if ($ExpectedRaw) {
        $rawPath = (Resolve-Path -LiteralPath $ExpectedRaw).Path
        $reportPath = [System.IO.Path]::GetFullPath($ComparisonReport)
        if (Test-Path -LiteralPath $reportPath) {
            throw "Refusing to overwrite comparison report: $reportPath"
        }
        & python (Join-Path $PSScriptRoot 'compare-pixel-oracles.py') `
            --raw $rawPath --render ([System.IO.Path]::GetFullPath($OutputPng)) `
            --width $Width --height $Height --raw-format $RawFormat `
            --tolerance $Tolerance --out $reportPath
        $comparisonExit = $LASTEXITCODE
        if ($comparisonExit -gt 1) {
            throw "Pixel oracle comparison failed with exit code $comparisonExit."
        }
        Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json |
            ConvertTo-Json -Depth 10
        if ($comparisonExit -eq 1) {
            throw 'AE oracle output exceeded the configured comparison tolerance.'
        }
    }
} finally {
    if (Get-Process AfterFX,aerender,aerendercore -ErrorAction SilentlyContinue) {
        Get-Process AfterFX,aerender,aerendercore -ErrorAction SilentlyContinue |
            Stop-Process -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $installRoot) {
        $resolvedRoot = (Resolve-Path -LiteralPath $installRoot).Path
        $resolvedPluginRoot = [System.IO.Path]::GetFullPath($pluginRoot)
        if (-not $resolvedRoot.StartsWith($resolvedPluginRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove path outside the selected AE Plug-ins directory: $resolvedRoot"
        }
        Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
    }
}
