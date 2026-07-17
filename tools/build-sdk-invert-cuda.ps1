param(
    [string]$AfterEffectsSdk = "C:\Program Files\Adobe\AfterEffectsSDK\Examples",
    [string]$CudaRoot = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2",
    [string]$BoostInclude = "C:\Program Files\Autodesk\MotionBuilder 2024\OpenRealitySDK\include",
    [string]$VisualStudio = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools",
    [string]$Architecture = "sm_75"
)

$ErrorActionPreference = "Stop"
$repository = Split-Path -Parent $PSScriptRoot
$build = Join-Path $repository "target\sdk-fixtures\invert-procamp-cuda-build"
$output = Join-Path $repository "target\sdk-fixtures\invert-procamp\SDK_Invert_ProcAmp_CUDA.aex"
$effect = Join-Path $AfterEffectsSdk "Effect\SDK_Invert_ProcAmp"
$vcvars = Join-Path $VisualStudio "VC\Auxiliary\Build\vcvars64.bat"
$compiler = Get-ChildItem (Join-Path $VisualStudio "VC\Tools\MSVC") -Directory |
    Sort-Object Name -Descending |
    Select-Object -First 1
$hostCompiler = Join-Path $compiler.FullName "bin\Hostx64\x64"
$nvcc = Join-Path $CudaRoot "bin\nvcc.exe"

foreach ($required in @($vcvars, $nvcc, (Join-Path $BoostInclude "boost\preprocessor\list\for_each.hpp"))) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Required CUDA fixture build input is missing: $required"
    }
}
New-Item -ItemType Directory -Force $build | Out-Null
New-Item -ItemType Directory -Force (Split-Path -Parent $output) | Out-Null

function Invoke-DeveloperCommand([string]$command) {
    $full = "call `"$vcvars`" >nul && $command"
    & cmd.exe /d /s /c $full
    if ($LASTEXITCODE -ne 0) {
        throw "CUDA fixture build command failed with exit code $LASTEXITCODE"
    }
}

$kernel = Join-Path $build "kernel.obj"
Invoke-DeveloperCommand "`"$nvcc`" -arch=$Architecture -use_fast_math -m64 -c -ccbin `"$hostCompiler`" -Xcompiler /MD,/EHsc,/W3,/nologo -I`"$CudaRoot\include`" -I`"$AfterEffectsSdk\GPUUtils`" -I`"$BoostInclude`" -o `"$kernel`" `"$effect\SDK_Invert_ProcAmp_Kernel.cu`""

$includes = @(
    "$AfterEffectsSdk\Headers", "$AfterEffectsSdk\Headers\SP",
    "$AfterEffectsSdk\Headers\Win", "$AfterEffectsSdk\Resources",
    "$AfterEffectsSdk\Util", "$AfterEffectsSdk\GPUUtils",
    "$effect\Win\x64\Debug\PreprocessedOpenCL", "$CudaRoot\include"
) | ForEach-Object { "/I`"$_`"" }
$includeFlags = $includes -join " "
$definitions = "/DMSWindows /DWIN32 /D_WINDOWS"

Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions /DHAS_CUDA=1 $includeFlags /Fo`"$build\effect.obj`" `"$effect\SDK_Invert_ProcAmp.cpp`""
Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions $includeFlags /Fo`"$build\directx.obj`" `"$AfterEffectsSdk\Util\DirectXUtils.cpp`""
Invoke-DeveloperCommand "cl /nologo /c /std:c++17 /EHsc /MD /O2 $definitions $includeFlags /Fo`"$build\smart.obj`" `"$AfterEffectsSdk\Util\Smart_Utils.cpp`""
Invoke-DeveloperCommand "rc /nologo /fo `"$build\pip.res`" `"$effect\Win\SDK_Invert_ProcAmpPiPL.rc`""
Invoke-DeveloperCommand "link /nologo /DLL /OUT:`"$output`" `"$build\effect.obj`" `"$build\directx.obj`" `"$build\smart.obj`" `"$kernel`" `"$build\pip.res`" /LIBPATH:`"$CudaRoot\lib\x64`" cudart_static.lib OpenCL.lib d3dcompiler.lib"

$artifact = Get-Item -LiteralPath $output
$hash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
[pscustomobject]@{ path = $artifact.FullName; size = $artifact.Length; sha256 = $hash }
