$ErrorActionPreference = 'Stop'

# Ninja 系 generator で cl / link / rc / mt を PATH から解決できるよう、
# vcvars64.bat が作る環境を GITHUB_ENV へ書き出す (#1510)。VS generator は
# MSBuild が toolset を自分で解決するのでこの環境を要らないが、Ninja 系は
# 呼び出し側の環境に依存する。job の各 step (と pytest から spawn される
# build-*.ps1) 全体へ効かせるため、step 内で vcvars を呼ぶのではなく
# GITHUB_ENV へ書く。

if (-not $env:GITHUB_ENV) {
    throw 'GITHUB_ENV is not set; this script only runs inside a GitHub Actions step'
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw 'vswhere.exe was not found; a Visual Studio C++ x64 toolset is required'
}
$installation = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $installation) {
    throw 'no Visual Studio installation with the C++ x64 toolset was found'
}
$vcvars = Join-Path $installation 'VC\Auxiliary\Build\vcvars64.bat'
if (-not (Test-Path -LiteralPath $vcvars -PathType Leaf)) {
    throw "vcvars64.bat was not found at $vcvars"
}

$exported = @{}
foreach ($line in (cmd /c "`"$vcvars`" >nul 2>&1 && set")) {
    $separator = $line.IndexOf('=')
    if ($separator -lt 1) { continue }
    $name = $line.Substring(0, $separator)
    if ($name -notin @('PATH', 'INCLUDE', 'LIB', 'LIBPATH')) { continue }
    $exported[$name] = $line.Substring($separator + 1)
}
foreach ($name in @('PATH', 'INCLUDE', 'LIB')) {
    if (-not $exported.ContainsKey($name)) {
        throw "vcvars64.bat did not export $name"
    }
}

foreach ($name in $exported.Keys) {
    "$name=$($exported[$name])" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
    Write-Host "exported $name ($($exported[$name].Length) chars) from $vcvars"
}

# SDK tests may omit their repeated vcvars call only while this exact exported
# compiler/include/library environment still agrees. PATH itself can acquire
# unrelated Actions tools; consumers compare the cl/link it actually resolves.
function Resolve-ExportedToolPath([string]$tool) {
    foreach ($entry in $exported['PATH'].Split(';')) {
        if (-not $entry) { continue }
        $candidate = Join-Path $entry.Trim('"') $tool
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return [System.IO.Path]::GetFullPath($candidate)
        }
    }
    return $null
}
$compiler = Resolve-ExportedToolPath 'cl.exe'
$linker = Resolve-ExportedToolPath 'link.exe'
$state = ''
if ($compiler -and $linker) {
    $state = [ordered]@{
        installation = $installation
        compiler = $compiler
        linker = $linker
        include = $exported['INCLUDE']
        lib = $exported['LIB']
        libpath = $(if ($exported.ContainsKey('LIBPATH')) { $exported['LIBPATH'] } else { '' })
    } | ConvertTo-Json -Compress
}
"AEXCOMPAT_MSVC_ENV_STATE=$state" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
