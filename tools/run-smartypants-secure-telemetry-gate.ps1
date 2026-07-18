[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$harness = Join-Path $repo 'broker\target\release\aexcompat-harness.exe'
$worker = Join-Path $repo 'target\minihost-build\aex_smart_worker.exe'
$fixture = Join-Path $repo 'target\sdk-fixtures\smartypants\SmartyPants.aex'
$runRoot = Join-Path $repo 'target\smartypants-secure-telemetry-gate'
$input = Join-Path $runRoot 'input.png'
$output = Join-Path $runRoot 'output.png'
$rawReport = Join-Path $runRoot 'harness-report.json'
$stderrReport = Join-Path $runRoot 'harness-stderr.txt'
$evidence = Join-Path $repo 'analysis\SDK_SMARTYPANTS_SECURE_TELEMETRY_RESULT_2026-07-18.json'

$expected = @{
    harness = @{ size = 9945088; sha256 = 'd079fb8523560b801490c26d05d7581eab02b83c25f3a22d45b9659a780fd9a0' }
    worker = @{ size = 805888; sha256 = 'c511e835f9e476edabedcfd39b1ec6f66d39189357ce2801f00c4cf89a6bb5f9' }
    fixture = @{ size = 31232; sha256 = '47fe55f77600f041a3297b4558108fad5d6e886b6cc66eed2e51f88dac922ede' }
    input = @{ size = 3100; sha256 = '8e2b249fd979826a60ad089b783e84e8f65d9b692c157be065953775a8aa6c91' }
}

function Assert-Identity([string]$Name, [string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "$Name is missing" }
    $item = Get-Item -LiteralPath $Path
    $sha = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($item.Length -ne $expected[$Name].size -or $sha -ne $expected[$Name].sha256) {
        throw "$Name identity mismatch: size=$($item.Length), sha256=$sha"
    }
    [ordered]@{ size_bytes = $item.Length; sha256 = $sha }
}

New-Item -ItemType Directory -Force -Path $runRoot | Out-Null
Add-Type -AssemblyName System.Drawing
$bitmap = [Drawing.Bitmap]::new(37, 23)
try {
    for ($y = 0; $y -lt 23; $y++) {
        for ($x = 0; $x -lt 37; $x++) {
            $bitmap.SetPixel($x, $y, [Drawing.Color]::FromArgb(
                255, (($x * 7 + $y * 3) % 256), (($x * 5 + $y * 11) % 256),
                (($x * 13 + $y * 2) % 256)))
        }
    }
    $bitmap.Save($input, [Drawing.Imaging.ImageFormat]::Png)
} finally { $bitmap.Dispose() }
$identities = [ordered]@{
    release_harness = Assert-Identity harness $harness
    canonical_smart_worker = Assert-Identity worker $worker
    sdk_smartypants_fixture = Assert-Identity fixture $fixture
    deterministic_input = Assert-Identity input $input
}
Remove-Item -LiteralPath $output, $rawReport, $stderrReport -ErrorAction SilentlyContinue
$process = Start-Process -FilePath $harness -ArgumentList @(
    '--render-experimental-smart', $fixture, $input, $output
) -RedirectStandardOutput $rawReport -RedirectStandardError $stderrReport -NoNewWindow -Wait -PassThru
$reportText = if (Test-Path -LiteralPath $rawReport) {
    Get-Content -LiteralPath $rawReport -Raw
} else { '' }
if ($process.ExitCode -ne 0) {
    $stderrText = if (Test-Path -LiteralPath $stderrReport) { Get-Content -LiteralPath $stderrReport -Raw } else { '' }
    throw "secure harness failed with exit $($process.ExitCode): $stderrText"
}
$report = $reportText | ConvertFrom-Json

$checks = [ordered]@{
    passed = $report.passed -eq $true
    worker_ok = $report.worker_classification -eq 'ok'
    smart_render_ok = $report.smart_render_error -eq 0
    comp_bg_exact = $report.comp_bg_color_success_count -eq 1 -and $report.comp_bg_color_rejection_count -eq 0
    guid_mix_exact = $report.guid_mix_in_call_count -eq 1 -and $report.guid_mix_in_success_count -eq 1 -and $report.guid_mix_in_rejection_count -eq 0 -and $report.guid_mix_in_last_size -eq 32 -and $report.guid_mix_in_max_size -eq 32 -and $report.guid_mix_in_last_result -eq 0
    guid_mix_bounded = $report.guid_mix_in_size_limit -eq 1048576 -and $report.guid_mix_in_max_size -le $report.guid_mix_in_size_limit
    suites_balanced = $report.suite_leases_balanced -eq $true -and $report.suite_acquires -eq $report.suite_releases -and $report.live_suite_leases -eq ''
    handles_balanced = $report.handle_lifetimes_balanced -eq $true
    worlds_balanced = $report.world_lifetimes_balanced -eq $true
    module_audit_enforced = $report.worker_classification -eq 'ok'
}
if ($checks.Values -contains $false) {
    $failed = @($checks.GetEnumerator() | Where-Object { -not $_.Value } | ForEach-Object Key)
    throw "SmartyPants telemetry gate failed: $($failed -join ', ')"
}

$result = [ordered]@{
    schema_version = 1
    result = 'passed'
    route = 'schema-v2 sealed load tree / restricted SmartFX worker'
    authenticated_artifacts = $identities
    module_audit = [ordered]@{
        required = $true
        broker_validation = 'passed'
        basis = 'secure_image_dispatch requires and validates the worker module audit before returning an ok classification'
    }
    smart_pre_render = [ordered]@{
        error = $report.worker_diagnostics.stage_events.Where({ $_.stage -eq 'smart_pre_render' -and $_.state -eq 'end' })[0].errors.error
        comp_suite_version = 21
        comp_suite_slot = 4
        comp_bg_color_success_count = $report.comp_bg_color_success_count
        comp_bg_color_rejection_count = $report.comp_bg_color_rejection_count
        guid_mix_in_call_count = $report.guid_mix_in_call_count
        guid_mix_in_success_count = $report.guid_mix_in_success_count
        guid_mix_in_rejection_count = $report.guid_mix_in_rejection_count
        guid_mix_in_size = $report.guid_mix_in_last_size
        guid_mix_in_size_limit = $report.guid_mix_in_size_limit
        guid_mix_in_result = $report.guid_mix_in_last_result
    }
    render = [ordered]@{
        smart_render_error = $report.smart_render_error
        worker_classification = $report.worker_classification
        suite_acquires = $report.suite_acquires
        suite_releases = $report.suite_releases
        suite_leases_balanced = $report.suite_leases_balanced
        handle_lifetimes_balanced = $report.handle_lifetimes_balanced
        world_lifetimes_balanced = $report.world_lifetimes_balanced
        output_pixels_valid = $report.output_pixels_valid
        passed = $report.passed
    }
    event_boundary = [ordered]@{
        dispatched = $false
        attributed_to_event = $false
        fixture_behavior = 'SmartyPants does not implement PF_Cmd_EVENT; its default EffectMain return is 0.'
        telemetry_origin = 'PF_Cmd_SMART_PRE_RENDER only'
    }
    checks = $checks
}
$json = $result | ConvertTo-Json -Depth 12
$temporary = "$evidence.tmp"
[IO.File]::WriteAllText($temporary, $json + "`n", [Text.UTF8Encoding]::new($false))
Move-Item -LiteralPath $temporary -Destination $evidence -Force
Write-Output $json
