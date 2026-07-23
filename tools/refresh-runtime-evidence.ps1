[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$VerifyOnly
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$powerShell = Join-Path $PSHOME 'powershell.exe'
$pythonCommand = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $pythonCommand) { $pythonCommand = (Get-Command py -ErrorAction SilentlyContinue).Source }
if (-not $pythonCommand) { throw 'Python was not found.' }
$cargoCommand = (Get-Command cargo -ErrorAction SilentlyContinue).Source
$sessionAdapter = Join-Path $PSScriptRoot 'refresh-runtime-session.py'
$sessionHarness = Join-Path $root 'broker/target/release/aexcompat-harness.exe'
$cmake = (Get-Command cmake -ErrorAction SilentlyContinue).Source
if (-not $cmake) {
    $cmake = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
}
if (-not (Test-Path -LiteralPath $cmake -PathType Leaf)) { throw 'CMake was not found.' }
$vcvars = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
if (-not $env:INCLUDE) {
    cmd.exe /d /c "`"$vcvars`" >nul && set" | ForEach-Object {
        if ($_ -match '^([^=]+)=(.*)$') { Set-Item -LiteralPath "Env:$($matches[1])" -Value $matches[2] }
    }
}

function Get-Identity([string]$Path) {
    $item = Get-Item -LiteralPath $Path
    [ordered]@{
        path = $Path.Replace($root + '\', '').Replace('\', '/')
        size_bytes = $item.Length
        sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function Write-Json($Value, [string]$Path) {
    [IO.File]::WriteAllText($Path, (($Value | ConvertTo-Json -Depth 100) + "`n"), [Text.UTF8Encoding]::new($false))
}

function Invoke-Checked([string]$File, [string[]]$Arguments, [hashtable]$Environment = @{}) {
    $saved = @{}
    foreach ($name in $Environment.Keys) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $Environment[$name], 'Process')
    }
    try {
        & $File @Arguments
        if ($LASTEXITCODE -ne 0) { throw "$File failed with exit code $LASTEXITCODE" }
    } finally {
        foreach ($name in $Environment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process')
        }
    }
}

