[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$resultRelative = "analysis/SDK_GAMMA_CLASSIC_RENDER_GATE_RESULT_2026-07-18.json"

$artifacts = [ordered]@{
    fixture = [ordered]@{
        path = "target/sdk-fixtures/gamma/Gamma_Table.aex"
        size_bytes = 45056
        sha256 = "6fcb4946c77a9fcb4fab8e15dbbc54ffb4b1ab08656ce9acea39c44099b6dfa8"
    }
    worker = [ordered]@{
        path = "target/minihost-build/aex_render_worker.exe"
        size_bytes = 811008
        sha256 = "2cba31875b7e0f39fff870a3fbff4fc14442fe4d73755dbb9b1f9b6be96d0e8e"
    }
    input = [ordered]@{
        path = "target/image-transport/colorgrid-click-input.rgba"
        size_bytes = 768
        sha256 = "c9ee521c7d71cbf6a41cb1a2d075add64676b912f5870098417b75ca727ba807"
    }
}

$expected = [ordered]@{
    identity = [ordered]@{
        gamma = 1.0
        internal_sha256 = "0d11e1fe35958d6d9cc29520a22212093d70e24f406350d7d785a0e6ff36923e"
        transport_sha256 = "c9ee521c7d71cbf6a41cb1a2d075add64676b912f5870098417b75ca727ba807"
    }
    changed = [ordered]@{
        gamma = 1.5
        internal_sha256 = "b70debc5ae1c2d1217da1de787b4fd097d006c9a92757e15407c70bc6eabc4a5"
        transport_sha256 = "8c65d686a935fc7aeb8a5d943cd2e1c3511d5c54e15349f7c318127dffcc72af"
    }
}

function Resolve-AuthenticatedArtifact([System.Collections.IDictionary]$artifact, [string]$label) {
    $path = Join-Path $root ($artifact.path -replace '/', '\')
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "$label is missing" }
    if ((Get-Item -LiteralPath $path).Length -ne $artifact.size_bytes) { throw "$label size mismatch" }
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $artifact.sha256) { throw "$label SHA-256 mismatch" }
    return $path
}

function Get-SelectorErrors([string]$nativeStderr) {
    $required = @(
        "global_setup", "params_setup", "sequence_setup", "frame_setup",
        "frame_setdown", "sequence_setdown", "render", "global_setdown"
    )
    $errors = [ordered]@{}
    foreach ($selector in $required) {
        $matches = [regex]::Matches($nativeStderr, "(?m)^stage:$($selector)_end error=(-?\d+)\s*$")
        if ($matches.Count -ne 1) { throw "Missing or duplicate selector result: $selector" }
        $errors[$selector] = [int]$matches[0].Groups[1].Value
    }
    return $errors
}

