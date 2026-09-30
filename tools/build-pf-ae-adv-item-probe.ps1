param([string]$AfterEffectsSdk=$env:AFTER_EFFECTS_SDK_ROOT,
      [string]$Generator="",[string]$Architecture="x64",
      [string]$CMake="")
$ErrorActionPreference="Stop";$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$ConfigureArgs = & "$PSScriptRoot\resolve-cmake-configure-args.ps1" $Generator $Architecture
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$repo=Split-Path -Parent $PSScriptRoot
$src=Join-Path $repo "instruments\pf-ae-adv-item-probe";$build=Join-Path $repo "target\pf-ae-adv-item-probe-build"
$env:AE_SDK_ROOT=$AfterEffectsSdk
& $CMake -S $src -B $build -G $Generator @ConfigureArgs;if($LASTEXITCODE){throw "configure failed"}
& $CMake --build $build --config Release --target pf_ae_adv_item_probe;if($LASTEXITCODE){throw "build failed"}
$artifact=Join-Path $build "Release\pf_ae_adv_item_probe.aex";Get-Item $artifact
Get-FileHash -Algorithm SHA256 $artifact
