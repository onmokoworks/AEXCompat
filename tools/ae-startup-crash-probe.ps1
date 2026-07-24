[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$AfterEffects,
    [Parameter(Mandatory = $true)][string]$TestedAex,
    [Parameter(Mandatory = $true)][string]$Bundle,
    [Parameter(Mandatory = $true)][string]$AfterEffectsArguments,
    [ValidateSet('RealAE', 'ControlledFixture')][string]$EvidenceOrigin = 'RealAE',
    [string]$PythonPath = 'python',
    [ValidateRange(5, 600)][int]$TimeoutSeconds = 60,
    [switch]$EnableMinidump,
    [string]$DumpDirectory,
    [ValidateRange(1, 67108864)][long]$MaxDumpBytes = 67108864,
    [ValidateRange(1, 268435456)][long]$MaxTotalDumpBytes = 268435456,
    [ValidateRange(1, 16)][int]$MaxRetainedDumps = 16
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'sha256.ps1')
. (Join-Path $PSScriptRoot 'windows-file-identity.ps1')

function Write-Utf8Json([string]$Path, [object]$Value) {
    $json = $Value | ConvertTo-Json -Depth 32
    [System.IO.File]::WriteAllText($Path, $json + "`n", [System.Text.UTF8Encoding]::new($false))
}

function Limit-Text([string]$Value, [int]$Maximum = 4096) {
    if ($null -eq $Value) { return '' }
    if ($Value.Length -le $Maximum) { return $Value }
    return $Value.Substring(0, $Maximum) + '...[truncated]'
}

function Get-SafeFullPath([string]$Path) {
    try { return [System.IO.Path]::GetFullPath($Path) } catch { return $Path }
}

function Get-PlaceholderIdentity([string]$Path) {
    $full = Get-SafeFullPath $Path
    [ordered]@{
        path = $full
        sha256 = ('0' * 64)
        size_bytes = 0
        file_id = 'unavailable'
        canonical_path_sha256 = (Get-Sha256HexFromText $full.ToLowerInvariant())
    }
}

function Get-ExternalIdentity([string]$Path) {
    $stream = [System.IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
    try {
        $identity = Get-LockedFileIdentity $stream
        [ordered]@{
            path = [System.IO.Path]::GetFullPath($Path)
            sha256 = ([string]$identity.sha256).ToLowerInvariant()
            size_bytes = [int64]$stream.Length
            file_id = [string]$identity.file_id
            canonical_path_sha256 = ([string]$identity.canonical_path_sha256).ToLowerInvariant()
        }
    } finally { $stream.Dispose() }
}

function Get-IdentityOrPlaceholder([string]$Path) {
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        try { return Get-ExternalIdentity $Path } catch { }
    }
    Get-PlaceholderIdentity $Path
}

function Get-BundleArtifact([string]$Root, [string]$Path, [string]$DisplayPath = '') {
    $full = [System.IO.Path]::GetFullPath($Path)
    $relative = $DisplayPath
    if (-not $relative) {
        $prefix = $Root.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
        if (-not $full.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) { throw "artifact is outside bundle: $full" }
        $relative = $full.Substring($prefix.Length) -replace '\\', '/'
    }
    [ordered]@{
        path = ($relative -replace '\\', '/')
        sha256 = (Get-Sha256Hex $full).ToLowerInvariant()
        size_bytes = [int64](Get-Item -LiteralPath $full).Length
    }
}

function Get-ProcessTreeIds([int]$RootId) {
    $rows = @()
    try { $rows = @(Get-CimInstance Win32_Process -ErrorAction Stop | Select-Object ProcessId, ParentProcessId) } catch { return @($RootId) }
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    [void]$ids.Add($RootId)
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($row in $rows) {
            if ($ids.Contains([int]$row.ParentProcessId) -and $ids.Add([int]$row.ProcessId)) { $changed = $true }
        }
    }
    @($ids | Sort-Object)
}

