[CmdletBinding()]
param(
    [string]$SdkRoot = $env:AFTER_EFFECTS_SDK_ROOT,
    [switch]$SkipBuild,
    [string]$CrashKit = 'target\classic-failure-probes-build\pf-crashkit\Release\pf_crashkit.aex',
    [string]$InputWriteDenied = 'target\classic-failure-probes-build\pf-input-write-probe\Release\pf_input_write_denied_probe.aex',
    [string]$Harness = '',
    [string]$CrashEvidence = 'analysis\PF_CRASHKIT_UI_ISOLATION_RESULT_2026-07-15.json',
    [string]$InputWriteEvidence = 'analysis\PF_INPUT_BUFFER_WRITE_RESULT_2026-07-15.json',
    [string]$CrashEvidenceOut = $CrashEvidence,
    [string]$InputWriteEvidenceOut = $InputWriteEvidence
)

# Replays the three classic-selector failures whose frozen records predate the
# classic_render stage marker. The committed JSON remains provenance rather
# than a regression test: this runner verifies the live RenderSession result
# before changing only those three failure_stage fields.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
if (-not $Harness) {
    $profile = if ($SkipBuild -and $env:AEXCOMPAT_CARGO_PROFILE) {
        $env:AEXCOMPAT_CARGO_PROFILE
    } else {
        'release'
    }
    $Harness = "broker\target\$profile\aexcompat-harness.exe"
}

function Invoke-Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE"
    }
}

if (-not $SkipBuild) {
    $SdkRoot = & "$PSScriptRoot\resolve-after-effects-sdk.ps1" $SdkRoot
    & "$PSScriptRoot\build-classic-failure-probes.ps1" -AfterEffectsSdk $SdkRoot
    if ($LASTEXITCODE -ne 0) { throw 'classic failure probe build failed' }
    $vsRoot = & "$PSScriptRoot\resolve-msvc-tools.ps1" ''
    $vcvars = Join-Path $vsRoot 'VC\Auxiliary\Build\vcvars64.bat'
    cmd.exe /d /c "`"$vcvars`" >nul && set" | ForEach-Object {
        if ($_ -match '^([^=]+)=(.*)$') {
            Set-Item -LiteralPath "Env:$($matches[1])" -Value $matches[2]
        }
    }
    # One worker binary now serves the discovery and classic routes this
    # evidence exercises (issue #1495): a single `aex_worker` target replaces
    # the two link targets this used to name.
    & (Join-Path $PSScriptRoot 'build-native.ps1') -Target aex_worker
    if ($LASTEXITCODE -ne 0) { throw 'aex_worker build failed' }
    Invoke-Checked cargo @(
        'build', '--manifest-path', (Join-Path $root 'broker\Cargo.toml'),
        '-p', 'aexcompat-harness', '--release'
    )
}

$harnessPath = (Resolve-Path -LiteralPath $Harness).Path
$crashKitPath = (Resolve-Path -LiteralPath $CrashKit).Path
$inputWriteDeniedPath = (Resolve-Path -LiteralPath $InputWriteDenied).Path
$scratch = Join-Path $root ("target\classic-failure-evidence-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null

function Invoke-Harness([string[]]$Arguments) {
    function ConvertTo-WindowsCommandLineArgument([string]$Argument) {
        if ($Argument.Length -gt 0 -and $Argument -notmatch '[\s"]') {
            return $Argument
        }
        $quoted = [Text.StringBuilder]::new()
        $null = $quoted.Append('"')
        $slashes = 0
        foreach ($character in $Argument.ToCharArray()) {
            if ($character -eq '\') {
                $slashes++
            } elseif ($character -eq '"') {
                $null = $quoted.Append(('\' * ($slashes * 2 + 1))).Append('"')
                $slashes = 0
            } else {
                if ($slashes) { $null = $quoted.Append(('\' * $slashes)) }
                $null = $quoted.Append($character)
                $slashes = 0
            }
        }
        if ($slashes) { $null = $quoted.Append(('\' * ($slashes * 2))) }
        $null = $quoted.Append('"')
        $quoted.ToString()
    }

    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $harnessPath
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.Arguments = (($Arguments | ForEach-Object {
        ConvertTo-WindowsCommandLineArgument $_
    }) -join ' ')
    $process = [Diagnostics.Process]::Start($start)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    $outerTimeout = $false
    if (-not $process.WaitForExit(45000)) {
        $outerTimeout = $true
        $taskkill = Join-Path $env:SystemRoot 'System32\taskkill.exe'
        $taskkillOutput = & $taskkill /PID $process.Id /T /F 2>&1
        $taskkillExit = $LASTEXITCODE
        if (-not $process.WaitForExit(5000)) {
            throw "harness tree did not exit within 5s after taskkill exit $taskkillExit`: $taskkillOutput"
        }
    }
    if (-not $stdout.Wait(5000) -or -not $stderr.Wait(5000)) {
        throw 'harness redirected output did not close within the 5s collection bound'
    }
    $result = [ordered]@{
        exit_code = $process.ExitCode
        stdout = $stdout.GetAwaiter().GetResult()
        stderr = $stderr.GetAwaiter().GetResult()
    }
    if ($outerTimeout) { throw 'harness exceeded the outer 45s evidence bound' }
    $result
}

