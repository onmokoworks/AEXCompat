[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$runRoot = Join-Path $root 'target\shifter-production-matrix-gate'
$resultPath = Join-Path $root 'analysis\SDK_SHIFTER_PRODUCTION_MATRIX_RESULT_2026-07-18.json'
$harness = Join-Path $root 'broker\target\release\aexcompat-harness.exe'
$fixture = Join-Path $root 'target\sdk-fixtures\shifter\Shifter.aex'
$inputImage = Join-Path $root 'target\ae-oracle-colorgrid-input.png'

$expectedArtifacts = [ordered]@{
    release_harness = [ordered]@{ path = 'broker/target/release/aexcompat-harness.exe'; size_bytes = 9945600; sha256 = '0927d2ec34677b3326d210fa482ccf9035891413a03e25134f197238939b378f' }
    sdk_shifter_fixture = [ordered]@{ path = 'target/sdk-fixtures/shifter/Shifter.aex'; size_bytes = 23552; sha256 = 'f1f7c17eca0cef1f786f3770243f2727d174e45aee9b50b7d3982c188921f4c4' }
    deterministic_input = [ordered]@{ path = 'target/ae-oracle-colorgrid-input.png'; size_bytes = 677; sha256 = 'aad2e9973bc87d70af998a0522c4b83060539183230a4937744c6838fb66e2db' }
}
$cases = @(
    [ordered]@{ id = 'classic_argb8'; command = '--render-experimental'; render_path = 'classic'; pixel_format = 'argb8'; output_sha256 = '53f924951ebb8ad7a6cba214dac79f73ff3008d68dcdb681a0b7b91e08036f76' },
    [ordered]@{ id = 'classic_argb16'; command = '--render-experimental-16'; render_path = 'classic'; pixel_format = 'argb16'; output_sha256 = '80422bc3d307b4a25bdafcc84ac7fb01cb55a09810e8b0f37bb12e0edb5c48ca' },
    [ordered]@{ id = 'classic_argb32'; command = '--render-experimental-32'; render_path = 'classic'; pixel_format = 'argb32f'; output_sha256 = 'e80232b4d18d0bb7e794be263ba937626f383f9917d4b8a737ba893a8f752293' },
    [ordered]@{ id = 'smart_argb8'; command = '--render-experimental-smart'; render_path = 'smartfx'; pixel_format = 'argb8'; output_sha256 = '035b570ce7263e9b8d0dabefaba9b85c068bc6d4221c72e397df8fe6653a47f2' },
    [ordered]@{ id = 'smart_argb16'; command = '--render-experimental-smart-16'; render_path = 'smartfx'; pixel_format = 'argb16'; output_sha256 = '789bf72e48415186b1d028642795416bdf41b636e3500c413b7e5a34187f1868' },
    [ordered]@{ id = 'smart_argb32_cpu'; command = '--render-experimental-smart-32-cpu'; render_path = 'smartfx'; pixel_format = 'argb32f'; output_sha256 = 'beab8e9e207bb415c69d5948523e47ef92634aeb7de4efa22d114a52bb8f9ef6' }
)

function Assert-ArtifactIdentity([string]$name, [string]$path) {
    $expected = $expectedArtifacts[$name]
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "$name is missing" }
    $item = Get-Item -LiteralPath $path
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($item.Length -ne $expected.size_bytes -or $hash -ne $expected.sha256) {
        throw "$name identity mismatch: size=$($item.Length), sha256=$hash"
    }
}

function Test-StageDiagnostics($report, [string]$renderPath) {
    $events = @($report.worker_diagnostics.stage_events)
    $required = @('global_setup', 'params_setup', 'sequence_setup', 'frame_setup', 'frame_setdown', 'sequence_setdown', 'global_setdown')
    $required += if ($renderPath -eq 'classic') { @('render') } else { @('smart_render', 'smart_pre_render', 'smart_render_cpu') }
    foreach ($stage in $required) {
        if (@($events | Where-Object { $_.stage -eq $stage -and $_.state -eq 'begin' }).Count -ne 1 -or
            @($events | Where-Object { $_.stage -eq $stage -and $_.state -eq 'end' }).Count -ne 1) { return $false }
    }
    $errorEnds = @($events | Where-Object { $_.state -eq 'end' -and $null -ne $_.errors.error -and $_.errors.error -ne 0 })
    return $report.worker_diagnostics.classification -eq 'ok' -and
        [string]::IsNullOrEmpty($report.worker_diagnostics.failure_stage) -and
        $report.worker_diagnostics.last_completed_stage -eq 'global_setdown' -and $errorEnds.Count -eq 0
}

