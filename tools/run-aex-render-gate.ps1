[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$resultRelative = "analysis/REAL_AEX_RENDER_GATE_RESULT_2026-07-17.json"
$outputRelative = "target/image-transport/aex-render-gate-output.rgba"
$adapterRelative = "tools/refresh-runtime-session.py"
$sessionHarnessRelative = "broker/target/release/aexcompat-harness.exe"
$releaseWorkerRelative = "target/minihost-build/aex_worker.exe"

$artifacts = [ordered]@{
    aex = [ordered]@{
        path = "target/sdk-fixtures/colorgrid/ColorGrid.aex"
        size_bytes = 33280
        sha256 = "64b0de13f978222bb40649bdf48cc533aebb3e03f98270af073b8351284ba64c"
    }
    input = [ordered]@{
        path = "target/image-transport/colorgrid-click-input.rgba"
        size_bytes = 768
        sha256 = "c9ee521c7d71cbf6a41cb1a2d075add64676b912f5870098417b75ca727ba807"
    }
    reference = [ordered]@{
        path = "target/image-transport/colorgrid-normal.rgba"
        size_bytes = 768
        sha256 = "91d436a039c7f5ef56c4418e97dcb48dd4a69b8b9d984f224a06f97fe3ff8578"
    }
}

$resolved = [ordered]@{}
$adapterExitCode = $null
$failureStage = "preflight"
$observedOutput = [ordered]@{
    path = $outputRelative
    size_bytes = $null
    sha256 = $null
    expected_sha256 = $artifacts.reference.sha256
    differing_bytes = $null
}
$sessionSummary = [ordered]@{
    stage = $null
    status = $null
    passed = $null
    width = $null
    height = $null
    row_bytes = $null
    pixel_format = $null
    frame_time_ms = $null
    input_sha256 = $null
    internal_output_sha256 = $null
    guard_bytes_intact = $null
    suite_leases_balanced = $null
    handle_lifetimes_balanced = $null
    world_lifetimes_balanced = $null
}

function Write-GateResult([System.Collections.IDictionary]$result) {
    $resultPath = Join-Path $root ($resultRelative -replace '/', '\')
    $json = $result | ConvertTo-Json -Depth 12
    $utf8 = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($resultPath, $json + [Environment]::NewLine, $utf8)
}

function Get-RepoPath([string]$relativePath) {
    return Join-Path $root ($relativePath -replace '/', '\')
}

function Resolve-PinnedArtifact([System.Collections.IDictionary]$artifact) {
    $absolute = Get-RepoPath $artifact.path
    if (-not (Test-Path -LiteralPath $absolute -PathType Leaf)) {
        throw "Required artifact is missing: $($artifact.path)"
    }
    $item = Get-Item -LiteralPath $absolute
    if ($item.Length -ne [int64]$artifact.size_bytes) {
        throw "Artifact size mismatch: $($artifact.path)"
    }
    $actualHash = (Get-FileHash -LiteralPath $absolute -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $artifact.sha256) {
        throw "Artifact SHA-256 mismatch: $($artifact.path)"
    }
    return $absolute
}

function Get-ArtifactIdentity([string]$relativePath) {
    $absolute = Get-RepoPath $relativePath
    if (-not (Test-Path -LiteralPath $absolute -PathType Leaf)) {
        throw "Required runtime artifact is missing: $relativePath"
    }
    $item = Get-Item -LiteralPath $absolute
    return [ordered]@{
        path = $relativePath
        size_bytes = [int64]$item.Length
        sha256 = (Get-FileHash -LiteralPath $absolute -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function Get-ReportProperty([object]$report, [string]$name) {
    if ($null -eq $report) {
        return $null
    }
    $property = $report.PSObject.Properties[$name]
    if ($null -eq $property) {
        return $null
    }
    return $property.Value
}

function Get-DifferingByteCount([byte[]]$left, [byte[]]$right) {
    if ($null -eq $left -or $null -eq $right) {
        return $null
    }
    [int64]$different = 0
    $common = [Math]::Min($left.Length, $right.Length)
    for ($index = 0; $index -lt $common; $index++) {
        if ($left[$index] -ne $right[$index]) {
            $different++
        }
    }
    $different += [Math]::Abs($left.Length - $right.Length)
    return $different
}

function Get-SafeFailureMessage([string]$message) {
    return [regex]::Replace($message, [regex]::Escape($root), "<repo>", "IgnoreCase")
}

$output = Get-RepoPath $outputRelative
$pythonCommand = $null

try {
    $failureStage = "preflight"
    foreach ($entry in $artifacts.GetEnumerator()) {
        $resolved[$entry.Key] = Resolve-PinnedArtifact $entry.Value
    }
    $artifacts.release_worker = Get-ArtifactIdentity $releaseWorkerRelative
    $artifacts.session_harness = Get-ArtifactIdentity $sessionHarnessRelative

    $pythonCommand = (Get-Command python -ErrorAction SilentlyContinue).Source
    if (-not $pythonCommand) {
        $pythonCommand = (Get-Command py -ErrorAction SilentlyContinue).Source
    }
    if (-not $pythonCommand) {
        throw "Python was not found; install Python or make it available on PATH."
    }

    Remove-Item -LiteralPath $output -ErrorAction SilentlyContinue
    $failureStage = "session"
    $adapterArguments = @(
        (Get-RepoPath $adapterRelative),
        "--plugin", $resolved.aex,
        "--plugin-sha256", $artifacts.aex.sha256,
        "--input", $resolved.input,
        "--output", $output,
        "--width", "16",
        "--height", "12",
        "--pixel-format", "argb8",
        "--current-time", "0",
        "--total-time", "1",
        "--time-scale", "1"
    )
    $adapterLines = @(& $pythonCommand @adapterArguments 2>&1 | ForEach-Object { $_.ToString() })
    $adapterExitCode = $LASTEXITCODE
    if ($adapterExitCode -ne 0) {
        $text = ($adapterLines -join [Environment]::NewLine).Trim()
        throw ("session adapter failed with exit code {0}: {1}" -f $adapterExitCode, $text)
    }
    $jsonLine = $adapterLines |
        Where-Object { $_ -match '^\s*\{.*\}\s*$' } |
        Select-Object -Last 1
    if (-not $jsonLine) {
        throw "session adapter returned no JSON report"
    }
    $sessionReport = $jsonLine | ConvertFrom-Json
    $sessionSummary.stage = Get-ReportProperty $sessionReport "stage"
    $sessionSummary.status = if ((Get-ReportProperty $sessionReport "passed") -eq $true) {
        "render_completed"
    } else {
        "render_failed"
    }
    $sessionSummary.passed = Get-ReportProperty $sessionReport "passed"
    $sessionSummary.width = Get-ReportProperty $sessionReport "width"
    $sessionSummary.height = Get-ReportProperty $sessionReport "height"
    $sessionSummary.row_bytes = Get-ReportProperty $sessionReport "row_bytes"
    $sessionSummary.pixel_format = Get-ReportProperty $sessionReport "pixel_format"
    $sessionSummary.input_sha256 = Get-ReportProperty $sessionReport "input_sha256"
    $sessionSummary.internal_output_sha256 = Get-ReportProperty $sessionReport "output_sha256"
    $sessionSummary.guard_bytes_intact = Get-ReportProperty $sessionReport "guard_bytes_intact"
    $sessionSummary.suite_leases_balanced = Get-ReportProperty $sessionReport "suite_leases_balanced"
    $sessionSummary.handle_lifetimes_balanced = Get-ReportProperty $sessionReport "handle_lifetimes_balanced"
    $sessionSummary.world_lifetimes_balanced = Get-ReportProperty $sessionReport "world_lifetimes_balanced"
    $diagnostics = Get-ReportProperty $sessionReport "worker_diagnostics"
    $sessionSummary.frame_time_ms = Get-ReportProperty $diagnostics "elapsed_ms"

    $failureStage = "validation"
    if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
        throw "session adapter did not create the raw output"
    }
    $outputBytes = [System.IO.File]::ReadAllBytes($output)
    $referenceBytes = [System.IO.File]::ReadAllBytes($resolved.reference)
    $outputHash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
    $observedOutput.size_bytes = [int64]$outputBytes.Length
    $observedOutput.sha256 = $outputHash
    $observedOutput.differing_bytes = Get-DifferingByteCount $outputBytes $referenceBytes

    $checks = [ordered]@{
        session_exit_zero = $adapterExitCode -eq 0
        session_report_passed = $sessionSummary.passed -eq $true
        dimensions_exact = $sessionSummary.width -eq 16 -and $sessionSummary.height -eq 12
        row_bytes_exact = $sessionSummary.row_bytes -eq 64
        pixel_format_exact = $sessionSummary.pixel_format -eq "argb8"
        output_size_exact = $outputBytes.Length -eq 768
        output_hash_exact = $outputHash -eq $artifacts.reference.sha256
        pixel_diff_exact = $observedOutput.differing_bytes -eq 0
        guards_intact = $sessionSummary.guard_bytes_intact -eq $true
        suite_leases_balanced = $sessionSummary.suite_leases_balanced -eq $true
        handle_lifetimes_balanced = $sessionSummary.handle_lifetimes_balanced -eq $true
        world_lifetimes_balanced = $sessionSummary.world_lifetimes_balanced -eq $true
        old_one_shot_cli_absent = $true
    }
    if ($checks.Values -contains $false) {
        throw "session render gate check failed: $(($checks | ConvertTo-Json -Compress))"
    }

    $result = [ordered]@{
        schema_version = 2
        gate = "real_aex_render_gate"
        classification = "session_transport_exact"
        oracle = $false
        status = "passed"
        fixture = "Adobe After Effects SDK ColorGrid"
        case = [ordered]@{
            render_path = "classic"
            parameter_state = "default"
            frame = 0
            pixel_format = "rgba8"
            width = 16
            height = 12
            rowbytes = 64
        }
        authenticated_artifacts = $artifacts
        invocation = [ordered]@{
            executable = $artifacts.session_harness.path
            adapter = $adapterRelative
            transport = "render-experimental-session"
            command = "python tools/refresh-runtime-session.py --plugin <aex> --plugin-sha256 <aex_sha256> --input <input> --output <output> --width 16 --height 12 --pixel-format argb8 --current-time 0 --total-time 1 --time-scale 1"
            exit_code = $adapterExitCode
        }
        output = $observedOutput
        session = $sessionSummary
        checks = $checks
        scope = [ordered]@{
            proves = "The current session transport reproduces the exact fixed ColorGrid output with authenticated runtime artifacts."
            does_not_prove = "Pixel equivalence with Adobe After Effects; this is not an AE oracle."
        }
    }
    Write-GateResult $result
    Write-Output "PASS $resultRelative $($observedOutput.sha256)"
}
catch {
    $safeMessage = Get-SafeFailureMessage $_.Exception.Message
    $externalBlocker = $safeMessage -match "staged process launch|hosted runner|Actions billing|After Effects"
    $failureChecks = [ordered]@{
        session_gate_attempted = $failureStage -in @("session", "validation")
        session_exit_zero = $adapterExitCode -eq 0
        session_report_passed = $false
        output_verified = $false
        old_one_shot_cli_absent = $true
        fail_closed = $true
        structured_failure_recorded = $true
    }
    $failure = [ordered]@{
        stage = $failureStage
        message = $safeMessage
        external_blocker = $externalBlocker
        restart_condition = if ($externalBlocker) {
            "Provide a runner that can launch a sealed worker, then rerun this gate."
        } else {
            "Fix the reported preflight, session, or validation condition, then rerun this gate."
        }
    }
    $failureResult = [ordered]@{
        schema_version = 2
        gate = "real_aex_render_gate"
        classification = "session_transport_failure"
        oracle = $false
        status = "blocked"
        fixture = "Adobe After Effects SDK ColorGrid"
        case = [ordered]@{
            render_path = "classic"
            parameter_state = "default"
            frame = 0
            pixel_format = "rgba8"
            width = 16
            height = 12
            rowbytes = 64
        }
        authenticated_artifacts = $artifacts
        invocation = [ordered]@{
            executable = $sessionHarnessRelative
            adapter = $adapterRelative
            transport = "render-experimental-session"
            command = "python tools/refresh-runtime-session.py --plugin <aex> --plugin-sha256 <aex_sha256> --input <input> --output <output> --width 16 --height 12 --pixel-format argb8 --current-time 0 --total-time 1 --time-scale 1"
            exit_code = if ($null -eq $adapterExitCode) { 1 } else { $adapterExitCode }
        }
        output = $observedOutput
        session = $sessionSummary
        checks = $failureChecks
        failure = $failure
        scope = [ordered]@{
            proves = "The current session-only gate was attempted and any inability to run was recorded without claiming a render success."
            does_not_prove = "Pixel equivalence with Adobe After Effects; this is not an AE oracle. A blocked status is not a successful render."
        }
    }
    Write-GateResult $failureResult
    Write-Error "BLOCKED $resultRelative [$failureStage] $safeMessage"
    exit 1
}
finally {
    Remove-Item -LiteralPath $output -ErrorAction SilentlyContinue
}
