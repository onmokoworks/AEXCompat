<#
.SYNOPSIS
Reports where an AEX's runtime modules come from once it is loaded from a staged
folder (issue #304, L2 diagnosis).

.DESCRIPTION
The worker's sealed-tree module audit fails closed when a module loads from
anywhere other than the sealed root, System32, or an authorized policy path, but
it only reports how many such modules there were, never which. This script
reproduces the worker's load conditions outside the worker: it loads the staged
plug-in with the same search flags
(LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) and diffs the
process module list before and after, classifying each newly loaded module the
way `minihost/src/runtime_module_audit.cpp` does.

Stage the plug-in and its dependency closure first:

  cargo run --release --manifest-path bridges/aviutl2-multifilter/Cargo.toml `
    --example stage_closure -- "<effect.aex>" "<AE Support Files>" "<stage-dir>"

Then point this script at the staged copy it printed. The plug-in runs in this
PowerShell process, so use it for diagnosis on a machine you are willing to
crash, never as part of a gate.

.PARAMETER StagedAex
The staged plug-in path printed by `stage_closure`.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$StagedAex
)

$ErrorActionPreference = 'Stop'

$resolved = (Resolve-Path -LiteralPath $StagedAex).Path
$stageDir = Split-Path -Parent $resolved
$system32 = [Environment]::GetFolderPath('System')

Add-Type -Namespace AexCompat -Name Loader -MemberDefinition @'
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern bool SetDefaultDllDirectories(uint directoryFlags);
'@

# Mirrors worker_runtime_admission.cpp.
$LOAD_LIBRARY_SEARCH_SYSTEM32 = 0x800
$LOAD_LIBRARY_SEARCH_USER_DIRS = 0x400
$LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR = 0x100

[void][AexCompat.Loader]::SetDefaultDllDirectories($LOAD_LIBRARY_SEARCH_SYSTEM32 -bor $LOAD_LIBRARY_SEARCH_USER_DIRS)

$before = @((Get-Process -Id $PID).Modules | ForEach-Object { $_.FileName })
$handle = [AexCompat.Loader]::LoadLibraryExW(
    $resolved, [IntPtr]::Zero,
    $LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR -bor $LOAD_LIBRARY_SEARCH_SYSTEM32)
if ($handle -eq [IntPtr]::Zero) {
    $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    Write-Output "LoadLibraryExW failed (Win32 error $code) — this is the worker's exit 11."
    exit 1
}

$after = @((Get-Process -Id $PID).Modules | ForEach-Object { $_.FileName })
$loaded = $after | Where-Object { $before -notcontains $_ }

$staged = @()
$system = @()
$unknown = @()
foreach ($module in $loaded) {
    $parent = Split-Path -Parent $module
    if ($parent -ieq $stageDir) { $staged += $module }
    elseif ($parent -ieq $system32) { $system += $module }
    else { $unknown += $module }
}

Write-Output "loaded modules: $($loaded.Count) (staged: $($staged.Count), System32: $($system.Count), unknown: $($unknown.Count))"
if ($unknown.Count -gt 0) {
    Write-Output ''
    Write-Output 'unknown modules (what the sealed-tree module audit rejects):'
    foreach ($module in $unknown) { Write-Output "  $module" }
}
