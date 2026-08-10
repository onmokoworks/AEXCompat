import importlib.util
import json
import math
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
import hashlib
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
TOOL = ROOT / "tools" / "compare-pixel-oracles.py"
SPEC = importlib.util.spec_from_file_location("compare_pixel_oracles", TOOL)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


def write_rgba16_png(path: Path, width: int, height: int, samples: tuple[int, ...]):
    signature = b"\x89PNG\r\n\x1a\n"

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return (struct.pack(">I", len(payload)) + kind + payload +
                struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF))

    rows = b"".join(
        b"\0" + struct.pack(f">{width * 4}H", *samples[y * width * 4:(y + 1) * width * 4])
        for y in range(height)
    )
    path.write_bytes(
        signature + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 16, 6, 0, 0, 0)) +
        chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
    )


class ComparePixelOraclesTests(unittest.TestCase):
    def test_raw_u32_binds_metadata_and_detects_special_word_mutations(self):
        try:
            import OpenEXR
            import numpy as np
        except ImportError:
            self.skipTest("OpenEXR dev dependency unavailable")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "output.bin"
            exr = root / "output.exr"
            raw_meta_path = root / "raw.json"
            exr_meta_path = root / "exr.json"
            rgba_words = np.array([
                [0x7FC12345, 0x80000000, 0x00000001, 0x3F800000],
                [0x3F000000, 0x40000000, 0x40400000, 0x00000000],
            ], dtype=np.uint32)
            argb_words = rgba_words[:, [3, 0, 1, 2]].reshape(-1)
            raw.write_bytes(argb_words.astype("<u4").tobytes())

            def write_exr(words):
                pixels = words.view(np.float32).reshape(1, 2, 4)
                with OpenEXR.File({}, {"RGBA": pixels}) as outfile:
                    outfile.write(str(exr))

            def metadata(schema, data_path, **extra):
                identity = {
                    "plugin_sha256": "11" * 32, "input_sha256": "22" * 32,
                    "world_sha256": "33" * 32, "render_path": "smartfx",
                    "pixel_format": "argb32f",
                    "timing": {"current_time": 0, "time_step": 1,
                               "total_time": 300, "time_scale": 30},
                    "requested_parameters": [], "origin": {"x": 0, "y": 0},
                }
                value = {
                    "schema": schema, "schema_version": 1, "width": 2, "height": 1,
                    "channel_order": "ARGB" if schema.endswith("raw") else "RGBA",
                    "endianness": "little", "premultiplication": "straight",
                    "working_space": "None", "render_mode": "software",
                    "data_file": data_path.name, "data_size_bytes": data_path.stat().st_size,
                    "data_sha256": hashlib.sha256(data_path.read_bytes()).hexdigest(),
                    "comparison_identity": identity, **extra,
                    "origin": {"x": 0, "y": 0},
                }
                if schema.endswith("raw"):
                    value.update({
                        "component_bytes": 4,
                        "component_representation": "ieee754_binary32_raw_words",
                        "rowbytes": 32, "row_padding": "excluded",
                        "source_world_rowbytes": None,
                        "source_world_row_padding": "not_transported",
                        "comparison_boundaries": {"aex_arithmetic": "internal_world_raw",
                                                  "host_export": "not_applicable"},
                    })
                else:
                    value.update({
                        "exr_file_channel_order": ["A", "B", "G", "R"],
                        "channel_type": "FLOAT32", "storage": "scanline",
                        "compression": "none", "rgb_policy": "preserve",
                        "word_comparison": "raw_u32_little_endian",
                        "source_world_rowbytes": None,
                        "source_world_row_padding": "not_transported",
                        "source_transport_order": "RGBA",
                        "comparison_boundaries": {
                            "aex_arithmetic": "compare_source_raw_world",
                            "host_export": "compare_float32_exr_raw_u32"},
                    })
                return value

            raw_meta_path.write_text(json.dumps(metadata(
                "aexcompat.render_raw", raw, pixel_format="argb32f")))
            write_exr(rgba_words.copy())
            exr_meta_path.write_text(json.dumps(metadata(
                "aexcompat.render_exr", exr, pixel_format="float32")))
            report = MODULE.compare_raw_u32(raw, exr, raw_meta_path, exr_meta_path)
            self.assertTrue(report["match"])
            self.assertEqual(report["difference_layers"]["host_export"]["status"], "exact")

            for index, mutated in enumerate((0x7FC12346, 0x00000000, 0x00000002, 0x3F000000)):
                changed = rgba_words.copy()
                changed[0, index] = mutated
                write_exr(changed)
                exr_meta_path.write_text(json.dumps(metadata(
                    "aexcompat.render_exr", exr, pixel_format="float32")))
                report = MODULE.compare_raw_u32(raw, exr, raw_meta_path, exr_meta_path)
                self.assertFalse(report["match"])
                self.assertEqual(report["first_mismatch"]["channel"], MODULE.CHANNELS[index])

            write_exr(rgba_words[:, [2, 1, 0, 3]].copy())
            exr_meta_path.write_text(json.dumps(metadata(
                "aexcompat.render_exr", exr, pixel_format="float32")))
            self.assertFalse(MODULE.compare_raw_u32(
                raw, exr, raw_meta_path, exr_meta_path)["match"])

            broken = json.loads(raw_meta_path.read_text())
            broken["data_sha256"] = "0" * 64
            raw_meta_path.write_text(json.dumps(broken))
            with self.assertRaisesRegex(MODULE.InputError, "sha256"):
                MODULE.compare_raw_u32(raw, exr, raw_meta_path, exr_meta_path)

            for key, bad_value in (("working_space", "sRGB"),
                                   ("compression", "zip"),
                                   ("comparison_boundaries", {})):
                good_raw = metadata("aexcompat.render_raw", raw, pixel_format="argb32f")
                raw_meta_path.write_text(json.dumps(good_raw))
                broken_exr = metadata("aexcompat.render_exr", exr, pixel_format="float32")
                broken_exr[key] = bad_value
                exr_meta_path.write_text(json.dumps(broken_exr))
                with self.assertRaisesRegex(MODULE.InputError, "canonical"):
                    MODULE.compare_raw_u32(raw, exr, raw_meta_path, exr_meta_path)

            good_exr = metadata("aexcompat.render_exr", exr, pixel_format="float32")
            exr_meta_path.write_text(json.dumps(good_exr))
            wrong_identity = metadata("aexcompat.render_raw", raw, pixel_format="argb32f")
            wrong_identity["comparison_identity"]["world_sha256"] = "44" * 32
            raw_meta_path.write_text(json.dumps(wrong_identity))
            with self.assertRaisesRegex(MODULE.InputError, "comparison_identity"):
                MODULE.compare_raw_u32(raw, exr, raw_meta_path, exr_meta_path)

            wrong_origin = metadata("aexcompat.render_raw", raw, pixel_format="argb32f")
            wrong_origin["origin"] = {"x": 1, "y": 0}
            raw_meta_path.write_text(json.dumps(wrong_origin))
            with self.assertRaisesRegex(MODULE.InputError, "origin"):
                MODULE.compare_raw_u32(raw, exr, raw_meta_path, exr_meta_path)

    def test_render_raw_argb_formats_are_reinterpreted_without_word_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            argb8 = root / "pf8.bin"
            argb16 = root / "pf16.bin"
            argb32 = root / "pf32.bin"
            argb8.write_bytes(bytes((4, 1, 2, 3)))
            argb16.write_bytes(struct.pack("<4H", 32768, 1, 2, 3))
            words = (0x3f800000, 0x7fc12345, 0x80000000, 0x00000001)
            argb32.write_bytes(struct.pack("<4I", *words))
            self.assertEqual(MODULE.load_raw(argb8, 1, 1, "argb8"), [1/255, 2/255, 3/255, 4/255])
            self.assertEqual(MODULE.load_raw(argb16, 1, 1, "argb16le-ae"), [1/32768, 2/32768, 3/32768, 1.0])
            values = MODULE.load_raw(argb32, 1, 1, "argb32f-le")
            self.assertTrue(math.isnan(values[0]))
            self.assertEqual(struct.unpack("<I", struct.pack("<f", values[1]))[0], 0x80000000)
            self.assertEqual(struct.unpack("<I", struct.pack("<f", values[2]))[0], 1)
            self.assertEqual(values[3], 1.0)
    def test_exact_rgba8_match(self):
        pixels = bytes((1, 2, 3, 4, 250, 100, 0, 255))
        with tempfile.TemporaryDirectory() as directory:
            raw = Path(directory) / "expected.rgba"
            png = Path(directory) / "actual.png"
            raw.write_bytes(pixels)
            Image.frombytes("RGBA", (2, 1), pixels).save(png)
            report = MODULE.compare(raw, png, 2, 1)
        self.assertTrue(report["match"])
        self.assertEqual(report["comparison_boundary"]["claim_level"], "export_exact")
        self.assertFalse(report["comparison_boundary"]["raw_world_exact"])
        self.assertEqual(
            report["difference_layers"]["aex_arithmetic"]["status"],
            "not_evaluated_by_export_comparison",
        )
        self.assertEqual(report["difference_layers"]["host_export"]["status"], "exact")
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
            report16 = MODULE.compare(
                raw16, png, 1, 1, "rgba16le", 1 / 32768, 32768)
            self.assertTrue(report16["match"])
            self.assertEqual(
                report16["comparison_boundary"]["claim_level"],
                "cross_precision_export_only",
            )
            raw32 = root / "expected32.rgba"
            raw32.write_bytes(struct.pack("<4f", 1.0, 128 / 255, 0.0, 1.0))
            self.assertTrue(MODULE.compare(
                raw32, png, 1, 1, "rgba32f-le", tolerance=1e-7)["match"])

    def test_rgba16_png_is_compared_without_pillow_truncation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(struct.pack("<4H", 32768, 16384, 1, 32768))
            write_rgba16_png(png, 1, 1, (65535, 32768, 2, 65535))
            report = MODULE.compare(
                raw, png, 1, 1, "rgba16le", tolerance=1 / 32768,
                raw_integer_max=32768,
            )
        self.assertTrue(report["match"])
        self.assertEqual(report["formats"]["render"], "png_rgba16")
        self.assertEqual(report["comparison_boundary"]["claim_level"], "export_tolerance")

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
