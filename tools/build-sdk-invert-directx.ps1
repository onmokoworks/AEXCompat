param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$BoostInclude = "C:\Program Files\Autodesk\MotionBuilder 2024\OpenRealitySDK\include",
    [string]$OpenClSdk = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2",
    [string]$VisualStudio = "",
    [string]$Dxc = "",
    [string]$OutputDirectory = "",
    [string]$EvidencePath = ""
)

$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$AfterEffectsSdk = Join-Path $AfterEffectsSdk "Examples"
$repository = Split-Path -Parent $PSScriptRoot
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $repository "target\sdk-fixtures\invert-procamp-directx" }
if (-not $EvidencePath) { $EvidencePath = Join-Path $repository "analysis\SDK_DIRECTX_FIXTURE_BUILD_RESULT_2026-07-16.json" }
$build = Join-Path $repository "target\sdk-fixtures\invert-procamp-directx-build"
$effect = Join-Path $AfterEffectsSdk "Effect\SDK_Invert_ProcAmp"
$output = Join-Path $OutputDirectory "SDK_Invert_ProcAmp_DirectX.aex"
$assets = Join-Path $OutputDirectory "DirectX_Assets"

function Find-NewestFile([string[]]$patterns) {
    $matches = foreach ($pattern in $patterns) { Get-Item $pattern -ErrorAction SilentlyContinue }
    $file = $matches | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
    if (-not $file) { throw "Could not find required tool: $($patterns -join ', ')" }
    return $file.FullName
}

if (-not $VisualStudio) {
    $vcvars = Find-NewestFile @(
        "C:\Program Files\Microsoft Visual Studio\*\*\VC\Auxiliary\Build\vcvars64.bat",
        "C:\Program Files (x86)\Microsoft Visual Studio\*\*\VC\Auxiliary\Build\vcvars64.bat"
    )
} else {
    $vcvars = Join-Path $VisualStudio "VC\Auxiliary\Build\vcvars64.bat"
}
if (-not $Dxc) {
    $Dxc = Find-NewestFile @("C:\Program Files (x86)\Windows Kits\10\bin\*\x64\dxc.exe")
}

$kernelSource = Join-Path $effect "SDK_Invert_ProcAmp_Kernel.chlsl"
$parseHlsl = Join-Path $AfterEffectsSdk "GPUUtils\ParseHLSL.py"
$createCString = Join-Path $AfterEffectsSdk "GPUUtils\CreateCString.py"
$openClInclude = Join-Path $OpenClSdk "include"
$openClLibrary = Join-Path $OpenClSdk "lib\x64\OpenCL.lib"
$required = @(
    $vcvars, $Dxc, $kernelSource, $parseHlsl, $createCString,
    (Join-Path $BoostInclude "boost\preprocessor\list\for_each.hpp"),
    (Join-Path $openClInclude "CL\cl.h"), $openClLibrary
)
foreach ($path in $required) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Required DirectX fixture build input is missing: $path" }
}

New-Item -ItemType Directory -Force $build, $OutputDirectory, $assets, (Split-Path -Parent $EvidencePath) | Out-Null

