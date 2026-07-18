import hashlib
import importlib.util
import json
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
GENERATOR = ROOT / "tools" / "generate-oracle-rgba-input.py"
CONVERTER = ROOT / "tools" / "png-to-rgba-raw.py"


def load_tool(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


GENERATOR_MODULE = load_tool(GENERATOR, "generate_oracle_rgba_input")
CONVERTER_MODULE = load_tool(CONVERTER, "png_to_rgba_raw")
INSPECT_MODULE = load_tool(ROOT / "tools" / "ae_png_depth_inspect.py",
                           "ae_png_depth_inspect_for_input_tools")


def expected_pixel(x: int, y: int, width: int, height: int, alpha_mode: str):
    # Documented formula: integer round-half-up of value * 255 / maximum.
    def scaled(value: int, maximum: int) -> int:
        if maximum <= 0:
            return 0
        return (value * 255 * 2 + maximum) // (maximum * 2)

    return (
        scaled(x, width - 1),
        scaled(y, height - 1),
        scaled(x + y, width + height - 2),
        255 if alpha_mode == "opaque" else scaled(y, height - 1),
    )


class GenerateOracleRgbaInputTests(unittest.TestCase):
    def test_generated_pixels_match_the_documented_formula(self):
        for alpha_mode in ("opaque", "vertical-gradient"):
            with tempfile.TemporaryDirectory() as directory:
                out = Path(directory) / "input.png"
                result = subprocess.run(
                    [sys.executable, str(GENERATOR), "--width", "5", "--height", "3",
                     "--alpha-mode", alpha_mode, "--out", str(out)],
                    capture_output=True, text=True, check=False)
                self.assertEqual(result.returncode, 0, result.stderr)
                metadata, decoded = INSPECT_MODULE.decode_png(out)
                self.assertEqual((metadata["width"], metadata["height"],
                                  metadata["bit_depth"]), (5, 3, 8))
                for y in range(3):
                    for x in range(5):
                        offset = (y * 5 + x) * 4
                        self.assertEqual(
                            tuple(decoded[offset:offset + 4]),
                            expected_pixel(x, y, 5, 3, alpha_mode),
                            f"pixel ({x},{y}) alpha_mode={alpha_mode}")
                report = json.loads(result.stdout)
                self.assertEqual(report["alpha_mode"], alpha_mode)
                self.assertEqual(len(report["png_sha256"]), 64)
                # The machine-portable identity is the decoded RGBA hash,
                # since the PNG container bytes depend on the local zlib.
                self.assertEqual(report["decoded_rgba_sha256"],
                                 hashlib.sha256(bytes(decoded)).hexdigest())

    def test_odd_dimensions_are_supported(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "odd.png"
            result = subprocess.run(
                [sys.executable, str(GENERATOR), "--width", "17", "--height", "13",
                 "--out", str(out)],
                capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            metadata, _ = INSPECT_MODULE.decode_png(out)
            self.assertEqual((metadata["width"], metadata["height"]), (17, 13))

    def test_generation_is_deterministic(self):
        digests = []
        for _ in range(2):
            with tempfile.TemporaryDirectory() as directory:
                out = Path(directory) / "input.png"
                result = subprocess.run(
                    [sys.executable, str(GENERATOR), "--width", "9", "--height", "7",
                     "--alpha-mode", "vertical-gradient", "--out", str(out)],
                    capture_output=True, text=True, check=False)
                self.assertEqual(result.returncode, 0, result.stderr)
                report = json.loads(result.stdout)
                digests.append((report["decoded_rgba_sha256"], report["png_sha256"]))
        self.assertEqual(digests[0], digests[1])

    def test_existing_output_and_bad_dimensions_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "input.png"
            out.write_bytes(b"occupied")
            result = subprocess.run(
                [sys.executable, str(GENERATOR), "--width", "2", "--height", "2",
                 "--out", str(out)],
                capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2)
            self.assertIn("refusing to overwrite", result.stderr)
            self.assertEqual(out.read_bytes(), b"occupied")
            oversize = subprocess.run(
                [sys.executable, str(GENERATOR), "--width", "4097", "--height", "2",
                 "--out", str(Path(directory) / "big.png")],
                capture_output=True, text=True, check=False)
            self.assertEqual(oversize.returncode, 2)


class PngToRgbaRawTests(unittest.TestCase):
    def test_rgba8_png_round_trips_to_identical_raw_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            png = root / "input.png"
            raw = root / "input.rgba"
            subprocess.run(
                [sys.executable, str(GENERATOR), "--width", "4", "--height", "2",
                 "--out", str(png)],
                capture_output=True, text=True, check=True)
            result = subprocess.run(
                [sys.executable, str(CONVERTER), "--png", str(png), "--out", str(raw)],
                capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout)
            _, decoded = INSPECT_MODULE.decode_png(png)
            self.assertEqual(raw.read_bytes(), bytes(decoded))
            self.assertEqual(report["raw_format"], "rgba8")

    def test_rgba16_png_converts_to_little_endian_raw(self):
        def chunk(kind: bytes, payload: bytes) -> bytes:
            return (struct.pack(">I", len(payload)) + kind + payload +
                    struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF))

        samples = (65535, 32768, 2, 65535)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            png = root / "deep.png"
            raw = root / "deep.raw"
            rows = b"\0" + struct.pack(">4H", *samples)
            png.write_bytes(
                b"\x89PNG\r\n\x1a\n" +
                chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 16, 6, 0, 0, 0)) +
                chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
            result = subprocess.run(
                [sys.executable, str(CONVERTER), "--png", str(png), "--out", str(raw)],
                capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)["raw_format"], "rgba16le")
            self.assertEqual(raw.read_bytes(), struct.pack("<4H", *samples))

    def test_existing_output_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            png = root / "input.png"
            raw = root / "input.rgba"
            raw.write_bytes(b"occupied")
            subprocess.run(
                [sys.executable, str(GENERATOR), "--width", "2", "--height", "2",
                 "--out", str(png)],
                capture_output=True, text=True, check=True)
            result = subprocess.run(
                [sys.executable, str(CONVERTER), "--png", str(png), "--out", str(raw)],
                capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(raw.read_bytes(), b"occupied")


if __name__ == "__main__":
    unittest.main()
