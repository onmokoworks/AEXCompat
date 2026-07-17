param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string]$HarnessPath = "",
    [string]$OutputDirectory = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($HarnessPath)) {
    $HarnessPath = Join-Path $RepositoryRoot "broker\target\release\aexcompat-harness.exe"
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $RepositoryRoot "target\conformance-inspect"
}

$HarnessPath = (Resolve-Path -LiteralPath $HarnessPath).Path
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

$cases = @(
    @{
        Name = "paramarama"
        RelativePath = "target\sdk-fixtures\paramarama\Paramarama.aex"
        Count = 8
        RequiredKinds = @("integer", "color", "float", "angle", "point3d", "button")
    },
    @{
        Name = "colorgrid"
        RelativePath = "target\sdk-fixtures\colorgrid\ColorGrid.aex"
        Count = 1
        RequiredKinds = @("arbitrary_data")
    },
    @{
        Name = "pathmaster"
        RelativePath = "target\sdk-fixtures\pathmaster\PathMaster.aex"
        Count = 6
        RequiredKinds = @("path")
    },
    @{
        Name = "smartypants"
        RelativePath = "target\sdk-fixtures\smartypants\SmartyPants.aex"
        Count = 2
        RequiredKinds = @("integer", "float")
    }
)

$summary = @()
foreach ($case in $cases) {
    $plugin = (Resolve-Path -LiteralPath (Join-Path $RepositoryRoot $case.RelativePath)).Path
    $stdout = Join-Path $OutputDirectory "$($case.Name).json"
    $stderr = Join-Path $OutputDirectory "$($case.Name).err"
    $process = Start-Process $HarnessPath `
        -ArgumentList @("--inspect-experimental", $plugin) `
        -RedirectStandardOutput $stdout `
        -RedirectStandardError $stderr `
        -Wait -PassThru -NoNewWindow

    if ($process.ExitCode -ne 0) {
        throw "$($case.Name) failed with exit code $($process.ExitCode): $(Get-Content $stderr -Raw)"
    }
    [array]$parameters = Get-Content $stdout -Raw | ConvertFrom-Json
    if ($parameters.Count -ne $case.Count) {
        throw "$($case.Name) returned $($parameters.Count) parameters; expected $($case.Count)"
    }
    $kinds = @($parameters.kind | Sort-Object -Unique)
    foreach ($required in $case.RequiredKinds) {
        if ($kinds -notcontains $required) {
            throw "$($case.Name) did not expose required parameter kind '$required'"
        }
    }
    $summary += [ordered]@{
        name = $case.Name
        aex_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $plugin).Hash
        parameter_count = $parameters.Count
        kinds = $kinds
        passed = $true
    }
}

$report = [ordered]@{
    schema_version = 1
    stage = "aex_discovery_conformance_matrix"
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
    passed = $true
    cases = $summary
}
$reportPath = Join-Path $OutputDirectory "summary.json"
$report | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 -LiteralPath $reportPath
$report | ConvertTo-Json -Depth 8
