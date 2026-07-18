param(
    [Parameter(Mandatory = $true)][string]$TestedAex,
    [string]$Harness = 'broker\target\release\aexcompat-harness.exe',
    [string]$CorpusRoot = 'target\oracle-deep16',
    [string]$OutJson = 'analysis\NTSC_RS_ORACLE_DEEP16_RESULT_2026-07-19.json'
)

# Regenerates the ntsc-rs full-precision 16 bpc oracle evidence document from
# the executed capture/render/comparison artifacts under $CorpusRoot
# (issue #53, hardened per the PR #56 review in issue #61). Evidence
# documents in analysis/ are refresh-script territory
# (docs/EVIDENCE_POLICY_2026-07-18.md section 5.2); this script only reads
# artifacts that the recorded commands produced and never serializes absolute
# paths or raw image contents. Every judgment value is recomputed here: the
# pixel comparisons are re-executed and required to equal the stored
# comparison JSONs, the host renders (including the smart-input world dump)
# are re-executed and required byte-identical, and the promotion/export
# mechanism manifest is recomputed by tools/verify-deep16-mechanism.py.

$ErrorActionPreference = 'Stop'

function Sha256([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function ReadJson([string]$Path) {
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function DecodedRgbaSha256([string]$PngPath) {
    # The PNG container bytes depend on the local zlib, so the portable input
    # identity is the decoded RGBA hash from ae_png_depth_inspect.
    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("aexcompat-refresh-" + [guid]::NewGuid().ToString('N') + '.json')
    try {
        & python (Join-Path $PSScriptRoot 'ae_png_depth_inspect.py') --png $PngPath --out $scratch | Out-Null
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $scratch)) {
            throw "ae_png_depth_inspect.py failed for $PngPath"
        }
        ([string](ReadJson $scratch).decoded_rgba_sha256).ToLowerInvariant()
    } finally {
        Remove-Item -LiteralPath $scratch -ErrorAction SilentlyContinue
    }
}

$corpus = (Resolve-Path -LiteralPath $CorpusRoot).Path
$aexPath = (Resolve-Path -LiteralPath $TestedAex).Path
$harnessPath = (Resolve-Path -LiteralPath $Harness).Path
$repoRoot = Split-Path -Parent $PSScriptRoot
$expectedMatchName = 'ntsc-rs'

# Fail-closed identity validation for one capture result (issue #61): the
# capture runner records the installed plug-in hash before and after the AE
# session plus a match-name collision scan over the plug-in search roots.
# A result without those fields, or with a hash that differs from the tested
# AEX, is refused instead of being copied into evidence.
function Assert-CaptureIdentity([object]$Capture, [string]$CaseName, [string]$TestedHash) {
    foreach ($field in 'installed_aex_sha256_before', 'installed_aex_sha256_after', 'match_name_scan') {
        if ($null -eq $Capture.$field) {
            throw "capture result for $CaseName lacks the AE-loaded-module identity field '$field' (re-capture with the current runner)"
        }
    }
    if ([string]$Capture.installed_aex_sha256_before -ne $TestedHash -or
        [string]$Capture.installed_aex_sha256_after -ne $TestedHash) {
        throw "capture result for $CaseName does not pin the installed plug-in to the tested AEX for the whole AE session"
    }
    $scan = $Capture.match_name_scan
    if ([string]$scan.effect_name -ne $expectedMatchName -or
        -not [bool]$scan.ae_plugins_root_scanned -or
        [int]$scan.other_files_containing_effect_name -ne 0 -or
        -not [bool]$scan.installed_contains_effect_name) {
        throw "capture result for $CaseName does not establish a collision-free match-name scan for '$expectedMatchName'"
    }
}

# Re-run one pixel comparison with the frozen arguments and require the
# regenerated report to deep-equal the stored comparison JSON (issue #61):
# without this, a stored comparison with only its hash fields rewritten
# could smuggle arbitrary judgment values into the evidence.
function Assert-ComparisonReproduces([string]$HostRaw, [string]$AePng, [int]$Width, [int]$Height, [string]$StoredPath, [string]$CaseName) {
    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("aexcompat-refresh-" + [guid]::NewGuid().ToString('N') + '.json')
    try {
        & python (Join-Path $PSScriptRoot 'compare-pixel-oracles.py') `
            --raw $HostRaw --render $AePng --width $Width --height $Height `
            --raw-format rgba16le --raw-integer-max 32768 --tolerance 0.000125 |
            Set-Content -LiteralPath $scratch -Encoding utf8
        if ($LASTEXITCODE -ne 0) {
            throw "re-running compare-pixel-oracles.py failed for $CaseName"
        }
        & python -c "import json, sys; a = json.load(open(sys.argv[1], encoding='utf-8-sig')); b = json.load(open(sys.argv[2], encoding='utf-8-sig')); sys.exit(0 if a == b else 1)" $scratch $StoredPath
        if ($LASTEXITCODE -ne 0) {
            throw "stored comparison for $CaseName does not equal the recomputed comparison report"
        }
    } finally {
        Remove-Item -LiteralPath $scratch -ErrorAction SilentlyContinue
    }
}

$cases = @(
    [ordered]@{
        name = 'gradient-fps24'
        input_file = 'input-gradient-1920x1080.png'
        input_provenance = 'pre-existing 1920x1080 gradient oracle input (docs/AE_ORACLE_NTSC_RS_CAPTURE_2026-07-18.md)'
        width = 1920; height = 1080
        host_prefix = 'host-gradient'
        ae_prefix = 'ae-gradient-fps24'
        expected_fps = 24
    },
    [ordered]@{
        name = 'gradient-fps1'
        input_file = 'input-gradient-1920x1080.png'
        input_provenance = 'pre-existing 1920x1080 gradient oracle input (docs/AE_ORACLE_NTSC_RS_CAPTURE_2026-07-18.md)'
        width = 1920; height = 1080
        host_prefix = 'host-gradient'
        ae_prefix = 'ae-gradient-fps1'
        expected_fps = 1
    },
    [ordered]@{
        name = 'generated-fps24'
        input_file = 'input-generated-1920x1080.png'
        input_provenance = 'tools/generate-oracle-rgba-input.py --width 1920 --height 1080 --alpha-mode opaque'
        width = 1920; height = 1080
        host_prefix = 'host-generated'
        ae_prefix = 'ae-generated-fps24'
        expected_fps = 24
    }
)

$aeVersion = $null
$rerendered = @{}
$caseRecords = foreach ($case in $cases) {
    $inputPath = Join-Path $corpus $case.input_file
    $hostPng = Join-Path $corpus ("{0}.png" -f $case.host_prefix)
    $hostRaw = Join-Path $corpus ("{0}.rgba16le" -f $case.host_prefix)
    $aePng = Join-Path $corpus ("{0}.png" -f $case.ae_prefix)
    $aeResult = Join-Path $corpus ("{0}.result.json" -f $case.ae_prefix)
    $comparePath = Join-Path $corpus ("compare-{0}.json" -f $case.name)
    foreach ($required in @($inputPath, $hostPng, $hostRaw, $aePng, $aeResult, $comparePath)) {
        if (-not (Test-Path -LiteralPath $required)) {
            throw "missing deep16 oracle artifact: $required"
        }
    }

    $capture = ReadJson $aeResult
    if ($capture.status -ne 'captured') {
        throw "capture result for $($case.name) is not 'captured'"
    }
    # Bind the AE capture to the recorded input and plug-in: the capture
    # runner hashes both before launch and records them in the result, so a
    # replaced input image next to stale capture artifacts fails here.
    if ([string]$capture.input_sha256 -ne (Sha256 $inputPath)) {
        throw "capture result for $($case.name) does not record this input image (input_sha256 mismatch or missing)"
    }
    if ([string]$capture.tested_aex_sha256 -ne (Sha256 $aexPath)) {
        throw "capture result for $($case.name) does not record the tested AEX"
    }
    if ([int]$capture.width -ne $case.width -or [int]$capture.height -ne $case.height) {
        throw "capture dimensions for $($case.name) do not match the case definition"
    }
    if ([int]$capture.bpc -ne 16) {
        throw "capture for $($case.name) is not a 16 bpc capture"
    }
    if ([int]$capture.fps -ne $case.expected_fps) {
        throw "capture fps for $($case.name) does not match the case definition"
    }
    if (-not [bool]$capture.effect_applied -or
        [string]$capture.effect_match_name -ne $expectedMatchName) {
        throw "capture for $($case.name) did not resolve the effect match name '$expectedMatchName'"
    }
    Assert-CaptureIdentity $capture $case.name (Sha256 $aexPath)
    if ($null -eq $aeVersion) { $aeVersion = [string]$capture.ae_version }
    elseif ($aeVersion -ne [string]$capture.ae_version) {
        throw 'captures span more than one After Effects version'
    }

    $comparison = ReadJson $comparePath
    if ($comparison.hashes.raw_sha256 -ne (Sha256 $hostRaw)) {
        throw "comparison for $($case.name) was not produced from $($case.host_prefix).rgba16le"
    }
    if ($comparison.hashes.render_sha256 -ne (Sha256 $aePng)) {
        throw "comparison for $($case.name) was not produced from $($case.ae_prefix).png"
    }
    Assert-ComparisonReproduces $hostRaw $aePng $case.width $case.height $comparePath $case.name
    # Bind the host render to the recorded input: re-execute the recorded
    # render command against this input and require byte-identical deep
    # outputs (both the RGBA16 PNG and the rgba16le transport sidecar; the
    # render is deterministic, ntsc-rs seeds its noise from the frame number).
    if (-not $rerendered.ContainsKey($case.host_prefix)) {
        $scratchBase = Join-Path ([System.IO.Path]::GetTempPath()) ("aexcompat-refresh-" + [guid]::NewGuid().ToString('N'))
        $rerenderPng = "$scratchBase.png"
        $rerenderRaw = "$scratchBase.rgba16le"
        # The world-dump directory must sit under <repository>/target and
        # start empty (broker constraint); dumping during the re-render binds
        # the stored smart-input world snapshot to this input as well.
        $dumpDir = Join-Path $repoRoot ("target\refresh-dumps-" + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $dumpDir | Out-Null
        try {
            $env:AEXCOMPAT_DUMP_WORLDS_DIR = $dumpDir
            & $harnessPath --render-experimental-smart-16-deep $aexPath $inputPath $rerenderPng | Out-Null
            if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $rerenderPng) -or
                -not (Test-Path -LiteralPath $rerenderRaw)) {
                throw "host re-render failed for $($case.name)"
            }
            if ((Sha256 $rerenderPng) -ne (Sha256 $hostPng)) {
                throw "$($case.host_prefix).png is not the deep render of the recorded input (re-render differs)"
            }
            if ((Sha256 $rerenderRaw) -ne (Sha256 $hostRaw)) {
                throw "$($case.host_prefix).rgba16le is not the transport sidecar of the recorded render (re-render differs)"
            }
            $storedDump = Join-Path $corpus ("{0}-smart-input.rgba16le" -f $case.host_prefix)
            if (Test-Path -LiteralPath $storedDump) {
                $freshDump = Join-Path $dumpDir ("000-smart-input-{0}x{1}.rgba16le" -f $case.width, $case.height)
                if (-not (Test-Path -LiteralPath $freshDump)) {
                    throw "re-render for $($case.name) produced no smart-input world snapshot"
                }
                if ((Sha256 $freshDump) -ne (Sha256 $storedDump)) {
                    throw "$($case.host_prefix)-smart-input.rgba16le is not the smart-input world of the recorded render (re-dump differs)"
                }
            }
        } finally {
            Remove-Item "Env:AEXCOMPAT_DUMP_WORLDS_DIR" -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $rerenderPng -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $rerenderRaw -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $dumpDir -Recurse -Force -ErrorAction SilentlyContinue
        }
        $rerendered[$case.host_prefix] = $true
    }

    [ordered]@{
        name = $case.name
        input = [ordered]@{
            file = "target/oracle-deep16/$($case.input_file)"
            sha256 = Sha256 $inputPath
            decoded_rgba_sha256 = DecodedRgbaSha256 $inputPath
            width = $case.width
            height = $case.height
            provenance = $case.input_provenance
        }
        host = [ordered]@{
            render_flag = '--render-experimental-smart-16-deep'
            output_png16_sha256 = Sha256 $hostPng
            output_raw_rgba16le_sha256 = Sha256 $hostRaw
        }
        ae_capture = [ordered]@{
            output_png_sha256 = Sha256 $aePng
            frame = [int]$capture.frame
            fps = [int]$capture.fps
            bpc = [int]$capture.bpc
            color_pinned = [bool]$capture.color_pinned
            working_space = [string]$capture.working_space
            effect_match_name = [string]$capture.effect_match_name
            installed_aex_pinned_for_session = $true
            match_name_scan = [ordered]@{
                ae_plugins_root_scanned = [bool]$capture.match_name_scan.ae_plugins_root_scanned
                aex_files_scanned = [int]$capture.match_name_scan.aex_files_scanned
                other_files_containing_effect_name = [int]$capture.match_name_scan.other_files_containing_effect_name
                installed_contains_effect_name = [bool]$capture.match_name_scan.installed_contains_effect_name
            }
        }
        comparison = $comparison
    }
}