function Get-ProcessTree([int]$RootId) {
    $records = @()
    foreach ($id in @(Get-ProcessTreeIds $RootId)) {
        try { $row = Get-CimInstance Win32_Process -Filter "ProcessId = $id" -ErrorAction Stop } catch { $row = $null }
        if ($null -ne $row) {
            $records += [ordered]@{
                pid = [int]$row.ProcessId
                parent_pid = [int]$row.ParentProcessId
                name = [string]$row.Name
                path = if ($row.ExecutablePath) { [string]$row.ExecutablePath } else { $null }
            }
        } else {
            $records += [ordered]@{ pid = [int]$id; parent_pid = 0; name = ''; path = $null }
        }
    }
    @($records)
}

function Get-ModuleRecords([int]$RootId, [string]$TargetFinalPath, [string]$TargetSha, [string]$TargetFileId) {
    $modules = @()
    $seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $loadedTarget = $null
    foreach ($id in @(Get-ProcessTreeIds $RootId)) {
        try { $proc = Get-Process -Id $id -ErrorAction Stop } catch { continue }
        try { $items = @($proc.Modules) } catch { $items = @() }
        foreach ($module in $items) {
            if (-not $module.FileName) { continue }
            try { $path = [System.IO.Path]::GetFullPath([string]$module.FileName) } catch { continue }
            if (-not $seen.Add($path)) { continue }
            $item = [ordered]@{ process_id = [int]$id; path = $path }
            try {
                $stream = [System.IO.File]::Open($path, 'Open', 'Read', 'ReadWrite')
                try {
                    $identity = Get-LockedFileIdentity $stream
                    $item.sha256 = ([string]$identity.sha256).ToLowerInvariant()
                    $item.file_id = [string]$identity.file_id
                } finally { $stream.Dispose() }
            } catch { }
            $modules += $item
            if ($TargetFinalPath -and $path -ieq $TargetFinalPath) {
                $loadedTarget = [ordered]@{
                    state = 'module_loaded'
                    process_id = [int]$id
                    path = $path
                    sha256 = $TargetSha
                    file_id = $TargetFileId
                    identity_match = ($item.sha256 -eq $TargetSha -and $item.file_id -eq $TargetFileId)
                }
            }
        }
    }
    [ordered]@{ modules = @($modules); target = $loadedTarget }
}

function Invoke-JsonPreflight([string]$Name, [string]$Arguments, [string]$OutputPath, [string]$TempRoot) {
    $stdoutPath = Join-Path $TempRoot ($Name + '.stdout')
    $stderrPath = Join-Path $TempRoot ($Name + '.stderr')
    $result = [ordered]@{ status = 'unavailable'; exit_code = $null; artifact_path = $OutputPath; stderr = ''; summary = [ordered]@{} }
    $child = $null
    try {
        $psi = [System.Diagnostics.ProcessStartInfo]::new()
        $psi.FileName = $PythonPath
        # PythonPath is the executable; the script is the first argument.
        $psi.Arguments = $Arguments
        $psi.WorkingDirectory = (Split-Path -Parent $PSScriptRoot)
        $psi.UseShellExecute = $false
        $psi.CreateNoWindow = $true
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $child = [System.Diagnostics.Process]::new()
        $child.StartInfo = $psi
        if (-not $child.Start()) { throw "unable to start $Name preflight" }
        $stdout = $child.StandardOutput.ReadToEndAsync()
        $stderr = $child.StandardError.ReadToEndAsync()
        if (-not $child.WaitForExit(60000)) {
            try { $child.Kill() } catch { }
            $result.status = 'failed'
            $result.stderr = 'preflight timed out'
        } else {
            $result.exit_code = [int]$child.ExitCode
            $result.stderr = Limit-Text $stderr.Result
            try {
                $document = $stdout.Result | ConvertFrom-Json
                Write-Utf8Json $OutputPath $document
                if ($Name -eq 'static-pipl') {
                    $result.summary = [ordered]@{
                        report_kind = $document.report_kind
                        schema_version = $document.schema_version
                        pipl_signal_count = $document.summary.pipl_signal_count
                        pipl_resource_entry_count = $document.summary.pipl_resource_entry_count
                    }
                } else {
                    $result.summary = [ordered]@{
                        status = $document.status
                        export_variant = $document.export_variant
                        worker_accepted = $document.verdict.accepted
                    }
                }
                $result.status = if ($child.ExitCode -eq 0) { 'completed' } else { 'failed' }
            } catch {
                $result.status = 'failed'
                $result.stderr = Limit-Text ($result.stderr + ' ' + $_.Exception.Message)
                Write-Utf8Json $OutputPath ([ordered]@{ probe = $Name; status = 'failed'; error = $result.stderr })
            }
        }
    } catch {
        $result.status = 'unavailable'
        $result.stderr = Limit-Text $_.Exception.ToString()
        Write-Utf8Json $OutputPath ([ordered]@{ probe = $Name; status = 'unavailable'; error = $result.stderr })
    } finally {
        if ($null -ne $child) { $child.Dispose() }
    }
    if (-not (Test-Path -LiteralPath $OutputPath)) {
        Write-Utf8Json $OutputPath ([ordered]@{ probe = $Name; status = $result.status; error = $result.stderr })
    }
    $result
}

