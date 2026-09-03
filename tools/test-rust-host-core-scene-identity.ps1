param(
    [string]$BuildDirectory = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $PSScriptRoot "..")
)
if ([string]::IsNullOrWhiteSpace($BuildDirectory)) {
    $BuildDirectory = Join-Path $repoRoot "target\issue628-native"
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
$installation = (
    & $vswhere -latest -products "*" `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath -utf8
) -join ""
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($installation)) {
    throw "No Visual Studio installation with the x64 C++ toolset was found"
}
$vsdev = Join-Path $installation.Trim() `
    "Common7\Tools\VsDevCmd.bat"
if (-not (Test-Path -LiteralPath $vsdev -PathType Leaf)) {
    throw "VsDevCmd.bat is unavailable: $vsdev"
}

# CI passes the producer-built DLL via AEXCOMPAT_HOST_CORE_FFI_DLL so the
# four gate scripts do not serialize on the same target dir's cargo file
# lock (#1537). Local runs leave it unset and build as before.
$rustDll = $env:AEXCOMPAT_HOST_CORE_FFI_DLL
if ([string]::IsNullOrWhiteSpace($rustDll)) {
    $manifest = Join-Path $repoRoot "broker\Cargo.toml"
    & cargo build --manifest-path $manifest `
        --package aexcompat-host-core-ffi --release --quiet
    if ($LASTEXITCODE -ne 0) {
        throw "Release Rust host-core FFI build failed"
    }
    $rustDll = Join-Path $repoRoot `
        "broker\target\release\aexcompat_host_core_ffi.dll"
    if (-not (Test-Path -LiteralPath $rustDll -PathType Leaf)) {
        throw "Release Rust host-core FFI DLL was not produced: $rustDll"
    }
} else {
    # The gate exe resolves the path against its own working directory,
    # which is $buildRoot by the time it runs, so anchor a relative value to
    # the caller's directory before the existence check agrees with it.
    $rustDll = $rustDll.Trim()
    if (-not [System.IO.Path]::IsPathRooted($rustDll)) {
        $rustDll = Join-Path (Get-Location).Path $rustDll
    }
    if (-not (Test-Path -LiteralPath $rustDll -PathType Leaf)) {
        throw "AEXCOMPAT_HOST_CORE_FFI_DLL does not name a file: $rustDll"
    }
}

$abiInclude = Join-Path $repoRoot "broker\crates\broker\include"
$minihostInclude = Join-Path $repoRoot "minihost\src"
$nativeSource = Join-Path $repoRoot `
    "tests\native\rust_host_core_scene_identity_dual_run_selftest.cpp"
$oracleSource = Join-Path $repoRoot `
    "minihost\src\worker_aegp_scene_model.cpp"
$executable = Join-Path $buildRoot `
    "rust_host_core_scene_identity_dual_run_selftest.exe"
# sccache cannot cache a cl invocation that also links (those pass
# through), so compile each translation unit separately and link after.
# AEXCOMPAT_COMPILE_CACHE=sccache wraps only the /c compiles (#1533).
# Keep /Fo outside the colon-quoted form: sccache mis-parses /Fo:"..."
# as a relative path and fails to zip the outputs.
$compileDriver = 'cl.exe'
if ($env:AEXCOMPAT_COMPILE_CACHE -eq 'sccache') {
    $compileDriver = 'sccache cl.exe'
}
$compileFlags = (
    '/nologo /std:c++17 /O2 /DNDEBUG /EHsc /W4 /WX ' +
    '/DUNICODE /D_UNICODE /DWIN32_LEAN_AND_MEAN /DNOMINMAX ' +
    '/I"' + $abiInclude + '" /I"' + $minihostInclude + '"'
)
$nativeObject = Join-Path $buildRoot `
    ([IO.Path]::GetFileNameWithoutExtension($nativeSource) + '.obj')
$oracleObject = Join-Path $buildRoot `
    ([IO.Path]::GetFileNameWithoutExtension($oracleSource) + '.obj')
$compileCommand = (
    'call "' + $vsdev + '" -arch=x64 -host_arch=x64 >nul && ' +
    $compileDriver + ' ' + $compileFlags +
    ' /c "' + $nativeSource + '" /Fo"' + $nativeObject + '" && ' +
    $compileDriver + ' ' + $compileFlags +
    ' /c "' + $oracleSource + '" /Fo"' + $oracleObject + '" && ' +
    'cl.exe /nologo "' + $nativeObject + '" "' + $oracleObject + '" ' +
    '/Fe:"' + $executable + '"'
)

Push-Location $buildRoot
try {
    & $env:ComSpec /d /s /c $compileCommand
    if ($LASTEXITCODE -ne 0) {
        throw "MSVC scene identity dual-run build failed"
    }
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw "Native scene identity dual-run executable was not produced"
    }
    $nativeOutput = @(& $executable $rustDll 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "Native scene identity dual-run failed: $($nativeOutput -join ' ')"
    }
    $report = ($nativeOutput -join "`n") | ConvertFrom-Json
    if ($report.rust_host_core_scene_identity_dual_run -ne "passed" -or
        $report.cpp_registry -ne $true -or
        $report.balanced -ne $true) {
        throw "Native scene identity dual-run returned an invalid report"
    }
    $report | ConvertTo-Json -Compress
}
finally {
    Pop-Location
}
