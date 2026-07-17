[CmdletBinding()]
param(
    [string]$SdkRoot = 'C:\Program Files\Adobe\AfterEffectsSDK'
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $SdkRoot 'Examples\UI\Supervisor'
$project = Join-Path $sourceRoot 'Win\Supervisor.vcxproj'
$props = Join-Path $PSScriptRoot 'sdk-fixtures\supervisor\supervisor-v143.props'
$target = Join-Path $repoRoot 'target\sdk-fixtures\supervisor'
$intermediate = Join-Path $target 'obj'
$artifact = Join-Path $target 'Supervisor.aex'
$manifest = Join-Path $target 'build-result.json'
$vcvars = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
$msbuild = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe'
$pipTool = Join-Path $SdkRoot 'Examples\Resources\PiPLTool.exe'
$pipSource = Join-Path $sourceRoot 'SupervisorPiPL.r'
$headers = Join-Path $SdkRoot 'Examples\Headers'

foreach ($required in @($project, $props, $vcvars, $msbuild, $pipTool, $pipSource)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required Supervisor build input is missing: $required"
    }
}

New-Item -ItemType Directory -Force -Path $target, $intermediate | Out-Null
$env:TEMP = Join-Path $repoRoot 'target\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null

$sourceFiles = Get-ChildItem -LiteralPath $sourceRoot -Recurse -File |
    Where-Object { $_.FullName -notmatch '\\(Debug|Release)\\|\\x64\\' }
$before = @{}
foreach ($file in $sourceFiles) {
    $before[$file.FullName] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
}

$rr = Join-Path $target 'SupervisorPiPL.rr'
$rrc = Join-Path $target 'SupervisorPiPL.rrc'
$pipRc = Join-Path $target 'SupervisorPiPL.rc'
$buildLog = Join-Path $target 'build.log'
$targetMsBuild = ($target -replace '\\', '/') + '/'
$intermediateMsBuild = ($intermediate -replace '\\', '/') + '/'
$commands = @(
    "call `"$vcvars`"",
    "cl.exe /nologo /I `"$headers`" /EP `"$pipSource`" > `"$rr`"",
    "`"$pipTool`" `"$rr`" `"$rrc`"",
    "cl.exe /nologo /D MSWindows /EP `"$rrc`" > `"$pipRc`"",
    "`"$msbuild`" `"$project`" /nologo /m /t:Rebuild /p:Configuration=Release /p:Platform=x64 /p:PlatformToolset=v143 /p:ForceImportBeforeCppTargets=`"$props`" /p:AEXCompatSupervisorPiPLRc=`"$pipRc`" /p:OutDir=`"$targetMsBuild`" /p:IntDir=`"$intermediateMsBuild`" /p:AE_PLUGIN_BUILD_DIR=`"$target`" /bl:`"$target\build.binlog`""
)
$buildBatch = Join-Path $target 'build-supervisor.cmd'
@('@echo off', 'setlocal', ($commands -join ' && ')) |
    Set-Content -LiteralPath $buildBatch -Encoding ascii
$previousErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
& cmd.exe /d /c $buildBatch 2>&1 | Tee-Object -FilePath $buildLog
$buildExitCode = $LASTEXITCODE
$ErrorActionPreference = $previousErrorAction
if ($buildExitCode -ne 0) {
    throw "Supervisor v143 build failed with exit code $buildExitCode. See $buildLog"
}

foreach ($entry in $before.GetEnumerator()) {
    $after = (Get-FileHash -LiteralPath $entry.Key -Algorithm SHA256).Hash
    if ($after -ne $entry.Value) {
        throw "SDK source changed during build: $($entry.Key)"
    }
}
if (-not (Test-Path -LiteralPath $artifact -PathType Leaf)) {
    throw "MSBuild succeeded but did not produce $artifact"
}

$hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
$result = [ordered]@{
    status = 'built'
    source_project = $project
    platform_toolset = 'v143'
    configuration = 'Release|x64'
    artifact = $artifact
    artifact_size = (Get-Item -LiteralPath $artifact).Length
    artifact_sha256 = $hash
    sdk_source_file_count = $before.Count
    sdk_source_unchanged = $true
    build_log = $buildLog
}
$result | ConvertTo-Json | Set-Content -LiteralPath $manifest -Encoding utf8
Write-Output "Supervisor.aex SHA-256: $hash"
Write-Output ($result | ConvertTo-Json -Compress)