function Invoke-GammaRun([string]$caseName, [double]$gamma, [int]$runNumber) {
    $nonce = [guid]::NewGuid().ToString('N')
    $trustedRelative = "target/aexcompat-trusted-worker-gamma-$nonce"
    $sealedRelative = "target/aexcompat-sealed-gamma-$nonce"
    $trustedRoot = Join-Path $root ($trustedRelative -replace '/', '\')
    $sealedRoot = Join-Path $root ($sealedRelative -replace '/', '\')
    $trustedWorker = Join-Path $trustedRoot "trusted-worker.exe"
    $sealedFixture = Join-Path $sealedRoot "Gamma_Table.aex"
    $output = Join-Path $sealedRoot "output.rgba"
    $stdout = Join-Path $trustedRoot "stdout.json"
    $stderr = Join-Path $trustedRoot "stderr.log"

    New-Item -ItemType Directory -Path $trustedRoot, $sealedRoot | Out-Null
    try {
        Copy-Item -LiteralPath $resolved.worker -Destination $trustedWorker
        Copy-Item -LiteralPath $resolved.fixture -Destination $sealedFixture
        if ((Get-FileHash -LiteralPath $trustedWorker -Algorithm SHA256).Hash.ToLowerInvariant() -ne $artifacts.worker.sha256) {
            throw "Staged worker authentication failed"
        }
        if ((Get-FileHash -LiteralPath $sealedFixture -Algorithm SHA256).Hash.ToLowerInvariant() -ne $artifacts.fixture.sha256) {
            throw "Sealed fixture authentication failed"
        }

        $payload = "v2|param_1@1:f64=$($gamma.ToString('0.0', [Globalization.CultureInfo]::InvariantCulture))"
        $arguments = @(
            "--render-image", $sealedFixture, $artifacts.fixture.sha256, $payload,
            $resolved.input, $output, "16", "12", "0", "1", "1", "1"
        )
        $process = Start-Process -FilePath $trustedWorker -ArgumentList $arguments `
            -WorkingDirectory $trustedRoot -RedirectStandardOutput $stdout `
            -RedirectStandardError $stderr -Wait -PassThru -WindowStyle Hidden
        if ($process.ExitCode -ne 0) { throw "$caseName run $runNumber failed with exit code $($process.ExitCode)" }
        $report = Get-Content -LiteralPath $stdout -Raw | ConvertFrom-Json
        $selectorErrors = Get-SelectorErrors (Get-Content -LiteralPath $stderr -Raw)
        $transportHash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
        $audit = $report.module_audit

        $checks = [ordered]@{
            completed = $report.status -eq "render_completed" -and $report.render_error -eq 0
            dimensions_exact = $report.width -eq 16 -and $report.height -eq 12 -and $report.rowbytes -eq 64
            parameter_exact = @($report.requested_parameters).Count -eq 1 -and
                $report.requested_parameters[0].slot -eq 1 -and
                [double]$report.requested_parameters[0].value -eq $gamma
            output_exact = $report.output_sha256 -eq $expected[$caseName].internal_sha256 -and
                $transportHash -eq $expected[$caseName].transport_sha256 -and
                (Get-Item -LiteralPath $output).Length -eq 768
            selectors_success = @($selectorErrors.Values | Where-Object { $_ -ne 0 }).Count -eq 0
            guards_intact = $report.guard_bytes_intact -eq $true
            lifetimes_balanced = $report.suite_leases_balanced -eq $true -and
                $report.handle_lifetimes_balanced -eq $true -and
                $report.world_lifetimes_balanced -eq $true -and
                $report.param_checkouts_balanced -eq $true -and
                $report.pf_path_lifetimes_balanced -eq $true -and
                $report.receipt_lifetimes_balanced -eq $true -and
                $report.async_layer_requests_balanced -eq $true -and
                $report.gpu_memory_lifetimes_balanced -eq $true -and
                $report.audio_lifetimes_balanced -eq $true
            module_audit_passed = $audit.status -eq "passed" -and $audit.unknown_count -eq 0 -and
                $audit.phase_count -ge 3 -and $audit.post_load.status -eq "passed" -and
                $audit.pre_unload.status -eq "passed" -and $audit.observed_union.status -eq "passed"
        }
        if ($checks.Values -contains $false) { throw "$caseName run $runNumber contract failed" }

        return [ordered]@{
            run = $runNumber
            internal_output_sha256 = $report.output_sha256
            transport_output_sha256 = $transportHash
            selector_errors = $selectorErrors
            module_audit = [ordered]@{
                status = $audit.status
                unknown_count = $audit.unknown_count
                phase_count = $audit.phase_count
            }
            checks = $checks
        }
    }
    finally {
        Remove-Item -LiteralPath $output, $stdout, $stderr, $sealedFixture, $trustedWorker -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $sealedRoot, $trustedRoot -ErrorAction SilentlyContinue
    }
}

$resolved = @{}
foreach ($entry in $artifacts.GetEnumerator()) {
    $resolved[$entry.Key] = Resolve-AuthenticatedArtifact $entry.Value $entry.Key
}

$caseResults = [ordered]@{}
foreach ($caseName in @("identity", "changed")) {
    $runs = @(1..2 | ForEach-Object { Invoke-GammaRun $caseName $expected[$caseName].gamma $_ })
    $deterministic = $runs[0].internal_output_sha256 -eq $runs[1].internal_output_sha256 -and
        $runs[0].transport_output_sha256 -eq $runs[1].transport_output_sha256
    if (-not $deterministic) { throw "$caseName output is not deterministic" }
    $caseResults[$caseName] = [ordered]@{
        gamma = $expected[$caseName].gamma
        deterministic = $deterministic
        runs = $runs
    }
}

$outputsDiffer = $caseResults.identity.runs[0].internal_output_sha256 -ne $caseResults.changed.runs[0].internal_output_sha256 -and
    $caseResults.identity.runs[0].transport_output_sha256 -ne $caseResults.changed.runs[0].transport_output_sha256
if (-not $outputsDiffer) { throw "Gamma 1.0 and 1.5 outputs must differ" }
if ($caseResults.identity.runs[0].transport_output_sha256 -ne $artifacts.input.sha256) {
    throw "Gamma 1.0 must be transport-identical to the input"
}

$evidence = [ordered]@{
    schema_version = 1
    gate = "sdk_gamma_classic_render_gate"
    status = "passed"
    classification = "authenticated_host_regression_exact"
    fixture = "Adobe After Effects SDK Examples/Effect/Gamma_Table"
    authenticated_artifacts = $artifacts
    execution = [ordered]@{
        render_path = "classic"
        pixel_format = "argb8"
        width = 16
        height = 12
        worker_processes = 4
        fresh_authenticated_stage_per_run = $true
    }
    cases = $caseResults
    cross_case = [ordered]@{
        outputs_differ = $outputsDiffer
        identity_matches_input_transport = $true
    }
    scope = [ordered]@{
        proves = "The fixed authenticated Gamma_Table fixture is deterministic for Gamma 1.0 and 1.5 through the current canonical audited Classic worker."
        does_not_prove = "Adobe After Effects pixel equivalence or compatibility after an unreviewed worker change."
    }
}
$evidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $root ($resultRelative -replace '/', '\')) -Encoding utf8
Write-Output "PASS $resultRelative"
