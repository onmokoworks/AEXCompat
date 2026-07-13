import tempfile
import unittest
from pathlib import Path

from PIL import Image

from tools.ae_png_depth_inspect import compare_pngs, inspect_png, paeth


class AePngDepthInspectTests(unittest.TestCase):
    def test_paeth_predictor(self):
        self.assertEqual(paeth(20, 10, 0), 20)
        self.assertEqual(paeth(10, 20, 0), 20)
        self.assertEqual(paeth(10, 20, 15), 15)

    def test_decodes_rgba8_without_pillow_conversion(self):
        pixels = bytes((1, 2, 3, 4, 250, 251, 252, 253))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "two-pixels.png"
            Image.frombytes("RGBA", (2, 1), pixels).save(path)
            report = inspect_png(path)
        self.assertEqual(report["bit_depth"], 8)
        self.assertEqual(report["sample_count"], 8)
        self.assertEqual(report["sample_min"], 1)
        self.assertEqual(report["sample_max"], 253)

    def test_comparison_counts_changed_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.png"
            second = Path(directory) / "second.png"
            Image.frombytes("RGBA", (1, 1), bytes((1, 2, 3, 4))).save(first)
            Image.frombytes("RGBA", (1, 1), bytes((1, 9, 3, 8))).save(second)
            report = compare_pngs(first, second)
        self.assertFalse(report["decoded_match"])
        self.assertEqual(report["different_samples"], 2)
        self.assertEqual(report["stable_samples"], 2)


if __name__ == "__main__":
    unittest.main()
