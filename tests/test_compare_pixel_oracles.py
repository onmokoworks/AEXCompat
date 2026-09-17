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


def _premultiplied_rgba8(pixel: bytes) -> tuple[int, int, int, int]:
    """AE's measured 8-bit association: round-half-up on the integers."""
    alpha = pixel[3]
    return (*((channel * alpha + 127) // 255 for channel in pixel[:3]), alpha)


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

    def test_undeclared_alpha_association_is_named_not_just_counted(self):
        # The same picture in both buffers, straight in the raw and
        # premultiplied in the render. Every partially transparent pixel
        # differs and nothing else does; without the split that reads as a
        # rendering difference.
        straight = bytes((58, 11, 123, 106, 200, 100, 50, 255, 7, 9, 11, 0))
        premultiplied = b"".join(
            bytes(_premultiplied_rgba8(straight[index:index + 4]))
            for index in range(0, len(straight), 4))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(straight)
            Image.frombytes("RGBA", (3, 1), premultiplied).save(png)
            report = MODULE.compare(raw, png, 3, 1)
        self.assertFalse(report["match"])
        association = report["alpha_association"]
        self.assertEqual(association["raw"], "unspecified")
        self.assertEqual(association["compared_in"], "as_provided")
        self.assertEqual(association["pixels"],
                         {"transparent": 1, "partial": 1, "opaque": 1})
        self.assertEqual(association["mismatched_pixels"],
                         {"transparent": 1, "partial": 1, "opaque": 0})
        self.assertTrue(association["mismatches_spare_opaque_pixels"])
        self.assertIn("alpha association", association["diagnostic"])

    def test_declared_alpha_associations_are_compared_in_one_space(self):
        straight = bytes((58, 11, 123, 106, 200, 100, 50, 255, 7, 9, 11, 0))
        premultiplied = b"".join(
            bytes(_premultiplied_rgba8(straight[index:index + 4]))
            for index in range(0, len(straight), 4))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(straight)
            Image.frombytes("RGBA", (3, 1), premultiplied).save(png)
            report = MODULE.compare(raw, png, 3, 1, raw_alpha="straight",
                                    render_alpha="premultiplied")
        self.assertTrue(report["match"])
        self.assertEqual(report["exact_mismatched_channels"], 0)
        self.assertEqual(report["alpha_association"]["compared_in"], "premultiplied")
        self.assertFalse(
            report["alpha_association"]["mismatches_spare_opaque_pixels"])

    def test_premultiplied_raw_converts_the_render_instead(self):
        straight = bytes((58, 11, 123, 106))
        premultiplied = bytes(_premultiplied_rgba8(straight))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(premultiplied)
            Image.frombytes("RGBA", (1, 1), straight).save(png)
            report = MODULE.compare(raw, png, 1, 1, raw_alpha="premultiplied",
                                    render_alpha="straight")
        self.assertTrue(report["match"])
        self.assertEqual(report["alpha_association"]["compared_in"], "premultiplied")

    def test_matching_declared_associations_are_left_alone(self):
        pixels = bytes((58, 11, 123, 106))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(pixels)
            Image.frombytes("RGBA", (1, 1), pixels).save(png)
            report = MODULE.compare(raw, png, 1, 1, raw_alpha="straight",
                                    render_alpha="straight")
        self.assertTrue(report["match"])
        self.assertEqual(report["alpha_association"]["compared_in"], "as_provided")

    def test_association_agreement_is_not_claimed_as_byte_exactness(self):
        # Premultiplying is many-to-one: at alpha 1 every straight value from
        # 128 up collapses onto 1. The buffers do agree after the conversion,
        # and the report has to say that is what happened.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(bytes((255, 0, 0, 1)))
            Image.frombytes("RGBA", (1, 1), bytes((1, 0, 0, 1))).save(png)
            report = MODULE.compare(raw, png, 1, 1, raw_alpha="straight",
                                    render_alpha="premultiplied")
        self.assertTrue(report["match"])
        self.assertEqual(report["comparison_boundary"]["claim_level"],
                         "export_exact_after_alpha_association")
        self.assertEqual(
            report["alpha_association"]["differences_resolved_by_association"], 1)
        self.assertEqual(report["alpha_association"]["association_domain"], 255)

    def test_nothing_is_erased_when_no_association_conversion_runs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(bytes((1, 2, 3, 4)))
            Image.frombytes("RGBA", (1, 1), bytes((1, 2, 3, 4))).save(png)
            report = MODULE.compare(raw, png, 1, 1)
        self.assertEqual(report["comparison_boundary"]["claim_level"], "export_exact")
        self.assertEqual(
            report["alpha_association"]["differences_resolved_by_association"], 0)
        self.assertIsNone(report["alpha_association"]["association_domain"])

    def test_the_blind_spot_of_the_association_is_reported(self):
        # The least opaque pixel decides it. At alpha 128 two straight values
        # one step apart can still collapse; at alpha 255 nothing does.
        def step_for(alpha: int) -> object:
            straight = bytes((10, 20, 30, alpha))
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                raw = root / "expected.rgba"
                png = root / "actual.png"
                raw.write_bytes(straight)
                Image.frombytes("RGBA", (1, 1),
                                bytes(_premultiplied_rgba8(straight))).save(png)
                report = MODULE.compare(raw, png, 1, 1, raw_alpha="straight",
                                        render_alpha="premultiplied")
            self.assertTrue(report["match"])
            return report["alpha_association"]["worst_case_hidden_straight_step"]

        self.assertEqual(step_for(255), 0)
        self.assertEqual(step_for(128), 1)
        # Not 254: f(0)=0 and f(254)=1 at alpha 1, so a 254-step is witnessable.
        self.assertEqual(step_for(1), 127)
        # A fully transparent pixel hides the whole domain.
        self.assertEqual(step_for(0), 255)
        # Nothing to measure when no conversion ran.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(bytes((10, 20, 30, 128)))
            Image.frombytes("RGBA", (1, 1), bytes((10, 20, 30, 128))).save(png)
            report = MODULE.compare(raw, png, 1, 1)
        self.assertIsNone(
            report["alpha_association"]["worst_case_hidden_straight_step"])
        self.assertIsNone(report["alpha_association"]["association_is_lossless"])

    def test_collapse_width_matches_the_map_it_describes(self):
        # Brute force against the premultiply this tool actually applies, over
        # every alpha at three domains. (m - 1) // a passes none of this.
        for maximum in (255, 32768):
            for alpha in range(0, maximum + 1, max(1, maximum // 37)):
                outputs = [(v * alpha + maximum // 2) // maximum
                           for v in range(maximum + 1)]
                longest = run = 1
                for index in range(1, len(outputs)):
                    run = run + 1 if outputs[index] == outputs[index - 1] else 1
                    longest = max(longest, run)
                self.assertEqual(MODULE.collapse_width(maximum, alpha),
                                 longest - 1,
                                 f"maximum={maximum} alpha={alpha}")
        self.assertEqual(MODULE.collapse_width(255, 0), 255)
        self.assertEqual(MODULE.collapse_width(255, 255), 0)
        with self.assertRaises(MODULE.InputError):
            MODULE.collapse_width(0, 1)

    def test_the_blind_spot_follows_the_side_that_was_converted(self):
        # The raw is what gets premultiplied here, and its alpha is the
        # transparent one; the render is fully opaque. Reading the render's
        # alpha instead would report "nothing hidden".
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(bytes((10, 20, 30, 1)))
            Image.frombytes("RGBA", (1, 1), bytes((10, 20, 30, 255))).save(png)
            report = MODULE.compare(raw, png, 1, 1, raw_alpha="straight",
                                    render_alpha="premultiplied")
        self.assertEqual(
            report["alpha_association"]["worst_case_hidden_straight_step"], 127)
        self.assertFalse(report["alpha_association"]["association_is_lossless"])

    def test_alpha_classes_come_from_the_render_not_the_raw(self):
        # Opaque on the render side, partially transparent on the raw side:
        # the split has to describe the render.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(bytes((10, 20, 30, 128)))
            Image.frombytes("RGBA", (1, 1), bytes((10, 20, 30, 255))).save(png)
            report = MODULE.compare(raw, png, 1, 1)
        self.assertEqual(report["alpha_association"]["pixels"],
                         {"transparent": 0, "partial": 0, "opaque": 1})

    def test_the_blind_spot_is_the_frames_lowest_alpha_not_its_highest(self):
        # Two pixels, one opaque and one nearly transparent. The worst case is
        # the transparent one; reporting the other way round would call this
        # frame lossless.
        straight = bytes((10, 20, 30, 255, 10, 20, 30, 1))
        premultiplied = b"".join(
            bytes(_premultiplied_rgba8(straight[index:index + 4]))
            for index in range(0, len(straight), 4))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(straight)
            Image.frombytes("RGBA", (2, 1), premultiplied).save(png)
            report = MODULE.compare(raw, png, 2, 1, raw_alpha="straight",
                                    render_alpha="premultiplied")
        self.assertEqual(
            report["alpha_association"]["worst_case_hidden_straight_step"], 127)
        self.assertFalse(report["alpha_association"]["association_is_lossless"])

    def test_an_opaque_integer_association_is_reported_lossless(self):
        straight = bytes((10, 20, 30, 255))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(straight)
            Image.frombytes("RGBA", (1, 1),
                            bytes(_premultiplied_rgba8(straight))).save(png)
            report = MODULE.compare(raw, png, 1, 1, raw_alpha="straight",
                                    render_alpha="premultiplied")
        association = report["alpha_association"]
        self.assertEqual(association["worst_case_hidden_straight_step"], 0)
        self.assertTrue(association["association_is_lossless"])

    def test_an_infinite_alpha_is_not_lossless(self):
        # +inf premultiplies every non-zero colour to inf, so it collapses the
        # pixel outright; clamping it into [0,1] to get a floor would report
        # the least collapsing case for the most collapsing input.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(struct.pack("<8f", 0.25, 0.5, 0.75, float("inf"),
                                        0.9, 0.1, 0.2, 1.0))
            Image.frombytes("RGBA", (2, 1), bytes((64, 128, 192, 255,
                                                   230, 26, 51, 255))).save(png)
            report = MODULE.compare(raw, png, 2, 1, "rgba32f-le", tolerance=1e9,
                                    raw_alpha="straight",
                                    render_alpha="premultiplied")
        self.assertFalse(report["alpha_association"]["association_is_lossless"])

    def test_one_nan_alpha_unbounds_the_whole_side(self):
        # The other pixel's finite alpha must not carry a "lossless" verdict
        # for a frame that has a pixel the conversion destroyed.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(struct.pack("<8f", 0.25, 0.5, 0.75, float("nan"),
                                        0.9, 0.1, 0.2, 0.5))
            Image.frombytes("RGBA", (2, 1), bytes((64, 128, 192, 255,
                                                   115, 13, 26, 128))).save(png)
            report = MODULE.compare(raw, png, 2, 1, "rgba32f-le", tolerance=1e9,
                                    raw_alpha="straight",
                                    render_alpha="premultiplied")
        self.assertFalse(report["alpha_association"]["association_is_lossless"])
        self.assertIsNone(
            report["alpha_association"]["worst_case_hidden_straight_step"])

    def test_the_association_arguments_are_refused_before_anything_else(self):
        # A mistyped flag alongside a missing file and missing dimensions. Both
        # of the others are refusals the CLI reaches first unless the
        # argument-only check runs ahead of them, and the flag is the one the
        # operator can fix without touching the capture.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            absent = [str(TOOL), "--raw", str(root / "absent.rgba"),
                      "--render", str(root / "absent.png"), "--raw-alpha", "straight"]
            for extra in ([], ["--width", "1", "--height", "1"]):
                result = subprocess.run([sys.executable, *absent, *extra],
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 2)
                self.assertIn("go together", result.stderr)
            # The same invocation with both sides declared gets the next
            # refusal in line, which is what shows the ordering is real.
            paired = subprocess.run(
                [sys.executable, *absent, "--render-alpha", "premultiplied"],
                capture_output=True, text=True)
        self.assertEqual(paired.returncode, 2)
        self.assertIn("--width and --height", paired.stderr)

    def test_a_transparent_float_association_is_not_lossless(self):
        # No integer domain, so there is no step to name, but alpha 0 flattens
        # every colour to 0 all the same.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(struct.pack("<4f", 0.25, 0.5, 0.75, 0.0))
            Image.frombytes("RGBA", (1, 1), bytes((0, 0, 0, 0))).save(png)
            report = MODULE.compare(raw, png, 1, 1, "rgba32f-le", tolerance=1.0,
                                    raw_alpha="straight",
                                    render_alpha="premultiplied")
        association = report["alpha_association"]
        self.assertEqual(association["compared_in"], "premultiplied")
        self.assertIsNone(association["association_domain"])
        self.assertIsNone(association["worst_case_hidden_straight_step"])
        self.assertFalse(association["association_is_lossless"])

    def test_a_non_finite_sample_in_an_integer_domain_is_refused(self):
        # Unreachable through the formats this tool reads today, and a refusal
        # rather than the bare ValueError/OverflowError `round()` would raise
        # past main()'s handler if a future format made it reachable.
        with self.assertRaisesRegex(MODULE.InputError, "non-finite"):
            MODULE.premultiply([0.5, 0.5, 0.5, float("nan")], 255)
        with self.assertRaisesRegex(MODULE.InputError, "non-finite"):
            MODULE.premultiply([0.5, 0.5, float("inf"), 1.0], 255)

    def test_a_frame_with_no_opaque_pixel_does_not_claim_the_signature(self):
        # Every pixel partially transparent and genuinely different: there is
        # no opaque pixel for the mismatches to have spared.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(bytes((10, 20, 30, 128, 40, 50, 60, 128)))
            Image.frombytes("RGBA", (2, 1),
                            bytes((99, 88, 77, 128, 11, 22, 33, 128))).save(png)
            report = MODULE.compare(raw, png, 2, 1)
        self.assertFalse(report["match"])
        self.assertEqual(report["alpha_association"]["pixels"]["opaque"], 0)
        self.assertFalse(
            report["alpha_association"]["mismatches_spare_opaque_pixels"])
        self.assertNotIn("diagnostic", report["alpha_association"])

    def test_a_passing_comparison_does_not_tell_you_to_re_run_it(self):
        straight = bytes((58, 11, 123, 106, 200, 100, 50, 255))
        premultiplied = b"".join(
            bytes(_premultiplied_rgba8(straight[index:index + 4]))
            for index in range(0, len(straight), 4))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(straight)
            Image.frombytes("RGBA", (2, 1), premultiplied).save(png)
            report = MODULE.compare(raw, png, 2, 1, tolerance=1.0)
        self.assertTrue(report["match"])
        self.assertNotIn("diagnostic", report["alpha_association"])

    def test_an_unbounded_alpha_does_not_claim_a_lossless_association(self):
        # Every alpha on the converted side is NaN: nothing bounds the blind
        # spot, so the report must not answer 0 ("nothing is hidden") for it.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(struct.pack("<4f", 0.5, 0.5, 0.5, float("nan")))
            Image.frombytes("RGBA", (1, 1), bytes((128, 128, 128, 255))).save(png)
            report = MODULE.compare(raw, png, 1, 1, "rgba32f-le", tolerance=1.0,
                                    raw_alpha="straight",
                                    render_alpha="premultiplied")
        association = report["alpha_association"]
        self.assertEqual(association["compared_in"], "premultiplied")
        self.assertIsNone(association["worst_case_hidden_straight_step"])
        self.assertFalse(association["association_is_lossless"])

    def test_a_float_association_is_lossless_above_zero_alpha(self):
        # No integer domain to round in, so the multiply is exact and there is
        # no step to report - but the run did convert, and says so.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(struct.pack("<4f", 0.5, 0.25, 0.125, 1.0))
            Image.frombytes("RGBA", (1, 1), bytes((128, 64, 32, 255))).save(png)
            report = MODULE.compare(raw, png, 1, 1, "rgba32f-le", tolerance=1.0,
                                    raw_alpha="straight",
                                    render_alpha="premultiplied")
        association = report["alpha_association"]
        self.assertEqual(association["compared_in"], "premultiplied")
        self.assertIsNone(association["association_domain"])
        self.assertIsNone(association["worst_case_hidden_straight_step"])
        self.assertTrue(association["association_is_lossless"])

    def test_raw_u32_refuses_the_alpha_flags_instead_of_ignoring_them(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            exr = root / "render.exr"
            meta = root / "meta.json"
            raw.write_bytes(bytes(16))
            exr.write_bytes(b"")
            meta.write_text("{}", encoding="utf-8")
            result = subprocess.run(
                [sys.executable, str(TOOL), "--raw", str(raw), "--render", str(exr),
                 "--raw-u32", "--raw-metadata", str(meta),
                 "--render-metadata", str(meta), "--raw-alpha", "straight",
                 "--render-alpha", "premultiplied"],
                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("do not apply", result.stderr)

    def test_declaring_one_side_alone_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(bytes((0, 0, 0, 255)))
            Image.new("RGBA", (1, 1), (0, 0, 0, 255)).save(png)
            for kwargs in ({"raw_alpha": "straight"},
                           {"render_alpha": "premultiplied"}):
                with self.assertRaisesRegex(MODULE.InputError, "go together"):
                    MODULE.compare(raw, png, 1, 1, **kwargs)

    def test_integer_maximum_maps_each_format_to_its_own_domain(self):
        self.assertEqual(MODULE.integer_maximum("rgba8"), 255)
        self.assertEqual(MODULE.integer_maximum("argb8"), 255)
        self.assertEqual(MODULE.integer_maximum("png_rgba8"), 255)
        self.assertEqual(MODULE.integer_maximum("png_rgba16"), 65535)
        # AE's 16-bit channel domain is 0..32768, not 0..65535, and does not
        # take the --raw-integer-max override.
        self.assertEqual(MODULE.integer_maximum("argb16le-ae", 4095), 32768)
        self.assertEqual(MODULE.integer_maximum("rgba16le", 4095), 4095)
        # Float formats have no integer domain to round in.
        self.assertIsNone(MODULE.integer_maximum("rgba32f-le"))
        self.assertIsNone(MODULE.integer_maximum("argb32f-le"))
        self.assertIsNone(MODULE.integer_maximum("exr"))
        with self.assertRaises(MODULE.InputError):
            MODULE.integer_maximum("rgba16le", 0)

    def test_premultiply_rounds_in_the_integer_domain(self):
        # 58 * 106 / 255 = 24.11; the integer rule floors (v*a+127)/255 to 24.
        # Multiplying the normalized floats instead lands on 24.11/255, which
        # is a mismatch against an AE export at tolerance 0.
        associated = MODULE.premultiply([58 / 255, 11 / 255, 123 / 255, 106 / 255], 255)
        self.assertEqual(associated[0], 24 / 255)
        self.assertEqual(associated[3], 106 / 255)
        self.assertNotAlmostEqual(associated[0], (58 / 255) * (106 / 255), places=6)
        # 11 * 106 / 255 = 4.57: half-up gives 5, truncation 4.
        self.assertEqual(associated[1], 5 / 255)
        # Half-up is not Python's round(), which is banker's. 5 * 16384 / 32768
        # is exactly 2.5: half-up gives 3, round() gives 2.
        deep = MODULE.premultiply([5 / 32768, 0.0, 0.0, 16384 / 32768], 32768)
        self.assertEqual(deep[0], 3 / 32768)

    def test_premultiply_without_an_integer_domain_is_a_plain_product(self):
        associated = MODULE.premultiply([0.5, 0.25, 1.0, 0.5], None)
        self.assertEqual(associated[:3], [0.25, 0.125, 0.5])
        self.assertEqual(associated[3], 0.5)

    def test_unknown_alpha_association_is_refused(self):
        # Both sides given, so the pairing check cannot be what refuses these:
        # an unknown name must be rejected on its own, not fall through to the
        # "premultiplied" branch.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "expected.rgba"
            png = root / "actual.png"
            raw.write_bytes(bytes((0, 0, 0, 255)))
            Image.new("RGBA", (1, 1), (0, 0, 0, 255)).save(png)
            for kwargs in ({"raw_alpha": "matted", "render_alpha": "premultiplied"},
                           {"raw_alpha": "straight", "render_alpha": "matted"},
                           {"raw_alpha": "matted", "render_alpha": "matted"}):
                with self.assertRaisesRegex(MODULE.InputError, "must be one of"):
                    MODULE.compare(raw, png, 1, 1, **kwargs)

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