function Get-WerSnapshot([string]$KeyPath) {
    $values = [ordered]@{}
    if (-not (Test-Path -LiteralPath $KeyPath)) { return [ordered]@{ exists = $false; values = $values } }
    try {
        $props = Get-ItemProperty -LiteralPath $KeyPath -ErrorAction Stop
        foreach ($name in @('DumpFolder', 'DumpType', 'DumpCount')) {
            $property = $props.PSObject.Properties[$name]
            if ($null -ne $property) { $values[$name] = $property.Value }
        }
        return [ordered]@{ exists = $true; values = $values }
    } catch { return [ordered]@{ exists = $false; values = [ordered]@{} } }
}

function Set-WerLocalDump([string]$KeyPath, [string]$Directory, [int]$Count) {
    New-Item -ItemType Directory -Path $Directory -Force | Out-Null
    New-Item -ItemType Directory -Path $KeyPath -Force | Out-Null
    New-ItemProperty -LiteralPath $KeyPath -Name DumpFolder -Value $Directory -PropertyType String -Force | Out-Null
    New-ItemProperty -LiteralPath $KeyPath -Name DumpType -Value 2 -PropertyType DWord -Force | Out-Null
    New-ItemProperty -LiteralPath $KeyPath -Name DumpCount -Value $Count -PropertyType DWord -Force | Out-Null
}

function Restore-WerLocalDump([string]$KeyPath, [object]$Snapshot) {
    if (-not $Snapshot.exists) {
        if (Test-Path -LiteralPath $KeyPath) { Remove-Item -LiteralPath $KeyPath -Recurse -Force -ErrorAction Stop }
        return
    }
    New-Item -ItemType Directory -Path $KeyPath -Force | Out-Null
    foreach ($name in @('DumpFolder', 'DumpType', 'DumpCount')) {
        if ($Snapshot.values.Contains($name)) {
            $value = $Snapshot.values[$name]
            $type = if ($value -is [int] -or $value -is [long]) { 'DWord' } else { 'String' }
            New-ItemProperty -LiteralPath $KeyPath -Name $name -Value $value -PropertyType $type -Force | Out-Null
        } else { Remove-ItemProperty -LiteralPath $KeyPath -Name $name -ErrorAction SilentlyContinue }
    }
}

function Stop-LaunchedTree([int]$ProcessId) {
    try { & taskkill.exe /PID $ProcessId /T /F 2>$null | Out-Null } catch { }
}

