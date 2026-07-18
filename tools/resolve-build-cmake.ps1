param(
    [AllowEmptyString()][string]$CMake,
    [AllowEmptyString()][string]$Generator
)

$ErrorActionPreference = 'Stop'

if ($CMake) {
    if (-not (Test-Path -LiteralPath $CMake -PathType Leaf)) {
        throw "cmake.exe was not found at the requested path: $CMake"
    }
    (Resolve-Path -LiteralPath $CMake).Path
    return
}

$bundledSuffix = 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
$candidates = [System.Collections.Generic.List[string]]::new()
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path -LiteralPath $vswhere -PathType Leaf) {
    foreach ($installation in & $vswhere -products * -sort -format value -property installationPath) {
        $candidates.Add((Join-Path $installation $bundledSuffix))
    }
}
foreach ($visualStudioRoot in @("$env:ProgramFiles\Microsoft Visual Studio", "${env:ProgramFiles(x86)}\Microsoft Visual Studio")) {
    foreach ($release in '18', '2022') {
        foreach ($edition in 'Enterprise', 'Professional', 'Community', 'BuildTools') {
            $candidates.Add("$visualStudioRoot\$release\$edition\$bundledSuffix")
        }
    }
}
$candidates.Add("$env:ProgramFiles\CMake\bin\cmake.exe")
$pathCommand = Get-Command cmake -ErrorAction SilentlyContinue
if ($pathCommand) { $candidates.Add($pathCommand.Source) }

$firstExisting = $null
foreach ($candidate in $candidates) {
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { continue }
    if (-not $firstExisting) { $firstExisting = $candidate }
    if (-not $Generator) { break }
    $help = (& $candidate --help 2>$null | Out-String)
    if ($help.Contains($Generator)) {
        (Resolve-Path -LiteralPath $candidate).Path
        return
    }
}
if (-not $Generator -and $firstExisting) {
    (Resolve-Path -LiteralPath $firstExisting).Path
    return
}
if ($firstExisting) {
    throw "No cmake.exe supporting generator '$Generator' was found; pass -CMake with its absolute path"
}
throw 'cmake.exe was not found; pass -CMake with its absolute path'
