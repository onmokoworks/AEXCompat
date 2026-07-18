param(
    [Parameter(Mandatory = $true)][string]$AfterEffects,
    [Parameter(Mandatory = $true)][string]$TestedAex,
    [Parameter(Mandatory = $true)][string]$InstalledAex,
    [Parameter(Mandatory = $true)][string]$InputImage,
    [Parameter(Mandatory = $true)][string]$OutputPng,
    [Parameter(Mandatory = $true)][string]$EffectName,
    [string]$ScriptPath,
    [string]$ParamName = '',
    [string]$ParamValue = '',
    [ValidateRange(0, 10000000)][int]$Frame = 0,
    [ValidateRange(1, 1000)][int]$Fps = 30,
    [ValidateRange(1, 10000001)][int]$DurationFrames = 300,
    [ValidateSet(8, 16, 32)][int]$Bpc = 8,
    [string]$WorkingSpace = '',
    [ValidateSet('', '0', '1')][string]$LinearizeWorkingSpace = '',
    [switch]$NoEffect,
    [switch]$RequireLoadedAexIdentity,
    [ValidateRange(5, 600)][int]$TimeoutSeconds = 120
)

. (Join-Path $PSScriptRoot 'sha256.ps1')
. (Join-Path $PSScriptRoot 'windows-file-identity.ps1')

$ErrorActionPreference = 'Stop'
# 'AfterFX.com' is the console shim's own process name; a lingering shim
# (e.g. orphaned by an interrupted capture) would otherwise pass this gate.
if (Get-Process AfterFX,'AfterFX.com',aerender,aerendercore -ErrorAction SilentlyContinue) {
    throw 'After Effects is already running; refusing to touch an existing user session.'
}
if ($DurationFrames -le $Frame) {
    throw 'DurationFrames must be greater than Frame.'
}
if ($ParamName -and -not ($ParamValue -match '^-?\d+(\.\d+)?$')) {
    throw 'ParamValue must be a number when ParamName is set.'
}
if ($ParamValue -and -not $ParamName) {
    throw 'ParamName is required when ParamValue is set.'
}
if ($ParamName -and $NoEffect) {
    throw 'ParamName cannot be combined with NoEffect; there is no effect to set the parameter on.'
}

$afterEffectsPath = (Resolve-Path -LiteralPath $AfterEffects).Path
# AfterFX.exe launches, loads plug-ins, and exits with code 0 WITHOUT executing
# the -r script (observed on AE 25.3). Only AfterFX.com runs it, so refuse the
# silent no-op instead of timing out later.
if ([System.IO.Path]::GetFileName($afterEffectsPath) -ieq 'AfterFX.exe') {
    $comPath = Join-Path (Split-Path -Parent $afterEffectsPath) 'AfterFX.com'
    if (-not (Test-Path -LiteralPath $comPath)) {
        throw 'AfterFX.exe does not execute -r scripts; AfterFX.com is required but was not found next to it.'
    }
    Write-Warning 'AfterFX.exe does not execute -r scripts; using AfterFX.com instead.'
    $afterEffectsPath = $comPath
}
$testedPath = (Resolve-Path -LiteralPath $TestedAex).Path
$installedPath = (Resolve-Path -LiteralPath $InstalledAex).Path
$inputPath = (Resolve-Path -LiteralPath $InputImage).Path
$outputPath = [System.IO.Path]::GetFullPath($OutputPng)
if (Test-Path -LiteralPath $outputPath) {
    throw 'OutputPng already exists.'
}
$outputParent = Split-Path -Parent $outputPath
if (-not (Test-Path -LiteralPath $outputParent -PathType Container)) {
    throw 'OutputPng parent directory does not exist.'
}

$testedHash = Get-Sha256Hex $testedPath
$installedHash = Get-Sha256Hex $installedPath
if ($testedHash -ne $installedHash) {
    throw "Installed AEX hash does not match tested AEX: $installedHash != $testedHash"
}
$scriptPath = if ($ScriptPath) {
    (Resolve-Path -LiteralPath $ScriptPath).Path
} else {
    Join-Path $PSScriptRoot 'ae-reference-capture.jsx'
}
$resultPath = [System.IO.Path]::ChangeExtension($outputPath, '.result.json')
if (Test-Path -LiteralPath $resultPath) {
    throw 'Reference result file already exists.'
}

