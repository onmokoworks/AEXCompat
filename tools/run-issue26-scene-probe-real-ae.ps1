[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$AfterEffectsPath,
    [string]$ProbePath,
    [string]$FixturePath,
    [string]$OutputRoot,
    [int]$TimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $ProbePath) {
    $ProbePath = Join-Path $repoRoot 'target\issue26-scene-probe-build\Release\issue26_scene_probe.aex'
}
if (-not $FixturePath) {
    $FixturePath = Join-Path $repoRoot 'instruments\aex\issue26-scene-probe\fixture.jsx'
}
if (-not $OutputRoot) {
    $OutputRoot = Join-Path $repoRoot 'target\issue26-scene-probe-real-ae'
}
foreach ($required in @($AfterEffectsPath, $ProbePath, $FixturePath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required real-AE probe input is missing: $required"
    }
}
if ($TimeoutSeconds -lt 20 -or $TimeoutSeconds -gt 300) {
    throw 'TimeoutSeconds must be between 20 and 300'
}

$running = Get-Process -Name AfterFX -ErrorAction SilentlyContinue
if ($running) {
    $details = $running | ForEach-Object {
        "pid=$($_.Id); title=$($_.MainWindowTitle); responding=$($_.Responding)"
    }
    throw "After Effects is already running; refusing to alter its plug-in set: $($details -join ' | ')"
}

$probeHash = (Get-FileHash -LiteralPath $ProbePath -Algorithm SHA256).Hash.ToLowerInvariant()
$supportFiles = Split-Path -Parent $AfterEffectsPath
$pluginRoot = Join-Path $supportFiles 'Plug-ins'
$pluginDirectory = Join-Path $pluginRoot "Issue26SceneProbe-$($probeHash.Substring(0, 12))"
$resolvedPluginRoot = [System.IO.Path]::GetFullPath($pluginRoot)
$resolvedPluginDirectory = [System.IO.Path]::GetFullPath($pluginDirectory)
if (-not $resolvedPluginDirectory.StartsWith(
        $resolvedPluginRoot + [System.IO.Path]::DirectorySeparatorChar,
        [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing unsafe plug-in target: $resolvedPluginDirectory"
}
if (Test-Path -LiteralPath $pluginDirectory) {
    throw "Probe plug-in directory already exists: $pluginDirectory"
}

New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$rawReport = Join-Path $OutputRoot 'probe-report.json'
$fixtureMetadata = Join-Path $OutputRoot 'fixture-metadata.json'
foreach ($output in @($rawReport, $fixtureMetadata)) {
    if (Test-Path -LiteralPath $output) {
        Remove-Item -LiteralPath $output
    }
}

$process = $null
try {
    New-Item -ItemType Directory -Path $pluginDirectory | Out-Null
    $installedProbe = Join-Path $pluginDirectory 'issue26_scene_probe.aex'
    Copy-Item -LiteralPath $ProbePath -Destination $installedProbe
    $installedHash = (Get-FileHash -LiteralPath $installedProbe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($installedHash -ne $probeHash) {
        throw "Installed probe hash differs from the built probe"
    }

    $env:ISSUE26_SCENE_PROBE_EVIDENCE = $rawReport
    $env:ISSUE26_SCENE_FIXTURE_METADATA = $fixtureMetadata
    $process = Start-Process -FilePath $AfterEffectsPath `
        -ArgumentList @('-r', "`"$FixturePath`"") `
        -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while (-not $process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
    }
    if (-not $process.HasExited) {
        throw "After Effects did not exit within $TimeoutSeconds seconds"
    }
    if (-not (Test-Path -LiteralPath $rawReport -PathType Leaf)) {
        throw "After Effects exited without a probe report (exit $($process.ExitCode))"
    }
    if (-not (Test-Path -LiteralPath $fixtureMetadata -PathType Leaf)) {
        throw "After Effects exited without fixture metadata (exit $($process.ExitCode))"
    }
    Write-Output ([ordered]@{
        status = 'captured'
        exit_code = $process.ExitCode
        probe_sha256 = $probeHash
        installed_probe_sha256 = $installedHash
        raw_report = $rawReport
        fixture_metadata = $fixtureMetadata
    } | ConvertTo-Json -Compress)
}
finally {
    Remove-Item -LiteralPath Env:ISSUE26_SCENE_PROBE_EVIDENCE -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath Env:ISSUE26_SCENE_FIXTURE_METADATA -ErrorAction SilentlyContinue
    if ($process -and -not $process.HasExited) {
        Write-Warning "After Effects remains active; preserving $pluginDirectory for safety"
    }
    elseif (Test-Path -LiteralPath $pluginDirectory) {
        $resolvedCurrent = [System.IO.Path]::GetFullPath($pluginDirectory)
        if ($resolvedCurrent -eq $resolvedPluginDirectory) {
            Remove-Item -LiteralPath $pluginDirectory -Recurse
        }
    }
}