function Invoke-DeveloperCommand([string]$command) {
    & cmd.exe /d /s /c "call `"$vcvars`" >nul && $command"
    if ($LASTEXITCODE -ne 0) { throw "DirectX fixture build command failed with exit code $LASTEXITCODE" }
}

$preprocessed = Join-Path $build "SDK_Invert_ProcAmp_Kernel.i"
Invoke-DeveloperCommand "cl /nologo /TP /P /DGF_DEVICE_TARGET_HLSL=1 /I`"$BoostInclude`" /I`"$AfterEffectsSdk\GPUUtils`" /Fi`"$preprocessed`" `"$kernelSource`""

foreach ($entryPoint in @("ProcAmp2Kernel", "InvertColorKernel")) {
    & python $parseHlsl -i $preprocessed -o $build -e $entryPoint
    if ($LASTEXITCODE -ne 0) { throw "ParseHLSL failed for $entryPoint" }
    & $Dxc (Join-Path $build "$entryPoint.hlsl") -E main -Fo (Join-Path $assets "$entryPoint.cso") -Frs (Join-Path $assets "$entryPoint.rs") -T cs_6_5 -enable-16bit-types -ignore-line-directives
    if ($LASTEXITCODE -ne 0) { throw "DXC failed for $entryPoint" }
}

$openClPreprocessed = Join-Path $build "SDK_Invert_ProcAmp_Kernel_OpenCL.i"
$openClHeader = Join-Path $build "SDK_Invert_ProcAmp_Kernel.cl.h"
Invoke-DeveloperCommand "cl /nologo /TP /P /DGF_DEVICE_TARGET_OPENCL=1 /I`"$BoostInclude`" /I`"$AfterEffectsSdk\GPUUtils`" /Fi`"$openClPreprocessed`" `"$effect\SDK_Invert_ProcAmp_Kernel.cl`""
& python $createCString -i $openClPreprocessed -o $openClHeader --name kSDK_Invert_ProcAmp_Kernel_OpenCLString
if ($LASTEXITCODE -ne 0) { throw "OpenCL kernel string generation failed" }

$includes = @(
    $build, "$AfterEffectsSdk\Headers", "$AfterEffectsSdk\Headers\SP",
    "$AfterEffectsSdk\Headers\Win", "$AfterEffectsSdk\Resources",
    "$AfterEffectsSdk\Util", "$AfterEffectsSdk\GPUUtils", $openClInclude
) | ForEach-Object { "/I`"$_`"" }
$includeFlags = $includes -join " "
$definitions = "/DMSWindows /DWIN32 /D_WINDOWS /DHAS_CUDA=0 /DHAS_HLSL=1"

Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions $includeFlags /Fo`"$build\effect.obj`" `"$effect\SDK_Invert_ProcAmp.cpp`""
Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions $includeFlags /Fo`"$build\directx.obj`" `"$AfterEffectsSdk\Util\DirectXUtils.cpp`""
Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions $includeFlags /Fo`"$build\smart.obj`" `"$AfterEffectsSdk\Util\Smart_Utils.cpp`""
Invoke-DeveloperCommand "rc /nologo /fo `"$build\pip.res`" `"$effect\Win\SDK_Invert_ProcAmpPiPL.rc`""
Invoke-DeveloperCommand "link /nologo /DLL /OUT:`"$output`" `"$build\effect.obj`" `"$build\directx.obj`" `"$build\smart.obj`" `"$build\pip.res`" /LIBPATH:`"$OpenClSdk\lib\x64`" OpenCL.lib d3d11.lib d3dcompiler.lib dxguid.lib"

$inputPaths = @($kernelSource, (Join-Path $effect "SDK_Invert_ProcAmp_Kernel.cu"), (Join-Path $effect "SDK_Invert_ProcAmp.cpp"), $parseHlsl)
$artifactPaths = @(
    $output,
    (Join-Path $assets "InvertColorKernel.cso"),
    (Join-Path $assets "InvertColorKernel.rs"),
    (Join-Path $assets "ProcAmp2Kernel.cso"),
    (Join-Path $assets "ProcAmp2Kernel.rs")
)
$evidence = [ordered]@{
    schema_version = 1
    generated_at = (Get-Date).ToUniversalTime().ToString("o")
    status = "built"
    sdk_root = $AfterEffectsSdk
    configuration = [ordered]@{ architecture = "x64"; shader_profile = "cs_6_5"; has_hlsl = 1; has_cuda = 0 }
    tools = [ordered]@{ vcvars64 = $vcvars; dxc = $Dxc; dxc_version = (Get-Item $Dxc).VersionInfo.FileVersion; python = (& python --version 2>&1 | Out-String).Trim() }
    inputs = @($inputPaths | ForEach-Object { $item = Get-Item $_; [ordered]@{ path = $item.FullName; size = $item.Length; sha256 = (Get-FileHash $_ -Algorithm SHA256).Hash.ToLowerInvariant() } })
    artifacts = @($artifactPaths | ForEach-Object { $item = Get-Item $_; [ordered]@{ path = $item.FullName; size = $item.Length; sha256 = (Get-FileHash $_ -Algorithm SHA256).Hash.ToLowerInvariant() } })
}
$evidence | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $EvidencePath -Encoding utf8
$evidence
