import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools.ae_scattermap_downsample_verify import premultiply_argb, verify
from tools.scattermap_reference_oracle import gradient, render_source


def argb_to_rgba(argb: bytes) -> bytes:
    return bytes(
        channel
        for offset in range(0, len(argb), 4)
        for channel in (argb[offset + 1], argb[offset + 2], argb[offset + 3], argb[offset])
    )


class AeScatterMapDownsampleVerifyTests(unittest.TestCase):
    def test_arbitrary_8_by_6_input_matches_reference(self):
        source = gradient(8, 6)
        expected = render_source(source, 8, 6)
        with tempfile.TemporaryDirectory() as directory:
            identity = Path(directory) / "identity.png"
            output = Path(directory) / "output.png"
            Image.frombytes("RGBA", (8, 6), argb_to_rgba(source)).save(identity)
            Image.frombytes("RGBA", (8, 6), argb_to_rgba(expected)).save(output)
            report = verify(identity, output)
        self.assertTrue(report["pixel_match"])
        self.assertEqual(report["different_bytes"], 0)

    def test_custom_case_id_is_preserved(self):
        source = gradient(4, 4)
        expected = render_source(source, 4, 4)
        with tempfile.TemporaryDirectory() as directory:
            identity = Path(directory) / "identity.png"
            output = Path(directory) / "output.png"
            Image.frombytes("RGBA", (4, 4), argb_to_rgba(source)).save(identity)
            Image.frombytes("RGBA", (4, 4), argb_to_rgba(expected)).save(output)
            report = verify(identity, output, "variable_alpha")
        self.assertEqual(report["case_id"], "variable_alpha")

    def test_mixed_case_uses_requested_ratio(self):
        source = gradient(4, 4)
        expected = render_source(source, 4, 4, mix=37.5)
        with tempfile.TemporaryDirectory() as directory:
            identity = Path(directory) / "identity.png"
            output = Path(directory) / "output.png"
            Image.frombytes("RGBA", (4, 4), argb_to_rgba(source)).save(identity)
            Image.frombytes("RGBA", (4, 4), argb_to_rgba(expected)).save(output)
            report = verify(identity, output, "mixed", mix=37.5)
        self.assertTrue(report["pixel_match"])
        self.assertEqual(report["parameters"]["mix"], 37.5)

    def test_premultiply_output_uses_round_to_nearest(self):
        self.assertEqual(premultiply_argb(bytes((128, 255, 3, 1))), bytes((128, 128, 2, 1)))


if __name__ == "__main__":
    unittest.main()