function Get-RelativePath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    $rootFull = ([IO.Path]::GetFullPath($root)).TrimEnd('\') + '\'
    if ($full.StartsWith($rootFull, [StringComparison]::OrdinalIgnoreCase)) {
        return $full.Substring($rootFull.Length).Replace('\', '/')
    }
    return $Path.Replace('\', '/')
}

function Quote-CommandToken([string]$Value) {
    return ('"' + $Value.Replace('"', '\"') + '"')
}

function Ensure-SessionHarness {
    if (Test-Path -LiteralPath $sessionHarness -PathType Leaf) { return }
    if ($SkipBuild) {
        throw "Session harness is missing: $sessionHarness (omit -SkipBuild to build it)."
    }
    if (-not $cargoCommand) { throw 'Cargo was not found; cannot build the session harness.' }
    Invoke-Checked $cargoCommand @(
        'build', '--manifest-path', 'broker/Cargo.toml', '-p', 'aexcompat-harness', '--locked', '--release'
    )
    if (-not (Test-Path -LiteralPath $sessionHarness -PathType Leaf)) {
        throw "Session harness build did not create: $sessionHarness"
    }
}

$buildScripts = @(
    'build-pf-adv-time-probe.ps1',
    'build-pf-composite-rect-probe.ps1',
    'build-pf-aegp-async-layer-receipt-probe.ps1',
    'build-pf-aegp-layer-receipt-probe.ps1',
    'build-pf-fill-premultiply-probe.ps1',
    'build-pf-sampling-probe.ps1',
    'build-pf-smart-timed-multilayer-probe.ps1'
)

if (-not $SkipBuild) {
    Invoke-Checked $cmake @('--build', 'target/minihost-build', '--config', 'Release')
    foreach ($script in $buildScripts) {
        $scriptPath = Join-Path $PSScriptRoot $script
        $sourceText = Get-Content -LiteralPath $scriptPath -Raw
        if ($sourceText -match 'target\\([^"'']+-build)') {
            $buildRoot = [IO.Path]::GetFullPath((Join-Path $root "target/$($matches[1])"))
            $targetRoot = [IO.Path]::GetFullPath((Join-Path $root 'target')) + [IO.Path]::DirectorySeparatorChar
            if (-not $buildRoot.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase)) {
                throw "Refusing to clear CMake cache outside target: $buildRoot"
            }
            Remove-Item -LiteralPath (Join-Path $buildRoot 'CMakeCache.txt') -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath (Join-Path $buildRoot 'CMakeFiles') -Recurse -Force -ErrorAction SilentlyContinue
        }
        $arguments = @('-NoProfile', '-File', $scriptPath)
        $parameters = (Get-Command $scriptPath).Parameters
        if ($parameters.ContainsKey('Generator')) { $arguments += @('-Generator', 'Visual Studio 17 2022') }
        if ($parameters.ContainsKey('CMake')) { $arguments += @('-CMake', $cmake) }
        Invoke-Checked $powerShell $arguments
    }
}

$evidenceNames = @(
    'PF_ADV_TIME_V4_RUNTIME_RESULT_2026-07-16.json',
    'PF_COMPOSITE_RECT_16_RUNTIME_RESULT_2026-07-16.json',
    'PF_AEGP_ASYNC_CANCEL_RUNTIME_RESULT_2026-07-16.json',
    'PF_AEGP_ASYNC_LAYER_RECEIPT_RUNTIME_RESULT_2026-07-16.json',
    'PF_AEGP_LAYER_RECEIPT_RUNTIME_RESULT_2026-07-16.json',
    'PF_SAMPLING_FILL_RUNTIME_RESULT_2026-07-16.json',
    'PF_SAMPLING_DEPTH_MATRIX_RESULT_2026-07-17.json',
    'SDK_COLORGRID_ARBITRARY_TIMELINE_RESULT_2026-07-17.json',
    'REAL_AEX_SMART_TIMED_MULTI_LAYER_RESULT_2026-07-17.json',
    'AEGP_ASYNC_RECEIPT_RUNTIME_RESULT_2026-07-16.json'
)

function Get-Commands($Value) {
    $result = @()
    if ($null -eq $Value) { return $result }
    if ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) {
        foreach ($entry in $Value) { $result += Get-Commands $entry }
    } elseif ($Value -is [pscustomobject]) {
        foreach ($property in $Value.psobject.Properties) {
            if ($property.Name -eq 'command' -and $property.Value -is [string] -and $property.Value -match '(aex_(render|smart)_worker\.exe|refresh-runtime-session\.py)') {
                $result += $property.Value
            } else { $result += Get-Commands $property.Value }
        }
    }
    return $result
}

$commandRewrites = @{}