function Get-EventEvidence([datetime]$StartTime, [string]$ExecutableName, [string]$OutputPath) {
    $events = @()
    try {
        $events = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = $StartTime } -ErrorAction Stop |
            Where-Object { $_.ProviderName -in @('Application Error', 'Windows Error Reporting', 'Application Hang') -or $_.Message -match [regex]::Escape($ExecutableName) } |
            Select-Object -First 32 | ForEach-Object {
                [ordered]@{ id = [int]$_.Id; provider = [string]$_.ProviderName; time_utc = $_.TimeCreated.ToUniversalTime().ToString('o'); message = Limit-Text $_.Message 2048 }
            })
    } catch { $events = @([ordered]@{ error = Limit-Text $_.Exception.ToString() }) }
    Write-Utf8Json $OutputPath $events
    $joined = ($events | ForEach-Object { $_.message }) -join "`n"
    [ordered]@{
        count = @($events).Count
        faulting_module = if ($joined -match 'Faulting module name:\s*([^,\r\n]+)') { $matches[1].Trim() } else { $null }
        exception_code = if ($joined -match 'Exception code:\s*(0x[0-9a-fA-F]+)') { $matches[1] } else { $null }
        fault_offset = if ($joined -match 'Fault offset:\s*(0x[0-9a-fA-F]+)') { $matches[1] } else { $null }
        report_id = if ($joined -match 'Report Id:\s*([^\r\n]+)') { $matches[1].Trim() } else { $null }
    }
}

function Get-DumpInventory([string]$Directory, [long]$SingleLimit, [long]$TotalLimit, [int]$CountLimit) {
    $removed = 0
    if (-not (Test-Path -LiteralPath $Directory)) { return [ordered]@{ files = @(); removed = 0 } }
    $files = @(Get-ChildItem -LiteralPath $Directory -File -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -ieq '.dmp' -or $_.Extension -ieq '.part' } |
        Sort-Object LastWriteTimeUtc -Descending)
    $kept = @()
    $total = [int64]0
    foreach ($file in $files) {
        $size = [int64]$file.Length
        $reject = $file.Extension -ieq '.part' -or $size -gt $SingleLimit -or $kept.Count -ge $CountLimit -or ($total + $size) -gt $TotalLimit
        if ($reject) {
            try { Remove-Item -LiteralPath $file.FullName -Force -ErrorAction Stop; $removed++ } catch { }
        } else { $kept += $file; $total += $size }
    }
    [ordered]@{ files = @($kept); removed = $removed }
}

$bundleRoot = [System.IO.Path]::GetFullPath($Bundle)
if (Test-Path -LiteralPath $bundleRoot) { throw "Bundle already exists: $bundleRoot" }
New-Item -ItemType Directory -Path $bundleRoot -Force | Out-Null
$preflightRoot = Join-Path $bundleRoot 'preflight'
$telemetryRoot = Join-Path $bundleRoot 'telemetry'
$dumpRoot = if ($DumpDirectory) { [System.IO.Path]::GetFullPath($DumpDirectory) } else { Join-Path $bundleRoot 'dumps' }
New-Item -ItemType Directory -Path $preflightRoot,$telemetryRoot -Force | Out-Null
$tempRoot = Join-Path $bundleRoot '.probe-temp'
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null

$probeId = ([guid]::NewGuid().ToString('N'))
$probeStartUtc = [DateTime]::UtcNow
$afterEffectsPath = Get-SafeFullPath $AfterEffects
$aexPath = Get-SafeFullPath $TestedAex
$aeTarget = Get-IdentityOrPlaceholder $afterEffectsPath
$aexTarget = Get-IdentityOrPlaceholder $aexPath
$aeVersion = ''
$aeBuild = ''
if (Test-Path -LiteralPath $afterEffectsPath -PathType Leaf) {
    try {
        $versionInfo = (Get-Item -LiteralPath $afterEffectsPath).VersionInfo
        $aeVersion = "{0}.{1}.{2}.{3}" -f $versionInfo.FileMajorPart,$versionInfo.FileMinorPart,$versionInfo.FileBuildPart,$versionInfo.FilePrivatePart
        $aeBuild = [string]$versionInfo.FileVersion
    } catch { }
}
if ($EvidenceOrigin -eq 'RealAE' -and [System.IO.Path]::GetFileName($afterEffectsPath) -match '(?i)mock|fixture') {
    throw 'mock/fixture executables require -EvidenceOrigin ControlledFixture; they are never real-AE evidence.'
}

