import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools.ae_scattermap_downsample_verify import verify
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


if __name__ == "__main__":
    unittest.main()