function Invoke-LegacySessionRender([string[]]$LegacyArgs) {
    $command = [string]$LegacyArgs[0]
    if ($command -notmatch '^--(render|smart)-image(16|32)?$') {
        throw "Unsupported legacy render command in runtime evidence: $command"
    }
    if ($LegacyArgs.Count -lt 11) {
        throw "Legacy render command has too few arguments: $command"
    }
    Ensure-SessionHarness
    $plugin = [string]$LegacyArgs[1]
    $input = [string]$LegacyArgs[4]
    $output = [string]$LegacyArgs[5]
    $width = [int]$LegacyArgs[6]
    $height = [int]$LegacyArgs[7]
    $currentTime = [int]$LegacyArgs[8]
    $totalTime = [int]$LegacyArgs[9]
    $timeScale = [int]$LegacyArgs[10]
    $pixelFormat = if ($command -match '16$') { 'argb16' } elseif ($command -match '32$') { 'argb32f' } else { 'argb8' }
    $smart = $command.StartsWith('--smart-')
    if (-not (Test-Path -LiteralPath $plugin -PathType Leaf)) { throw "Legacy plugin is missing: $plugin" }
    if (-not (Test-Path -LiteralPath $input -PathType Leaf)) { throw "Legacy input is missing: $input" }
    $pluginSha = (Get-FileHash -LiteralPath $plugin -Algorithm SHA256).Hash.ToLowerInvariant()
    Remove-Item -LiteralPath $output -Force -ErrorAction SilentlyContinue
    $adapterArgs = @(
        $sessionAdapter, '--plugin', $plugin, '--plugin-sha256', $pluginSha,
        '--input', $input, '--output', $output, '--width', $width, '--height', $height,
        '--pixel-format', $pixelFormat, '--current-time', $currentTime,
        '--total-time', $totalTime, '--time-scale', $timeScale
    )
    if ($smart) { $adapterArgs += '--smart' }
    Write-Host "Migrating legacy render through session adapter: $command"
    Invoke-Checked $pythonCommand $adapterArgs @{
        TEMP = (Join-Path $root 'target/tmp')
        TMP = (Join-Path $root 'target/tmp')
        PYTHONUTF8 = '1'
    } | Out-Host
    $canonical = 'python tools/refresh-runtime-session.py --plugin {0} --plugin-sha256 {1} --input {2} --output {3} --width {4} --height {5} --pixel-format {6} --current-time {7} --total-time {8} --time-scale {9}' -f @(
        (Quote-CommandToken (Get-RelativePath $plugin)), $pluginSha,
        (Quote-CommandToken (Get-RelativePath $input)), (Quote-CommandToken (Get-RelativePath $output)),
        $width, $height, $pixelFormat, $currentTime, $totalTime, $timeScale
    )
    if ($smart) { $canonical += ' --smart' }
    return $canonical
}

function Invoke-RecordedCommand([string]$Command) {
    $tokens = [regex]::Matches($Command, '"[^"]*"|\S+') | ForEach-Object { $_.Value.Trim('"') }
    if ($tokens.Count -lt 1) { throw 'Recorded command is empty.' }
    if ($tokens.Count -gt 1 -and $tokens[1] -match 'refresh-runtime-session\.py') {
        Ensure-SessionHarness
        $adapterArgs = @($tokens[2..($tokens.Count - 1)])
        Invoke-Checked $pythonCommand $adapterArgs @{
            TEMP = (Join-Path $root 'target/tmp')
            TMP = (Join-Path $root 'target/tmp')
            PYTHONUTF8 = '1'
        }
        return $Command
    }
    $exe = $tokens[0]
    $recordedArgs = @($tokens[1..($tokens.Count - 1)])
    if ($recordedArgs.Count -gt 0 -and $recordedArgs[0] -match '^--(render|smart)-image') {
        return (Invoke-LegacySessionRender $recordedArgs)
    }
    if ($recordedArgs.Count -gt 2 -and $recordedArgs[0] -match '^--render-' -and (Test-Path $recordedArgs[1])) {
        $recordedArgs[2] = (Get-FileHash -LiteralPath $recordedArgs[1] -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($recordedArgs.Count -gt 5) { Remove-Item -LiteralPath $recordedArgs[5] -Force -ErrorAction SilentlyContinue }
    }
    Write-Host "Running recorded command: $exe $($recordedArgs -join ' ')"
    Invoke-Checked $exe $recordedArgs @{
        TEMP = (Join-Path $root 'target/tmp')
        TMP = (Join-Path $root 'target/tmp')
    }
    return $Command
}

foreach ($name in $evidenceNames) {
    $path = Join-Path $root "analysis/$name"
    $document = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    foreach ($command in (Get-Commands $document | Select-Object -Unique)) {
        $rewritten = Invoke-RecordedCommand $command
        if ($rewritten -ne $command) { $commandRewrites[$command] = $rewritten }
    }
}

function Update-Identities($Value) {
    if ($null -eq $Value) { return }
    if ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) {
        foreach ($entry in $Value) { Update-Identities $entry }
        return
    }
    if ($Value -isnot [pscustomobject]) { return }
    $names = @($Value.psobject.Properties.Name)
    if ($names -contains 'command' -and $commandRewrites.ContainsKey([string]$Value.command)) {
        $Value.command = $commandRewrites[[string]$Value.command]
    }
    if ($names -contains 'path' -and $names -contains 'size_bytes' -and $names -contains 'sha256') {
        $candidate = [string]$Value.path
        if (-not [IO.Path]::IsPathRooted($candidate)) { $candidate = Join-Path $root $candidate }
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            $item = Get-Item -LiteralPath $candidate
            $Value.size_bytes = $item.Length
            $Value.sha256 = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }
    if ($names -contains 'artifact' -and $names -contains 'size_bytes' -and $names -contains 'sha256') {
        $candidate = Join-Path $root ([string]$Value.artifact)
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            $oldHash = [string]$Value.sha256
            $Value.size_bytes = (Get-Item -LiteralPath $candidate).Length
            $Value.sha256 = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($names -contains 'command') { $Value.command = ([string]$Value.command).Replace($oldHash, $Value.sha256) }
        }
    }
    foreach ($property in $Value.psobject.Properties) { Update-Identities $property.Value }
}

