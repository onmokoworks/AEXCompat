[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Spec,
    [Parameter(Mandatory = $true)][string]$OffsetMap,
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)][string]$ModulePath,
    [Parameter(Mandatory = $true)][string]$PluginLabel,
    [Parameter(Mandatory = $true)][string[]]$SessionArgs,
    [string]$Harness = "broker/target/release/aexcompat-harness.exe",
    [int]$TimeoutSeconds = 30
)

# Gated launcher for known-function observation. This is a reverse-engineering /
# observation path, not evidence generation: the output uses host_kind
# native_observation and must never be promoted into an AE-equivalence corpus.
# See docs/KNOWN_FUNCTION_OBSERVATION_2026-07-19.md.
#
# It spawns the supported session harness command under Frida (Frida owns the
# PID). Deleted one-shot worker render verbs are rejected as a structured blocker;
# this launcher never treats them as a successful observation path.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

$harnessAbsolute = Join-Path $root ($Harness -replace '/', '\')
if (-not (Test-Path -LiteralPath $harnessAbsolute -PathType Leaf)) {
    throw "Session harness is missing: $Harness. Build it first (see docs/COMPATIBILITY_STATUS) before observation."
}

# Frida is an observation-only dependency, intentionally absent from the
# uv-managed dev dependencies so the machine-portable suite does not require it.
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
    "--module-path", $ModulePath,
    "--plugin-label", $PluginLabel,
    "--harness", $Harness,
    "--timeout-seconds", $TimeoutSeconds,
    "--"
) + $SessionArgs

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