function Extract-JsonAfter([string]$Text, [string]$Marker, [string]$Delimiter) {
    $start = $Text.IndexOf($Marker, [StringComparison]::Ordinal)
    if ($start -lt 0) { throw "missing $Marker in harness failure" }
    $start += $Marker.Length
    $end = $Text.IndexOf($Delimiter, $start, [StringComparison]::Ordinal)
    if ($end -lt 0) { throw "missing $Delimiter after $Marker" }
    ($Text.Substring($start, $end - $start) | ConvertFrom-Json)
}

function Assert-ClassicFailure($Run, [string]$Case, [switch]$Deadline) {
    if ($Run.exit_code -ne 1 -or $Run.stdout) {
        throw "$Case did not fail through the headless harness contract"
    }
    $delimiter = if ($Deadline) { '). Diagnose' } else { ', report=' }
    $diagnostics = Extract-JsonAfter $Run.stderr 'diagnostics=' $delimiter
    if ($diagnostics.failure_stage -ne 'classic_render' -or
        $diagnostics.first_failure_stage -ne 'classic_render') {
        throw "$Case was not attributed to classic_render"
    }
    if ($Deadline) {
        if ($diagnostics.active_stage -ne 'classic_render' -or
            $diagnostics.last_completed_stage -ne 'frame_setup' -or
            $diagnostics.exit_code -ne 57005) {
            throw 'hang did not retain the bounded active classic selector state'
        }
    } else {
        $classicEnd = @($diagnostics.stage_events | Where-Object {
            $_.stage -eq 'classic_render' -and $_.state -eq 'end'
        })
        if ($classicEnd.Count -ne 1 -or $classicEnd[0].errors.error -ne 512) {
            throw "$Case did not contain the SEH-converted classic selector failure"
        }
    }
    $diagnostics
}

function Replace-One([string]$Text, [string]$Pattern, [string]$Description) {
    $expression = [regex]::new($Pattern, [Text.RegularExpressions.RegexOptions]::Singleline)
    if ($expression.Matches($Text).Count -ne 1) {
        throw "$Description did not identify exactly one evidence field"
    }
    $expression.Replace($Text, '${1}classic_render${2}', 1)
}

function Resolve-OutputPath([string]$Path) {
    if ([IO.Path]::IsPathRooted($Path)) { return $Path }
    Join-Path $root $Path
}

try {
    Add-Type -AssemblyName System.Drawing
    $input = Join-Path $scratch 'input.png'
    $bitmap = [Drawing.Bitmap]::new(16, 12, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        for ($y = 0; $y -lt 12; $y++) {
            for ($x = 0; $x -lt 16; $x++) {
                $bitmap.SetPixel($x, $y, [Drawing.Color]::FromArgb(
                    255, ($x * 17) % 256, ($y * 23) % 256, (($x + $y) * 11) % 256))
            }
        }
        $bitmap.Save($input, [Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $bitmap.Dispose()
    }

    $common = @($input, '', 'argb8', 'classic', '0', '1', '1')
    $crashArgs = @('--render-experimental-session-param', $crashKitPath) + $common + @('1', '2')
    $crashArgs[3] = Join-Path $scratch 'crash.png'
    $crash = Invoke-Harness $crashArgs
    $crashDiagnostics = Assert-ClassicFailure $crash 'crash'

    $hangArgs = @('--render-experimental-session-param', $crashKitPath) + $common + @('1', '3')
    $hangArgs[3] = Join-Path $scratch 'hang.png'
    $hang = Invoke-Harness $hangArgs
    $hangDiagnostics = Assert-ClassicFailure $hang 'hang' -Deadline

    $deniedArgs = @('--render-experimental-session', $inputWriteDeniedPath) + $common
    $deniedArgs[3] = Join-Path $scratch 'input-write-denied.png'
    $denied = Invoke-Harness $deniedArgs
    $deniedDiagnostics = Assert-ClassicFailure $denied 'unadvertised input write'

    $crashText = Get-Content -LiteralPath $CrashEvidence -Raw
    $null = $crashText | ConvertFrom-Json
    $crashText = Replace-One $crashText `
        '("mode"\s*:\s*"crash"(?:(?!"mode"\s*:).)*?"failure_stage"\s*:\s*")(?:render|classic_render)(")' `
        'crash case'
    $crashText = Replace-One $crashText `
        '("mode"\s*:\s*"hang"(?:(?!"mode"\s*:).)*?"failure_stage"\s*:\s*")(?:render|classic_render)(")' `
        'hang case'
    $inputText = Get-Content -LiteralPath $InputWriteEvidence -Raw
    $null = $inputText | ConvertFrom-Json
    $inputText = Replace-One $inputText `
        '("unadvertised_write"\s*:\s*\{.*?"failure_stage"\s*:\s*")(?:render|classic_render)(")' `
        'unadvertised input-write case'

    [IO.File]::WriteAllText((Resolve-OutputPath $CrashEvidenceOut),
        $crashText, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Resolve-OutputPath $InputWriteEvidenceOut),
        $inputText, [Text.UTF8Encoding]::new($false))

    [ordered]@{
        crash_failure_stage = $crashDiagnostics.failure_stage
        hang_failure_stage = $hangDiagnostics.failure_stage
        input_write_failure_stage = $deniedDiagnostics.failure_stage
        hang_exit_code = $hangDiagnostics.exit_code
    } | ConvertTo-Json -Compress
} finally {
    if (Test-Path -LiteralPath $scratch) {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
}
