import importlib.util
import json
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
TOOL = ROOT / "tools" / "compare-pixel-oracles.py"
SPEC = importlib.util.spec_from_file_location("compare_pixel_oracles", TOOL)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class ComparePixelOraclesTests(unittest.TestCase):
    def test_exact_rgba8_match(self):
        pixels = bytes((1, 2, 3, 4, 250, 100, 0, 255))
        with tempfile.TemporaryDirectory() as directory:
            raw = Path(directory) / "expected.rgba"
            png = Path(directory) / "actual.png"
            raw.write_bytes(pixels)
            Image.frombytes("RGBA", (2, 1), pixels).save(png)
            report = MODULE.compare(raw, png, 2, 1)
        self.assertTrue(report["match"])
        self.assertEqual(report["exact_mismatched_channels"], 0)
        self.assertIsNone(report["first_mismatch"])
        self.assertEqual(report["hashes"]["raw_sha256"],
                         "95f152aac8ca424c5763114207cbeda1ceb714380886fc0cc9fb20cbaf66e9fa")

    def test_mismatch_stats_and_first_location(self):
        raw_pixels = bytes((0, 10, 20, 255, 30, 40, 50, 255))
        png_pixels = bytes((0, 12, 20, 255, 30, 40, 55, 255))
        with tempfile.TemporaryDirectory() as directory:
            raw = Path(directory) / "expected.rgba"
            png = Path(directory) / "actual.png"
            raw.write_bytes(raw_pixels)
            Image.frombytes("RGBA", (2, 1), png_pixels).save(png)
            report = MODULE.compare(raw, png, 2, 1, tolerance=1 / 255)
        self.assertFalse(report["match"])
        self.assertEqual(report["exact_mismatched_channels"], 2)
        self.assertEqual(report["over_tolerance_channels"], 2)
        self.assertEqual(report["first_mismatch"]["x"], 0)
        self.assertEqual(report["first_mismatch"]["channel"], "g")
        self.assertAlmostEqual(report["max_abs_error"]["b"], 5 / 255)
        self.assertAlmostEqual(report["mean_abs_error"]["g"], 1 / 255)

    def test_tolerance_allows_difference_but_keeps_exact_count(self):
        with tempfile.TemporaryDirectory() as directory:
            raw = Path(directory) / "expected.rgba"
            png = Path(directory) / "actual.png"
            raw.write_bytes(bytes((0, 0, 0, 255)))
            Image.new("RGBA", (1, 1), (1, 0, 0, 255)).save(png)
            report = MODULE.compare(raw, png, 1, 1, tolerance=1 / 255)
        self.assertTrue(report["match"])
        self.assertEqual(report["exact_mismatched_channels"], 1)
        self.assertEqual(report["over_tolerance_channels"], 0)

    def test_rgba16_integer_max_and_float_raw(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            png = root / "actual.png"
            Image.new("RGBA", (1, 1), (255, 128, 0, 255)).save(png)
            raw16 = root / "expected16.rgba"
            raw16.write_bytes(struct.pack("<4H", 32768, 16448, 0, 32768))
            self.assertTrue(MODULE.compare(
                raw16, png, 1, 1, "rgba16le", 1 / 32768, 32768)["match"])
            raw32 = root / "expected32.rgba"
            raw32.write_bytes(struct.pack("<4f", 1.0, 128 / 255, 0.0, 1.0))
            self.assertTrue(MODULE.compare(
                raw32, png, 1, 1, "rgba32f-le", tolerance=1e-7)["match"])

    def test_bad_raw_size_and_dimensions_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "bad.rgba"
            png = root / "actual.png"
            raw.write_bytes(b"\0")
            Image.new("RGBA", (2, 1)).save(png)
            with self.assertRaisesRegex(MODULE.InputError, "raw byte count"):
                MODULE.compare(raw, png, 1, 1)

    def test_cli_exit_codes_and_deterministic_json(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            output = root / "report.json"
            raw.write_bytes(bytes((0, 0, 0, 255)))
            Image.new("RGBA", (1, 1), (9, 0, 0, 255)).save(png)
            command = [sys.executable, str(TOOL), "--raw", str(raw), "--render",
                       str(png), "--width", "1", "--height", "1", "--out", str(output)]
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            first = output.read_bytes()
            result2 = subprocess.run(command, capture_output=True, text=True, check=False)
            second = output.read_bytes()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result2.returncode, 1)
        self.assertEqual(first, second)
        self.assertFalse(json.loads(first)["match"])

    def test_exr_failure_is_graceful(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            exr = root / "bad.exr"
            raw.write_bytes(bytes((0, 0, 0, 255)))
            exr.write_bytes(b"not an exr")
            with self.assertRaisesRegex(MODULE.InputError, "EXR"):
                MODULE.compare(raw, exr, 1, 1)

    def test_nonfinite_float_is_valid_deterministic_json(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(struct.pack("<4f", float("nan"), 0.0, 0.0, 1.0))
            Image.new("RGBA", (1, 1), (0, 0, 0, 255)).save(png)
            report = MODULE.compare(raw, png, 1, 1, "rgba32f-le")
            encoded = json.dumps(report, allow_nan=False, sort_keys=True)
        self.assertFalse(report["match"])
        self.assertEqual(report["first_mismatch"]["expected"], "NaN")
        self.assertIn('"abs_error": "NaN"', encoded)


if __name__ == "__main__":
    unittest.main()
