param([string]$AfterEffectsSdk=$env:AFTER_EFFECTS_SDK_ROOT,
      [string]$Generator="",[string]$Architecture="x64",
      [string]$CMake="")
$ErrorActionPreference="Stop";$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$repo=Split-Path -Parent $PSScriptRoot
$src=Join-Path $repo "instruments\pf-transform-multimatrix-oracle"
$build=Join-Path $repo "target\pf-transform-multimatrix-oracle-build"
$env:AE_SDK_ROOT=$AfterEffectsSdk
& $CMake -S $src -B $build -G $Generator -A $Architecture;if($LASTEXITCODE){throw "configure failed"}
& $CMake --build $build --config Release --target pf_transform_multimatrix_oracle --clean-first;if($LASTEXITCODE){throw "build failed"}
Get-Item (Join-Path $build "Release\pf_transform_multimatrix_oracle.aex")
