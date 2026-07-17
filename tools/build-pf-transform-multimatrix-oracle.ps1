param([string]$AfterEffectsSdk="C:\Program Files\Adobe\AfterEffectsSDK",
      [string]$Generator="Visual Studio 18 2026",[string]$Architecture="x64",
      [string]$CMake="C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe")
$ErrorActionPreference="Stop";$repo=Split-Path -Parent $PSScriptRoot
$src=Join-Path $repo "instruments\pf-transform-multimatrix-oracle"
$build=Join-Path $repo "target\pf-transform-multimatrix-oracle-build"
$env:AE_SDK_ROOT=$AfterEffectsSdk
& $CMake -S $src -B $build -G $Generator -A $Architecture;if($LASTEXITCODE){throw "configure failed"}
& $CMake --build $build --config Release --target pf_transform_multimatrix_oracle --clean-first;if($LASTEXITCODE){throw "build failed"}
Get-Item (Join-Path $build "Release\pf_transform_multimatrix_oracle.aex")