# Stage a private copy of the input and hash that copy: hashing the original
# would leave a window (before AE imports it, or after the run) in which a
# rewritten file makes the recorded identity diverge from the bytes AE
# actually rendered. AE is pointed at the staged copy, so the hash and the
# rendered bytes are the same file by construction. This is the last
# preflight step, so every refusal above leaves nothing behind in the temp
# directory and the cleanup block below removes the copy on every later path.
$stagedInput = Join-Path ([System.IO.Path]::GetTempPath()) `
    ("aexcompat-ae-input-" + [guid]::NewGuid().ToString('N') + [System.IO.Path]::GetExtension($inputPath))
Copy-Item -LiteralPath $inputPath -Destination $stagedInput
$inputHash = Get-Sha256Hex $stagedInput

$env:AEXCOMPAT_AE_INPUT = $stagedInput
$env:AEXCOMPAT_AE_OUTPUT = $outputPath
$env:AEXCOMPAT_AE_RESULT = $resultPath
$env:AEXCOMPAT_AE_EFFECT = $EffectName
$env:AEXCOMPAT_AE_FRAME = [string]$Frame
$env:AEXCOMPAT_AE_FPS = [string]$Fps
$env:AEXCOMPAT_AE_DURATION = [string]$DurationFrames
$env:AEXCOMPAT_AE_BPC = [string]$Bpc
$env:AEXCOMPAT_AE_NO_EFFECT = if ($NoEffect) { '1' } else { '0' }
# Unset means "leave the fresh project's defaults untouched"; the JSX records
# the observed color state either way and fails closed when a pin does not
# apply verbatim. Clear any inherited values first so an ambient environment
# can never pin the color pipeline when the parameters were omitted.
Remove-Item Env:AEXCOMPAT_AE_WORKING_SPACE -ErrorAction SilentlyContinue
Remove-Item Env:AEXCOMPAT_AE_LINEARIZE -ErrorAction SilentlyContinue
if ($WorkingSpace) { $env:AEXCOMPAT_AE_WORKING_SPACE = $WorkingSpace }
if ($LinearizeWorkingSpace) { $env:AEXCOMPAT_AE_LINEARIZE = $LinearizeWorkingSpace }
# The JSX polls for the asynchronous saveFrameToPng output; keep its bound
# inside the outer watchdog so the wait can never outlive this script.
$env:AEXCOMPAT_AE_SAVE_TIMEOUT_MS = [string]($TimeoutSeconds * 1000)
if ($ParamName) {
    $env:AEXCOMPAT_AE_PARAM_NAME = $ParamName
    $env:AEXCOMPAT_AE_PARAM_VALUE = $ParamValue
} else {
    # The JSX reads these directly; clear inherited values so a default
    # capture cannot pick up a stale override from the calling environment.
    Remove-Item 'Env:AEXCOMPAT_AE_PARAM_NAME','Env:AEXCOMPAT_AE_PARAM_VALUE' -ErrorAction SilentlyContinue
}
# Lock immediately before launch, after every other preflight/staging step.
# Denying write/delete sharing binds the hash to the file AE can map and
# prevents replacement until the launched process has finished or is killed.
$installedLock = [System.IO.File]::Open(
    $installedPath, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read,
    [System.IO.FileShare]::Read)
try {
$lockedIdentity = Get-LockedFileIdentity $installedLock
$lockedInstalledHash = $lockedIdentity.sha256
} catch {
    $installedLock.Dispose()
    throw
}
$installedLock.Position = 0
if ($lockedInstalledHash -ne $testedHash) {
    $installedLock.Dispose()
    throw "Installed AEX changed before launch: $lockedInstalledHash != $testedHash"
}
$installedHash = $lockedInstalledHash
$lockedFinalPath = $lockedIdentity.final_path
$lockedFileId = $lockedIdentity.file_id
$process = $null
$captureFailed = $true
try {
    $escapedScriptPath = $scriptPath.Replace('"', '\"')
    # AE 25.2 can abort before JSX execution when -noui hits a failed GPU3
    # sanity state; there the UI launch still runs the script and the JSX
    # quits AE. On AE 25.3.1 `-noui` was measured to execute the JSX and exit
    # cleanly (issue #54, 2026-07-19), so 25.3+ runs headless and older
    # versions keep the UI fallback.
    $versionSource = Join-Path (Split-Path -Parent $afterEffectsPath) 'AfterFX.exe'
    if (-not (Test-Path -LiteralPath $versionSource)) {
        $versionSource = $afterEffectsPath
    }
    # Build the version from the numeric File*Part fields: the FileVersion
    # string can carry trailing text (e.g. "10.0.26100.1 (WinBuild...)") that
    # [Version] cannot parse, and the parts do not depend on the PowerShell
    # ETS-provided FileVersionRaw property. A file without version info
    # yields 0.0.0.0, i.e. the UI fallback.
    $versionInfo = (Get-Item -LiteralPath $versionSource).VersionInfo
    $aeFileVersion = [Version]::new($versionInfo.FileMajorPart, $versionInfo.FileMinorPart,
        $versionInfo.FileBuildPart, $versionInfo.FilePrivatePart)
    $arguments = if ($aeFileVersion -ge [Version]'25.3') {
        '-m -noui -r "{0}"' -f $escapedScriptPath
    } else {
        '-m -r "{0}"' -f $escapedScriptPath
    }
    $process = Start-Process -FilePath $afterEffectsPath -ArgumentList $arguments -PassThru
    $loadedAexIdentity = $null
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline -and -not (Test-Path -LiteralPath $resultPath)) {
        # Bind the requested plug-in to the module that this exact AE process
        # mapped. ExtendScript exposes matchName but not the backing module.
        # Process.Modules supplies the missing host-side observation.
        if ($null -eq $loadedAexIdentity -and -not $process.HasExited) {
            try {
                $process.Refresh()
                $loadedModule = @($process.Modules) | Where-Object {
                    $_.FileName -and
                    ([System.IO.Path]::GetFullPath($_.FileName) -ieq $lockedFinalPath)
                } | Select-Object -First 1
                if ($null -ne $loadedModule) {
                    $loadedAexIdentity = [ordered]@{
                        state = 'verified'
                        sha256 = $installedHash.ToLowerInvariant()
                        file_name = [System.IO.Path]::GetFileName($loadedModule.FileName)
                        canonical_path_sha256 = $lockedIdentity.canonical_path_sha256
                        file_id = $lockedFileId
                        process_id = $process.Id
                        replacement_locked = $true
                    }
                }
            } catch {
                if ($RequireLoadedAexIdentity) {
                    throw "Unable to inspect the launched AE process module identity: $($_.Exception.Message)"
                }
            }
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not (Test-Path -LiteralPath $resultPath)) {
        # Kill only this launch's process tree, addressed by the PID from
        # Start-Process: /T reaches a GUI-mode child AfterFX or aerendercore,
        # while an unrelated AE session started after the startup gate is
        # never touched (a name-based kill could hit it), and the shim
        # itself is the tree root, so no lingering shim can block the gate.
        # If the launch already exited on its own there is nothing to kill
        # and the numeric PID may have been reused - never taskkill it then.
        if (-not $process.HasExited) {
            & taskkill.exe /PID $process.Id /T /F 2>$null | Out-Null
        }
        throw 'After Effects reference capture timed out without a result.'
    }
    if ($RequireLoadedAexIdentity -and $null -eq $loadedAexIdentity) {
        # JSX may already have written status=captured. Rewrite it before
        # throwing so no later consumer can mistake an unbound artifact for
        # verified oracle evidence.
        $unverifiedResult = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
        $unverifiedResult.status = 'identity_unverified'
        $unverifiedResult | Add-Member -Force -NotePropertyName 'loaded_aex_identity' `
            -NotePropertyValue ([ordered]@{
                state = 'unverified'; reason = 'loaded_module_not_observed'
            })
        $unverifiedResult | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $resultPath -Encoding utf8
        throw 'AE capture completed without verified loaded AEX module identity.'
    }
    # Wait only on this launch identity; never kill by process name. Check
    # the original handle before taskkill so a reused numeric PID is safe.
    if (-not $process.WaitForExit($TimeoutSeconds * 1000) -and -not $process.HasExited) {
        & taskkill.exe /PID $process.Id /T /F 2>$null | Out-Null
        throw 'After Effects did not exit after writing the capture result.'
    }
    $captureFailed = $false
} finally {
    if ($captureFailed -and $null -ne $process -and -not $process.HasExited) {
        & taskkill.exe /PID $process.Id /T /F 2>$null | Out-Null
        try { $process.WaitForExit(30000) | Out-Null } catch {}
    }
    'AEXCOMPAT_AE_INPUT','AEXCOMPAT_AE_OUTPUT','AEXCOMPAT_AE_RESULT','AEXCOMPAT_AE_EFFECT',
    'AEXCOMPAT_AE_FRAME','AEXCOMPAT_AE_FPS','AEXCOMPAT_AE_DURATION','AEXCOMPAT_AE_BPC',
    'AEXCOMPAT_AE_SAVE_TIMEOUT_MS','AEXCOMPAT_AE_NO_EFFECT',
    'AEXCOMPAT_AE_WORKING_SPACE','AEXCOMPAT_AE_LINEARIZE',
    'AEXCOMPAT_AE_PARAM_NAME','AEXCOMPAT_AE_PARAM_VALUE' |
        ForEach-Object { Remove-Item "Env:$_" -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $stagedInput -ErrorAction SilentlyContinue
    $installedLock.Dispose()
}

$result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
if ($result.status -ne 'captured') {
    throw "After Effects reference capture failed: $($result.error)"
}
if (-not (Test-Path -LiteralPath $outputPath)) {
    throw 'After Effects reported capture success without creating the PNG.'
}
# Bind the capture evidence to its verified inputs: both hashes were taken
# before After Effects launched, so record those identities in the result
# document (the cross-machine runbook requires the input hash in the returned
# manifest, and evidence refresh scripts verify against it).
$result | Add-Member -NotePropertyName 'input_sha256' -NotePropertyValue $inputHash
$result | Add-Member -NotePropertyName 'tested_aex_sha256' -NotePropertyValue $testedHash.ToLowerInvariant()
$identityRecord = if ($null -ne $loadedAexIdentity) {
    $loadedAexIdentity
} else {
    [ordered]@{ state = 'unverified'; reason = 'loaded_module_not_observed' }
}
$result | Add-Member -NotePropertyName 'loaded_aex_identity' -NotePropertyValue $identityRecord
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $resultPath -Encoding utf8
$result | ConvertTo-Json -Depth 8
