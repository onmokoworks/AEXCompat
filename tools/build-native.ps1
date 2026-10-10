<#
.SYNOPSIS
Builds a native CMake tree in this repository (issue #1495).

.DESCRIPTION
The single entry point for the C++ builds. It resolves the MSVC developer
environment itself, so it works from a plain PowerShell instead of requiring
the caller to have run vcvars64 first. Without that, `cmake --build` fails with

    fatal error C1083: Cannot open include file: 'cstddef'

which reads like a source problem and is not one. That trap used to be
documented in CLAUDE.md and copied into the CI workflow in three places; this
script is where it lives now.

The default source tree is `minihost`, which produces `aex_worker.exe` and the
native self-tests. There is one worker executable and it selects its route at
run time with `--kind discovery|classic|smart`. It used to be three binaries
built from three five-line entry files, where building one target relinked only
that one and left the others on the previous build.

.PARAMETER Source
CMake source directory, relative to the repository root. Defaults to `minihost`.

.PARAMETER BuildDir
Build directory, relative to the repository root unless absolute. Defaults to
`target\<source>-build`, which is where `docs/BUILD_REQUIREMENTS.md` and the
test fixtures look.

.PARAMETER Target
Build only these CMake targets instead of everything. Intended for iterating on
one self-test; the worker itself is a single target, so there is no way to ask
for a partial worker.

.PARAMETER CompileCache
Name of a compiler cache (sccache, ccache) to run every compile through. It is
a configure-time choice, so passing it against a build directory configured
without it re-runs configure rather than building uncached. CI uses it because
every run starts from a clean machine and would otherwise recompile all 244
translation units (issue #1501).

.PARAMETER Configure
Re-run the CMake configure step even when the build directory already has a
cache.

.PARAMETER RepairHeaderDependencies
Re-probe generated C++ compiler metadata and clean-build the whole tree while
preserving CMakeCache options. Use this for an existing Ninja cache whose MSVC
include prefix was captured under a different console encoding. Cannot be
combined with Target: a partial rebuild could leave mixed ABI objects behind.

.EXAMPLE
pwsh -File tools\build-native.ps1

.EXAMPLE
pwsh -File tools\build-native.ps1 -Source instruments -Target trace_writer_selftest
#>
[CmdletBinding()]
param(
    [string]$Source = 'minihost',
    [string]$BuildDir,
    [string[]]$Target,
    [string]$CompileCache,
    [switch]$Configure,
    [switch]$RepairHeaderDependencies
)

$ErrorActionPreference = 'Stop'

$repository = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$sourceDir = Join-Path $repository $Source
if (-not (Test-Path -LiteralPath (Join-Path $sourceDir 'CMakeLists.txt') -PathType Leaf)) {
    throw "no CMakeLists.txt under $sourceDir"
}
if (-not $BuildDir) { $BuildDir = "target\$Source-build" }
if (-not [System.IO.Path]::IsPathRooted($BuildDir)) {
    $BuildDir = Join-Path $repository $BuildDir
}

# vswhere ships with any Visual Studio installer since 2017 and lives at a fixed
# location, so finding it needs no environment of its own.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw "vswhere.exe is missing at $vswhere; install the Visual Studio C++ build tools"
}
$vsRoot = & $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $vsRoot) {
    throw 'no Visual Studio installation with the x64 C++ toolset was found'
}
$vcvars = Join-Path $vsRoot 'VC\Auxiliary\Build\vcvars64.bat'
if (-not (Test-Path -LiteralPath $vcvars -PathType Leaf)) {
    throw "the located Visual Studio has no vcvars64.bat at $vcvars"
}

# cmd carries the environment vcvars64 sets; running the whole build inside one
# invocation keeps it, rather than trying to import the variables back into this
# session and hoping the set is complete.
$cmakeCache = Join-Path $BuildDir 'CMakeCache.txt'
$configureNeeded = $Configure -or -not (Test-Path -LiteralPath $cmakeCache -PathType Leaf)

