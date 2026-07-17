param(
    [string]$AfterEffectsSdk = "C:\Program Files\Adobe\AfterEffectsSDK\Examples",
    [string]$OpenClSdk = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2",
    [string]$BoostInclude = "C:\Program Files\Autodesk\MotionBuilder 2024\OpenRealitySDK\include",
    [string]$VisualStudio = "C:\Program Files\Microsoft Visual Studio\18\Community"
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$build = Join-Path $repository "target\sdk-fixtures\invert-procamp-opencl-build"
$output = Join-Path $repository "target\sdk-fixtures\invert-procamp\SDK_Invert_ProcAmp_OpenCL.aex"
$effect = Join-Path $AfterEffectsSdk "Effect\SDK_Invert_ProcAmp"
$vcvars = Join-Path $VisualStudio "VC\Auxiliary\Build\vcvars64.bat"
$createCString = Join-Path $AfterEffectsSdk "GPUUtils\CreateCString.py"

foreach ($required in @(
    $vcvars,
    (Join-Path $OpenClSdk "include\CL\cl.h"),
    (Join-Path $OpenClSdk "lib\x64\OpenCL.lib"),
    (Join-Path $BoostInclude "boost\preprocessor\list\for_each.hpp"),
    $createCString
)) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Required OpenCL fixture build input is missing: $required"
    }
}
New-Item -ItemType Directory -Force $build | Out-Null
New-Item -ItemType Directory -Force (Split-Path -Parent $output) | Out-Null

function Invoke-DeveloperCommand([string]$command) {
    & cmd.exe /d /s /c "call `"$vcvars`" >nul && $command"
    if ($LASTEXITCODE -ne 0) {
        throw "OpenCL fixture build command failed with exit code $LASTEXITCODE"
    }
}

$preprocessed = Join-Path $build "SDK_Invert_ProcAmp_Kernel.i"
$kernelHeader = Join-Path $build "SDK_Invert_ProcAmp_Kernel.cl.h"
$kernelSource = Join-Path $effect "SDK_Invert_ProcAmp_Kernel.cl"
Invoke-DeveloperCommand "cl /nologo /TP /P /DGF_DEVICE_TARGET_OPENCL=1 /I`"$BoostInclude`" /I`"$AfterEffectsSdk\GPUUtils`" /Fi`"$preprocessed`" `"$kernelSource`""
& python $createCString -i $preprocessed -o $kernelHeader --name kSDK_Invert_ProcAmp_Kernel_OpenCLString
if ($LASTEXITCODE -ne 0) { throw "OpenCL kernel string generation failed" }

$includes = @(
    $build, "$AfterEffectsSdk\Headers", "$AfterEffectsSdk\Headers\SP",
    "$AfterEffectsSdk\Headers\Win", "$AfterEffectsSdk\Resources",
    "$AfterEffectsSdk\Util", "$AfterEffectsSdk\GPUUtils", "$OpenClSdk\include"
) | ForEach-Object { "/I`"$_`"" }
$includeFlags = $includes -join " "
$definitions = "/DMSWindows /DWIN32 /D_WINDOWS /DHAS_CUDA=0"

Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions $includeFlags /Fo`"$build\effect.obj`" `"$effect\SDK_Invert_ProcAmp.cpp`""
Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions $includeFlags /Fo`"$build\directx.obj`" `"$AfterEffectsSdk\Util\DirectXUtils.cpp`""
Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions $includeFlags /Fo`"$build\smart.obj`" `"$AfterEffectsSdk\Util\Smart_Utils.cpp`""
Invoke-DeveloperCommand "rc /nologo /fo `"$build\pip.res`" `"$effect\Win\SDK_Invert_ProcAmpPiPL.rc`""
Invoke-DeveloperCommand "link /nologo /DLL /OUT:`"$output`" `"$build\effect.obj`" `"$build\directx.obj`" `"$build\smart.obj`" `"$build\pip.res`" /LIBPATH:`"$OpenClSdk\lib\x64`" OpenCL.lib d3dcompiler.lib"

$artifact = Get-Item -LiteralPath $output
$hash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{ path = $artifact.FullName; size = $artifact.Length; sha256 = $hash }