foreach ($name in $evidenceNames) {
    $path = Join-Path $root "analysis/$name"
    $document = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    Update-Identities $document
    if (-not $VerifyOnly) { Write-Json $document $path }
}

if (-not $VerifyOnly) {
    # PlanOnly is the only permitted AE refresh here. The generator records a live AE process as blocked.
    $planPath = Join-Path $root 'analysis/AE_ORACLE_COLORGRID_CAPTURE_PLAN_2026-07-17.json'
    $oldPlan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    $temporaryPlan = Join-Path $root 'target/ae-oracle-colorgrid-plan.refresh.json'
    Remove-Item -LiteralPath $temporaryPlan -ErrorAction SilentlyContinue
    Invoke-Checked $powerShell @(
        '-NoProfile', '-File', (Join-Path $PSScriptRoot 'capture-ae-probe-oracle.ps1'),
        '-AfterEffects', 'C:/Program Files/Adobe/Adobe After Effects 2025/Support Files/AfterFX.exe',
        '-ProbeAex', 'target/sdk-fixtures/colorgrid/ColorGrid.aex',
        '-InputImage', 'target/ae-oracle-colorgrid-input.png',
        '-OutputPng', 'target/ae-oracle-colorgrid-reference.png',
        '-EffectName', 'Color Grid', '-Bpc', '8',
        '-ExpectedRaw', 'target/image-transport/colorgrid-normal.rgba',
        '-Width', '16', '-Height', '12', '-RawFormat', 'rgba8', '-Tolerance', '0',
        '-ComparisonReport', 'analysis/AE_ORACLE_COLORGRID_COMPARISON_2026-07-17.json',
        '-PlanOnly', '-PlanPath', $temporaryPlan
    )
    $newPlan = Get-Content -LiteralPath $temporaryPlan -Raw | ConvertFrom-Json
    $newPlan | Add-Member plan_mode PlanOnly -Force
    $newPlan | Add-Member current_artifact_snapshot $oldPlan.current_artifact_snapshot -Force
    $newPlan | Add-Member runtime_evidence_authentication $oldPlan.runtime_evidence_authentication -Force
    $newPlan | Add-Member expected_raw_provenance $oldPlan.expected_raw_provenance -Force
    $newPlan.input | Add-Member size_bytes (Get-Item -LiteralPath $newPlan.input.path).Length -Force
    Update-Identities $newPlan
    Write-Json $newPlan $planPath
    Remove-Item -LiteralPath $temporaryPlan

    $aggregatePath = Join-Path $root 'analysis/GENERAL_EFFECT_RUNTIME_COVERAGE_2026-07-16.json'
    $aggregate = Get-Content -LiteralPath $aggregatePath -Raw | ConvertFrom-Json
    Update-Identities $aggregate
    $aggregate.current_artifact_snapshot.captured_at = (Get-Date).ToString('yyyy-MM-dd')
    Write-Json $aggregate $aggregatePath
}

Write-Host "Refreshed runtime evidence from canonical executions."
