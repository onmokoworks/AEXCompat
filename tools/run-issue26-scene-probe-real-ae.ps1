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

$running = Get-Process -Name AfterFX,'AfterFX.com' -ErrorAction SilentlyContinue
if ($running) {
    $details = $running | ForEach-Object {
        "pid=$($_.Id); title=$($_.MainWindowTitle); responding=$($_.Responding)"
    }
    throw "After Effects is already running; refusing to alter its plug-in set: $($details -join ' | ')"
}

$probeHash = (Get-FileHash -LiteralPath $ProbePath -Algorithm SHA256).Hash.ToLowerInvariant()
$supportFiles = Split-Path -Parent $AfterEffectsPath
$scriptHost = Join-Path $supportFiles 'AfterFX.com'
if (-not (Test-Path -LiteralPath $scriptHost -PathType Leaf)) {
    throw "AfterFX.com is required next to AfterFX.exe: $scriptHost"
}
$pluginRoot = Join-Path $supportFiles 'Plug-ins'
$resolvedPluginRoot = [System.IO.Path]::GetFullPath($pluginRoot)
$pluginDirectory = Join-Path $resolvedPluginRoot "Issue26SceneProbe-$($probeHash.Substring(0, 12))"
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
$initReport = "$rawReport.init.json"
$fixtureProject = Join-Path $OutputRoot 'fixture-project.aep'
$fixtureMetadata = Join-Path $OutputRoot 'fixture-metadata.json'
$fixtureDiagnostic = "$fixtureMetadata.error.json"
$fixtureStandardOutput = Join-Path $OutputRoot 'fixture-authoring.stdout.txt'
$fixtureStandardError = Join-Path $OutputRoot 'fixture-authoring.stderr.txt'
$standardOutput = Join-Path $OutputRoot 'after-effects.stdout.txt'
$standardError = Join-Path $OutputRoot 'after-effects.stderr.txt'
foreach ($output in @(
        $rawReport, $initReport,
        $fixtureProject, $fixtureMetadata, $fixtureDiagnostic,
        $fixtureStandardOutput, $fixtureStandardError,
        $standardOutput, $standardError)) {
    if (Test-Path -LiteralPath $output) {
        Remove-Item -LiteralPath $output
    }
}

