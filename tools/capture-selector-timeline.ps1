param(
    [Parameter(Mandatory = $true)][string]$AfterEffects,
    [Parameter(Mandatory = $true)][string]$ProbeAex,
    [Parameter(Mandatory = $true)][string]$InputImage,
    [Parameter(Mandatory = $true)][string]$EffectName,
    [Parameter(Mandatory = $true)][string]$RunRoot,
    [ValidateRange(1, 1000)][int]$Fps = 30,
    [ValidateRange(1, 10000)][int]$DurationFrames = 24,
    [switch]$AnimateDrive,
    [ValidateRange(0, 10)][int]$ProbeMode = 0,
    [string]$PluginRoot,
    [ValidateRange(30, 3600)][int]$TimeoutSeconds = 300
)

# Renders a multi-frame comp through aerender with a selector-timeline probe
# installed (issue #98 stage 0 items 1/2/6, issue #102) and collects the
# probe's JSONL sidecar. The rendered movie is a throwaway; the observation is
# the selector log. ASCII-only on purpose (Windows PowerShell 5.1 reads
# BOM-less files as ANSI).

. (Join-Path $PSScriptRoot 'sha256.ps1')
$ErrorActionPreference = 'Stop'

if (Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue) {
    throw 'After Effects is already running; refusing to install the probe or render.'
}

$afterEffectsPath = (Resolve-Path -LiteralPath $AfterEffects).Path
if ([System.IO.Path]::GetFileName($afterEffectsPath) -ine 'AfterFX.exe') {
    throw 'AfterEffects must point to AfterFX.exe.'
}
$supportFiles = Split-Path -Parent $afterEffectsPath
$scriptHost = Join-Path $supportFiles 'AfterFX.com'
if (-not (Test-Path -LiteralPath $scriptHost -PathType Leaf)) {
    throw 'AfterFX.com is required next to AfterFX.exe (AfterFX.exe -r does not run scripts).'
}
$aerender = Join-Path $supportFiles 'aerender.exe'
if (-not (Test-Path -LiteralPath $aerender -PathType Leaf)) {
    throw 'aerender.exe was not found next to AfterFX.exe.'
}
$probePath = (Resolve-Path -LiteralPath $ProbeAex).Path
$probeHash = Get-Sha256Hex $probePath
$inputPath = (Resolve-Path -LiteralPath $InputImage).Path
if (-not $PluginRoot) {
    $PluginRoot = Join-Path $supportFiles 'Plug-ins'
}
$pluginRoot = (Resolve-Path -LiteralPath $PluginRoot).Path
$installRoot = Join-Path $pluginRoot "AEXCompatOracle-$($probeHash.Substring(0, 12))"
$installedPath = Join-Path $installRoot ([System.IO.Path]::GetFileName($probePath))

$runRoot = [System.IO.Path]::GetFullPath($RunRoot)
New-Item -ItemType Directory -Force -Path $runRoot | Out-Null
$projectPath = Join-Path $runRoot 'selector-timeline.aep'
$outputPath = Join-Path $runRoot 'render-output.avi'
$prepareResult = Join-Path $runRoot 'prepare-result.json'
$logPath = Join-Path $runRoot 'selector-timeline.jsonl'
$summaryPath = Join-Path $runRoot 'capture-summary.json'
foreach ($stale in @($projectPath, $outputPath, $prepareResult, $logPath, $summaryPath)) {
    if (Test-Path -LiteralPath $stale) { throw "Refusing to overwrite existing output: $stale" }
}
# The default output module renders an image sequence next to $outputPath
# (render-output_00000.png ...), so stale sequence files from a reused run
# root are refused too.
$staleSequence = Get-ChildItem -LiteralPath $runRoot -Filter 'render-output*' -ErrorAction SilentlyContinue
if ($staleSequence) {
    throw "Refusing to overwrite existing render outputs in: $runRoot"
}
if (Test-Path -LiteralPath $installRoot) {
    throw "Temporary oracle plug-in directory already exists: $installRoot"
}

function Wait-BoundedExit([System.Diagnostics.Process]$process, [int]$seconds, [string]$label) {
    if (-not $process.WaitForExit($seconds * 1000)) {
        try { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue } catch {}
        throw "$label timed out after $seconds seconds."
    }
    # The parameterless overload flushes the exit state; without it,
    # Start-Process -PassThru handles report a null ExitCode on 5.1.
    $process.WaitForExit()
}

