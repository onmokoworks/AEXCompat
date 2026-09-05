# SDK-independent Windows validation on an ephemeral GitLab-hosted VM.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    $timer = [Diagnostics.Stopwatch]::StartNew()
    try {
        & $Program @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Program failed with exit code $LASTEXITCODE"
        }
    } finally {
        $timer.Stop()
        @{ command = $Program; arguments = $Arguments; seconds = $timer.Elapsed.TotalSeconds } |
            ConvertTo-Json -Compress | Out-File gitlab-timings.jsonl -Append -Encoding utf8
    }
}

Set-Location (Join-Path $PSScriptRoot '../..')
# Use job-local paths; archiving the toolchain was slower than installation.
if (-not $env:CARGO_HOME -or -not $env:RUSTUP_HOME) { throw 'CI cache paths are required' }
$env:Path = "$env:CARGO_HOME\bin;$env:Path"
if (-not (Test-Path "$env:CARGO_HOME\bin\rustup.exe")) {
    # rustup dispatches by executable basename; retain rustup-init.exe.
    $installerDir = Join-Path $env:TEMP ('aexcompat-rustup-' + [guid]::NewGuid())
    [void](New-Item -ItemType Directory -Path $installerDir)
    $installer = Join-Path $installerDir 'rustup-init.exe'
    $url = 'https://static.rust-lang.org/rustup/archive/1.27.1/x86_64-pc-windows-msvc/rustup-init.exe'
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $installer
    $expected = '193d6c727e18734edbf7303180657e96e9d5a08432002b4e6c5bbe77c60cb3e8'
    if ((Get-FileHash $installer -Algorithm SHA256).Hash -ne $expected) { throw 'rustup checksum mismatch' }
    Invoke-Checked $installer @('-y', '--no-modify-path', '--profile', 'minimal', '--default-toolchain', 'none')
    Remove-Item $installerDir -Recurse
}
Invoke-Checked rustup @('set', 'profile', 'minimal')
$packages = @()
foreach ($tool in @(
    @{ Command = 'uv'; Package = 'uv' },
    @{ Command = 'pwsh'; Package = 'powershell-core' },
    @{ Command = 'ninja'; Package = 'ninja' },
    @{ Command = 'sccache'; Package = 'sccache' }
)) {
    if (-not (Get-Command $tool.Command -CommandType Application -ErrorAction SilentlyContinue)) {
        $packages += $tool.Package
    }
}
if ($packages.Count -gt 0) {
    Invoke-Checked choco (@('install') + $packages + @('-y', '--no-progress'))
}
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
    [Environment]::GetEnvironmentVariable('Path', 'User')
$env:Path = "$env:CARGO_HOME\bin;$env:Path"
try {
    $channelMatch = Select-String -Path rust-toolchain.toml -Pattern '^channel\s*=\s*"([^"]+)"'
    if (-not $channelMatch -or @($channelMatch).Count -ne 1) { throw 'Expected one Rust toolchain channel' }
    $channel = $channelMatch.Matches[0].Groups[1].Value
    Invoke-Checked rustup @('toolchain', 'install', $channel, '--profile', 'minimal', '--component', 'rustfmt,clippy')
    $env:RUSTUP_TOOLCHAIN = $channel
    Invoke-Checked rustup @('show')
    Invoke-Checked uv @('sync', '--locked')
    $files = @(git ls-files '*.rs')
    if ($LASTEXITCODE -ne 0 -or $files.Count -eq 0) { throw 'Could not enumerate Rust sources' }
    Invoke-Checked rustfmt (@('--edition', '2024', '--check', '--config', 'skip_children=true') + $files)
    Invoke-Checked pwsh @('-NoProfile', '-File', 'tools/build-native.ps1', '-CompileCache', 'sccache')
    Invoke-Checked pwsh @('-NoProfile', '-File', 'tools/verify-minihost-build-deps.ps1')
    Invoke-Checked pwsh @('-NoProfile', '-File', 'tools/build-native.ps1', '-Source', 'instruments', '-Target', 'trace_writer_selftest')
    Invoke-Checked cargo @('build', '--manifest-path', 'broker/Cargo.toml', '--workspace', '--locked')
    Invoke-Checked cargo @('test', '--manifest-path', 'broker/Cargo.toml', '--workspace', '--locked')
    $env:PYTEST_ADDOPTS = '--junitxml=gitlab-test-results.xml'
    Invoke-Checked uv @('run', '--locked', 'python', 'tools/run-python-ci-tests.py', 'main', '--output', 'gitlab-python-tests.log')

} finally {
    # Keep cache hit statistics even when compilation/tests fail. Do not mask
    # the original failure if diagnostic collection itself is unavailable.
    if (Get-Command sccache -ErrorAction SilentlyContinue) {
        try {
            & sccache --show-stats 2>&1 | Out-File gitlab-sccache.log -Encoding utf8
            & sccache --stop-server 2>&1 | Out-File gitlab-sccache.log -Append -Encoding utf8
        } catch {
            Write-Warning "Could not collect sccache diagnostics: $_"
        }
    }
}
