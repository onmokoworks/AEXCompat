param(
    [string]$AfterEffectsSdk = $env:AFTER_EFFECTS_SDK_ROOT,
    [string]$Generator = "",
    [string]$Architecture = "x64",
    [string]$CMake = ""
)

$ErrorActionPreference = "Stop"
$AfterEffectsSdk = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $AfterEffectsSdk
$repository = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repository "instruments\pf-wgpu-dx12-probe"
$runtimeManifest = Join-Path $source "runtime\Cargo.toml"
$runtimeTarget = Join-Path $repository "target\pf-wgpu-dx12-probe-runtime"
$build = Join-Path $repository "target\pf-wgpu-dx12-probe-build"
$release = Join-Path $build "Release"

$Generator = & "$PSScriptRoot\resolve-cmake-generator.ps1" $Generator
$CMake = & "$PSScriptRoot\resolve-build-cmake.ps1" $CMake $Generator
$env:AE_SDK_ROOT = $AfterEffectsSdk

& cargo build --manifest-path $runtimeManifest --target-dir $runtimeTarget --release --locked
if ($LASTEXITCODE -ne 0) { throw "wgpu DX12 runtime Release build failed" }

& $CMake -S $source -B $build -G $Generator -A $Architecture
if ($LASTEXITCODE -ne 0) { throw "PF wgpu DX12 probe configure failed" }
& $CMake --build $build --config Release --target pf_wgpu_dx12_probe
if ($LASTEXITCODE -ne 0) { throw "PF wgpu DX12 probe Release build failed" }

$aex = Join-Path $release "pf_wgpu_dx12_probe.aex"
$runtimeSource = Join-Path $runtimeTarget "release\aexcompat_wgpu_dx12_runtime.dll"
$runtime = Join-Path $release "aexcompat_wgpu_dx12_runtime.dll"
if (-not (Test-Path -LiteralPath $aex -PathType Leaf)) {
    throw "PF wgpu DX12 AEX was not produced: $aex"
}
if (-not (Test-Path -LiteralPath $runtimeSource -PathType Leaf)) {
    throw "wgpu DX12 runtime DLL was not produced: $runtimeSource"
}
Copy-Item -LiteralPath $runtimeSource -Destination $runtime -Force

function Get-Identity([string]$Path) {
    $item = Get-Item -LiteralPath $Path
    $rootPrefix = $repository.TrimEnd("\") + "\"
    if (-not $item.FullName.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Identity path is outside the repository: $($item.FullName)"
    }
    $relative = $item.FullName.Substring($rootPrefix.Length).Replace("\", "/")
    [ordered]@{
        path = $relative
        size = $item.Length
        sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

$manifest = [ordered]@{
    schema_version = 1
    probe = "aexcompat.pf-wgpu-dx12-compute"
    backend = "dx12"
    wgpu_version = "0.19.4"
    configuration = "Release"
    cargo_locked = $true
    artifacts = [ordered]@{
        aex = Get-Identity $aex
        runtime = Get-Identity $runtime
    }
    sources = [ordered]@{
        aex_cpp = Get-Identity (Join-Path $source "pf_wgpu_dx12_probe.cpp")
        runtime_cargo_toml = Get-Identity (Join-Path $source "runtime\Cargo.toml")
        runtime_rust = Get-Identity (Join-Path $source "runtime\src\lib.rs")
        cargo_lock = Get-Identity (Join-Path $source "runtime\Cargo.lock")
    }
    build_commands = @(
        "cargo build --manifest-path instruments/pf-wgpu-dx12-probe/runtime/Cargo.toml --target-dir target/pf-wgpu-dx12-probe-runtime --release --locked",
        "cmake -S instruments/pf-wgpu-dx12-probe -B target/pf-wgpu-dx12-probe-build -G <resolved-msvc-generator> -A x64",
        "cmake --build target/pf-wgpu-dx12-probe-build --config Release --target pf_wgpu_dx12_probe"
    )
}
$manifestPath = Join-Path $release "artifacts.json"
$json = $manifest | ConvertTo-Json -Depth 8
[IO.File]::WriteAllText($manifestPath, $json + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))

[pscustomobject]@{
    manifest = $manifestPath
    aex = $manifest.artifacts.aex
    runtime = $manifest.artifacts.runtime
    wgpu_version = $manifest.wgpu_version
}
