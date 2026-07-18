[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Spec,
    [Parameter(Mandatory = $true)][string]$OffsetMap,
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)][string]$ModuleFile,
    [Parameter(Mandatory = $true)][string]$PluginLabel,
    [Parameter(Mandatory = $true)][string[]]$RenderArgs,
    [string]$Worker = "target/minihost-build/aex_render_worker.exe",
    [int]$TimeoutSeconds = 30
)

# Gated launcher for known-function observation. This is a reverse-engineering /
# observation path, not evidence generation: the output uses host_kind
# native_observation and must never be promoted into an AE-equivalence corpus.
# See docs/KNOWN_FUNCTION_OBSERVATION_2026-07-19.md.
#
# It spawns the worker's own receipt-free --render-image CLI under Frida (Frida
# owns the PID), so no broker core path is touched. The render runs the worker,
# not After Effects, so AfterFX/aerender are not required and not contended.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$workerAbsolute = Join-Path $root ($Worker -replace '/', '\')
if (-not (Test-Path -LiteralPath $workerAbsolute -PathType Leaf)) {
    throw "Worker build is missing: $Worker. Build it first (see docs/COMPATIBILITY_STATUS) before observation."
}

# Frida is an observation-only dependency, intentionally absent from
# requirements-dev.txt so the machine-portable suite does not require it.
& python -c "import frida" 2>$null
if ($LASTEXITCODE -ne 0) {
    throw "frida is not importable in this Python. Install it in the observation environment (pip install frida)."
}

$launcher = Join-Path $PSScriptRoot "observe_known_functions.py"
$arguments = @(
    $launcher,
    "--spec", $Spec,
    "--offset-map", $OffsetMap,
    "--out", $Out,
    "--module-file", $ModuleFile,
    "--plugin-label", $PluginLabel,
    "--worker", $Worker,
    "--timeout-seconds", $TimeoutSeconds,
    "--"
) + $RenderArgs

Push-Location $root
try {
    & python @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Observation launcher failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}

Write-Output "OBSERVED $Out"
