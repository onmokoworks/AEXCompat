param([string]$AfterEffectsSdk=$env:AFTER_EFFECTS_SDK_ROOT,
      [string]$Generator="",[string]$Architecture="x64",
      [string]$CMake="")
$ErrorActionPreference="Stop";$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$repo=Split-Path -Parent $PSScriptRoot
$src=Join-Path $repo "instruments\pf-adv-time-probe";$build=Join-Path $repo "target\pf-adv-time-probe-build"
$env:AE_SDK_ROOT=$AfterEffectsSdk
$cache=Join-Path $build "CMakeCache.txt"
if(Test-Path -LiteralPath $cache){
  & $CMake -S $src -B $build
}else{
  & $CMake -S $src -B $build -G $Generator @ConfigureArgs
}
if($LASTEXITCODE){throw "configure failed"}
& $CMake --build $build --config Release --target pf_adv_time_probe;if($LASTEXITCODE){throw "build failed"}
$artifact=Join-Path $build "Release\pf_adv_time_probe.aex";Get-Item $artifact
Get-FileHash -Algorithm SHA256 $artifact