if ($RepairHeaderDependencies) {
    if ($Target) { throw '-RepairHeaderDependencies requires a whole-tree build (omit -Target)' }
    if (-not (Test-Path -LiteralPath $cmakeCache -PathType Leaf)) {
        throw '-RepairHeaderDependencies requires an existing CMake cache'
    }
    $cacheSource = Select-String -LiteralPath $cmakeCache -Encoding UTF8 `
        -Pattern '^CMAKE_HOME_DIRECTORY:INTERNAL=(.*)$' | Select-Object -First 1
    $cacheGenerator = Select-String -LiteralPath $cmakeCache -Encoding UTF8 `
        -Pattern '^CMAKE_GENERATOR:INTERNAL=(.*)$' | Select-Object -First 1
    if (-not $cacheSource -or -not $cacheGenerator -or
        $cacheGenerator.Matches[0].Groups[1].Value -ne 'Ninja' -or
        [System.IO.Path]::GetFullPath($cacheSource.Matches[0].Groups[1].Value) -ne
            [System.IO.Path]::GetFullPath($sourceDir)) {
        throw 'repair requires a Ninja cache for the requested source tree; no metadata was changed'
    }
    $cmakeFiles = Join-Path $BuildDir 'CMakeFiles'
    # A normal-looking BuildDir can still live beneath a junction (e.g.
    # target -> another volume). Inspect the complete ancestor chain before
    # deleting any generated state, not just the leaf's lexical name.
    $repairAncestor = [System.IO.Path]::GetFullPath($BuildDir)
    while ($repairAncestor) {
        $entry = Get-Item -LiteralPath $repairAncestor -Force
        if ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw 'repair refuses reparse-point build ancestors; no metadata was changed'
        }
        $repairAncestor = [System.IO.Path]::GetDirectoryName($repairAncestor.TrimEnd('\', '/'))
    }
    foreach ($directory in @($cmakeFiles)) {
        $entry = Get-Item -LiteralPath $directory -Force
        if ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw 'repair refuses reparse-point build directories; no metadata was changed'
        }
    }
    $versionDirectories = @(Get-ChildItem -LiteralPath $cmakeFiles -Directory -Force |
        Where-Object Name -Match '^\d+\.\d+(\.\d+)?$')
    foreach ($directory in $versionDirectories) {
        if ($directory.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw 'repair refuses reparse-point compiler directories; no metadata was changed'
        }
    }
    $compilerInfoFiles = @($versionDirectories |
        ForEach-Object { Join-Path $_.FullName 'CMakeCXXCompiler.cmake' } |
        Where-Object { Get-Item -LiteralPath $_ -Force -ErrorAction SilentlyContinue })
    if (-not $compilerInfoFiles) { throw 'no generated C++ compiler metadata found to re-probe' }
    # Delete only generated probe/dependency state, never the operator's cache
    # options. Re-configuring without this reprobe keeps the corrupt prefix;
    # --fresh would fix that but also discard custom flags and launchers.
    $repairRoot = [System.IO.Path]::GetFullPath($BuildDir).TrimEnd('\', '/') + '\'
    $deleteCandidates = @($compilerInfoFiles)
    $ninjaDeps = Join-Path $BuildDir '.ninja_deps'
    if (Get-Item -LiteralPath $ninjaDeps -Force -ErrorAction SilentlyContinue) {
        $deleteCandidates += $ninjaDeps
    }
    # Preflight EVERY entry before removing the first one. A lexical child path
    # alone is not sufficient: a junction could otherwise redirect deletion.
    foreach ($entryPath in $deleteCandidates) {
        $candidate = [System.IO.Path]::GetFullPath($entryPath)
        if (-not $candidate.StartsWith($repairRoot, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'generated compiler metadata escaped the build directory'
        }
        $entry = Get-Item -LiteralPath $candidate -Force
        if ($entry.PSIsContainer -or
            ($entry.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw 'repair refuses reparse-point/non-file metadata; no metadata was changed'
        }
    }
    foreach ($entryPath in $deleteCandidates) {
        Remove-Item -LiteralPath $entryPath
    }
    Write-Host 'repairing generated compiler/dependency state; preserving CMakeCache options'
    $configureNeeded = $true
}

# The launcher is baked into the ninja rules at configure time, so an existing
# build directory that was configured without the cache would quietly build
# uncached and look like a cache that does not work. Re-configure instead of
# reporting a hit rate for compiles that never reached the cache.
if (-not $configureNeeded -and $PSBoundParameters.ContainsKey('CompileCache')) {
    # A cache configured before this option existed has no such entry at all,
    # which is a mismatch like any other rather than an error to trip over.
    $match = Select-String -LiteralPath $cmakeCache `
        -Pattern '^AEXCOMPAT_COMPILE_CACHE:[^=]*=(.*)$' | Select-Object -First 1
    $recorded = if ($match) { $match.Matches[0].Groups[1].Value } else { '' }
    if ($recorded -ne $CompileCache) {
        Write-Host "reconfiguring: compile cache was '$recorded', now '$CompileCache'"
        $configureNeeded = $true
    }
}

$steps = @()
if ($configureNeeded) {
    # Not $configure: PowerShell variables are case-insensitive, so that name is
    # the -Configure switch parameter and assigning a string to it throws.
    $configureStep = "cmake -S `"$sourceDir`" -B `"$BuildDir`" -G Ninja -DCMAKE_BUILD_TYPE=Release"
    if ($PSBoundParameters.ContainsKey('CompileCache')) {
        $configureStep += " -DAEXCOMPAT_COMPILE_CACHE=`"$CompileCache`""
    }
    $steps += $configureStep
}
if ($Target) {
    $steps += "cmake --build `"$BuildDir`" --target $($Target -join ' ')"
} else {
    $buildStep = "cmake --build `"$BuildDir`""
    if ($RepairHeaderDependencies) { $buildStep += ' --clean-first' }
    $steps += $buildStep
}

# MSVC may have only Japanese resources, so VSLANG=1033 cannot guarantee ASCII.
# CMake decodes /showIncludes using the console code page while Ninja consumes
# UTF-8. Pin the probe and every build in this same batch to avoid mojibake and
# zero header dependencies (including when invoked from a legacy codepage).
$script = @("call `"$vcvars`" || exit /b 1", 'chcp 65001 >nul || exit /b 1') +
    ($steps | ForEach-Object { "$_ || exit /b 1" })
$batch = Join-Path ([System.IO.Path]::GetTempPath()) ("aexcompat-build-native-$PID.cmd")
try {
    Set-Content -LiteralPath $batch -Value (@('@echo off') + $script) -Encoding ascii
    & cmd /c "`"$batch`""
    if ($LASTEXITCODE -ne 0) { throw "native build failed with exit code $LASTEXITCODE" }
} finally {
    Remove-Item -LiteralPath $batch -ErrorAction SilentlyContinue
}

if ($Source -eq 'minihost' -and (-not $Target -or $Target -contains 'aex_worker' -or
        $Target -contains 'aex_worker_runtime_core')) {
    try {
        & "$PSScriptRoot\verify-minihost-build-deps.ps1" -BuildDir $BuildDir
    } catch {
        throw ("native worker header dependencies are not qualified; rerun build-native.ps1 " +
            "-RepairHeaderDependencies without -Target, keeping the same -BuildDir. " + $_)
    }
}

# A whole-tree minihost build must have produced the worker. Naming it here
# means a build that reports success while linking nothing is caught at the
# entry point rather than by whatever runs next.
if ($Source -eq 'minihost' -and -not $Target) {
    $worker = Join-Path $BuildDir 'aex_worker.exe'
    if (-not (Test-Path -LiteralPath $worker -PathType Leaf)) {
        throw "the build reported success but $worker is missing"
    }
    $file = Get-Item -LiteralPath $worker
    Write-Host "worker=$($file.FullName) size=$($file.Length) mtime=$($file.LastWriteTimeUtc.ToString('o'))"
}
