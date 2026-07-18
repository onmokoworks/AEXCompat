[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$resultRelative = "analysis/REAL_AEX_RENDER_GATE_RESULT_2026-07-17.json"
$outputRelative = "target/image-transport/aex-render-gate-output.rgba"

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
    release_worker = [ordered]@{
        path = "target/minihost-build/aex_render_worker.exe"
        size_bytes = 779776
        sha256 = "2d037812b6159b9cf00b1beec60b354c33ba20942a5899d1b85941b6d49f8757"
    }
}

function Assert-Artifact([System.Collections.IDictionary]$artifact) {
    $absolute = Join-Path $root ($artifact.path -replace '/', '\')
    if (-not (Test-Path -LiteralPath $absolute -PathType Leaf)) {
        throw "Required artifact is missing: $($artifact.path)"
    }
    $item = Get-Item -LiteralPath $absolute
    if ($item.Length -ne $artifact.size_bytes) {
        throw "Artifact size mismatch: $($artifact.path)"
    }
    $actualHash = (Get-FileHash -LiteralPath $absolute -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $artifact.sha256) {
        throw "Artifact SHA-256 mismatch: $($artifact.path)"
    }
    return $absolute
}

$resolved = @{}
foreach ($entry in $artifacts.GetEnumerator()) {
    $resolved[$entry.Key] = Assert-Artifact $entry.Value
}

$output = Join-Path $root ($outputRelative -replace '/', '\')
$stdout = Join-Path $env:TEMP ("aex-render-gate-{0}.stdout" -f [guid]::NewGuid().ToString('N'))
$stderr = Join-Path $env:TEMP ("aex-render-gate-{0}.stderr" -f [guid]::NewGuid().ToString('N'))
Remove-Item -LiteralPath $output -ErrorAction SilentlyContinue

try {
    $arguments = @(
        "--render-image", $resolved.aex, $artifacts.aex.sha256, "v5|",
        $resolved.input, $output, "16", "12", "0", "1", "1", "1"
    )
    $process = Start-Process -FilePath $resolved.release_worker -ArgumentList $arguments `
        -WorkingDirectory $root -RedirectStandardOutput $stdout -RedirectStandardError $stderr `
        -Wait -PassThru -WindowStyle Hidden
    if ($process.ExitCode -ne 0) {
        throw "Release render worker failed with exit code $($process.ExitCode)"
    }

    $workerReport = Get-Content -LiteralPath $stdout -Raw | ConvertFrom-Json
    if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
        throw "Release render worker did not create output"
    }
    $outputItem = Get-Item -LiteralPath $output
    $outputHash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()

    $checks = [ordered]@{
        worker_exit_zero = $process.ExitCode -eq 0
        worker_status_completed = $workerReport.status -eq "render_completed"
        dimensions_exact = $workerReport.width -eq 16 -and $workerReport.height -eq 12 -and $workerReport.rowbytes -eq 64
        output_size_exact = $outputItem.Length -eq 768
        output_hash_exact = $outputHash -eq $artifacts.reference.sha256
        guards_intact = $workerReport.guard_bytes_intact -eq $true
        suite_leases_balanced = $workerReport.suite_leases_balanced -eq $true -and $workerReport.live_suite_lease_count -eq 0
        handle_lifetimes_balanced = $workerReport.handle_lifetimes_balanced -eq $true
        world_lifetimes_balanced = $workerReport.world_lifetimes_balanced -eq $true
        selector_success = $workerReport.global_setup_error -eq 0 -and $workerReport.params_setup_error -eq 0 -and $workerReport.render_error -eq 0 -and $workerReport.global_setdown_error -eq 0
    }
    if ($checks.Values -contains $false) {
        throw "AEX render gate check failed"
    }

    $result = [ordered]@{
        schema_version = 1
        gate = "real_aex_render_gate"
        classification = "host_regression_exact"
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
            executable = $artifacts.release_worker.path
            cli = "--render-image <aex> <aex_sha256> v5| <input> <output> 16 12 0 1 1 1"
            exit_code = $process.ExitCode
        }
        output = [ordered]@{
            size_bytes = $outputItem.Length
            sha256 = $outputHash
            expected_sha256 = $artifacts.reference.sha256
        }
        worker = [ordered]@{
            schema_version = $workerReport.schema_version
            stage = $workerReport.stage
            status = $workerReport.status
            internal_output_sha256 = $workerReport.output_sha256
            guard_bytes_intact = $workerReport.guard_bytes_intact
            suite_acquires = $workerReport.suite_acquires
            suite_releases = $workerReport.suite_releases
            live_suite_lease_count = $workerReport.live_suite_lease_count
            suite_leases_balanced = $workerReport.suite_leases_balanced
            handle_lifetimes_balanced = $workerReport.handle_lifetimes_balanced
            world_lifetimes_balanced = $workerReport.world_lifetimes_balanced
            last_seh_exception_code = $workerReport.last_seh_exception_code
        }
        checks = $checks
        scope = [ordered]@{
            proves = "The authenticated AEXCompat release worker reproduces this exact host output for the fixed default/frame0 vector."
            does_not_prove = "Pixel equivalence with Adobe After Effects; this is not an AE oracle."
        }
    }
    $resultPath = Join-Path $root ($resultRelative -replace '/', '\')
    $result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $resultPath -Encoding utf8
    Write-Output "PASS $resultRelative $outputHash"
}
finally {
    Remove-Item -LiteralPath $output, $stdout, $stderr -ErrorAction SilentlyContinue
}
