import importlib.util
import json
import subprocess
from pathlib import Path

from _render_session import HARNESS

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "target" / "sdk-fixtures" / "shifter" / "Shifter.aex"
INPUT = ROOT / "target" / "ae-oracle-colorgrid-input.png"

INSPECT = ROOT / "tools" / "ae_png_depth_inspect.py"
SPEC = importlib.util.spec_from_file_location("ae_png_depth_inspect", INSPECT)
PNG = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(PNG)

def _decode(path: Path):
    header, pixels = PNG.decode_png(path)
    return header, pixels

def _samples(header, pixels):
    if header["bit_depth"] == 16:
        return [
            (pixels[offset] << 8) | pixels[offset + 1]
            for offset in range(0, len(pixels), 2)
        ]
    return list(pixels)

def test_deep16_png_output_matches_preview_quantization(tmp_path: Path) -> None:
    preview_output = tmp_path / "shifter-16.png"
    deep_output = tmp_path / "shifter-16-deep.png"
    runs = {}
    for command, output in (
        ("--render-experimental-smart-16", preview_output),
        ("--render-experimental-smart-16-deep", deep_output),
    ):
        completed = subprocess.run(
            [str(HARNESS), command, str(FIXTURE), str(INPUT), str(output)],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=60,
        )
        assert completed.returncode == 0, completed.stderr
        runs[command] = json.loads(completed.stdout)

    preview_report = runs["--render-experimental-smart-16"]
    deep_report = runs["--render-experimental-smart-16-deep"]
    for report in (preview_report, deep_report):
        assert report["passed"] is True
        assert report["pixel_format"] == "argb16"
        assert report["render_path"] == "smartfx"
    assert preview_report["output_transport"] == "native_raw+rgba8_png_preview"
    assert deep_report["output_transport"] == "native_raw+rgba16_png"
    assert deep_report["output_overrange_samples"] == 0

    preview_header, preview_pixels = _decode(preview_output)
    deep_header, deep_pixels = _decode(deep_output)
    assert preview_header["bit_depth"] == 8
    assert deep_header["bit_depth"] == 16
    assert (deep_header["width"], deep_header["height"]) == (
        preview_header["width"],
        preview_header["height"],
    )

    preview_samples = _samples(preview_header, preview_pixels)
    deep_samples = _samples(deep_header, deep_pixels)
    assert len(deep_samples) == len(preview_samples)
    # Rounding the full-range 16-bit samples to 8 bits must reproduce the
    # preview PNG exactly; anything else means the two routes diverged.
    mismatches = sum(
        1
        for deep, preview in zip(deep_samples, preview_samples)
        if (deep * 255 + 32767) // 65535 != preview
    )
    assert mismatches == 0

    # Both routes also leave the same depth-preserving raw sidecar bytes.
    preview_raw = Path(preview_report["output_raw"])
    deep_raw = Path(deep_report["output_raw"])
    assert preview_raw.suffix == ".rgba16le" and deep_raw.suffix == ".rgba16le"
    assert preview_raw.read_bytes() == deep_raw.read_bytes()

    # The 16-bit PNG must be a lossless expansion of the raw AE-range data.
    raw = deep_raw.read_bytes()
    raw_samples = [
        int.from_bytes(raw[offset:offset + 2], "little")
        for offset in range(0, len(raw), 2)
    ]
    assert len(raw_samples) == len(deep_samples)
    assert all(
        (min(value, 32768) * 65535 + 16384) // 32768 == deep
        for value, deep in zip(raw_samples, deep_samples)
    )
