param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$VisualStudio = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
)

$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$AfterEffectsSdk = Join-Path $AfterEffectsSdk "Examples"
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-fill-premultiply-probe"
$build = Join-Path $repository "target\pf-fill-premultiply-probe-build"
$output = Join-Path $repository "target\pf-fill-premultiply-probe\pf_fill_premultiply_probe.aex"
$vcvars = Join-Path $VisualStudio "VC\Auxiliary\Build\vcvars64.bat"

foreach ($required in @(
    $vcvars,
    (Join-Path $AfterEffectsSdk "Headers\AE_EffectCBSuites.h"),
    (Join-Path $source "pf_fill_premultiply_probe.cpp"),
    (Join-Path $source "pf_fill_premultiply_probe.rc")
)) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Required PF Fill premultiply probe build input is missing: $required"
    }
}

New-Item -ItemType Directory -Force $build | Out-Null
New-Item -ItemType Directory -Force (Split-Path -Parent $output) | Out-Null

function Invoke-DeveloperCommand([string]$command) {
    & cmd.exe /d /s /c "call `"$vcvars`" >nul && $command"
    if ($LASTEXITCODE -ne 0) {
        throw "PF Fill premultiply probe build failed with exit code $LASTEXITCODE"
    }
}

$includes = @(
    (Join-Path $AfterEffectsSdk "Headers"),
    (Join-Path $AfterEffectsSdk "Headers\SP"),
    (Join-Path $AfterEffectsSdk "Headers\Win"),
    (Join-Path $AfterEffectsSdk "Util")
) | ForEach-Object { "/I`"$_`"" }
$includeFlags = $includes -join " "
$cpp = Join-Path $source "pf_fill_premultiply_probe.cpp"
$rc = Join-Path $source "pf_fill_premultiply_probe.rc"
$object = Join-Path $build "pf_fill_premultiply_probe.obj"
$resource = Join-Path $build "pf_fill_premultiply_probe.res"

Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 /DMSWindows /DWIN32 /D_WINDOWS $includeFlags /Fo`"$object`" `"$cpp`""
Invoke-DeveloperCommand "rc /nologo /fo `"$resource`" `"$rc`""
Invoke-DeveloperCommand "link /nologo /DLL /OUT:`"$output`" `"$object`" `"$resource`""

$artifact = Get-Item -LiteralPath $output
$hash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{ path = $artifact.FullName; size = $artifact.Length; sha256 = $hash }