# Promotion/export mechanism manifest (issue #61): recomputed from the
# stored smart-input world snapshot and the no-effect 16 bpc control
# capture, so the mechanism claims in the capture note stay auditable from
# a clean clone. The no-effect capture goes through the same fail-closed
# identity checks as the effect captures (minus the effect resolution).
$noeffectPng = Join-Path $corpus 'ae-noeffect-16.png'
$noeffectResult = Join-Path $corpus 'ae-noeffect-16.result.json'
$gradientInput = Join-Path $corpus 'input-gradient-1920x1080.png'
$gradientDump = Join-Path $corpus 'host-gradient-smart-input.rgba16le'
foreach ($required in @($noeffectPng, $noeffectResult, $gradientDump)) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "missing mechanism artifact: $required"
    }
}
$noeffectCapture = ReadJson $noeffectResult
if ($noeffectCapture.status -ne 'captured' -or [bool]$noeffectCapture.effect_applied) {
    throw 'no-effect capture result is not a captured no-effect control'
}
if ([string]$noeffectCapture.input_sha256 -ne (Sha256 $gradientInput)) {
    throw 'no-effect capture does not record the gradient input image'
}
if ([int]$noeffectCapture.bpc -ne 16) {
    throw 'no-effect capture is not a 16 bpc capture'
}
Assert-CaptureIdentity $noeffectCapture 'noeffect-control' (Sha256 $aexPath)
if ($aeVersion -ne [string]$noeffectCapture.ae_version) {
    throw 'no-effect capture is from a different After Effects version'
}
$mechanismScratch = Join-Path ([System.IO.Path]::GetTempPath()) ("aexcompat-refresh-" + [guid]::NewGuid().ToString('N') + '.json')
try {
    & python (Join-Path $PSScriptRoot 'verify-deep16-mechanism.py') `
        --input-png $gradientInput --smart-input-dump $gradientDump `
        --noeffect-png $noeffectPng --out $mechanismScratch | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'verify-deep16-mechanism.py refuted a mechanism claim; refusing to write evidence'
    }
    $mechanism = ReadJson $mechanismScratch
} finally {
    Remove-Item -LiteralPath $mechanismScratch -ErrorAction SilentlyContinue
}
$mechanismRecord = [ordered]@{
    verified_by = 'tools/verify-deep16-mechanism.py'
    artifacts = [ordered]@{
        smart_input_dump = 'target/oracle-deep16/host-gradient-smart-input.rgba16le'
        noeffect_capture_png = 'target/oracle-deep16/ae-noeffect-16.png'
        noeffect_capture_fps = [int]$noeffectCapture.fps
    }
    manifest = $mechanism
}