# PIDs owned by this capture: the launchers it started plus every AE-named
# process whose parent chain reaches one of them. Only these may ever be
# terminated in cleanup; an AE session a user starts mid-run stays untouched.
$script:capturePids = @{}

function Register-CaptureDescendants {
    $processes = Get-CimInstance Win32_Process -Filter (
        "Name='AfterFX.exe' OR Name='AfterFX.com' OR Name='aerender.exe' OR Name='aerendercore.exe'"
    ) -ErrorAction SilentlyContinue
    $grew = $true
    while ($grew) {
        $grew = $false
        foreach ($process in $processes) {
            if (-not $script:capturePids.ContainsKey([int]$process.ProcessId) -and
                $script:capturePids.ContainsKey([int]$process.ParentProcessId)) {
                $script:capturePids[[int]$process.ProcessId] = $true
                $grew = $true
            }
        }
    }
}

# AfterFX.com and aerender both hand work to a separate render-engine process
# that outlives the launcher; the probe DLL stays loaded until it exits. Waits
# for every AE-named process to disappear before the next phase or cleanup,
# recording this capture's descendants along the way.
function Wait-AeProcessesGone([int]$seconds, [string]$label) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while (Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue) {
        Register-CaptureDescendants
        if ((Get-Date) -gt $deadline) {
            throw "$label engine processes did not exit within $seconds seconds."
        }
        Start-Sleep -Milliseconds 500
    }
}

# Start-Process joins -ArgumentList into one string, so any element that can
# contain spaces must be pre-quoted to stay a single argv entry.
function Quote-Argument([string]$value) {
    '"{0}"' -f $value
}

