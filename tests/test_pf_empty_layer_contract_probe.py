"""Pixel-level optional-layer SmartFX callback contract (#1679).

The probe encodes 32 big-endian u32 words as one black/white pixel per bit.
Words: 0 magic, 1 schema, 2 count, 3 PreRender map error,
4:8 result rect, 8:12 max rect, 12 SmartRender map error,
13 world present, 14 data present, 15:17 width/height,
17 rowbytes, 18:22 extent, 22 map-format error,
23:27 first map pixel ARGB (8-bit normalized), 27 map format,
28 output-format error, 29 output format, 30 primary data present,
31 output data present. Missing values are 0xffffffff.
"""

import json
import subprocess
import struct
import sys
from pathlib import Path

from PIL import Image
import pytest

from _render_session import HARNESS, assert_artifact_fresh


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from aex_pipl_identity import analyze_aex_file

PROBE_SOURCE = ROOT / "instruments/pf-empty-layer-contract-probe/pf_empty_layer_contract_probe.cpp"
PROBE_RESOURCE = PROBE_SOURCE.with_suffix(".rc")
PROBE = ROOT / "target/pf-empty-layer-contract-probe-build/Release/pf_empty_layer_contract_probe.aex"
WORKER = ROOT / "target/minihost-build/aex_worker.exe"


def output_samples(path: Path, depth: int) -> list[tuple]:
    if depth == 8:
        with Image.open(path) as image:
            assert image.size == (64, 32)
            return list(image.convert("RGBA").getdata())
    suffix, fmt = {16: (".rgba16le", "<4H"), 32: (".rgba32f-le", "<4I")}[depth]
    raw = path.with_suffix(suffix).read_bytes()
    assert len(raw) == 64 * 32 * (depth // 2)
    return list(struct.iter_unpack(fmt, raw))


def decode_contract(pixels: list[tuple], depth: int) -> list[int]:
    white = {8: 255, 16: 32768, 32: 0x3F800000}[depth]
    for red, green, blue, alpha in pixels:
        assert alpha == white and red == green == blue and red in (0, white)
    words = []
    for index in range(32):
        value = 0
        for bit in range(32):
            position = index * 32 + bit
            value = (value << 1) | (pixels[position][0] == white)
        words.append(value)
    assert words[:3] == [0x554C5031, 1, 32]
    return words


def render(plugin: Path, input_image: Path, output: Path, layer: Path | None, depth: int):
    request = output.with_suffix(".request.json")
    request.write_text(json.dumps({"schema_version": 1, "assignments":
        [{"slot": 1, "layer": str(layer)}] if layer else []}), encoding="utf-8")
    args = [str(HARNESS), "--headless"]
    args.append("--render-experimental-smart-request" + {8: "", 16: "-16", 32: "-32-cpu"}[depth])
    args += [str(plugin), str(input_image), str(output), str(request)]
    result = subprocess.run(
        args, cwd=ROOT, text=True, encoding="utf-8", errors="replace",
        capture_output=True, timeout=60,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    report = json.loads(result.stdout)
    assert report["passed"] is True, report
    assert report["suite_leases_balanced"] is True, report
    assert report["world_lifetimes_balanced"] is True, report
    assert report["output_pixels_valid"] is True, report
    assert report["depth_provenance"]["dispatch_pixel_bytes"] == depth // 2
    # AE chooses the render route from the compiled PiPL. A successful native
    # SmartFX dispatch alone misses a PiPL that advertises only Classic render.
    identity = analyze_aex_file(plugin)
    assert identity["classification"] == "effect", identity
    assert len(identity["pipl_records"]) == 1
    pipl = identity["pipl_records"][0]
    advertised = report["depth_provenance"]
    assert int(pipl["global_out_flags"], 16) == advertised["advertised_out_flags"]
    assert int(pipl["global_out_flags_2"], 16) == advertised["advertised_out_flags2"]
    assert int(pipl["global_out_flags_2"], 16) & (1 << 10)  # SUPPORTS_SMART_RENDER
    assert pipl["version"]["raw"] == 524288  # PF_VERSION(1, 0, 0, DEVELOP, 0)
    assert output.is_file() and output.stat().st_size > 0
    pixels = output_samples(output, depth)
    return decode_contract(pixels, depth), pixels


@pytest.mark.parametrize("depth", [8, 16, 32])
def test_unassigned_and_assigned_smart_layer_match_ae_cpu_oracle(tmp_path, depth):
    assert WORKER.is_file() and HARNESS.is_file()
    assert_artifact_fresh(PROBE, PROBE_SOURCE, PROBE_RESOURCE, WORKER, HARNESS)
    source = tmp_path / "primary.png"
    secondary = tmp_path / "secondary.png"
    Image.new("RGBA", (64, 32), (25, 43, 61, 255)).save(source)
    Image.new("RGBA", (64, 32), (137, 71, 223, 255)).save(secondary)

    # Independently captured AE 25.1 CPU/Software at time 0, no color
    # management. See docs/EMPTY_LAYER_AE_ORACLE.md for identities and limits.
    fmt = {8: 0x62677261, 16: 0x36316561, 32: 0x32336561}[depth]
    missing = 0xFFFFFFFF
    expected_absent = ([0x554C5031, 1, 32, 0] + [0] * 8 + [0, 0, 0] +
                       [missing] * 7 + [516] + [missing] * 4 + [0, 0, fmt, 1, 1])
    expected_assigned = ([0x554C5031, 1, 32, 0] + [0, 0, 64, 32] * 2 +
                         [0, 1, 1, 64, 32, 64 * (depth // 2), 0, 0, 64, 32, 0] +
                         [255, 137, 71, 223, fmt, 0, fmt, 1, 1])
    white = {8: 255, 16: 32768, 32: 0x3F800000}[depth]
    for name, layer, expected in [("absent", None, expected_absent),
                                   ("assigned", secondary, expected_assigned)]:
        words, pixels = render(PROBE, source, tmp_path / (name + ".png"), layer, depth)
        assert words == expected
        # All channel samples, including alpha and the unused lower half, are
        # checked as native words. No deep-to-preview quantization or tolerance.
        expected_pixels = []
        for index in range(64 * 32):
            bit = index < 1024 and (expected[index // 32] & (1 << (31 - index % 32)))
            color = white if bit else 0
            expected_pixels.append((color, color, color, white))
        assert pixels == expected_pixels
