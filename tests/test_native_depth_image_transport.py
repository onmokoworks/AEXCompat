from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")


def test_native_worker_output_reorders_channels_without_quantizing_depth():
    assert "void argb_to_rgba_native" in SOURCE
    assert "output[0] = pixel[1]" in SOURCE
    assert "height * pixel_bytes" in SOURCE
    deep_output_blocks = SOURCE.count("argb_to_rgba_native(rgba.data() + pixel * pixel_bytes")
    assert deep_output_blocks == 3
