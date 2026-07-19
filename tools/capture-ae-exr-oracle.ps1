param(
    [Parameter(Mandatory = $true)][string]$AfterEffects,
    [Parameter(Mandatory = $true)][string]$TestedAex,
    [Parameter(Mandatory = $true)][string]$InstalledAex,
    [Parameter(Mandatory = $true)][string]$InputImage,
    [Parameter(Mandatory = $true)][string]$ExpectedRaw,
    [Parameter(Mandatory = $true)][string]$EffectName,
    [Parameter(Mandatory = $true)][string]$OutputRoot,
    [Parameter(Mandatory = $true)][int]$Width,
    [Parameter(Mandatory = $true)][int]$Height,
    [double]$Tolerance = 0,
    [string]$PlanPath,
    [switch]$PlanOnly,
    [ValidateRange(5, 600)][int]$TimeoutSeconds = 180
)

. (Join-Path $PSScriptRoot 'sha256.ps1')

$ErrorActionPreference = 'Stop'

# Repo の dev 依存 (Pillow / OpenEXR) は uv 管理の .venv にあるため、Python
# ツールは uv run 経由で起動する (CWD に依存しないよう --project で固定)。
$uvProject = Split-Path -Parent $PSScriptRoot
if ($Width -le 0 -or $Height -le 0) { throw 'Width and Height must be positive.' }

$afterEffectsPath = (Resolve-Path -LiteralPath $AfterEffects).Path
$testedPath = (Resolve-Path -LiteralPath $TestedAex).Path
$installedPath = (Resolve-Path -LiteralPath $InstalledAex).Path
$inputPath = (Resolve-Path -LiteralPath $InputImage).Path
$rawPath = (Resolve-Path -LiteralPath $ExpectedRaw).Path
$testedHash = Get-Sha256Hex $testedPath
$installedHash = Get-Sha256Hex $installedPath
if ($testedHash -ne $installedHash) { throw 'Installed AEX hash does not match tested AEX.' }

$root = [System.IO.Path]::GetFullPath($OutputRoot)
$project = Join-Path $root 'oracle.aep'
$exr = Join-Path $root 'ae-reference.exr'
$setup = Join-Path $root 'ae-setup.json'
$comparison = Join-Path $root 'comparison.json'
$evidence = Join-Path $root 'evidence.json'
$aerender = Join-Path (Split-Path -Parent $afterEffectsPath) 'aerender.exe'
if (-not (Test-Path -LiteralPath $aerender -PathType Leaf)) {
    throw 'aerender.exe was not found next to AfterFX.exe.'
}

$plan = [ordered]@{
    schema = 'aexcompat-ae-exr-oracle-plan-v1'
    side_effects_performed = $false
    pixel_depth = 32
    comparison_boundary = 'host_raw_world_vs_ae_float_export'
    fixture = [ordered]@{ tested = $testedPath; installed = $installedPath; sha256 = $testedHash }
    input = [ordered]@{ path = $inputPath; sha256 = Get-Sha256Hex $inputPath }
    expected_raw = [ordered]@{ path = $rawPath; format = 'rgba32f-le'; width = $Width; height = $Height; sha256 = Get-Sha256Hex $rawPath }
    outputs = [ordered]@{ root = $root; project = $project; exr = $exr; setup = $setup; comparison = $comparison; evidence = $evidence }
}
if ($PlanOnly) {
    if (-not $PlanPath) { throw 'PlanPath is required with PlanOnly.' }
    $planFullPath = [System.IO.Path]::GetFullPath($PlanPath)
    if (Test-Path -LiteralPath $planFullPath) { throw "Refusing to overwrite plan: $planFullPath" }
    $plan | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $planFullPath -Encoding utf8
    $plan | ConvertTo-Json -Depth 8
    return
}

if (Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue) {
    throw 'After Effects rendering is active; refusing to start an oracle run.'
}
if (Test-Path -LiteralPath $root) { throw "Refusing to overwrite output root: $root" }
New-Item -ItemType Directory -Path $root | Out-Null

& (Join-Path $PSScriptRoot 'prepare-ae-oracle-project.ps1') `
    -AfterEffects $afterEffectsPath -InputImage $inputPath -OutputAep $project `
    -OutputExr $exr -ResultJson $setup -EffectName $EffectName -Bpc 32 `
    -DurationFrames 1 -TimeoutSeconds $TimeoutSeconds | Out-Null

$stdout = Join-Path $root 'aerender.stdout.log'
$stderr = Join-Path $root 'aerender.stderr.log'
$process = Start-Process -FilePath $aerender -ArgumentList @('-project', $project) `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru -WindowStyle Hidden
if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    throw 'aerender timed out and was terminated.'
}
if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $exr -PathType Leaf)) {
    throw "aerender failed to produce the EXR (exit code $($process.ExitCode))."
}

& uv run --project $uvProject python (Join-Path $PSScriptRoot 'compare-pixel-oracles.py') `
    --raw $rawPath --render $exr --width $Width --height $Height `
    --raw-format rgba32f-le --tolerance $Tolerance --out $comparison
$comparisonExit = $LASTEXITCODE
if ($comparisonExit -gt 1) { throw "EXR comparison failed with exit code $comparisonExit." }

function Artifact([string]$Path) {
    $item = Get-Item -LiteralPath $Path
    [ordered]@{ path = $item.FullName; size_bytes = $item.Length; sha256 = Get-Sha256Hex $item.FullName }
}
$result = [ordered]@{
    schema = 'aexcompat-ae-exr-oracle-evidence-v1'
    status = if ($comparisonExit -eq 0) { 'equivalent' } else { 'different' }
    pixel_depth = 32
    comparison_boundary = 'host_raw_world_vs_ae_float_export'
    raw_world_exact = $false
    fixture = $plan.fixture
    input = Artifact $inputPath
    expected_raw = Artifact $rawPath
    artifacts = [ordered]@{ project = Artifact $project; exr = Artifact $exr; setup = Artifact $setup; comparison = Artifact $comparison }
    comparison = Get-Content -LiteralPath $comparison -Raw | ConvertFrom-Json
}
$result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $evidence -Encoding utf8
$result | ConvertTo-Json -Depth 12
if ($comparisonExit -eq 1) { throw 'AE float oracle exceeded the configured tolerance.' }
