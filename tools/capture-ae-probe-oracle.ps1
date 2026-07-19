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
    [string]$ControlOutputPng,
    [string]$ControlExpectedRaw,
    [string]$ControlComparisonReport,
    [string]$PlanPath,
    [switch]$PlanOnly,
    [ValidateSet(8, 16, 32)][int]$Bpc = 8,
    [ValidateRange(5, 600)][int]$TimeoutSeconds = 120
)

. (Join-Path $PSScriptRoot 'sha256.ps1')

$ErrorActionPreference = 'Stop'

# The repo dev dependencies (Pillow / OpenEXR) live in the uv-managed .venv,
# so Python tools run through uv run (pinned via --project, CWD-independent).
# ASCII-only on purpose: tests execute this under Windows PowerShell 5.1,
# which reads BOM-less files as ANSI and corrupts multibyte comments.
$uvProject = Split-Path -Parent $PSScriptRoot
$probePath = (Resolve-Path -LiteralPath $ProbeAex).Path
$probeHash = Get-Sha256Hex $probePath
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
$controlValues = @($ControlOutputPng, $ControlExpectedRaw, $ControlComparisonReport) |
    Where-Object { $_ }
if ($controlValues.Count -gt 0 -and $controlValues.Count -ne 3) {
    throw 'ControlOutputPng, ControlExpectedRaw, and ControlComparisonReport must be supplied together.'
}