try {
    New-Item -ItemType Directory -Path $installRoot -Force | Out-Null
    Copy-Item -LiteralPath $probePath -Destination $installedPath
    if ((Get-Sha256Hex $installedPath) -ne $probeHash) {
        throw 'Temporary AEX hash mismatch after install.'
    }

    # Phase A: build the project. The probe DLL is loaded by this AE session
    # too, so the selector log is armed for it as well; records are separated
    # by pid downstream.
    $env:AEXCOMPAT_SELT_INPUT = $inputPath
    $env:AEXCOMPAT_SELT_PROJECT = $projectPath
    $env:AEXCOMPAT_SELT_OUTPUT = $outputPath
    $env:AEXCOMPAT_SELT_EFFECT = $EffectName
    $env:AEXCOMPAT_SELT_FPS = [string]$Fps
    $env:AEXCOMPAT_SELT_DURATION = [string]$DurationFrames
    $env:AEXCOMPAT_SELT_DRIVE_ANIMATE = if ($AnimateDrive) { '1' } else { '0' }
    $env:AEXCOMPAT_SELT_MODE = [string]$ProbeMode
    $env:AEXCOMPAT_SELT_RESULT = $prepareResult
    $env:AEXCOMPAT_SELECTOR_TIMELINE_LOG = $logPath
    $jsx = Join-Path $PSScriptRoot 'ae-selector-timeline-project.jsx'
    $prepare = Start-Process -FilePath $scriptHost `
        -ArgumentList @('-m', '-noui', '-r', (Quote-Argument $jsx)) `
        -PassThru -WindowStyle Hidden
    $script:capturePids[[int]$prepare.Id] = $true
    Wait-BoundedExit $prepare $TimeoutSeconds 'Project preparation'
    Wait-AeProcessesGone $TimeoutSeconds 'Project preparation'
    if (-not (Test-Path -LiteralPath $prepareResult)) {
        throw 'Project preparation produced no result JSON.'
    }
    $prepared = Get-Content -LiteralPath $prepareResult -Raw | ConvertFrom-Json
    if ($prepared.status -ne 'prepared') {
        throw "Project preparation failed: $($prepared | ConvertTo-Json -Depth 4)"
    }

    # The project-preparation session already appended its own selector
    # events; the render must grow the log beyond this mark, or the captured
    # evidence would silently be preparation-only.
    $preRenderLogLines = if (Test-Path -LiteralPath $logPath) {
        @(Get-Content -LiteralPath $logPath).Count
    } else { 0 }

    # Phase B: render every frame through aerender; the selector log grows in
    # the probe as frames render.
    $renderStdout = Join-Path $runRoot 'aerender.stdout.log'
    $renderStderr = Join-Path $runRoot 'aerender.stderr.log'
    $render = Start-Process -FilePath $aerender `
        -ArgumentList @('-project', (Quote-Argument $projectPath)) `
        -PassThru -WindowStyle Hidden -RedirectStandardOutput $renderStdout `
        -RedirectStandardError $renderStderr
    $script:capturePids[[int]$render.Id] = $true
    Wait-BoundedExit $render $TimeoutSeconds 'aerender'
    Wait-AeProcessesGone $TimeoutSeconds 'aerender'
    if ($render.ExitCode -ne 0) {
        throw "aerender failed with exit code $($render.ExitCode); see $renderStdout"
    }
    if (-not (Test-Path -LiteralPath $logPath)) {
        throw 'aerender completed but the probe wrote no selector log.'
    }
    if (@(Get-Content -LiteralPath $logPath).Count -le $preRenderLogLines) {
        throw 'aerender completed but appended no selector events; the render engine never invoked the probe.'
    }

    $events = Get-Content -LiteralPath $logPath | ForEach-Object { $_ | ConvertFrom-Json }
    $byName = $events | Group-Object cmd_name | Sort-Object Count -Descending
    $summary = [ordered]@{
        schema_version = 1
        status = 'captured'
        probe = [ordered]@{ path = $probePath; sha256 = $probeHash.ToLowerInvariant() }
        effect_match_name = $EffectName
        fps = $Fps
        duration_frames = $DurationFrames
        drive_animated = [bool]$AnimateDrive
        probe_mode = $ProbeMode
        event_count = @($events).Count
        process_ids = @($events | Select-Object -ExpandProperty pid -Unique)
        selector_counts = [ordered]@{}
        log = $logPath
    }
    foreach ($group in $byName) { $summary.selector_counts[$group.Name] = $group.Count }
    $summary | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $summaryPath -Encoding utf8
    $summary | ConvertTo-Json -Depth 6
} finally {
    foreach ($name in @('AEXCOMPAT_SELT_INPUT', 'AEXCOMPAT_SELT_PROJECT', 'AEXCOMPAT_SELT_OUTPUT',
        'AEXCOMPAT_SELT_EFFECT', 'AEXCOMPAT_SELT_FPS', 'AEXCOMPAT_SELT_DURATION',
        'AEXCOMPAT_SELT_DRIVE_ANIMATE', 'AEXCOMPAT_SELT_MODE', 'AEXCOMPAT_SELT_RESULT',
        'AEXCOMPAT_SELECTOR_TIMELINE_LOG')) {
        Remove-Item "Env:$name" -ErrorAction SilentlyContinue
    }
    # Give this capture's engines a bounded grace to exit, then terminate
    # them: leaving one alive would keep the probe DLL loaded and strand the
    # temporary install below. Termination is restricted to the PIDs whose
    # parent chain reaches a launcher this capture started (registered while
    # waiting); an AE session someone started mid-run is reported, not killed.
    $graceDeadline = (Get-Date).AddSeconds(30)
    while ((Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue) -and
        ((Get-Date) -lt $graceDeadline)) {
        Register-CaptureDescendants
        Start-Sleep -Milliseconds 500
    }
    Register-CaptureDescendants
    $lingering = @(Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue)
    $owned = @($lingering | Where-Object { $script:capturePids.ContainsKey([int]$_.Id) })
    $foreign = @($lingering | Where-Object { -not $script:capturePids.ContainsKey([int]$_.Id) })
    if ($owned) {
        Write-Warning ('Terminating AE engine processes launched by this capture: ' +
            (($owned | ForEach-Object { "$($_.ProcessName):$($_.Id)" }) -join ', '))
        $owned | Stop-Process -Force -ErrorAction SilentlyContinue
        $owned | ForEach-Object { try { $_.WaitForExit(10000) | Out-Null } catch {} }
    }
    if ($foreign) {
        Write-Warning ('Not terminating AE-named processes this capture cannot prove it owns: ' +
            (($foreign | ForEach-Object { "$($_.ProcessName):$($_.Id)" }) -join ', '))
    }
    if (Test-Path -LiteralPath $installRoot) {
        $resolvedRoot = (Resolve-Path -LiteralPath $installRoot).Path
        if (-not $resolvedRoot.StartsWith($pluginRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove path outside the selected AE Plug-ins directory: $resolvedRoot"
        }
        Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
    }
}
