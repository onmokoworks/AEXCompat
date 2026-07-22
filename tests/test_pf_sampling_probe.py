import hashlib
import json
import math
import struct
import subprocess
import unittest
from pathlib import Path

from _render_session import run_session_render

import pytest


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "instruments" / "pf-sampling-probe"
WORKER = ROOT / "target/minihost-build/aex_render_worker.exe"
AEX = ROOT / "target/pf-sampling-probe-build/Release/pf_sampling_probe.aex"
INPUT = ROOT / "target/gpu-effects/opencl-input.rgba"


class PfSamplingProbeSourceTest(unittest.TestCase):
    def test_probe_uses_normal_render_and_all_depth_suites(self):
        source = (PROBE / "pf_sampling_probe.cpp").read_text(encoding="utf-8")
        self.assertIn("case PF_Cmd_RENDER:", source)
        self.assertNotIn("PF_Cmd_SMART_RENDER", source)
        for suite in (
            "PF_Sampling8Suite1",
            "PF_Sampling16Suite1",
            "PF_SamplingFloatSuite1",
        ):
            self.assertIn(suite, source)
        for suite_name in (
            "kPFSampling8Suite",
            "kPFSampling16Suite",
            "kPFSamplingFloatSuite",
        ):
            self.assertIn(suite_name, source)
        self.assertIn("AcquireSuite", source)
        self.assertIn("ReleaseSuite", source)

    def test_oracle_covers_point_area_edge_and_outside_cases(self):
        source = (PROBE / "pf_sampling_probe.cpp").read_text(encoding="utf-8")
        for marker in (
            "kNearest",
            "kNearestHalf",
            "kSubpixelHalf",
            "kArea",
            "kEdge",
            "kOutside",
            "PF_SampleEdgeBehav_ZERO",
            "x_radius = kHalf",
            "y_radius = kHalf",
            "params.area = kOne",
        ):
            self.assertIn(marker, source)
        for callback in (
            "nn_sample(",
            "subpixel_sample(",
            "area_sample(",
            "nn_sample16(",
            "subpixel_sample16(",
            "area_sample16(",
            "nn_sample_float(",
            "subpixel_sample_float(",
            "area_sample_float(",
        ):
            self.assertIn(callback, source)
        self.assertIn("x % static_cast<A_long>(SampleCase::kCount)", source)
        self.assertIn("Oracle<Pixel>", source)
        self.assertIn("OracleMatches", source)

    def test_build_is_standalone_and_reproducible(self):
        cmake = (PROBE / "CMakeLists.txt").read_text(encoding="utf-8")
        script = (ROOT / "tools" / "build-pf-sampling-probe.ps1").read_text(
            encoding="utf-8"
        )
        self.assertIn("add_library(pf_sampling_probe MODULE", cmake)
        self.assertIn("PF_DEEP_COLOR_AWARE=1", cmake)
        self.assertIn("& $CMake -S $source -B $build", script)
        self.assertIn("--target pf_sampling_probe", script)
        self.assertIn("Get-FileHash", script)


if __name__ == "__main__":
    unittest.main()


def _float32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def _lround(value):
    lower = math.floor(value)
    return int(lower) + (1 if value - lower >= 0.5 else 0)


def _expected_output(raw, width, height, format_name):
    # Mirrors the worker exactly: rgba8_to_argb promotion into the input world,
    # then nearest/subpixel/area sampling in double, then the per-depth store
    # (lround with clamp for integer worlds, a float cast for the float world).
    maximum = {"argb8": 255.0, "argb16": 32768.0, "argb32f": 1.0}[format_name]
    float_world = format_name == "argb32f"

    def promote(value):
        if format_name == "argb16":
            return float((value * 32768 + 127) // 255)
        if float_world:
            return _float32(value / 255.0)
        return float(value)

    def pixel(x, y):
        if x < 0 or y < 0 or x >= width or y >= height:
            return (0.0, 0.0, 0.0, 0.0)
        offset = (y * width + x) * 4
        return tuple(promote(value) for value in raw[offset:offset + 4])

    def quantize(values):
        if float_world:
            return tuple(_float32(value) for value in values)
        return tuple(min(max(_lround(value), 0), int(maximum))
                     for value in values)

    x, y = width // 2, height // 2

    def subpixel():
        values = []
        for channel in range(4):
            value = 0.0
            for dy in (0, 1):
                for dx in (0, 1):
                    value += pixel(x + dx, y + dy)[channel] * 0.25
            values.append(value)
        return quantize(values)

    def area():
        weighted_alpha = 0.0
        weighted_color = [0.0, 0.0, 0.0]
        for dy in (0, 1):
            for dx in (0, 1):
                sample = pixel(x + dx, y + dy)
                alpha = sample[3] / maximum
                weighted_alpha += 0.25 * alpha
                for channel in range(3):
                    weighted_color[channel] += 0.25 * alpha * sample[channel]
        colors = [weighted_color[channel] / weighted_alpha
                  if weighted_alpha > 0.0 else 0.0 for channel in range(3)]
        return quantize(colors + [weighted_alpha / 1.0 * maximum])

    samples = [quantize(pixel(x, y)), quantize(pixel(x + 1, y + 1)),
               subpixel(), area(), quantize(pixel(0, y)),
               quantize((0.0, 0.0, 0.0, 0.0))]
    layout = {"argb8": "<4B", "argb16": "<4H", "argb32f": "<4f"}[format_name]
    return b"".join(struct.pack(layout, *samples[column % len(samples)])
                    for row in range(height) for column in range(width))


@pytest.mark.parametrize(("pixel_format", "format_name"), (
    ("argb8", "argb8"),
    ("argb16", "argb16"),
    ("argb32f", "argb32f"),
))
def test_real_probe_depth_matrix_has_numeric_oracle(tmp_path, pixel_format, format_name):
    output = tmp_path / f"sampling-{format_name}.rgba"
    report = run_session_render(
        tmp_path, AEX, INPUT, output, width=37, height=23,
        pixel_format=pixel_format,
    )
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["pixel_format"] == format_name
    assert report["suite_acquires"] == report["suite_releases"] == 1
    assert report["suite_leases_balanced"] is True
    assert report["guard_bytes_intact"] is True
    assert report["last_seh_exception_code"] == 0
    assert output.read_bytes() == _expected_output(
        INPUT.read_bytes(), 37, 23, format_name)
