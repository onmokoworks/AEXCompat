import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools.ae_scattermap_render_verify import rgba_to_argb, verify
from tools.scattermap_reference_oracle import generated_luma_map, render_case, render_default


def argb_to_rgba(argb: bytes) -> bytes:
    return bytes(
        channel
        for offset in range(0, len(argb), 4)
        for channel in (argb[offset + 1], argb[offset + 2], argb[offset + 3], argb[offset])
    )


class AeScatterMapRenderVerifyTests(unittest.TestCase):
    def test_rgba_channel_normalization(self):
        self.assertEqual(rgba_to_argb(bytes((1, 2, 3, 4))), bytes((4, 1, 2, 3)))

    def test_exact_oracle_png_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "reference.png"
            Image.frombytes("RGBA", (16, 12), argb_to_rgba(render_default())).save(path)
            report = verify(path)
        self.assertTrue(report["pixel_match"])
        self.assertTrue(report["non_identity_output"])
        self.assertEqual(report["different_bytes"], 0)
        self.assertEqual(report["different_pixels"], 0)

    def test_changed_pixel_is_reported(self):
        argb = bytearray(render_default())
        argb[5] ^= 0xFF
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "changed.png"
            Image.frombytes("RGBA", (16, 12), argb_to_rgba(argb)).save(path)
            report = verify(path)
        self.assertFalse(report["pixel_match"])
        self.assertEqual(report["different_bytes"], 1)
        self.assertEqual(report["different_pixels"], 1)

    def test_identity_case_matches_source(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "identity.png"
            Image.frombytes("RGBA", (16, 12), argb_to_rgba(render_case(amount=0))).save(path)
            report = verify(path, "identity")
        self.assertTrue(report["pixel_match"])
        self.assertFalse(report["non_identity_output"])

    def test_connected_map_case_uses_11_by_7_oracle(self):
        expected = render_case(11, 7, luma_map=generated_luma_map(11, 7, 5, 3))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "connected.png"
            Image.frombytes("RGBA", (11, 7), argb_to_rgba(expected)).save(path)
            report = verify(path, "connected_map")
        self.assertTrue(report["pixel_match"])
        self.assertEqual((report["width"], report["height"]), (11, 7))


if __name__ == "__main__":
    unittest.main()
