[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$harness = Join-Path $root 'broker\target\release\aexcompat-harness.exe'
$worker = Join-Path $root 'target\minihost-build\aex_l2_worker.exe'
$fixture = Join-Path $root 'target\sdk-fixtures\glator\GLator.aex'
$runRoot = Join-Path $root 'target\glator-runtime-policy-inspect'
$policyPath = Join-Path $runRoot 'opengl-policy.local.json'
$stdoutPath = Join-Path $runRoot 'stdout.json'
$stderrPath = Join-Path $runRoot 'stderr.txt'
$evidencePath = Join-Path $root 'analysis\SDK_GLATOR_RUNTIME_POLICY_INSPECT_RESULT_2026-07-18.json'

$expected = [ordered]@{
    harness = @{ path = $harness; size = 9945600; sha256 = 'b713fb4e4abb07b792943028c312027a76eb55b9c43c6cc9149c5937579ca80a' }
    worker = @{ path = $worker; size = 738304; sha256 = '6a4ef95698e107e4e532c8043101c6513f60926e44ef488aede0fdbab88766dc' }
    fixture = @{ path = $fixture; size = 6087168; sha256 = 'da8447f6f88e78fb00d5bd2d7e0cdc1d6bdcf1918b77e8684288a6d700c9f2ce' }
}
$moduleSpecs = @(
    @{ basename = 'nvoglv64.dll'; size = 46564584; sha256 = 'ca92775bcfa44eaa8ccb8c530e8df45bf88e19573cea4a6e0232342114355238'; version = '32.0.15.9579' },
    @{ basename = 'nvgpucomp64.dll'; size = 83758904; sha256 = 'c2d9c1d9a20b1275a6c4c66bd45e168088ffb3d5fdc928121a61531843f23098'; version = '32.0.15.9579' }
)

function Assert-Identity($spec) {
    if (-not (Test-Path -LiteralPath $spec.path -PathType Leaf)) { throw 'Required artifact is missing.' }
    $item = Get-Item -LiteralPath $spec.path
    $sha = (Get-FileHash -LiteralPath $spec.path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($item.Length -ne $spec.size -or $sha -ne $spec.sha256) { throw 'Artifact identity mismatch.' }
    [ordered]@{ size_bytes = [long]$item.Length; sha256 = $sha }
}

New-Item -ItemType Directory -Force -Path $runRoot | Out-Null
$authenticated = [ordered]@{
    release_harness = Assert-Identity $expected.harness
    canonical_l2_worker = Assert-Identity $expected.worker
    sdk_glator_fixture = Assert-Identity $expected.fixture
}

$modules = @()
foreach ($spec in $moduleSpecs) {
    $matches = @(Get-ChildItem 'C:\Windows\System32\DriverStore\FileRepository' -Recurse -Filter $spec.basename -ErrorAction Stop | Where-Object {
        $_.Length -eq $spec.size -and
        (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -eq $spec.sha256 -and
        $_.VersionInfo.FileVersion -eq $spec.version
    })
    if ($matches.Count -ne 1) { throw "Expected exactly one authenticated $($spec.basename)." }
    $modules += [ordered]@{
        path = '\\?\' + $matches[0].FullName
        basename = $spec.basename
        sha256 = $spec.sha256
        size = [long]$spec.size
        backend = 'opengl'
        version = $spec.version
    }
}

$policy = [ordered]@{
    schema_version = 1
    expires = [DateTimeOffset]::UtcNow.AddHours(1).ToString('yyyy-MM-ddTHH:mm:ssZ')
    modules = $modules
}
Remove-Item -LiteralPath $stdoutPath, $stderrPath -ErrorAction SilentlyContinue

try {
    $undercomplete = [ordered]@{ schema_version = 1; expires = $policy.expires; modules = @($modules[0]) }
    [IO.File]::WriteAllText($policyPath, ($undercomplete | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    $negativeProcess = Start-Process -FilePath $harness -WindowStyle Hidden -Wait -PassThru `
        -ArgumentList @('--inspect-experimental-runtime-policy', $fixture, 'opengl', $policyPath) `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    if ($negativeProcess.ExitCode -eq 0) { throw 'Undercomplete runtime policy unexpectedly passed.' }
    $negativeText = Get-Content -LiteralPath $stderrPath -Raw
    $negativeMarker = 'AEX parameter inspection worker failed safely: '
    $negativeMarkerIndex = $negativeText.IndexOf($negativeMarker, [StringComparison]::Ordinal)
    if ($negativeMarkerIndex -lt 0) { throw 'Undercomplete policy diagnostic was malformed.' }
    $negative = $negativeText.Substring($negativeMarkerIndex + $negativeMarker.Length).Trim() | ConvertFrom-Json
    if ($negative.module_audit_failure.unknown_count -ne 1 -or
        @($negative.module_audit_failure.authorized_policy_modules).Count -ne 1 -or
        $negative.module_audit_failure.authorized_policy_modules[0] -ne 'nvoglv64.dll') {
        throw 'Undercomplete policy did not remain fail-closed.'
    }

    [IO.File]::WriteAllText($policyPath, ($policy | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    Remove-Item -LiteralPath $stdoutPath, $stderrPath -ErrorAction SilentlyContinue
    $process = Start-Process -FilePath $harness -WindowStyle Hidden -Wait -PassThru `
        -ArgumentList @('--inspect-experimental-runtime-policy', $fixture, 'opengl', $policyPath) `
        -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
    if ($process.ExitCode -ne 0) {
        throw "Runtime-policy inspect failed safely with exit $($process.ExitCode)."
    }
    $report = Get-Content -LiteralPath $stdoutPath -Raw | ConvertFrom-Json
    $policyModules = @($report.diagnostics.module_audit.authorized_policy_modules | Sort-Object)
    $expectedModules = @($moduleSpecs.basename | Sort-Object)
    $passed = $report.diagnostics.classification -eq 'ok' -and
        $report.diagnostics.runtime_module_policy_applied -eq $true -and
        $report.diagnostics.module_audit.status -eq 'passed' -and
        $report.diagnostics.module_audit.unknown_count -eq 0 -and
        $policyModules.Count -eq 2 -and
        (Compare-Object $policyModules $expectedModules).Count -eq 0 -and
        $report.parameters.Count -eq 1 -and $report.parameters[0].name -eq 'GLator'
    if (-not $passed) { throw 'Runtime-policy inspect contract failed.' }

    $evidence = [ordered]@{
        schema_version = 1
        result = 'passed'
        route = 'sealed restricted L2 worker / purpose-bound OpenGL runtime module authorization'
        authenticated_artifacts = $authenticated
        authorized_runtime_modules = @($moduleSpecs | ForEach-Object {
            [ordered]@{ basename = $_.basename; size_bytes = [long]$_.size; sha256 = $_.sha256; version = $_.version; backend = 'opengl' }
        })
        inspect = [ordered]@{
            classification = $report.diagnostics.classification
            runtime_module_policy_applied = $report.diagnostics.runtime_module_policy_applied
            module_audit_status = $report.diagnostics.module_audit.status
            unknown_count = $report.diagnostics.module_audit.unknown_count
            policy_modules = $policyModules
            global_setup_error = $report.diagnostics.stage_events[1].errors.error
            params_setup_error = $report.diagnostics.stage_events[3].errors.error
            global_setdown_error = $report.diagnostics.stage_events[5].errors.error
            parameter_count = $report.parameters.Count
            parameter_name = $report.parameters[0].name
        }
        fail_closed_control = [ordered]@{
            omitted_module = 'nvgpucomp64.dll'
            worker_exit_code = $negative.exit_code
            first_failure_stage = $negative.first_failure_stage
            unknown_count = $negative.module_audit_failure.unknown_count
            authorized_policy_modules = @($negative.module_audit_failure.authorized_policy_modules)
            passed = $false
        }
        privacy = [ordered]@{ local_paths_exported = $false; private_stderr_exported = $false }
    }
    [IO.File]::WriteAllText($evidencePath, (($evidence | ConvertTo-Json -Depth 12) + "`n"), [Text.UTF8Encoding]::new($false))
    Write-Output "PASS analysis/SDK_GLATOR_RUNTIME_POLICY_INSPECT_RESULT_2026-07-18.json"
} finally {
    Remove-Item -LiteralPath $policyPath, $stdoutPath, $stderrPath -ErrorAction SilentlyContinue
}
