param(
    [Parameter(Mandatory = $true)][ValidateSet('Install', 'Remove')][string]$Action,
    [Parameter(Mandatory = $true)][string]$BundleRoot,
    [string]$PluginRoot = 'C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore'
)

$ErrorActionPreference = 'Stop'
if (Get-Process AfterFX,AfterFX.com,aerender,aerendercore -ErrorAction SilentlyContinue) {
    throw 'After Effects is running; refusing to modify its plug-in directory.'
}

$bundle = (Resolve-Path -LiteralPath $BundleRoot).Path
$manifestPath = Join-Path $bundle 'manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Bundle manifest is missing: $manifestPath"
}
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.schema -ne 'aexcompat-ae-oracle-bundle-v1' -or
    $manifest.install_directory -ne 'AEXCompatOracleBundle' -or
    -not $manifest.probes -or $manifest.probes.Count -ne 3) {
    throw 'Bundle manifest schema, install directory, or probe count is invalid.'
}

$pluginRootFull = [System.IO.Path]::GetFullPath($PluginRoot).TrimEnd('\')
$installRoot = Join-Path $pluginRootFull $manifest.install_directory
if ([System.IO.Path]::GetFileName($installRoot) -ne 'AEXCompatOracleBundle') {
    throw "Refusing unexpected install directory: $installRoot"
}

function Assert-ManifestPayload {
    param([string]$Root, [switch]$Installed)
    foreach ($entry in $manifest.probes) {
        $relative = if ($Installed) { [System.IO.Path]::GetFileName([string]$entry.file) } else { [string]$entry.file }
        if ([System.IO.Path]::IsPathRooted($relative) -or $relative.Contains('..')) {
            throw "Unsafe manifest path: $relative"
        }
        $path = Join-Path $Root ($relative.Replace('/', '\'))
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Manifest payload is missing: $path" }
        $file = Get-Item -LiteralPath $path
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($file.Length -ne [long]$entry.size -or $hash -ne [string]$entry.sha256) {
            throw "Manifest payload mismatch: $path"
        }
    }
}

if ($Action -eq 'Install') {
    Assert-ManifestPayload -Root $bundle
    if (Test-Path -LiteralPath $installRoot) {
        throw "Install directory already exists; remove it explicitly first: $installRoot"
    }
    New-Item -ItemType Directory -Path $installRoot | Out-Null
    try {
        foreach ($entry in $manifest.probes) {
            $source = Join-Path $bundle ([string]$entry.file).Replace('/', '\')
            Copy-Item -LiteralPath $source -Destination (Join-Path $installRoot ([System.IO.Path]::GetFileName($source)))
        }
        Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $installRoot 'manifest.json')
        Assert-ManifestPayload -Root $installRoot -Installed
    } catch {
        if (Test-Path -LiteralPath $installRoot) { Remove-Item -LiteralPath $installRoot -Recurse -Force }
        throw
    }
    Write-Output "Installed verified oracle bundle: $installRoot"
} else {
    if (-not (Test-Path -LiteralPath $installRoot -PathType Container)) {
        Write-Output "Oracle bundle is not installed: $installRoot"
        exit 0
    }
    $installedManifestPath = Join-Path $installRoot 'manifest.json'
    if (-not (Test-Path -LiteralPath $installedManifestPath -PathType Leaf)) {
        throw "Refusing removal without installed manifest: $installedManifestPath"
    }
    $expectedManifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash
    $installedManifestHash = (Get-FileHash -LiteralPath $installedManifestPath -Algorithm SHA256).Hash
    if ($expectedManifestHash -ne $installedManifestHash) {
        throw 'Refusing removal because the installed manifest differs from the supplied bundle.'
    }
    Assert-ManifestPayload -Root $installRoot -Installed
    Remove-Item -LiteralPath $installRoot -Recurse -Force
    Write-Output "Removed verified oracle bundle: $installRoot"
}
