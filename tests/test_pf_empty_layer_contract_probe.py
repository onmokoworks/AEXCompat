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
import sys
from pathlib import Path

from PIL import Image

from _render_session import HARNESS, assert_artifact_fresh


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from aex_pipl_identity import analyze_aex_file

PROBE_SOURCE = ROOT / "instruments/pf-empty-layer-contract-probe/pf_empty_layer_contract_probe.cpp"
PROBE_RESOURCE = PROBE_SOURCE.with_suffix(".rc")
PROBE = ROOT / "target/pf-empty-layer-contract-probe-build/Release/pf_empty_layer_contract_probe.aex"
WORKER = ROOT / "target/minihost-build/aex_worker.exe"


def decode_contract(path: Path) -> list[int]:
    image = Image.open(path).convert("RGBA")
    assert image.width * image.height >= 32 * 32
    assert image.getpixel((0, 0))[3] == 255
    words = []
    for index in range(32):
        value = 0
        for bit in range(32):
            position = index * 32 + bit
            red, green, blue, alpha = image.getpixel(
                (position % image.width, position // image.width)
            )
            assert alpha == 255 and red == green == blue and red in (0, 255)
            value = (value << 1) | (red == 255)
        words.append(value)
    assert words[:3] == [0x554C5031, 1, 32]
    return words


def render(plugin: Path, input_image: Path, output: Path, layer: Path | None):
    args = [str(HARNESS), "--headless"]
    args.append("--render-experimental-smart-layer" if layer else "--render-experimental-smart")
    args += [str(plugin), str(input_image)]
    if layer:
        args.append(str(layer))
    args.append(str(output))
    result = subprocess.run(
        args, cwd=ROOT, text=True, encoding="utf-8", errors="replace",
        capture_output=True, timeout=60,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    report = json.loads(result.stdout)
    assert report["passed"] is True, report
    assert report["suite_leases_balanced"] is True, report
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
    return decode_contract(output)


def test_unassigned_and_assigned_smart_layer_emit_decodable_contract(tmp_path):
    assert WORKER.is_file() and HARNESS.is_file()
    assert_artifact_fresh(PROBE, PROBE_SOURCE, PROBE_RESOURCE, WORKER, HARNESS)
    source = tmp_path / "primary.png"
    secondary = tmp_path / "secondary.png"
    Image.new("RGBA", (64, 32), (25, 43, 61, 255)).save(source)
    Image.new("RGBA", (64, 32), (137, 71, 223, 255)).save(secondary)

    absent = render(PROBE, source, tmp_path / "absent.png", None)
    assigned = render(PROBE, source, tmp_path / "assigned.png", secondary)
    assert absent[30:32] == assigned[30:32] == [1, 1]
    assert absent[28] == assigned[28] == 0  # output format checkout
    assert assigned[3] == assigned[12] == assigned[22] == 0
    assert assigned[13:15] == [1, 1]
    assert assigned[15:17] == [64, 32]
    assert assigned[17:22] == [256, 0, 0, 64, 32]
    assert assigned[23:27] == [255, 137, 71, 223]
    assert absent[3] == absent[12] == absent[22] == 0
    assert absent[4:12] == [0] * 8  # no PreRender map geometry
    assert assigned[4:12] == [0, 0, 64, 32] * 2
    assert absent[13:17] == [1, 1, 64, 32]
    assert absent[17:22] == [256, 0, 0, 64, 32]
    assert absent[23:27] == [0, 0, 0, 0]  # host's transparent null-layer world
    assert absent[27] == assigned[27] == 0x62677261  # SDK ARGB32 FourCC
    assert absent[29] == assigned[29] == 0x62677261