function Invoke-OracleComparison {
    param([string]$Raw, [string]$Render, [string]$Report)
    $rawPath = (Resolve-Path -LiteralPath $Raw).Path
    $reportPath = [System.IO.Path]::GetFullPath($Report)
    if (Test-Path -LiteralPath $reportPath) {
        throw "Refusing to overwrite comparison report: $reportPath"
    }
    & uv run --project $uvProject python (Join-Path $PSScriptRoot 'compare-pixel-oracles.py') `
        --raw $rawPath --render ([System.IO.Path]::GetFullPath($Render)) `
        --width $Width --height $Height --raw-format $RawFormat `
        --tolerance $Tolerance --out $reportPath
    $exitCode = $LASTEXITCODE
    if ($exitCode -gt 1) { throw "Pixel oracle comparison failed with exit code $exitCode." }
    Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json |
        ConvertTo-Json -Depth 10
    if ($exitCode -eq 1) {
        throw 'AE oracle output exceeded the configured comparison tolerance.'
    }
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
    $controlOutputPath = if ($ControlOutputPng) {
        [System.IO.Path]::GetFullPath($ControlOutputPng)
    } else { $null }
    $controlReportPath = if ($ControlComparisonReport) {
        [System.IO.Path]::GetFullPath($ControlComparisonReport)
    } else { $null }
    foreach ($newPath in @($outputPath, $reportPath, $planFullPath,
            $controlOutputPath, $controlReportPath) | Where-Object { $_ }) {
        if (Test-Path -LiteralPath $newPath) { throw "Refusing to overwrite planned output: $newPath" }
        if (-not (Test-Path -LiteralPath (Split-Path -Parent $newPath) -PathType Container)) {
            throw "Planned output parent does not exist: $newPath"
        }
    }
    $running = @(Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue)
    $capture = @(
        '&', (Join-Path $PSScriptRoot 'capture-ae-probe-oracle.ps1'),
        '-AfterEffects', $afterEffectsPath, '-ProbeAex', $probePath,
        '-InputImage', $inputPath, '-OutputPng', $outputPath,
        '-EffectName', $EffectName, '-Bpc', [string]$Bpc
    )
    if ($CaptureScript) { $capture += @('-CaptureScript', (Resolve-Path -LiteralPath $CaptureScript).Path) }
    $compare = @(
        'uv', 'run', '--project', $uvProject, 'python', (Join-Path $PSScriptRoot 'compare-pixel-oracles.py'),
        '--raw', $rawPath, '--render', $outputPath,
        '--width', [string]$Width, '--height', [string]$Height,
        '--raw-format', $RawFormat, '--tolerance', [string]$Tolerance,
        '--out', $reportPath
    )
    $controlRawPath = if ($ControlExpectedRaw) {
        (Resolve-Path -LiteralPath $ControlExpectedRaw).Path
    } else { $null }
    $controlCapture = if ($controlOutputPath) {
        @(
            '&', (Join-Path $PSScriptRoot 'capture-ae-reference.ps1'),
            '-AfterEffects', $afterEffectsPath, '-TestedAex', $probePath,
            '-InstalledAex', $installedPath,
            '-InputImage', $inputPath, '-OutputPng', $controlOutputPath,
            '-EffectName', $EffectName, '-Bpc', [string]$Bpc, '-NoEffect'
        )
    } else { $null }
    $controlCompare = if ($controlOutputPath) {
        @(
            'uv', 'run', '--project', $uvProject, 'python', (Join-Path $PSScriptRoot 'compare-pixel-oracles.py'),
            '--raw', $controlRawPath, '--render', $controlOutputPath,
            '--width', [string]$Width, '--height', [string]$Height,
            '--raw-format', $RawFormat, '--tolerance', [string]$Tolerance,
            '--out', $controlReportPath
        )
    } else { $null }
    $plan = [ordered]@{
        schema_version = 1
        status = if ($running.Count) { 'blocked_existing_ae_session' } else { 'ready_to_capture' }
        side_effects_performed = $false
        blocker = if ($running.Count) { 'After Effects is already running; capture was not launched.' } else { $null }
        running_process_ids = @($running | ForEach-Object Id)
        fixture = [ordered]@{ path = $probePath; size_bytes = (Get-Item $probePath).Length; sha256 = $probeHash.ToLowerInvariant() }
        input = [ordered]@{ path = $inputPath; sha256 = Get-Sha256Hex $inputPath }
        expected_raw = [ordered]@{ path = $rawPath; size_bytes = (Get-Item $rawPath).Length; sha256 = Get-Sha256Hex $rawPath; width = $Width; height = $Height; format = $RawFormat }
        capture_argv = $capture
        compare_argv = $compare
        no_effect_control = if ($controlOutputPath) {
            [ordered]@{
                expected_raw = [ordered]@{
                    path = $controlRawPath
                    size_bytes = (Get-Item $controlRawPath).Length
                    sha256 = Get-Sha256Hex $controlRawPath
                }
                capture_argv = $controlCapture
                compare_argv = $controlCompare
            }
        } else { $null }
    }
    $plan | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $planFullPath -Encoding utf8
    $plan | ConvertTo-Json -Depth 8
    return
}

if (Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue) {
    throw 'After Effects is already running; refusing to install or capture an oracle.'
}
if (Test-Path -LiteralPath $installRoot) {
    throw "Temporary oracle plug-in directory already exists: $installRoot"
}

try {
    New-Item -ItemType Directory -Path $installRoot -Force | Out-Null
    Copy-Item -LiteralPath $probePath -Destination $installedPath
    $installedHash = Get-Sha256Hex $installedPath
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
        Invoke-OracleComparison -Raw $ExpectedRaw -Render $OutputPng -Report $ComparisonReport
    }

    if ($ControlOutputPng) {
        $controlArgs = $captureArgs.Clone()
        $controlArgs.OutputPng = $ControlOutputPng
        $controlArgs.NoEffect = $true
        & (Join-Path $PSScriptRoot 'capture-ae-reference.ps1') @controlArgs
        Invoke-OracleComparison -Raw $ControlExpectedRaw -Render $ControlOutputPng `
            -Report $ControlComparisonReport
    }
} finally {
    # capture-ae-reference.ps1 shuts down or kills its own launch tree by
    # PID before returning, so an AE-named process still alive here is most
    # likely an unrelated session started after the startup gate; a
    # name-based kill would terminate it (review on #59). Report it instead
    # and let the plug-in removal below fail explicitly if that process
    # still holds the probe binary.
    $lingering = Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue
    if ($lingering) {
        Write-Warning ('Not terminating AE-named processes this run did not launch: ' +
            (($lingering | ForEach-Object { "$($_.ProcessName):$($_.Id)" }) -join ', '))
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
