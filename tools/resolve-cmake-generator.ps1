param([AllowEmptyString()][string]$Generator)

$ErrorActionPreference = 'Stop'

if ($Generator) {
    $Generator
    return
}

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw 'vswhere.exe was not found; pass -Generator with an explicit CMake generator name'
}
$version = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationVersion
if (-not $version) {
    throw 'No Visual Studio installation with the C++ x64 toolset was found; pass -Generator with an explicit CMake generator name'
}
$major = [int](([string]$version).Split('.')[0])
$knownGeneratorYears = @{ 17 = '2022'; 18 = '2026' }
if (-not $knownGeneratorYears.ContainsKey($major)) {
    throw "No known CMake generator for Visual Studio major version $major; pass -Generator with an explicit CMake generator name"
}
"Visual Studio $major $($knownGeneratorYears[$major])"
