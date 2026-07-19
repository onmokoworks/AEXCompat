from pathlib import Path

import source_owners

ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.contract_text("native_depth_image_transport")
PIXEL_TRANSPORT = (ROOT / "minihost/src/render_pixel_transport.cpp").read_text(
    encoding="utf-8"
)


def test_native_worker_output_reorders_channels_without_quantizing_depth():
    assert "void argb_to_rgba_native" in PIXEL_TRANSPORT
    assert "output[0] = pixel[1]" in PIXEL_TRANSPORT
    assert "height * pixel_bytes" in SOURCE
    # Three external-output transport blocks, the world snapshot dump helper,
    # and the render session output slot reuse the same depth-preserving
    # reorder.
    deep_output_blocks = SOURCE.count("argb_to_rgba_native(")
    assert deep_output_blocks == 5
