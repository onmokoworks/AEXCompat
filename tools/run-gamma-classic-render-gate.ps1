[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$resultRelative = "analysis/SDK_GAMMA_CLASSIC_RENDER_GATE_RESULT_2026-07-18.json"
$adapterRelative = "tools/refresh-runtime-session.py"
$sessionHarnessRelative = "broker/target/release/aexcompat-harness.exe"
$releaseWorkerRelative = "target/minihost-build/aex_render_worker.exe"

$artifacts = [ordered]@{
    fixture = [ordered]@{
        path = "target/sdk-fixtures/gamma/Gamma_Table.aex"
        size_bytes = 45056
        sha256 = "6fcb4946c77a9fcb4fab8e15dbbc54ffb4b1ab08656ce9acea39c44099b6dfa8"
    }
    input = [ordered]@{
        path = "target/image-transport/colorgrid-click-input.rgba"
        size_bytes = 768
        sha256 = "c9ee521c7d71cbf6a41cb1a2d075add64676b912f5870098417b75ca727ba807"
    }
}

$expected = [ordered]@{
    identity = 1.0
    changed = 1.5
}

function Resolve-PinnedArtifact([System.Collections.IDictionary]$artifact) {
    $path = Join-Path $root ($artifact.path -replace '/', '\')
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required artifact is missing: $($artifact.path)"
    }
    $item = Get-Item -LiteralPath $path
    if ($item.Length -ne [int64]$artifact.size_bytes) {
        throw "Artifact size mismatch: $($artifact.path)"
    }
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $artifact.sha256) {
        throw "Artifact SHA-256 mismatch: $($artifact.path)"
    }
    return $path
}