$fixtureProcess = $null
$process = $null
try {
    $env:ISSUE26_SCENE_FIXTURE_PROJECT = $fixtureProject
    $env:ISSUE26_SCENE_FIXTURE_METADATA = $fixtureMetadata
    $fixtureArguments = '-m -r "{0}"' -f $FixturePath
    $fixtureProcess = Start-Process -FilePath $scriptHost `
        -ArgumentList $fixtureArguments `
        -RedirectStandardOutput $fixtureStandardOutput `
        -RedirectStandardError $fixtureStandardError `
        -PassThru -WindowStyle Hidden
    $fixtureDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 500
        $fixtureProcess.Refresh()
        $activeAe = @(
            Get-Process -Name AfterFX,'AfterFX.com',aerender,aerendercore `
                -ErrorAction SilentlyContinue)
        if ($fixtureProcess.HasExited -and $activeAe.Count -eq 0) {
            break
        }
    } while ([DateTime]::UtcNow -lt $fixtureDeadline)
    if ($activeAe.Count -ne 0) {
        throw "After Effects fixture authoring did not exit within $TimeoutSeconds seconds"
    }
    $fixtureProcess.WaitForExit()
    if (-not (Test-Path -LiteralPath $fixtureProject -PathType Leaf)) {
        throw "After Effects exited without a saved fixture project (exit $($fixtureProcess.ExitCode))"
    }
    if (-not (Test-Path -LiteralPath $fixtureMetadata -PathType Leaf)) {
        $diagnostic = if (Test-Path -LiteralPath $fixtureDiagnostic -PathType Leaf) {
            "; diagnostic: $fixtureDiagnostic"
        } else {
            ""
        }
        throw "After Effects exited without fixture metadata (exit $($fixtureProcess.ExitCode))$diagnostic"
    }
    Remove-Item -LiteralPath Env:ISSUE26_SCENE_FIXTURE_PROJECT -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath Env:ISSUE26_SCENE_FIXTURE_METADATA -ErrorAction SilentlyContinue

    New-Item -ItemType Directory -Path $pluginDirectory | Out-Null
    $installedProbe = Join-Path $pluginDirectory 'issue26_scene_probe.aex'
    Copy-Item -LiteralPath $ProbePath -Destination $installedProbe
    $installedHash = (Get-FileHash -LiteralPath $installedProbe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($installedHash -ne $probeHash) {
        throw "Installed probe hash differs from the built probe"
    }

    $env:ISSUE26_SCENE_PROBE_EVIDENCE = $rawReport
    $probeRunId = [Guid]::NewGuid().ToString('D')
    $env:ISSUE26_SCENE_PROBE_RUN_ID = $probeRunId
    $arguments = '-m "{0}"' -f $fixtureProject
    $process = Start-Process -FilePath $AfterEffectsPath `
        -ArgumentList $arguments `
        -RedirectStandardOutput $standardOutput `
        -RedirectStandardError $standardError `
        -PassThru -WindowStyle Hidden
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
        $outputsReady = Test-Path -LiteralPath $rawReport -PathType Leaf
        $activeAe = @(
            Get-Process -Name AfterFX,'AfterFX.com',aerender,aerendercore `
                -ErrorAction SilentlyContinue)
        if ($outputsReady -and $activeAe.Count -eq 0) {
            break
        }
        if (-not $outputsReady -and $process.HasExited -and
            $activeAe.Count -eq 0) {
            break
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($activeAe.Count -ne 0) {
        throw "After Effects did not exit within $TimeoutSeconds seconds"
    }
    if (-not (Test-Path -LiteralPath $rawReport -PathType Leaf)) {
        throw "After Effects exited without a probe report (exit $($process.ExitCode))"
    }
    $process.WaitForExit()
    Write-Output ([ordered]@{
        status = 'captured'
        exit_code = $process.ExitCode
        fixture_exit_code = $fixtureProcess.ExitCode
        script_host = $scriptHost
        probe_sha256 = $probeHash
        installed_probe_sha256 = $installedHash
        fixture_project = $fixtureProject
        fixture_project_sha256 = (
            Get-FileHash -LiteralPath $fixtureProject -Algorithm SHA256
        ).Hash.ToLowerInvariant()
        raw_report = $rawReport
        init_report = $initReport
        probe_run_id = $probeRunId
        fixture_metadata = $fixtureMetadata
        fixture_diagnostic = $fixtureDiagnostic
        fixture_stdout = $fixtureStandardOutput
        fixture_stderr = $fixtureStandardError
        stdout = $standardOutput
        stderr = $standardError
    } | ConvertTo-Json -Compress)
}
finally {
    Remove-Item -LiteralPath Env:ISSUE26_SCENE_PROBE_EVIDENCE -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath Env:ISSUE26_SCENE_FIXTURE_PROJECT -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath Env:ISSUE26_SCENE_FIXTURE_METADATA -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath Env:ISSUE26_SCENE_PROBE_RUN_ID -ErrorAction SilentlyContinue
    $activeAe = @(
        Get-Process -Name AfterFX,'AfterFX.com',aerender,aerendercore `
            -ErrorAction SilentlyContinue)
    if ($activeAe.Count -ne 0) {
        $details = $activeAe | ForEach-Object {
            "pid=$($_.Id); name=$($_.ProcessName); title=$($_.MainWindowTitle)"
        }
        Write-Warning (
            "After Effects remains active; preserving $pluginDirectory for safety: " +
            ($details -join ' | '))
    }
    elseif (Test-Path -LiteralPath $pluginDirectory) {
        $resolvedCurrent = [System.IO.Path]::GetFullPath($pluginDirectory)
        if ($resolvedCurrent -eq $resolvedPluginDirectory) {
            Remove-Item -LiteralPath $pluginDirectory -Recurse
        }
    }
}