$launchProcess = $null
$launchPid = $null
$launchError = $null
$launchStarted = $false
$startUtc = $null
$deadlineUtc = $null
$endUtc = $null
$timedOut = $false
$dialogObserved = $false
$loadedAexIdentity = $null
$loadedModules = @()
$processTree = @()
$exitCode = $null
$launchClass = 'startup_failed'
$staticResult = $null
$plugindataResult = $null
$eventLogPath = Join-Path $telemetryRoot 'event-log.json'
$wer = [ordered]@{ count = 0; faulting_module = $null; exception_code = $null; fault_offset = $null; report_id = $null }
$registryKey = "HKCU:\Software\Microsoft\Windows\Windows Error Reporting\LocalDumps\$([System.IO.Path]::GetFileName($afterEffectsPath))"
$registrySnapshot = Get-WerSnapshot $registryKey
$registryRollback = if ($EnableMinidump) { 'failed' } else { 'not_requested' }
$dumpSetupReason = $null

try {
    $staticResult = Invoke-JsonPreflight 'static-pipl' ('tools\aex_static_probe.py --input "{0}"' -f $aexPath) (Join-Path $preflightRoot 'static-pipl.json') $tempRoot
    $plugindataResult = Invoke-JsonPreflight 'plugindata' ('tools\aex_plugindata_probe.py --child "{0}"' -f $aexPath) (Join-Path $preflightRoot 'plugindata.json') $tempRoot

    if ($EnableMinidump) {
        try { Set-WerLocalDump $registryKey $dumpRoot $MaxRetainedDumps } catch { $dumpSetupReason = Limit-Text $_.Exception.ToString() }
    }

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $afterEffectsPath
    $psi.Arguments = $AfterEffectsArguments
    $psi.WorkingDirectory = if (Test-Path -LiteralPath (Split-Path -Parent $afterEffectsPath)) { Split-Path -Parent $afterEffectsPath } else { (Get-Location).Path }
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $environment = $psi.EnvironmentVariables
    if ($null -ne $environment) {
        $environment['AEXCOMPAT_PROBE_AEX'] = $aexPath
        $environment['AEXCOMPAT_PROBE_ID'] = $probeId
    }
    $launchProcess = [System.Diagnostics.Process]::new()
    $launchProcess.StartInfo = $psi
    try {
        if (-not $launchProcess.Start()) { throw "unable to start After Effects process" }
        $launchStarted = $true
        $launchPid = [int]$launchProcess.Id
        $startUtc = [DateTime]::UtcNow
        $deadlineUtc = $startUtc.AddSeconds($TimeoutSeconds)
        while ([DateTime]::UtcNow -lt $deadlineUtc) {
            if ($launchProcess.HasExited) { break }
            try {
                $processTree = @(Get-ProcessTree $launchPid)
                $moduleScan = Get-ModuleRecords $launchPid ([string]$aexTarget.path) ([string]$aexTarget.sha256).ToLowerInvariant() ([string]$aexTarget.file_id)
                $loadedModules = @($moduleScan.modules)
                if ($null -ne $moduleScan.target) { $loadedAexIdentity = $moduleScan.target }
                foreach ($id in @(Get-ProcessTreeIds $launchPid)) {
                    try { $proc = Get-Process -Id $id -ErrorAction Stop; if ($proc.MainWindowTitle) { $dialogObserved = $true } } catch { }
                }
            } catch { }
            Start-Sleep -Milliseconds 250
        }
        if (-not $launchProcess.HasExited) {
            $timedOut = $true
            Stop-LaunchedTree $launchPid
            try { $launchProcess.WaitForExit(30000) | Out-Null } catch { }
        }
        if ($launchProcess.HasExited) { $exitCode = [int]$launchProcess.ExitCode }
        $endUtc = [DateTime]::UtcNow
        try { $processTree = @(Get-ProcessTree $launchPid) } catch { }
        try {
            $moduleScan = Get-ModuleRecords $launchPid ([string]$aexTarget.path) ([string]$aexTarget.sha256).ToLowerInvariant() ([string]$aexTarget.file_id)
            $loadedModules = @($moduleScan.modules)
            if ($null -ne $moduleScan.target) { $loadedAexIdentity = $moduleScan.target }
        } catch { }
    } catch {
        $launchError = Limit-Text ($_.Exception.ToString() + ' ' + $_.InvocationInfo.PositionMessage)
        $launchClass = 'startup_failed'
        $endUtc = [DateTime]::UtcNow
    }
} catch {
    $launchError = Limit-Text ($_.Exception.ToString() + ' ' + $_.InvocationInfo.PositionMessage)
    $launchClass = 'startup_failed'
    $endUtc = [DateTime]::UtcNow
} finally {
    if ($null -ne $launchProcess) { $launchProcess.Dispose() }
    if ($EnableMinidump) {
        try { Restore-WerLocalDump $registryKey $registrySnapshot; $registryRollback = 'restored' } catch { $registryRollback = 'failed' }
    }
}

