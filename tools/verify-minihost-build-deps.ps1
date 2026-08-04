<#
.SYNOPSIS
Fails closed when a minihost build directory has lost header-dependency tracking (issue #657).

.DESCRIPTION
With the Ninja generator, cl's header dependencies are recovered by matching
`/showIncludes` output against the localized `msvc_deps_prefix` that CMake
captured while probing the compiler. When the build later emits a different
language - or the same words under a different console codepage - nothing
matches, ninja records ZERO dependencies per object, and every later build
reports "no work to do" no matter which headers changed. The directory then
accumulates objects compiled against older headers while looking current, and
the linked worker mixes them.

That is how #651 happened: `ninja -t deps` reported `#deps 0` for every object
in `target\minihost-build`, including ones rebuilt that same day, and the
resulting worker access-violated in GLOBAL_SETUP for every AEX.

This script inspects what ninja actually recorded. Every translation unit that
includes at least one project header ("...") must have at least one recorded
dependency; a unit that includes only system headers is allowed to have none.

Run it after building the workers. It reads the build directory only.
#>
[CmdletBinding()]
param(
    [string]$BuildDir = '',
    [string]$Ninja = 'ninja'
)

$ErrorActionPreference = 'Stop'

# Resolved here rather than in the param default: $PSScriptRoot is not reliably
# populated while default expressions are evaluated under `powershell -File`.
$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptRoot
if (-not $BuildDir) {
    $BuildDir = Join-Path $repoRoot 'target\minihost-build'
}

if (-not (Test-Path -LiteralPath $BuildDir)) {
    throw "minihost build directory not found: $BuildDir"
}
$rulesPath = Join-Path $BuildDir 'CMakeFiles\rules.ninja'
if (-not (Test-Path -LiteralPath (Join-Path $BuildDir 'build.ninja'))) {
    throw "not a ninja build directory (no build.ninja): $BuildDir"
}

# Diagnostic only: the prefix itself is not the verdict. A mismatch shows up as
# missing dependencies below, which is what actually breaks the build.
$prefix = $null
if (Test-Path -LiteralPath $rulesPath) {
    $prefix = (Select-String -LiteralPath $rulesPath -Pattern '^\s*msvc_deps_prefix = (.*)$' |
        Select-Object -First 1).Matches.Groups[1].Value
}

$depsText = & $Ninja -C $BuildDir -t deps 2>&1
if ($LASTEXITCODE -ne 0) {
    throw "ninja -t deps failed in $BuildDir : $depsText"
}

# `ninja -t deps` prints one header line per output ("<out>: #deps N, ...")
# followed by the recorded dependencies, indented.
$records = @{}
foreach ($line in $depsText) {
    if ($line -match '^(?<out>\S.*?): #deps (?<count>\d+),') {
        $records[$matches['out']] = [int]$matches['count']
    }
}
if ($records.Count -eq 0) {
    throw "ninja recorded no dependency entries at all in $BuildDir; rebuild after removing the directory"
}

$sourceRoot = Join-Path $repoRoot 'minihost\src'
$checked = 0
$vacuous = New-Object System.Collections.Generic.List[string]
foreach ($out in $records.Keys) {
    if ($out -notmatch '([^\\/]+\.cpp)\.obj$') { continue }
    $source = Join-Path $sourceRoot $matches[1]
    if (-not (Test-Path -LiteralPath $source)) { continue }
    # Only units that include a project header can be judged: one including
    # nothing but <system> headers legitimately records no dependency.
    if (-not (Select-String -LiteralPath $source -Pattern '^\s*#include\s*"' -Quiet)) { continue }
    $checked++
    if ($records[$out] -eq 0) { $vacuous.Add($out) }
}

if ($checked -eq 0) {
    throw "no minihost translation unit could be checked in $BuildDir; rebuild after removing the directory"
}

if ($vacuous.Count -gt 0) {
    Write-Host "msvc_deps_prefix = $prefix"
    foreach ($out in ($vacuous | Sort-Object | Select-Object -First 10)) {
        Write-Host "  no recorded header dependency: $out"
    }
    if ($vacuous.Count -gt 10) { Write-Host "  ... and $($vacuous.Count - 10) more" }
    throw ("header dependency tracking is dead in ${BuildDir}: $($vacuous.Count) of $checked " +
        'translation units recorded no header dependency. Objects there may have been compiled ' +
        'against older headers while ninja reports the build as current (issue #657/#651). ' +
        'Delete the build directory and configure it again.')
}

Write-Host "header dependency tracking verified: $checked translation units, all with recorded dependencies"
Write-Host "msvc_deps_prefix = $prefix"
