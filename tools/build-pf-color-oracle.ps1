param([string]$AfterEffectsSdk=$env:AFTER_EFFECTS_SDK_ROOT,
      [string]$Generator="Visual Studio 18 2026",[string]$Architecture="x64",
      [string]$CMake="C:\Program Files\Microsoft Visual Studio\18\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe")
$ErrorActionPreference="Stop";$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repo=Split-Path -Parent $PSScriptRoot
$src=Join-Path $repo "instruments\pf-color-oracle";$build=Join-Path $repo "target\pf-color-oracle-build"
$env:AE_SDK_ROOT=$AfterEffectsSdk
& $CMake -S $src -B $build -G $Generator -A $Architecture;if($LASTEXITCODE){throw "configure failed"}
& $CMake --build $build --config Release --target pf_color_oracle;if($LASTEXITCODE){throw "build failed"}
$artifact=Join-Path $build "Release\pf_color_oracle.aex";Get-Item $artifact
