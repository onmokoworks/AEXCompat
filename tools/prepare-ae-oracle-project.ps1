param(
    [Parameter(Mandatory = $true)][string]$AfterEffects,
    [Parameter(Mandatory = $true)][string]$InputImage,
    [Parameter(Mandatory = $true)][string]$OutputAep,
    [Parameter(Mandatory = $true)][string]$OutputExr,
    [Parameter(Mandatory = $true)][string]$ResultJson,
    [Parameter(Mandatory = $true)][string]$EffectName,
    [ValidateSet(8, 16, 32)][int]$Bpc = 32,
    [ValidateRange(1, 1000)][int]$Fps = 30,
    [ValidateRange(1, 10000000)][int]$DurationFrames = 1,
    [ValidateRange(5, 600)][int]$TimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
if (Get-Process AfterFX,aerender,aerendercore -ErrorAction SilentlyContinue) {
    throw 'After Effects rendering is active; refusing to touch an existing user session.'
}

$afterEffectsPath = (Resolve-Path -LiteralPath $AfterEffects).Path
$inputPath = (Resolve-Path -LiteralPath $InputImage).Path
if ([System.IO.Path]::GetFileName($afterEffectsPath) -ine 'AfterFX.exe') {
    throw 'AfterEffects must point to AfterFX.exe.'
}
$scriptHostPath = Join-Path (Split-Path -Parent $afterEffectsPath) 'AfterFX.com'
if (-not (Test-Path -LiteralPath $scriptHostPath -PathType Leaf)) {
    throw 'AfterFX.com is required next to AfterFX.exe to execute the oracle setup script.'
}

$projectPath = [System.IO.Path]::GetFullPath($OutputAep)
$outputPath = [System.IO.Path]::GetFullPath($OutputExr)
$resultPath = [System.IO.Path]::GetFullPath($ResultJson)
foreach ($path in @($projectPath, $outputPath, $resultPath)) {
    if (Test-Path -LiteralPath $path) {
        throw "Refusing to overwrite existing output: $path"
    }
    $parent = Split-Path -Parent $path
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        throw "Output parent directory does not exist: $parent"
    }
}
if ([System.IO.Path]::GetExtension($projectPath) -ine '.aep' -or
    [System.IO.Path]::GetExtension($outputPath) -ine '.exr' -or
    [System.IO.Path]::GetExtension($resultPath) -ine '.json') {
    throw 'OutputAep, OutputExr, and ResultJson must use .aep, .exr, and .json extensions.'
}

$scriptPath = Join-Path $PSScriptRoot 'ae-oracle-project.jsx'
$environmentNames = @(
    'AEXCOMPAT_AE_ORACLE_INPUT', 'AEXCOMPAT_AE_ORACLE_PROJECT',
    'AEXCOMPAT_AE_ORACLE_OUTPUT', 'AEXCOMPAT_AE_ORACLE_RESULT',
    'AEXCOMPAT_AE_ORACLE_EFFECT', 'AEXCOMPAT_AE_ORACLE_BPC',
    'AEXCOMPAT_AE_ORACLE_FPS', 'AEXCOMPAT_AE_ORACLE_DURATION'
)
$env:AEXCOMPAT_AE_ORACLE_INPUT = $inputPath
$env:AEXCOMPAT_AE_ORACLE_PROJECT = $projectPath
$env:AEXCOMPAT_AE_ORACLE_OUTPUT = $outputPath
$env:AEXCOMPAT_AE_ORACLE_RESULT = $resultPath
$env:AEXCOMPAT_AE_ORACLE_EFFECT = $EffectName
$env:AEXCOMPAT_AE_ORACLE_BPC = [string]$Bpc
$env:AEXCOMPAT_AE_ORACLE_FPS = [string]$Fps
$env:AEXCOMPAT_AE_ORACLE_DURATION = [string]$DurationFrames
try {
    # Direct invocation is required on AE 25.2; Start-Process can return zero
    # without dispatching the JSX even with identical arguments.
    & $scriptHostPath -m -r $scriptPath
    $processExitCode = $LASTEXITCODE
} finally {
    $environmentNames | ForEach-Object { Remove-Item "Env:$_" -ErrorAction SilentlyContinue }
}

if (-not (Test-Path -LiteralPath $resultPath) -or -not (Test-Path -LiteralPath $projectPath)) {
    $detail = if (Test-Path -LiteralPath $resultPath) {
        Get-Content -LiteralPath $resultPath -Raw
    } else { 'result missing' }
    throw "After Effects did not create the oracle project and result (exit code $processExitCode): $detail"
}
$result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
if ($result.status -ne 'prepared') {
    throw "After Effects oracle project preparation failed: $($result.error)"
}
if ([System.IO.Path]::GetFullPath([string]$result.project) -ne $projectPath -or
    [System.IO.Path]::GetFullPath([string]$result.output) -ne $outputPath -or
    [int]$result.bpc -ne $Bpc) {
    throw 'After Effects oracle result does not match the requested paths or pixel depth.'
}
if (-not [bool]$result.full_float_settings_applied) {
    throw 'After Effects did not confirm full-float straight-alpha OpenEXR settings.'
}
$result | ConvertTo-Json -Depth 8
