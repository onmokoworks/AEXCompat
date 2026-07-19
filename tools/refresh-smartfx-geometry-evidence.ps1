param(
    [Parameter(Mandatory = $true)][string]$TestedAex,
    [string]$Worker = 'target\minihost-build\aex_smart_worker.exe',
    [string]$Probe = 'target\pf-smart-geometry-probe-build\Release\pf_smart_geometry_probe.aex',
    [string]$OutJson = 'analysis\SMARTFX_GEOMETRY_CONTRACT_RESULT_2026-07-19.json'
)

# Regenerates the SmartFX geometry-contract evidence document (issue #8) by
# executing the geometry probe across its four time-selected modes and the
# supplied independent SmartFX AEX, each at ARGB8/16/32F. Evidence documents
# in analysis/ are refresh-script territory
# (docs/EVIDENCE_POLICY_2026-07-18.md section 5.2); this script records only
# repo-relative artifact identities and geometry report fields, never
# absolute paths, plug-in bytes, or raw image contents.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Identity([string]$Path) {
    $item = Get-Item -LiteralPath $Path
    $relative = $item.FullName
    $prefix = (Get-Item -LiteralPath $root).FullName + [IO.Path]::DirectorySeparatorChar
    if ($relative.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        $relative = $relative.Substring($prefix.Length)
    }
    [ordered]@{
        path = $relative.Replace('\', '/')
        size_bytes = $item.Length
        sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

$workerPath = (Resolve-Path -LiteralPath $Worker).Path
$probePath = (Resolve-Path -LiteralPath $Probe).Path
$aexPath = (Resolve-Path -LiteralPath $TestedAex).Path
# Evidence must never carry machine-absolute paths. The tested AEX is
# caller-supplied and may live anywhere; require a copy under the repo (for
# example target\<name>.aex) so its recorded identity stays repo-relative.
$repoPrefix = (Get-Item -LiteralPath $root).FullName + [IO.Path]::DirectorySeparatorChar
foreach ($artifact in @($workerPath, $probePath, $aexPath)) {
    if (-not $artifact.StartsWith($repoPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "artifact lies outside the repository and would freeze an absolute path: $artifact (copy it under the repo, e.g. target\, first)"
    }
}
$probeHash = (Get-FileHash -LiteralPath $probePath -Algorithm SHA256).Hash.ToLowerInvariant()
$aexHash = (Get-FileHash -LiteralPath $aexPath -Algorithm SHA256).Hash.ToLowerInvariant()

$scratch = Join-Path $root 'target\smartfx-geometry-evidence'
if (Test-Path -LiteralPath $scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
New-Item -ItemType Directory -Force $scratch | Out-Null

function WriteInput([string]$Path, [int]$Width, [int]$Height, [int]$Stride) {
    $bytes = [byte[]]::new($Width * $Height * 4)
    for ($i = 0; $i -lt $bytes.Length; $i++) { $bytes[$i] = (($i * $Stride) % 251) }
    [IO.File]::WriteAllBytes($Path, $bytes)
}

$geometryFields = @(
    'result_rect', 'max_result_rect', 'returns_extra_pixels',
    'result_within_request', 'extra_pixels_contract_violation',
    'empty_result_rect', 'smart_render_selector_dispatched',
    'input_checkout_request', 'input_checkout_result_rect',
    'malformed_checkout_request_count', 'empty_checkout_pixel_denial_count',
    'width', 'height', 'pre_render_error', 'smart_render_error',
    'result_rects_valid', 'pixel_format'
)

function RunSmart([string]$Command, [string]$Plugin, [string]$PluginHash,
                  [string]$InputPath, [string]$OutputPath,
                  [int]$Width, [int]$Height, [int]$Time) {
    # The worker narrates stages on stderr; under ErrorActionPreference Stop a
    # redirected native stderr line becomes a terminating NativeCommandError
    # in Windows PowerShell, so the preference is relaxed for the invocation.
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $json = & $workerPath $Command $Plugin $PluginHash 'v2|' $InputPath $OutputPath `
        $Width $Height $Time 1 4 1 2>$null
    $ErrorActionPreference = $saved
    if ($LASTEXITCODE -ne 0 -or -not $json) {
        throw "worker run failed: $Command time=$Time exit=$LASTEXITCODE"
    }
    $report = $json | ConvertFrom-Json
    $entry = [ordered]@{ command = $Command; render_time = $Time }
    foreach ($field in $geometryFields) { $entry[$field] = $report.$field }
    $entry
}

$depthCommands = @('--smart-image', '--smart-image16', '--smart-image32')

$probeInput = Join-Path $scratch 'probe-input.rgba'
WriteInput $probeInput 16 12 1
$probeRuns = @()
foreach ($command in $depthCommands) {
    foreach ($mode in 0, 1, 2, 3) {
        $output = Join-Path $scratch "probe-$($command.Substring(2))-$mode.bin"
        $probeRuns += RunSmart $command $probePath $probeHash $probeInput $output 16 12 $mode
    }
}

$aexInput = Join-Path $scratch 'real-aex-input.rgba'
WriteInput $aexInput 64 48 7
$realRuns = @()
foreach ($command in $depthCommands) {
    $output = Join-Path $scratch "real-aex-$($command.Substring(2)).bin"
    $realRuns += RunSmart $command $aexPath $aexHash $aexInput $output 64 48 0
}

function GeometryKey($Run) {
    (@($Run.result_rect) -join ',') + '|' + (@($Run.max_result_rect) -join ',') + '|' +
    "$($Run.returns_extra_pixels)|$($Run.result_within_request)|" +
    "$($Run.extra_pixels_contract_violation)|$($Run.empty_result_rect)|" +
    "$($Run.smart_render_selector_dispatched)|$($Run.width)x$($Run.height)|" +
    "$($Run.pre_render_error)|$($Run.smart_render_error)|$($Run.result_rects_valid)"
}

$probeModeKeys = @{}
$probeDepthConsistent = $true
foreach ($run in $probeRuns) {
    $key = GeometryKey $run
    if (-not $probeModeKeys.ContainsKey($run.render_time)) {
        $probeModeKeys[$run.render_time] = $key
    } elseif ($probeModeKeys[$run.render_time] -ne $key) {
        $probeDepthConsistent = $false
    }
}
$realKeys = @($realRuns | ForEach-Object { GeometryKey $_ } | Select-Object -Unique)
$assertions = [ordered]@{
    probe_all_runs_completed = -not (@($probeRuns | Where-Object {
        $_.pre_render_error -ne 0 -or $_.smart_render_error -ne 0 -or -not $_.result_rects_valid
    })).Count
    probe_geometry_identical_across_depths = $probeDepthConsistent
    probe_extra_pixels_flag_admits_overrun = -not (@($probeRuns | Where-Object {
        $_.render_time -eq 1 -and (-not $_.returns_extra_pixels -or $_.result_within_request -or $_.extra_pixels_contract_violation)
    })).Count
    probe_flagless_overrun_is_flagged = -not (@($probeRuns | Where-Object {
        $_.render_time -eq 2 -and -not $_.extra_pixels_contract_violation
    })).Count
    probe_empty_result_skips_selector = -not (@($probeRuns | Where-Object {
        $_.render_time -eq 3 -and ($_.smart_render_selector_dispatched -or -not $_.empty_result_rect)
    })).Count
    real_aex_geometry_identical_across_depths = $realKeys.Count -eq 1
    real_aex_all_runs_completed = -not (@($realRuns | Where-Object {
        $_.pre_render_error -ne 0 -or $_.smart_render_error -ne 0 -or -not $_.result_rects_valid
    })).Count
    ae_process_touched = $false
}
foreach ($name in $assertions.Keys) {
    if ($name -ne 'ae_process_touched' -and -not $assertions[$name]) {
        throw "assertion failed: $name"
    }
}

$document = [ordered]@{
    title = 'SmartFX geometry contract: probe modes and independent real AEX (issue #8)'
    generated_by = 'tools/refresh-smartfx-geometry-evidence.ps1'
    artifacts = [ordered]@{
        source = Identity (Join-Path $root 'minihost\src\l2_main.cpp')
        worker = Identity $workerPath
        probe = Identity $probePath
        probe_source = Identity (Join-Path $root 'instruments\pf-smart-geometry-probe\pf_smart_geometry_probe.cpp')
        tested_aex = Identity $aexPath
    }
    probe_runs = $probeRuns
    real_aex_runs = $realRuns
    assertions = $assertions
}
[IO.File]::WriteAllText((Join-Path $root $OutJson),
    (($document | ConvertTo-Json -Depth 16) + "`n"), [Text.UTF8Encoding]::new($false))
Write-Host "Wrote $OutJson"