# Event evidence is unconditional: a failed Start(), preflight exception, or
# unsupported Event Log provider must still leave a hashable artifact behind.
if (-not (Test-Path -LiteralPath $eventLogPath)) {
    $eventStartUtc = $probeStartUtc
    if ($startUtc) { $eventStartUtc = $startUtc }
    $wer = Get-EventEvidence $eventStartUtc ([System.IO.Path]::GetFileName($afterEffectsPath)) $eventLogPath
}

$preflightFailed = ($null -eq $staticResult -or $staticResult.status -ne 'completed' -or $null -eq $plugindataResult -or $plugindataResult.status -ne 'completed')
if ($launchStarted) {
    if ($timedOut) { $launchClass = if ($dialogObserved) { 'dialog_or_lingering_process' } else { 'timeout_killed' } }
    elseif ($null -eq $exitCode) { $launchClass = 'startup_failed' }
    elseif ($exitCode -eq 0 -and $null -ne $loadedAexIdentity -and $loadedAexIdentity.identity_match) { $launchClass = 'ok' }
    elseif ($exitCode -eq 0) { $launchClass = 'plugin_not_loaded' }
    elseif ($null -ne $loadedAexIdentity -and $wer.faulting_module -and $wer.faulting_module -ieq [System.IO.Path]::GetFileName($aexPath)) { $launchClass = 'plugin_load_crash' }
    elseif ($null -ne $loadedAexIdentity) { $launchClass = 'host_crash_after_plugin_load' }
    else { $launchClass = 'startup_failed' }
}

$inventory = Get-DumpInventory $dumpRoot $MaxDumpBytes $MaxTotalDumpBytes $MaxRetainedDumps
$dumpFiles = @($inventory.files | Where-Object { $_.Extension -ieq '.dmp' })
$dumpState = if (-not $EnableMinidump) { 'disabled' } elseif ($dumpFiles.Count -gt 0) { 'captured' } else { 'unavailable' }
$dumpReason = if ($dumpState -eq 'unavailable') { if ($dumpSetupReason) { $dumpSetupReason } else { 'no_dump_observed' } } else { $null }

$cleanup = [ordered]@{
    staging_removed = $false
    registry_rollback = $registryRollback
    registry_originally_present = [bool]$registrySnapshot.exists
    removed_dump_count = [int]$inventory.removed
    retained_dump_count = [int]$dumpFiles.Count
}
Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
$cleanup.staging_removed = -not (Test-Path -LiteralPath $tempRoot)
Write-Utf8Json (Join-Path $bundleRoot 'cleanup.json') $cleanup
Write-Utf8Json (Join-Path $bundleRoot 'process-tree.json') ([ordered]@{ root_pid = $launchPid; processes = @($processTree) })

$staticArtifact = Get-BundleArtifact $bundleRoot (Join-Path $preflightRoot 'static-pipl.json')
$plugindataArtifact = Get-BundleArtifact $bundleRoot (Join-Path $preflightRoot 'plugindata.json')
$eventArtifact = Get-BundleArtifact $bundleRoot $eventLogPath
$dumpArtifacts = @()
foreach ($file in $dumpFiles) {
    try {
        $dumpArtifacts += Get-BundleArtifact $bundleRoot $file.FullName ('dumps/' + $file.Name)
    } catch { }
}
$artifactRecords = @()
foreach ($file in @(Get-ChildItem -LiteralPath $bundleRoot -File -Recurse -ErrorAction SilentlyContinue)) {
    if ($file.Name -ne 'manifest.json') { $artifactRecords += Get-BundleArtifact $bundleRoot $file.FullName }
}