New-Item -ItemType Directory -Force -Path $runRoot | Out-Null
Assert-ArtifactIdentity release_harness $harness
Assert-ArtifactIdentity sdk_shifter_fixture $fixture
Assert-ArtifactIdentity deterministic_input $inputImage

$caseResults = @()
foreach ($case in $cases) {
    $runs = @()
    foreach ($runNumber in 1..2) {
        $stem = "$($case.id)-run-$runNumber"
        $output = Join-Path $runRoot "$stem.png"
        $stdout = Join-Path $runRoot "$stem.json"
        $stderr = Join-Path $runRoot "$stem.stderr.txt"
        Remove-Item -LiteralPath $output, $stdout, $stderr -ErrorAction SilentlyContinue
        $process = Start-Process -FilePath $harness -ArgumentList @($case.command, $fixture, $inputImage, $output) `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr -NoNewWindow -Wait -PassThru
        if ($process.ExitCode -ne 0) {
            $diagnostic = if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr -Raw } else { '' }
            throw "$($case.id) run $runNumber exited $($process.ExitCode): $diagnostic"
        }
        $report = Get-Content -LiteralPath $stdout -Raw | ConvertFrom-Json
        $checks = [ordered]@{
            output_identity = $report.output_sha256 -eq $case.output_sha256 -and (Test-Path -LiteralPath $output -PathType Leaf) -and (Get-Item -LiteralPath $output).Length -gt 0
            worker_ok = $report.passed -eq $true -and $report.worker_classification -eq 'ok'
            route_exact = $report.render_path -eq $case.render_path -and $report.pixel_format -eq $case.pixel_format
            guards_intact = $report.guard_bytes_intact -eq $true
            lifetimes_balanced = $report.suite_leases_balanced -eq $true -and $report.handle_lifetimes_balanced -eq $true -and $report.world_lifetimes_balanced -eq $true -and $report.param_checkouts_balanced -eq $true
            stage_diagnostics = Test-StageDiagnostics $report $case.render_path
            smart32_cpu_only = $case.id -ne 'smart_argb32_cpu' -or ($report.gpu_render_dispatched -eq $false -and $report.gpu_fallback_used -eq $false)
        }
        if ($checks.Values -contains $false) {
            $failed = @($checks.GetEnumerator() | Where-Object { -not $_.Value } | ForEach-Object Key)
            throw "$($case.id) run $runNumber failed: $($failed -join ', ')"
        }
        $runs += [ordered]@{ run = $runNumber; output_sha256 = $report.output_sha256; checks = $checks }
    }
    if ($runs[0].output_sha256 -ne $runs[1].output_sha256) { throw "$($case.id) is not deterministic" }
    $caseResults += [ordered]@{
        id = $case.id; command = $case.command; render_path = $case.render_path; pixel_format = $case.pixel_format
        expected_output_sha256 = $case.output_sha256; deterministic = $true; runs = $runs
    }
}

$result = [ordered]@{
    schema_version = 1
    gate = 'sdk_shifter_production_matrix_gate'
    status = 'passed'
    fixture = 'Adobe After Effects SDK Shifter'
    run_count = 12
    runs_per_case = 2
    authenticated_artifacts = $expectedArtifacts
    coverage = [ordered]@{ route_count = 6; classic = @('argb8', 'argb16', 'argb32f'); smart_cpu = @('argb8', 'argb16', 'argb32f') }
    cases = $caseResults
}
$json = $result | ConvertTo-Json -Depth 12
$temporary = "$resultPath.tmp"
[IO.File]::WriteAllText($temporary, $json + "`n", [Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $temporary -Destination $resultPath -Force
Write-Output "PASS analysis/SDK_SHIFTER_PRODUCTION_MATRIX_RESULT_2026-07-18.json (12/12 runs)"
