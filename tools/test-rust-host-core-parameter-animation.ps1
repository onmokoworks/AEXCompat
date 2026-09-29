param([string]$BuildDirectory = "")

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
if ([string]::IsNullOrWhiteSpace($BuildDirectory)) {
    $BuildDirectory = Join-Path $repoRoot "target\issue636-native"
}
$buildRoot = [System.IO.Path]::GetFullPath($BuildDirectory)
New-Item -ItemType Directory -Force -Path $buildRoot | Out-Null

$programFilesX86 = ${env:ProgramFiles(x86)}
if ([string]::IsNullOrWhiteSpace($programFilesX86)) {
    $programFilesX86 = "C:\Program Files (x86)"
}
$vswhere = Join-Path $programFilesX86 `
    "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
    throw "vswhere.exe is unavailable: $vswhere"
}
$installation = (& $vswhere -latest -products "*" `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -property installationPath -utf8) -join ""
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($installation)) {
    throw "No Visual Studio installation with the x64 C++ toolset was found"
}
$vsdev = Join-Path $installation.Trim() "Common7\Tools\VsDevCmd.bat"
if (-not (Test-Path -LiteralPath $vsdev -PathType Leaf)) {
    throw "VsDevCmd.bat is unavailable: $vsdev"
}

$rustDll = $env:AEXCOMPAT_HOST_CORE_FFI_DLL
if ([string]::IsNullOrWhiteSpace($rustDll)) {
    & cargo build --manifest-path (Join-Path $repoRoot "broker\Cargo.toml") `
        --package aexcompat-host-core-ffi --release --quiet
    if ($LASTEXITCODE -ne 0) { throw "Release Rust host-core FFI build failed" }
    $rustDll = Join-Path $repoRoot `
        "broker\target\release\aexcompat_host_core_ffi.dll"
} elseif (-not [System.IO.Path]::IsPathRooted($rustDll)) {
    $rustDll = Join-Path (Get-Location).Path $rustDll.Trim()
}
if (-not (Test-Path -LiteralPath $rustDll -PathType Leaf)) {
    throw "Rust host-core FFI DLL is unavailable: $rustDll"
}

$abiInclude = Join-Path $repoRoot "broker\crates\broker\include"
$workerInclude = Join-Path $repoRoot "minihost\src"
$sources = @(
    (Join-Path $repoRoot `
        "tests\native\rust_host_core_parameter_animation_dual_run_selftest.cpp"),
    (Join-Path $workerInclude "parameter_animation_transport.cpp"),
    (Join-Path $workerInclude "strict_json.cpp")
)
$executable = Join-Path $buildRoot `
    "rust_host_core_parameter_animation_dual_run_selftest.exe"
$compileDriver = "cl.exe"
if ($env:AEXCOMPAT_COMPILE_CACHE -eq "sccache") {
    $compileDriver = "sccache cl.exe"
}
$flags = '/nologo /std:c++17 /O2 /DNDEBUG /EHsc /W4 /WX ' +
    '/DUNICODE /D_UNICODE /DWIN32_LEAN_AND_MEAN /DNOMINMAX ' +
    '/I"' + $abiInclude + '" /I"' + $workerInclude + '"'
$command = 'call "' + $vsdev + '" -arch=x64 -host_arch=x64 >nul'
$objects = @()
foreach ($source in $sources) {
    $object = Join-Path $buildRoot `
        ([IO.Path]::GetFileNameWithoutExtension($source) + '.obj')
    $command += ' && ' + $compileDriver + ' ' + $flags +
        ' /c "' + $source + '" /Fo"' + $object + '"'
    $objects += $object
}
$command += ' && cl.exe /nologo ' +
    (($objects | ForEach-Object { '"' + $_ + '"' }) -join ' ') +
    ' /Fe:"' + $executable + '"'

Push-Location $buildRoot
try {
    & $env:ComSpec /d /s /c $command
    if ($LASTEXITCODE -ne 0) {
        throw "MSVC parameter-animation dual-run build failed"
    }
    $nativeOutput = @(& $executable $rustDll 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "Parameter-animation dual-run failed: " +
            ($nativeOutput -join " ")
    }
    $report = ($nativeOutput -join "`n") | ConvertFrom-Json
    if ($report.rust_host_core_parameter_animation_dual_run -ne "passed" -or
        $report.checks -lt 100) {
        throw "Parameter-animation dual-run returned an invalid report"
    }
    $report | ConvertTo-Json -Compress
}
finally {
    Pop-Location
}
