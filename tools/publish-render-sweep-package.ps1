function Test-GeneratedRenderSweepPackagePath {
    param(
        [Parameter(Mandatory)]
        [string]$Path,
        [Parameter(Mandatory)]
        [string]$Parent,
        [Parameter(Mandatory)]
        [string]$OutputLeaf
    )

    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $fullParent = [System.IO.Path]::GetFullPath($Parent)
    $actualParent = [System.IO.Path]::GetDirectoryName($fullPath)
    $leaf = [System.IO.Path]::GetFileName($fullPath)
    $prefixes = @(
        ".$OutputLeaf.staging-",
        ".$OutputLeaf.previous-"
    )
    return $actualParent.Equals($fullParent, [System.StringComparison]::OrdinalIgnoreCase) -and
        ($prefixes | Where-Object {
            $leaf.StartsWith($_, [System.StringComparison]::OrdinalIgnoreCase)
        }).Count -eq 1
}

function Remove-GeneratedRenderSweepPackagePath {
    param(
        [Parameter(Mandatory)]
        [string]$Path,
        [Parameter(Mandatory)]
        [string]$Parent,
        [Parameter(Mandatory)]
        [string]$OutputLeaf
    )

    if (-not (Test-GeneratedRenderSweepPackagePath -Path $Path -Parent $Parent -OutputLeaf $OutputLeaf)) {
        throw "refusing to remove an unsafe generated package path: $Path"
    }
    Remove-Item -LiteralPath $Path -Recurse -Force
}

function Publish-RenderSweepPackage {
    <#
    Copies both executables to an isolated sibling, validates that complete
    layout, and only then replaces the requested package directory. If staging
    or validation fails, an existing package is left byte-for-byte unchanged.
    #>
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$Cli,
        [Parameter(Mandatory)]
        [string]$Worker,
        [Parameter(Mandatory)]
        [string]$OutputDirectory,
        [scriptblock]$ValidateStagedPackage,
        # Deterministic test seam for the destination-created publish race.
        # Normal callers must leave this unset.
        [scriptblock]$BeforeFinalMove
    )

    $rawOutput = [System.IO.Path]::GetFullPath($OutputDirectory)
    $root = [System.IO.Path]::GetPathRoot($rawOutput)
    if ($rawOutput.Equals($root, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "package output must not be a filesystem root: $rawOutput"
    }
    $separators = [char[]]@(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $output = $rawOutput.TrimEnd($separators)
    $parent = [System.IO.Path]::GetDirectoryName($output)
    $outputLeaf = [System.IO.Path]::GetFileName($output)
    if ([string]::IsNullOrWhiteSpace($parent) -or [string]::IsNullOrWhiteSpace($outputLeaf)) {
        throw "package output must name a directory: $output"
    }
    New-Item -ItemType Directory -Force -Path $parent | Out-Null

    $token = [guid]::NewGuid().ToString('N')
    $staging = Join-Path $parent ".$outputLeaf.staging-$token"
    $previous = Join-Path $parent ".$outputLeaf.previous-$token"
    if ((Test-Path -LiteralPath $staging) -or (Test-Path -LiteralPath $previous)) {
        throw 'generated package staging path already exists'
    }

    $publishLock = $null
    $previousMoved = $false
    try {
        $lockPath = Join-Path $parent ".$outputLeaf.publish.lock"
        try {
            $publishLock = [System.IO.FileStream]::new(
                $lockPath,
                [System.IO.FileMode]::OpenOrCreate,
                [System.IO.FileAccess]::ReadWrite,
                [System.IO.FileShare]::None,
                1,
                [System.IO.FileOptions]::DeleteOnClose
            )
        } catch {
            throw "another package publish holds $lockPath`: $($_.Exception.Message)"
        }

        $stagedWorkerDirectory = Join-Path $staging 'target\minihost-build'
        New-Item -ItemType Directory -Path $stagedWorkerDirectory | Out-Null
        Copy-Item -LiteralPath $Cli -Destination (Join-Path $staging 'aexcompat-render-sweep.exe')
        Copy-Item -LiteralPath $Worker -Destination (Join-Path $stagedWorkerDirectory 'aex_worker.exe')

        if ($null -ne $ValidateStagedPackage) {
            & $ValidateStagedPackage $staging
        }

        if (Test-Path -LiteralPath $output) {
            [System.IO.Directory]::Move($output, $previous)
            $previousMoved = $true
        }
        try {
            if ($null -ne $BeforeFinalMove) {
                & $BeforeFinalMove $output
            }
            # Directory.Move requires an absent exact destination. Move-Item
            # would silently nest staging inside a concurrently-created output.
            [System.IO.Directory]::Move($staging, $output)
        } catch {
            if ($previousMoved -and -not (Test-Path -LiteralPath $output)) {
                [System.IO.Directory]::Move($previous, $output)
                $previousMoved = $false
            }
            throw
        }

        if ($previousMoved) {
            Remove-GeneratedRenderSweepPackagePath `
                -Path $previous `
                -Parent $parent `
                -OutputLeaf $outputLeaf
            $previousMoved = $false
        }
        return $output
    } finally {
        try {
            if (Test-Path -LiteralPath $staging) {
                Remove-GeneratedRenderSweepPackagePath `
                    -Path $staging `
                    -Parent $parent `
                    -OutputLeaf $outputLeaf
            }
            if ($previousMoved -and
                (Test-Path -LiteralPath $previous) -and
                -not (Test-Path -LiteralPath $output)) {
                [System.IO.Directory]::Move($previous, $output)
            }
        } finally {
            if ($null -ne $publishLock) {
                $publishLock.Dispose()
            }
        }
    }
}