# The two gradient captures differ only in comp fps (24 vs 1, one-frame
# duration semantics aside); record whether AE produced byte-identical
# renders, which refutes any fps dependence of the frame-0 oracle.
$fps24 = $caseRecords | Where-Object { $_.name -eq 'gradient-fps24' }
$fps1 = $caseRecords | Where-Object { $_.name -eq 'gradient-fps1' }
$fpsInvariant = $fps24.ae_capture.output_png_sha256 -eq $fps1.ae_capture.output_png_sha256
if (-not $fpsInvariant) {
    throw 'AE gradient captures at fps 24 and fps 1 are not byte-identical; the fps-invariance record would be false'
}

$document = [ordered]@{
    schema_version = 1
    title = 'ntsc-rs AE oracle: full-precision 16 bpc comparison via deep transport (issue #53)'
    generated_by = 'tools/refresh-ntsc-rs-oracle-deep16-evidence.ps1'
    environment = [ordered]@{
        ae_version = $aeVersion
        plugin_file = Split-Path -Leaf $aexPath
        plugin_sha256 = Sha256 $aexPath
        host_render_path = 'smartfx'
        host_pixel_format = 'argb16'
        transport = 'rgba16le sidecar, AE range (white = 32768)'
    }
    comparison_tool = 'tools/compare-pixel-oracles.py'
    tolerance = 0.000125
    tolerance_meaning = 'accepts up to 4 AE 16-bpc transport codes (4/32768 ~= 1.22e-4), ~32x below one 8-bit LSB; observed residue stays within 2 codes and is explained by 8-to-16 input promotion rounding plus the downward AE PNG16 export quantization'
    ae_fps_invariance = [ordered]@{
        observed = $fpsInvariant
        meaning = 'AE 16 bpc captures of the gradient input at comp fps 24 and fps 1 (one-frame duration) are byte-identical'
    }
    mechanism = $mechanismRecord
    cases = @($caseRecords)
}

$outPath = [System.IO.Path]::GetFullPath($OutJson)
$document | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $outPath -Encoding utf8
Write-Output "wrote $OutJson with $($caseRecords.Count) cases"
