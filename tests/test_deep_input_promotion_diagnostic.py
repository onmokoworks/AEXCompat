import importlib.util
import struct
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
TOOL = ROOT / "tools" / "diagnose-deep-input-promotion.py"
SPEC = importlib.util.spec_from_file_location("deep_input_diagnostic", TOOL)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


def _write_rgba8_png(path: Path, pixels: list[int]) -> None:
    from PIL import Image

    image = Image.frombytes("RGBA", (len(pixels) // 4, 1), bytes(pixels))
    image.save(path)


def test_sdk_convert8to16_is_verified_for_every_8_bit_value(tmp_path: Path):
    source = tmp_path / "ramp.png"
    pixels = [channel for value in range(256) for channel in (value,) * 4]
    _write_rgba8_png(source, pixels)
    promoted = [(value * 32768 + 127) // 255 for value in pixels]
    raw = tmp_path / "smart-input.rgba16le"
    raw.write_bytes(struct.pack(f"<{len(promoted)}H", *promoted))

    report = MODULE.diagnose(source, raw)

    assert report["match"] is True
    assert report["mismatched_channels"] == 0
    assert report["conversion"]["white"] == 32768


def test_first_promotion_mismatch_is_reported(tmp_path: Path):
    source = tmp_path / "pixel.png"
    _write_rgba8_png(source, [1, 2, 3, 255])
    promoted = [(value * 32768 + 127) // 255 for value in (1, 2, 3, 255)]
    promoted[1] += 1
    raw = tmp_path / "smart-input.rgba16le"
    raw.write_bytes(struct.pack("<4H", *promoted))

    report = MODULE.diagnose(source, raw)

    assert report["match"] is False
    assert report["mismatched_channels"] == 1
    assert report["first_mismatch"]["channel"] == "g"


def test_wrong_raw_size_fails_closed(tmp_path: Path):
    source = tmp_path / "pixel.png"
    _write_rgba8_png(source, [0, 0, 0, 255])
    raw = tmp_path / "smart-input.rgba16le"
    raw.write_bytes(b"short")

    with pytest.raises(MODULE.InputError, match="byte count"):
        MODULE.diagnose(source, raw)
