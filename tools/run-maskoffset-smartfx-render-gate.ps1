[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$resultRelative = "analysis/MASKOFFSET_SMARTFX_RENDER_GATE_RESULT_2026-07-18.json"
$requestRelative = "target/render-requests/maskoffset-color-fill-20260713-001.json"
$allowlistRelative = "target/smart-allowlist/maskoffset.active.local.json"
$brokerRelative = "broker/target/release/broker.exe"
$workerRelative = "target/minihost-build/aex_smart_worker.exe"
$expectedOutput = "bf419f44e915901bac882e9b9e3411b8407df7f4e3a4a1c2719314bdfbb74b5f"
$expectedBrokerHash = "4cf1f06ca6ede35ee484e369f03591352a5b7bacdf002d04ba32094370ba9d02"
$expectedBrokerSize = 1147904
$expectedWorkerHash = "086df3b70d124b5f18c687702b0fbfe0d77d33fc954c10ea6e9e462fc4591ae0"
$expectedWorkerSize = 797696
$expectedRequestHash = "d92568e9f880ec9d06ceccb03de0ebb62d3dd00b0bdb9e3978c5fd687b7e26d7"

function Resolve-RepositoryFile([string]$relative) {
    $path = Join-Path $root ($relative -replace '/', '\')
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Required asset is missing: $relative" }
    return $path
}

function Assert-Identity([string]$path, [long]$size, [string]$sha256, [string]$label) {
    if ((Get-Item -LiteralPath $path).Length -ne $size) { throw "$label size mismatch" }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sha256) {
        throw "$label SHA-256 mismatch"
    }
}

$broker = Resolve-RepositoryFile $brokerRelative
$worker = Resolve-RepositoryFile $workerRelative
$request = Resolve-RepositoryFile $requestRelative
$allowlistPath = Resolve-RepositoryFile $allowlistRelative
Assert-Identity $broker $expectedBrokerSize $expectedBrokerHash "broker"
Assert-Identity $worker $expectedWorkerSize $expectedWorkerHash "SmartFX worker"
$requestSize = (Get-Item -LiteralPath $request).Length
Assert-Identity $request $requestSize $expectedRequestHash "render request"

$approval = @((Get-Content -LiteralPath $allowlistPath -Raw | ConvertFrom-Json).entries |
    Where-Object { $_.id -eq "maskoffset" })
if ($approval.Count -ne 1) { throw "Exactly one MaskOffset approval is required" }
$approval = $approval[0]
if ($approval.approved_stage -ne "smartfx_render" -or $approval.receipt_id -ne "maskoffset-smartfx-20260713-001") {
    throw "MaskOffset approval receipt mismatch"
}
if ($approval.trusted_worker.byte_size -ne $expectedWorkerSize -or
    -not $approval.trusted_worker.sha256.Equals($expectedWorkerHash, [StringComparison]::OrdinalIgnoreCase)) {
    throw "MaskOffset approval does not authenticate the fixed worker"
}
$fixture = $approval.load_tree.main
if (-not (Test-Path -LiteralPath $fixture.source_path -PathType Leaf)) { throw "Approved fixture is missing" }
Assert-Identity $fixture.source_path $fixture.byte_size $fixture.sha256.ToLowerInvariant() "MaskOffset fixture"

$receiptRelatives = 1..2 | ForEach-Object {
    "target/smart-request-render-results/maskoffset-gate-$([guid]::NewGuid().ToString('N')).local.json"
}
try {
    foreach ($receiptRelative in $receiptRelatives) {
        & $broker smart-parameter-request $requestRelative $receiptRelative
        if ($LASTEXITCODE -ne 0) { throw "Authenticated broker CLI failed with exit code $LASTEXITCODE" }
    }
    $receipts = @($receiptRelatives | ForEach-Object {
        Get-Content -LiteralPath (Join-Path $root ($_ -replace '/', '\')) -Raw | ConvertFrom-Json
    })
    $receiptChecks = foreach ($receipt in $receipts) {
        $workerRuns = @($receipt.run_1, $receipt.run_2)
        [ordered]@{
            receipt_passed = $receipt.passed -eq $true -and $receipt.accepted -eq $true -and $receipt.broker_survived -eq $true
            plugin_authenticated = $receipt.plugin_id -eq "maskoffset" -and $receipt.fixture_sha256.Equals($fixture.sha256, [StringComparison]::OrdinalIgnoreCase)
            request_authenticated = $receipt.assignment_count -eq 9 -and $workerRuns.Count -eq 2 -and ($workerRuns | Where-Object { $_.request_mode -ne $true }).Count -eq 0
            output_exact = $receipt.deterministic -eq $true -and $receipt.expected_oracle_sha256.Equals($expectedOutput, [StringComparison]::OrdinalIgnoreCase) -and
                ($workerRuns | Where-Object { -not $_.output_sha256.Equals($expectedOutput, [StringComparison]::OrdinalIgnoreCase) }).Count -eq 0
            selectors_success = ($workerRuns | Where-Object { $_.pre_render_error -ne 0 -or $_.smart_render_error -ne 0 -or $_.result_rects_valid -ne $true }).Count -eq 0
            guards_intact = ($workerRuns | Where-Object { $_.guard_bytes_intact -ne $true }).Count -eq 0
            secure_route = $receipt.secure_launch_count -eq 2 -and $receipt.normal_token_fallback -eq $false -and
                $receipt.secure_launch_1.launch_mode -eq "sealed_load_tree_restricted_token" -and
                $receipt.secure_launch_2.launch_mode -eq "sealed_load_tree_restricted_token" -and
                $receipt.secure_launch_1.worker_authenticated -eq $true -and
                $receipt.secure_launch_2.worker_authenticated -eq $true -and
                $receipt.secure_launch_1.module_audit_required -eq $true -and
                $receipt.secure_launch_2.module_audit_required -eq $true
            ownership_valid = ($workerRuns | Where-Object {
                $_.handle_lifetimes_balanced -ne $true -or $_.live_handle_count -ne 0 -or $_.live_handle_bytes -ne 0 -or
                $_.invalid_handle_operations -ne 0 -or $_.handles_created -ne $_.handles_disposed -or $_.handle_locks -ne $_.handle_unlocks -or
                $_.suite_acquires -lt $_.suite_releases -or $_.live_suite_reference_count -ne ($_.suite_acquires - $_.suite_releases)
            }).Count -eq 0
        }
    }
    if (($receiptChecks | ForEach-Object { $_.Values } | Where-Object { $_ -eq $false }).Count -ne 0) {
        throw "MaskOffset broker receipt validation failed"
    }
    $evidence = [ordered]@{
        schema_version = 2; gate = "maskoffset_smartfx_real_aex_render_gate"; classification = "host_regression_exact"
        oracle = $false; status = "passed"
        authenticated_artifacts = [ordered]@{
            broker = [ordered]@{ path = $brokerRelative; size_bytes = $expectedBrokerSize; sha256 = $expectedBrokerHash }
            worker = [ordered]@{ path = $workerRelative; size_bytes = $expectedWorkerSize; sha256 = $expectedWorkerHash }
            request = [ordered]@{ path = $requestRelative; size_bytes = $requestSize; sha256 = $expectedRequestHash }
            fixture = [ordered]@{ basename = $fixture.basename; size_bytes = $fixture.byte_size; sha256 = $fixture.sha256.ToLowerInvariant() }
            approval = [ordered]@{ receipt_id = $approval.receipt_id; approved_stage = $approval.approved_stage }
        }
        execution = [ordered]@{ invocation = "broker smart-parameter-request <request> <create-new-output>"; broker_invocation_count = 2; worker_runs_per_receipt = 2 }
        expected_output_sha256 = $expectedOutput
        receipts = @(0..1 | ForEach-Object { [ordered]@{ receipt = $_ + 1; output_sha256 = $receipts[$_].run_1.output_sha256; checks = $receiptChecks[$_] } })
        scope = [ordered]@{ does_not_prove = "Pixel equivalence with Adobe After Effects; this is not an AE pixel oracle." }
    }
    $evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $root ($resultRelative -replace '/', '\')) -Encoding utf8
    Write-Output "PASS $resultRelative $expectedOutput"
}
finally {
    $receiptRelatives | ForEach-Object { Remove-Item -LiteralPath (Join-Path $root ($_ -replace '/', '\')) -ErrorAction SilentlyContinue }
}
