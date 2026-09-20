<#
.SYNOPSIS
Builds and stages the supported AEXCompat batch render-sweep CLI.

.DESCRIPTION
Builds the canonical Release worker and Rust CLI, then stages the executable
beside the worker-root layout the CLI resolves automatically. The package does
not copy AEX files or vendor/Adobe runtime DLLs; discovery uses the same runtime
root resolution as the shipping multi-filter.

.PARAMETER OutputDirectory
Package directory. Relative paths are resolved from the repository root.

.PARAMETER CompileCache
Optional compiler cache forwarded to tools\build-native.ps1.

.PARAMETER SkipNativeBuild
Reuse the existing canonical worker after checking that it exists. Intended for
Rust-only iteration; release packaging should normally omit this switch.
#>
[CmdletBinding()]
param(
    [string]$OutputDirectory = 'target\aexcompat-render-sweep-package',
    [string]$CompileCache,
    [switch]$SkipNativeBuild
)

$ErrorActionPreference = 'Stop'

$repository = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'publish-render-sweep-package.ps1')
if (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory = Join-Path $repository $OutputDirectory
}
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)

if (-not $SkipNativeBuild) {
    $nativeArgs = @('-File', (Join-Path $PSScriptRoot 'build-native.ps1'))
    if ($PSBoundParameters.ContainsKey('CompileCache')) {
        $nativeArgs += @('-CompileCache', $CompileCache)
    }
    & pwsh @nativeArgs
    if ($LASTEXITCODE -ne 0) {
        throw "native worker build failed with exit code $LASTEXITCODE"
    }
}

$worker = Join-Path $repository 'target\minihost-build\aex_worker.exe'
if (-not (Test-Path -LiteralPath $worker -PathType Leaf)) {
    throw "canonical worker is missing: $worker"
}

& cargo build `
    --manifest-path (Join-Path $repository 'bridges\aviutl2-multifilter\Cargo.toml') `
    --locked `
    --release `
    --bin aexcompat-render-sweep
if ($LASTEXITCODE -ne 0) {
    throw "render-sweep Release build failed with exit code $LASTEXITCODE"
}

$cli = Join-Path $repository 'bridges\aviutl2-multifilter\target\release\aexcompat-render-sweep.exe'
if (-not (Test-Path -LiteralPath $cli -PathType Leaf)) {
    throw "Release CLI is missing: $cli"
}

$validateStagedPackage = {
    param($stagedPackage)

    # Prove the staged executable resolves its adjacent worker without
    # repository or runtime-folder configuration. Limit zero keeps this check
    # AEX-free. Validation happens before the existing package is replaced.
    $stagedCli = Join-Path $stagedPackage 'aexcompat-render-sweep.exe'
    $temporaryBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    $temporaryRoot = Join-Path $temporaryBase ("aexcompat-render-sweep-package-" + [guid]::NewGuid().ToString('N'))
    $temporaryRoot = [System.IO.Path]::GetFullPath($temporaryRoot)
    if (-not [System.IO.Path]::GetDirectoryName($temporaryRoot).Equals(
        $temporaryBase.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar),
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
        throw "temporary self-test path escaped the temporary directory: $temporaryRoot"
    }
    try {
        $scan = Join-Path $temporaryRoot 'empty-scan'
        $report = Join-Path $temporaryRoot 'report.json'
        $config = Join-Path $temporaryRoot 'missing-config.toml'
        New-Item -ItemType Directory -Force -Path $scan | Out-Null
        $priorRepository = $env:AEXCOMPAT_MULTIFILTER_REPOSITORY
        $priorConfig = $env:AEXCOMPAT_MULTIFILTER_CONFIG
        try {
            Remove-Item Env:AEXCOMPAT_MULTIFILTER_REPOSITORY -ErrorAction SilentlyContinue
            $env:AEXCOMPAT_MULTIFILTER_CONFIG = $config
            $selfTestOutput = & $stagedCli --limit 0 --json $report $scan
            if ($LASTEXITCODE -ne 0) {
                throw "packaged CLI self-test failed with exit code $LASTEXITCODE"
            }
        } finally {
            if ($null -eq $priorRepository) {
                Remove-Item Env:AEXCOMPAT_MULTIFILTER_REPOSITORY -ErrorAction SilentlyContinue
            } else {
                $env:AEXCOMPAT_MULTIFILTER_REPOSITORY = $priorRepository
            }
            if ($null -eq $priorConfig) {
                Remove-Item Env:AEXCOMPAT_MULTIFILTER_CONFIG -ErrorAction SilentlyContinue
            } else {
                $env:AEXCOMPAT_MULTIFILTER_CONFIG = $priorConfig
            }
        }
        $value = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
        if ($value.scan.swept -ne 0 -or $value.plugins.Count -ne 0 -or -not $value.build.complete) {
            throw 'packaged CLI self-test did not produce a complete zero-record report'
        }
    } finally {
        if (Test-Path -LiteralPath $temporaryRoot) {
            Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
        }
    }
}

$OutputDirectory = Publish-RenderSweepPackage `
    -Cli $cli `
    -Worker $worker `
    -OutputDirectory $OutputDirectory `
    -ValidateStagedPackage $validateStagedPackage

$packageCli = Join-Path $OutputDirectory 'aexcompat-render-sweep.exe'
$packageWorker = Join-Path $OutputDirectory 'target\minihost-build\aex_worker.exe'

$cliFile = Get-Item -LiteralPath $packageCli
$workerFile = Get-Item -LiteralPath $packageWorker
[pscustomobject]@{
    package = $OutputDirectory
    cli = $cliFile.FullName
    cli_size = $cliFile.Length
    cli_sha256 = (Get-FileHash -LiteralPath $packageCli -Algorithm SHA256).Hash.ToLowerInvariant()
    worker = $workerFile.FullName
    worker_size = $workerFile.Length
    worker_sha256 = (Get-FileHash -LiteralPath $packageWorker -Algorithm SHA256).Hash.ToLowerInvariant()
}