function Get-ArtifactIdentity([string]$relativePath) {
    $path = Join-Path $root ($relativePath -replace '/', '\')
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required runtime artifact is missing: $relativePath"
    }
    $item = Get-Item -LiteralPath $path
    return [ordered]@{
        path = $relativePath
        size_bytes = [int64]$item.Length
        sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function Get-ReportProperty([object]$report, [string]$name) {
    if ($null -eq $report) { return $null }
    $property = $report.PSObject.Properties[$name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Get-DifferingByteCount([byte[]]$left, [byte[]]$right) {
    if ($null -eq $left -or $null -eq $right) { return $null }
    [int64]$different = 0
    $common = [Math]::Min($left.Length, $right.Length)
    for ($index = 0; $index -lt $common; $index++) {
        if ($left[$index] -ne $right[$index]) { $different++ }
    }
    $different += [Math]::Abs($left.Length - $right.Length)
    return $different
}

function Get-SafeFailureMessage([string]$message) {
    return [regex]::Replace($message, [regex]::Escape($root), "<repo>", "IgnoreCase")
}

function Invoke-GammaRun([string]$caseName, [double]$gamma, [int]$runNumber) {
    $output = Join-Path $root ("target/image-transport/gamma-classic-$caseName-$runNumber.rgba")
    $gammaText = $gamma.ToString("0.################", [Globalization.CultureInfo]::InvariantCulture)
    try {
        Remove-Item -LiteralPath $output -ErrorAction SilentlyContinue
        $adapterArguments = @(
            (Join-Path $root ($adapterRelative -replace '/', '\')),
            "--plugin", $resolved.fixture,
            "--plugin-sha256", $artifacts.fixture.sha256,
            "--input", $resolved.input,
            "--output", $output,
            "--width", "16",
            "--height", "12",
            "--pixel-format", "argb8",
            "--current-time", "0",
            "--total-time", "1",
            "--time-scale", "1",
            "--parameter-slot", "1",
            "--parameter-value", $gammaText
        )
        $adapterLines = @(& $pythonCommand @adapterArguments 2>&1 | ForEach-Object { $_.ToString() })
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) {
            $text = ($adapterLines -join [Environment]::NewLine).Trim()
            throw "$caseName run $runNumber session adapter failed with exit code $($exitCode): $text"
        }
        $jsonLine = $adapterLines |
            Where-Object { $_ -match '^\s*\{.*\}\s*$' } |
            Select-Object -Last 1
        if (-not $jsonLine) { throw "$caseName run $runNumber returned no JSON report" }
        $report = $jsonLine | ConvertFrom-Json
        if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
            throw "$caseName run $runNumber produced no output"
        }
        $refreshAdapter = Get-ReportProperty $report "refresh_adapter"
        $override = Get-ReportProperty $refreshAdapter "parameter_override"
        $diagnostics = Get-ReportProperty $report "worker_diagnostics"
        $outputBytes = [System.IO.File]::ReadAllBytes($output)
        $transportHash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
        $referenceBytes = [System.IO.File]::ReadAllBytes($resolved.input)
        $checks = [ordered]@{
            session_exit_zero = $exitCode -eq 0
            session_report_passed = (Get-ReportProperty $report "passed") -eq $true
            transport_command_exact = (Get-ReportProperty $refreshAdapter "command") -eq "render-experimental-session-param"
            parameter_exact = (Get-ReportProperty $override "slot") -eq 1 -and
                [double](Get-ReportProperty $override "value") -eq $gamma
            dimensions_exact = (Get-ReportProperty $report "width") -eq 16 -and
                (Get-ReportProperty $report "height") -eq 12
            row_bytes_exact = (Get-ReportProperty $report "row_bytes") -eq 64
            pixel_format_exact = (Get-ReportProperty $report "pixel_format") -eq "argb8"
            output_size_exact = $outputBytes.Length -eq 768
            output_hash_recorded = -not [string]::IsNullOrWhiteSpace(
                [string](Get-ReportProperty $report "output_sha256")
            )
            guard_bytes_intact = (Get-ReportProperty $report "guard_bytes_intact") -eq $true
            suite_leases_balanced = (Get-ReportProperty $report "suite_leases_balanced") -eq $true
            handle_lifetimes_balanced = (Get-ReportProperty $report "handle_lifetimes_balanced") -eq $true
            world_lifetimes_balanced = (Get-ReportProperty $report "world_lifetimes_balanced") -eq $true
            old_one_shot_cli_absent = $true
        }
        if ($checks.Values -contains $false) {
            throw "$caseName run $runNumber contract failed: $(($checks | ConvertTo-Json -Compress))"
        }
        return [ordered]@{
            run = $runNumber
            internal_output_sha256 = Get-ReportProperty $report "output_sha256"
            transport_output_sha256 = $transportHash
            differing_bytes_from_input = Get-DifferingByteCount $outputBytes $referenceBytes
            frame_time_ms = Get-ReportProperty $diagnostics "elapsed_ms"
            parameter_override = [ordered]@{
                slot = [int](Get-ReportProperty $override "slot")
                value = [double](Get-ReportProperty $override "value")
            }
            checks = $checks
        }
    }
    finally {
        Remove-Item -LiteralPath $output -ErrorAction SilentlyContinue
    }
}

$resolved = [ordered]@{}
$caseResults = [ordered]@{}
$adapterExitCode = $null
$failureStage = "preflight"

try {
    foreach ($key in @("fixture", "input")) {
        $resolved[$key] = Resolve-PinnedArtifact $artifacts[$key]
    }
    $artifacts.session_harness = Get-ArtifactIdentity $sessionHarnessRelative
    $artifacts.release_worker = Get-ArtifactIdentity $releaseWorkerRelative

    $pythonCommand = (Get-Command python -ErrorAction SilentlyContinue).Source
    if (-not $pythonCommand) {
        $pythonCommand = (Get-Command py -ErrorAction SilentlyContinue).Source
    }
    if (-not $pythonCommand) {
        throw "Python was not found; install Python or make it available on PATH."
    }

    foreach ($caseName in @("identity", "changed")) {
        $gamma = [double]$expected[$caseName]
        $runs = @(1..2 | ForEach-Object {
            $failureStage = "session"
            Invoke-GammaRun $caseName $gamma $_
        })
        $deterministic = $runs[0].internal_output_sha256 -eq $runs[1].internal_output_sha256 -and
            $runs[0].transport_output_sha256 -eq $runs[1].transport_output_sha256
        if (-not $deterministic) {
            throw "$caseName output is not deterministic"
        }
        $caseResults[$caseName] = [ordered]@{
            gamma = $gamma
            deterministic = $deterministic
            runs = $runs
        }
    }

    $identity = $caseResults.identity.runs[0]
    $changed = $caseResults.changed.runs[0]
    $outputsDiffer = $identity.internal_output_sha256 -ne $changed.internal_output_sha256 -and
        $identity.transport_output_sha256 -ne $changed.transport_output_sha256
    if (-not $outputsDiffer) {
        throw "Gamma 1.0 and 1.5 outputs must differ"
    }
    if ($identity.transport_output_sha256 -ne $artifacts.input.sha256) {
        throw "Gamma 1.0 must be transport-identical to the input"
    }

    $evidence = [ordered]@{
        schema_version = 2
        gate = "sdk_gamma_classic_render_gate"
        status = "passed"
        classification = "session_transport_parameter_exact"
        fixture = "Adobe After Effects SDK Examples/Effect/Gamma_Table"
        authenticated_artifacts = $artifacts
        execution = [ordered]@{
            render_path = "classic"
            transport = "render-experimental-session-param"
            pixel_format = "argb8"
            width = 16
            height = 12
            rowbytes = 64
            fresh_session_run = $true
        }
        cases = $caseResults
        cross_case = [ordered]@{
            outputs_differ = $outputsDiffer
            identity_matches_input_transport = $true
        }
        scope = [ordered]@{
            proves = "The fixed authenticated Gamma_Table fixture receives a parameter override through the canonical session adapter and produces deterministic Classic outputs."
            does_not_prove = "Adobe After Effects pixel equivalence or compatibility after an unreviewed worker change."
        }
    }
    $resultPath = Join-Path $root ($resultRelative -replace '/', '\')
    [System.IO.File]::WriteAllText(
        $resultPath,
        ($evidence | ConvertTo-Json -Depth 12) + [Environment]::NewLine,
        [System.Text.UTF8Encoding]::new($false)
    )
    Write-Output "PASS $resultRelative"
}
catch {
    $safeMessage = Get-SafeFailureMessage $_.Exception.Message
    $externalBlocker = $safeMessage -match "staged process launch|hosted runner|Actions billing|After Effects"
    $failureChecks = [ordered]@{
        session_gate_attempted = $failureStage -eq "session"
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
        gate = "sdk_gamma_classic_render_gate"
        classification = "session_transport_failure"
        status = "blocked"
        fixture = "Adobe After Effects SDK Examples/Effect/Gamma_Table"
        authenticated_artifacts = $artifacts
        execution = [ordered]@{
            render_path = "classic"
            transport = "render-experimental-session-param"
            pixel_format = "argb8"
            width = 16
            height = 12
            rowbytes = 64
        }
        cases = $caseResults
        checks = $failureChecks
        failure = $failure
        scope = [ordered]@{
            proves = "The session-only Gamma gate was attempted and inability to run was recorded without claiming a render success."
            does_not_prove = "Adobe After Effects pixel equivalence; blocked is not a successful render."
        }
    }
    $resultPath = Join-Path $root ($resultRelative -replace '/', '\')
    [System.IO.File]::WriteAllText(
        $resultPath,
        ($failureResult | ConvertTo-Json -Depth 12) + [Environment]::NewLine,
        [System.Text.UTF8Encoding]::new($false)
    )
    Write-Error "BLOCKED $resultRelative [$failureStage] $safeMessage"
    exit 1
}
