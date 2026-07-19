param(
    [Parameter(Mandatory = $true)][string]$TestedAex,
    [string]$Harness = 'broker\target\release\aexcompat-harness.exe',
    [string]$CorpusRoot = 'target\oracle-corpus',
    [string]$OutJson = 'analysis\NTSC_RS_ORACLE_CORPUS_RESULT_2026-07-19.json'
)

# Regenerates the ntsc-rs oracle-corpus evidence document from the executed
# capture/render/comparison artifacts under $CorpusRoot (issue #31). Evidence
# documents in analysis/ are refresh-script territory
# (docs/EVIDENCE_POLICY_2026-07-18.md section 5.2); this script only reads
# artifacts that the recorded commands produced and never serializes absolute
# paths or raw image contents.

$ErrorActionPreference = 'Stop'

# Repo の dev 依存 (Pillow / OpenEXR) は uv 管理の .venv にあるため、Python
# ツールは uv run 経由で起動する (CWD に依存しないよう --project で固定)。
$uvProject = Split-Path -Parent $PSScriptRoot

function Sha256([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function ReadJson([string]$Path) {
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function ConvertPngToRaw([string]$PngPath) {
    # Re-runs the converter into a scratch file and returns its report, so a
    # recorded host raw can be verified as the conversion of the recorded PNG.
    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("aexcompat-refresh-" + [guid]::NewGuid().ToString('N') + '.rgba')
    try {
        $report = & uv run --project $uvProject python (Join-Path $PSScriptRoot 'png-to-rgba-raw.py') --png $PngPath --out $scratch | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or -not $report) {
            throw "png-to-rgba-raw.py failed for $PngPath"
        }
        $report
    } finally {
        Remove-Item -LiteralPath $scratch -ErrorAction SilentlyContinue
    }
}

function DecodedRgbaSha256([string]$PngPath) {
    # The PNG container bytes depend on the local zlib, so the portable input
    # identity is the decoded RGBA hash from ae_png_depth_inspect.
    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("aexcompat-refresh-" + [guid]::NewGuid().ToString('N') + '.json')
    try {
        & uv run --project $uvProject python (Join-Path $PSScriptRoot 'ae_png_depth_inspect.py') --png $PngPath --out $scratch | Out-Null
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

$cases = @(
    [ordered]@{
        name = 'popup-use-field-both'
        axis = 'popup_parameter'
        input_file = 'input-gradient-1920x1080.png'
        input_provenance = 'pre-existing 1920x1080 gradient oracle input (docs/AE_ORACLE_NTSC_RS_CAPTURE_2026-07-18.md)'
        width = 1920; height = 1080
        host_render_flag = '--render-experimental-smart-param'
        parameter = [ordered]@{
            host_slot = 4; ae_property_name = 'Use field'
            value = 6; choice_label = 'Both'; kind = 'popup'
        }
        prefix = 'usefield-both'
    },
    [ordered]@{
        name = 'alpha-gradient-input'
        axis = 'alpha_input'
        input_file = 'input-alpha-1920x1080.png'
        input_provenance = 'tools/generate-oracle-rgba-input.py --width 1920 --height 1080 --alpha-mode vertical-gradient'
        width = 1920; height = 1080
        host_render_flag = '--render-experimental-smart'
        parameter = $null
        prefix = 'alpha'
    },
    [ordered]@{
        name = 'odd-dimensions-input'
        axis = 'odd_dimensions'
        input_file = 'input-odd-1919x1077.png'
        input_provenance = 'tools/generate-oracle-rgba-input.py --width 1919 --height 1077 --alpha-mode opaque'
        width = 1919; height = 1077
        host_render_flag = '--render-experimental-smart'
        parameter = $null
        prefix = 'odd'
    },
    [ordered]@{
        name = '4k-input'
        axis = 'resolution_4k'
        input_file = 'input-4k-3840x2160.png'
        input_provenance = 'tools/generate-oracle-rgba-input.py --width 3840 --height 2160 --alpha-mode opaque'
        width = 3840; height = 2160
        host_render_flag = '--render-experimental-smart'
        parameter = $null
        prefix = '4k'
    }
)

$aeVersion = $null
$caseRecords = foreach ($case in $cases) {
    $inputPath = Join-Path $corpus $case.input_file
    $hostPng = Join-Path $corpus ("host-{0}.png" -f $case.prefix)
    $hostRaw = Join-Path $corpus ("host-{0}.rgba" -f $case.prefix)
    $aePng = Join-Path $corpus ("ae-{0}.png" -f $case.prefix)
    $aeResult = Join-Path $corpus ("ae-{0}.result.json" -f $case.prefix)
    $comparePath = Join-Path $corpus ("compare-{0}.json" -f $case.prefix)
    foreach ($required in @($inputPath, $hostPng, $hostRaw, $aePng, $aeResult, $comparePath)) {
        if (-not (Test-Path -LiteralPath $required)) {
            throw "missing corpus artifact: $required"
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
    if ($null -ne $case.parameter) {
        if ($capture.param_name -ne $case.parameter.ae_property_name -or
            [double]$capture.param_value -ne [double]$case.parameter.value) {
            throw "capture parameter for $($case.name) does not match the case definition"
        }
    }
    if ($null -eq $aeVersion) { $aeVersion = [string]$capture.ae_version }
    elseif ($aeVersion -ne [string]$capture.ae_version) {
        throw 'captures span more than one After Effects version'
    }

    $comparison = ReadJson $comparePath
    if ($comparison.hashes.raw_sha256 -ne (Sha256 $hostRaw)) {
        throw "comparison for $($case.name) was not produced from host-$($case.prefix).rgba"
    }
    if ($comparison.hashes.render_sha256 -ne (Sha256 $aePng)) {
        throw "comparison for $($case.name) was not produced from ae-$($case.prefix).png"
    }
    # Bind the compared raw bytes to the recorded host PNG: a stale or
    # replaced host-*.png must fail here instead of being recorded next to a
    # comparison that never saw it.
    $conversion = ConvertPngToRaw $hostPng
    if ($conversion.raw_sha256 -ne $comparison.hashes.raw_sha256) {
        throw "host-$($case.prefix).rgba is not the conversion of host-$($case.prefix).png"
    }
    # Bind the host render to the recorded input: re-execute the recorded
    # render command against this input and require a byte-identical PNG
    # (ntsc-rs seeds its noise from the frame number, so the render is
    # deterministic; two byte-identical renders are already on record).
    $rerender = Join-Path ([System.IO.Path]::GetTempPath()) ("aexcompat-refresh-" + [guid]::NewGuid().ToString('N') + '.png')
    try {
        $renderArgs = @($case.host_render_flag, $aexPath, $inputPath, $rerender)
        if ($null -ne $case.parameter) {
            $renderArgs += @([string]$case.parameter.host_slot, [string]$case.parameter.value)
        }
        & $harnessPath @renderArgs | Out-Null
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $rerender)) {
            throw "host re-render failed for $($case.name)"
        }
        if ((Sha256 $rerender) -ne (Sha256 $hostPng)) {
            throw "host-$($case.prefix).png is not the render of the recorded input (re-render differs)"
        }
    } finally {
        Remove-Item -LiteralPath $rerender -ErrorAction SilentlyContinue
    }

    [ordered]@{
        name = $case.name
        axis = $case.axis
        input = [ordered]@{
            file = "target/oracle-corpus/$($case.input_file)"
            sha256 = Sha256 $inputPath
            decoded_rgba_sha256 = DecodedRgbaSha256 $inputPath
            width = $case.width
            height = $case.height
            provenance = $case.input_provenance
        }
        parameter = $case.parameter
        host = [ordered]@{
            render_flag = $case.host_render_flag
            output_png_sha256 = Sha256 $hostPng
            output_raw_rgba8_sha256 = Sha256 $hostRaw
        }
        ae_capture = [ordered]@{
            output_png_sha256 = Sha256 $aePng
            frame = [int]$capture.frame
            fps = [int]$capture.fps
            bpc = [int]$capture.bpc
            color_pinned = [bool]$capture.color_pinned
            working_space = [string]$capture.working_space
        }
        comparison = $comparison
    }
}

$document = [ordered]@{
    schema_version = 1
    title = 'ntsc-rs AE oracle corpus: popup parameter and input-shape variations (issue #31)'
    generated_by = 'tools/refresh-ntsc-rs-oracle-corpus-evidence.ps1'
    environment = [ordered]@{
        ae_version = $aeVersion
        plugin_file = Split-Path -Leaf $aexPath
        plugin_sha256 = Sha256 $aexPath
        host_render_path = 'smartfx'
        host_pixel_format = 'argb8'
    }
    comparison_tool = 'tools/compare-pixel-oracles.py'
    tolerance = 0.004
    tolerance_meaning = 'accepts +/-1 LSB at 8-bit precision (1/255 ~= 0.00392), rejects +/-2 (2/255 ~= 0.00784)'
    cases = @($caseRecords)
}

$outPath = [System.IO.Path]::GetFullPath($OutJson)
$document | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $outPath -Encoding utf8
Write-Output "wrote $OutJson with $($caseRecords.Count) cases"
