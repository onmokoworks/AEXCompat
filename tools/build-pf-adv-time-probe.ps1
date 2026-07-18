param([string]$AfterEffectsSdk="C:\Program Files\Adobe\AfterEffectsSDK",
      [string]$Generator="Visual Studio 18 2026",[string]$Architecture="x64",
      [string]$CMake="C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe")
$ErrorActionPreference="Stop";$repo=Split-Path -Parent $PSScriptRoot
$src=Join-Path $repo "instruments\pf-adv-time-probe";$build=Join-Path $repo "target\pf-adv-time-probe-build"
$env:AE_SDK_ROOT=$AfterEffectsSdk
$cache=Join-Path $build "CMakeCache.txt"
if(Test-Path -LiteralPath $cache){
  & $CMake -S $src -B $build
}else{
  & $CMake -S $src -B $build -G $Generator -A $Architecture
}
if($LASTEXITCODE){throw "configure failed"}
& $CMake --build $build --config Release --target pf_adv_time_probe;if($LASTEXITCODE){throw "build failed"}
$artifact=Join-Path $build "Release\pf_adv_time_probe.aex";Get-Item $artifact
Get-FileHash -Algorithm SHA256 $artifact
