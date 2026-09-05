# SDK-independent Windows validation on an ephemeral GitLab-hosted VM.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Program failed with exit code $LASTEXITCODE"
    }
}

Set-Location (Join-Path $PSScriptRoot '../..')
Invoke-Checked choco @('install', 'rustup.install', 'uv', 'powershell-core', 'ninja', '-y', '--no-progress')
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
    [Environment]::GetEnvironmentVariable('Path', 'User') + ';' + "$env:USERPROFILE\.cargo\bin"
Invoke-Checked rustup @('show')
Invoke-Checked uv @('sync', '--locked')
$files = @(git ls-files '*.rs')
if ($LASTEXITCODE -ne 0 -or $files.Count -eq 0) { throw 'Could not enumerate Rust sources' }
Invoke-Checked rustfmt (@('--edition', '2024', '--check', '--config', 'skip_children=true') + $files)
Invoke-Checked pwsh @('-NoProfile', '-File', 'tools/build-native.ps1')
Invoke-Checked pwsh @('-NoProfile', '-File', 'tools/verify-minihost-build-deps.ps1')
Invoke-Checked pwsh @('-NoProfile', '-File', 'tools/build-native.ps1', '-Source', 'instruments', '-Target', 'trace_writer_selftest')
Invoke-Checked cargo @('build', '--manifest-path', 'broker/Cargo.toml', '--workspace', '--locked')
Invoke-Checked cargo @('test', '--manifest-path', 'broker/Cargo.toml', '--workspace', '--locked')
$env:PYTEST_ADDOPTS = '--junitxml=gitlab-test-results.xml'
Invoke-Checked uv @('run', '--locked', 'python', 'tools/run-python-ci-tests.py', 'main', '--output', 'gitlab-python-tests.log')
