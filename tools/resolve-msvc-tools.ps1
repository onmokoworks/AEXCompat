param(
    [AllowEmptyString()][string]$VisualStudioRoot,
    [switch]$RequireV143Toolset,
    [switch]$RequireMSBuild
)

$ErrorActionPreference = 'Stop'

function Test-Instance([string]$Root) {
    if (-not (Test-Path -LiteralPath (Join-Path $Root 'VC\Auxiliary\Build\vcvars64.bat') -PathType Leaf)) { return $false }
    if ($RequireMSBuild -and -not (Test-Path -LiteralPath (Join-Path $Root 'MSBuild\Current\Bin\MSBuild.exe') -PathType Leaf)) { return $false }
    if ($RequireV143Toolset) {
        $toolsets = Get-ChildItem -LiteralPath (Join-Path $Root 'VC\Tools\MSVC') -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^14\.[34]\d\.' }
        if (-not $toolsets) { return $false }
    }
    return $true
}

if ($VisualStudioRoot) {
    if (-not (Test-Instance $VisualStudioRoot)) {
        throw "The requested Visual Studio root does not satisfy the required components: $VisualStudioRoot"
    }
    (Resolve-Path -LiteralPath $VisualStudioRoot).Path
    return
}

$candidates = [System.Collections.Generic.List[string]]::new()
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (Test-Path -LiteralPath $vswhere -PathType Leaf) {
    foreach ($installation in & $vswhere -products * -sort -format value -property installationPath) {
        $candidates.Add($installation)
    }
}
foreach ($base in @("${env:ProgramFiles(x86)}\Microsoft Visual Studio", "$env:ProgramFiles\Microsoft Visual Studio")) {
    foreach ($release in '2022', '18') {
        foreach ($edition in 'BuildTools', 'Enterprise', 'Professional', 'Community') {
            $candidates.Add("$base\$release\$edition")
        }
    }
}

foreach ($candidate in $candidates) {
    if ((Test-Path -LiteralPath $candidate -PathType Container) -and (Test-Instance $candidate)) {
        (Resolve-Path -LiteralPath $candidate).Path
        return
    }
}
throw 'No Visual Studio installation satisfies the required components; pass -VisualStudioRoot with its absolute path'