$failureClasses = [System.Collections.Generic.List[string]]::new()
if ($launchClass -eq 'ok') { [void]$failureClasses.Add('ok') } else { [void]$failureClasses.Add($launchClass) }
if ($preflightFailed) { [void]$failureClasses.Add('preflight_failed') }
if ($dumpState -eq 'unavailable') { [void]$failureClasses.Add('dump_unavailable') }
$failureClasses = @($failureClasses | Select-Object -Unique)
$status = if (-not $launchStarted -and $launchError) { 'failed' } elseif ($failureClasses.Count -eq 1 -and $failureClasses[0] -eq 'ok') { 'completed' } else { 'completed_with_failures' }
$telemetry = [ordered]@{
    wer_event_count = [int]$wer.count
    events_artifact = $eventArtifact
    faulting_module = $wer.faulting_module
    exception_code = $wer.exception_code
    fault_offset = $wer.fault_offset
    report_id = $wer.report_id
}
$manifest = [ordered]@{
    schema_version = 1
    kind = 'aexcompat.ae-startup-crash-probe'
    probe_id = $probeId
    evidence_origin = if ($EvidenceOrigin -eq 'ControlledFixture') { 'controlled_fixture' } else { 'real_ae' }
    status = $status
    failure_classes = $failureClasses
    target = [ordered]@{ after_effects = $aeTarget; aex = $aexTarget; ae_version = $aeVersion; ae_build = $aeBuild }
    preflight = [ordered]@{
        static_pipl = [ordered]@{ status = if ($staticResult) { $staticResult.status } else { 'unavailable' }; exit_code = if ($staticResult) { $staticResult.exit_code } else { $null }; artifact = $staticArtifact; stderr = if ($staticResult) { Limit-Text $staticResult.stderr } else { 'preflight did not run' }; summary = if ($staticResult) { $staticResult.summary } else { [ordered]@{} } }
        plugindata = [ordered]@{ status = if ($plugindataResult) { $plugindataResult.status } else { 'unavailable' }; exit_code = if ($plugindataResult) { $plugindataResult.exit_code } else { $null }; artifact = $plugindataArtifact; stderr = if ($plugindataResult) { Limit-Text $plugindataResult.stderr } else { 'preflight did not run' }; summary = if ($plugindataResult) { $plugindataResult.summary } else { [ordered]@{} } }
        linked_to_launch = $true
    }
    launch = [ordered]@{
        started = $launchStarted
        pid = $launchPid
        start_error = $launchError
        start_time_utc = if ($startUtc) { $startUtc.ToString('o') } else { $null }
        deadline_utc = if ($deadlineUtc) { $deadlineUtc.ToString('o') } else { $null }
        end_time_utc = if ($endUtc) { $endUtc.ToString('o') } else { $null }
        exit_code = $exitCode
        process_tree = @($processTree)
        loaded_modules = @($loadedModules)
        loaded_aex_identity = $loadedAexIdentity
        classification = $launchClass
    }
    telemetry = $telemetry
    dump = [ordered]@{ enabled = [bool]$EnableMinidump; state = $dumpState; reason = $dumpReason; artifacts = @($dumpArtifacts); max_single_bytes = $MaxDumpBytes; max_total_bytes = $MaxTotalDumpBytes; max_retained = $MaxRetainedDumps }
    cleanup = $cleanup
    artifacts = @($artifactRecords)
    limitations = [ordered]@{
        real_ae_executed = ($EvidenceOrigin -eq 'RealAE' -and $launchStarted)
        mock_output_is_real_ae_evidence = $false
        resume_conditions = @('Run on a Windows host with the intended After Effects executable and AEX.', 'For real-AE evidence, review WER/Event Log permissions and opt-in dump policy.', 'Controlled-fixture output is not a substitute for a real-AE run.')
    }
}
Write-Utf8Json (Join-Path $bundleRoot 'manifest.json') $manifest
Write-Output (Join-Path $bundleRoot 'manifest.json')
