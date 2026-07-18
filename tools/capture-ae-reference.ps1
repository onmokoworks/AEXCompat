param(
    [Parameter(Mandatory = $true)][string]$AfterEffects,
    [Parameter(Mandatory = $true)][string]$TestedAex,
    [Parameter(Mandatory = $true)][string]$InstalledAex,
    [Parameter(Mandatory = $true)][string]$InputImage,
    [Parameter(Mandatory = $true)][string]$OutputPng,
    [Parameter(Mandatory = $true)][string]$EffectName,
    [string]$ScriptPath,
    [ValidateRange(0, 10000000)][int]$Frame = 0,
    [ValidateRange(1, 1000)][int]$Fps = 30,
    [ValidateRange(1, 10000001)][int]$DurationFrames = 300,
    [ValidateSet(8, 16, 32)][int]$Bpc = 8,
    [string]$WorkingSpace = '',
    [ValidateSet('', '0', '1')][string]$LinearizeWorkingSpace = '',
    [switch]$NoEffect,
    [ValidateRange(5, 600)][int]$TimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
if (Get-Process AfterFX,aerender,aerendercore -ErrorAction SilentlyContinue) {
    throw 'After Effects is already running; refusing to touch an existing user session.'
}
if ($DurationFrames -le $Frame) {
    throw 'DurationFrames must be greater than Frame.'
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

$testedHash = (Get-FileHash -LiteralPath $testedPath -Algorithm SHA256).Hash
$installedHash = (Get-FileHash -LiteralPath $installedPath -Algorithm SHA256).Hash
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

$env:AEXCOMPAT_AE_INPUT = $inputPath
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
try {
    $escapedScriptPath = $scriptPath.Replace('"', '\"')
    $arguments = '-m -noui -r "{0}"' -f $escapedScriptPath
    $process = Start-Process -FilePath $afterEffectsPath -ArgumentList $arguments -PassThru -WindowStyle Hidden
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
    'AEXCOMPAT_AE_WORKING_SPACE','AEXCOMPAT_AE_LINEARIZE' |
        ForEach-Object { Remove-Item "Env:$_" -ErrorAction SilentlyContinue }
}

$result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
if ($result.status -ne 'captured') {
    throw "After Effects reference capture failed: $($result.error)"
}
if (-not (Test-Path -LiteralPath $outputPath)) {
    throw 'After Effects reported capture success without creating the PNG.'
}
$result | ConvertTo-Json -Depth 8
