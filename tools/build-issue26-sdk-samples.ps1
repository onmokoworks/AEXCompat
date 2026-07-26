[CmdletBinding()]
param(
    [ValidateSet('All', 'Projector', 'Resizer')]
    [string]$Sample = 'All',
    [string]$SdkRoot = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$VisualStudioRoot
)

$ErrorActionPreference = 'Stop'
$SdkRoot = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $SdkRoot
$repoRoot = Split-Path -Parent $PSScriptRoot
$targetRoot = Join-Path $repoRoot 'target\issue26-sdk-samples'
$vsRoot = & "$PSScriptRoot\resolve-msvc-tools.ps1" $VisualStudioRoot -RequireV143Toolset -RequireMSBuild
$vcvars = Join-Path $vsRoot 'VC\Auxiliary\Build\vcvars64.bat'
$msbuild = Join-Path $vsRoot 'MSBuild\Current\Bin\MSBuild.exe'
$pipTool = Join-Path $SdkRoot 'Examples\Resources\PiPLTool.exe'
$headers = Join-Path $SdkRoot 'Examples\Headers'

foreach ($required in @($vcvars, $msbuild, $pipTool)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required Issue #26 SDK build input is missing: $required"
    }
}

function Build-Sample {
    param(
        [string]$Name,
        [string]$RelativeRoot,
        [string]$ProjectName,
        [string]$PiPLName,
        [string]$PropsName
    )

    $sourceRoot = Join-Path $SdkRoot $RelativeRoot
    $project = Join-Path $sourceRoot "Win\$ProjectName.vcxproj"
    $pipSource = Join-Path $sourceRoot $PiPLName
    $props = Join-Path $PSScriptRoot "sdk-fixtures\$PropsName"
    foreach ($required in @($project, $pipSource, $props)) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
            throw "Required $Name input is missing: $required"
        }
    }

    $target = Join-Path $targetRoot $Name.ToLowerInvariant()
    $intermediate = Join-Path $target 'obj'
    New-Item -ItemType Directory -Force -Path $target, $intermediate | Out-Null
    $rr = Join-Path $target "$Name.rr"
    $rrc = Join-Path $target "$Name.rrc"
    $pipRc = Join-Path $target "$Name-PiPL.rc"
    $artifact = Join-Path $target "$Name.aex"
    $log = Join-Path $target 'build.log'
    $batch = Join-Path $target "build-$($Name.ToLowerInvariant()).cmd"

    $sourceFiles = Get-ChildItem -LiteralPath $sourceRoot -Recurse -File |
        Where-Object { $_.FullName -notmatch '\\(Debug|Release)\\|\\x64\\' }
    $before = @{}
    foreach ($file in $sourceFiles) {
        $before[$file.FullName] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
    }

    $targetMsBuild = ($target -replace '\\', '/') + '/'
    $intermediateMsBuild = ($intermediate -replace '\\', '/') + '/'
    $commands = @(
        "call `"$vcvars`"",
        "cl.exe /nologo /I `"$headers`" /EP `"$pipSource`" > `"$rr`"",
        "`"$pipTool`" `"$rr`" `"$rrc`"",
        "cl.exe /nologo /D MSWindows /EP `"$rrc`" > `"$pipRc`"",
        "`"$msbuild`" `"$project`" /nologo /m /t:Rebuild /p:Configuration=Release /p:Platform=x64 /p:PlatformToolset=v143 /p:ForceImportBeforeCppTargets=`"$props`" /p:AEXCompatIssue26PiPLRc=`"$pipRc`" /p:OutDir=`"$targetMsBuild`" /p:IntDir=`"$intermediateMsBuild`" /p:AE_PLUGIN_BUILD_DIR=`"$target`" /bl:`"$target\build.binlog`""
    )
    @('@echo off', 'setlocal', ($commands -join ' && ')) |
        Set-Content -LiteralPath $batch -Encoding ascii
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    & cmd.exe /d /c $batch 2>&1 |
        Tee-Object -FilePath $log |
        Out-Host
    $buildExitCode = $LASTEXITCODE
    $ErrorActionPreference = $previousErrorAction
    if ($buildExitCode -ne 0) {
        throw "$Name v143 build failed with exit code $buildExitCode. See $log"
    }

    foreach ($entry in $before.GetEnumerator()) {
        $after = (Get-FileHash -LiteralPath $entry.Key -Algorithm SHA256).Hash
        if ($after -ne $entry.Value) {
            throw "SDK source changed during $Name build: $($entry.Key)"
        }
    }
    if (-not (Test-Path -LiteralPath $artifact -PathType Leaf)) {
        throw "$Name build did not produce $artifact"
    }
    return [ordered]@{
        sample = $Name
        source_project = $project
        platform_toolset = 'v143'
        configuration = 'Release|x64'
        artifact = $artifact
        artifact_size = (Get-Item -LiteralPath $artifact).Length
        artifact_sha256 = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
        sdk_source_unchanged = $true
    }
}

New-Item -ItemType Directory -Force -Path $targetRoot | Out-Null
$env:TEMP = Join-Path $repoRoot 'target\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
$results = @()
if ($Sample -in @('All', 'Projector')) {
    $results += Build-Sample 'Projector' 'Examples\AEGP\Projector' 'Projector' 'Projector_PiPL.r' 'projector-v143.props'
}
if ($Sample -in @('All', 'Resizer')) {
    $results += Build-Sample 'Resizer' 'Examples\Effect\Resizer' 'Resizer' 'ResizerPiPL.r' 'resizer-v143.props'
}
$manifest = [ordered]@{
    schema_version = 1
    status = 'built'
    samples = $results
}
$manifestPath = Join-Path $targetRoot 'build-result.json'
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath -Encoding utf8
Write-Output ($manifest | ConvertTo-Json -Depth 5 -Compress)
