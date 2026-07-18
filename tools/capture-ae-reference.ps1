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
    [ValidateRange(5, 600)][int]$TimeoutSeconds = 120
)

. (Join-Path $PSScriptRoot 'sha256.ps1')

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
try {
    $escapedScriptPath = $scriptPath.Replace('"', '\"')
    # AE 25.2 can abort before JSX execution when -noui hits a failed GPU3
    # sanity state. UI launch still runs the script and the JSX quits AE.
    $arguments = '-m -r "{0}"' -f $escapedScriptPath
    $process = Start-Process -FilePath $afterEffectsPath -ArgumentList $arguments -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline -and -not (Test-Path -LiteralPath $resultPath)) {
        Start-Sleep -Milliseconds 250
    }
    if (-not (Test-Path -LiteralPath $resultPath)) {
        Get-Process AfterFX,aerendercore -ErrorAction SilentlyContinue |
            Stop-Process -Force -ErrorAction SilentlyContinue
        throw 'After Effects reference capture timed out without a result.'
    }
} finally {
    'AEXCOMPAT_AE_INPUT','AEXCOMPAT_AE_OUTPUT','AEXCOMPAT_AE_RESULT','AEXCOMPAT_AE_EFFECT',
    'AEXCOMPAT_AE_FRAME','AEXCOMPAT_AE_FPS','AEXCOMPAT_AE_DURATION','AEXCOMPAT_AE_BPC',
    'AEXCOMPAT_AE_SAVE_TIMEOUT_MS','AEXCOMPAT_AE_NO_EFFECT',
    'AEXCOMPAT_AE_WORKING_SPACE','AEXCOMPAT_AE_LINEARIZE',
    'AEXCOMPAT_AE_PARAM_NAME','AEXCOMPAT_AE_PARAM_VALUE' |
        ForEach-Object { Remove-Item "Env:$_" -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $stagedInput -ErrorAction SilentlyContinue
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
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $resultPath -Encoding utf8
$result | ConvertTo-Json -Depth 8
