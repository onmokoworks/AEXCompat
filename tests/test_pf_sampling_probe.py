import hashlib
import json
import subprocess
import unittest
from pathlib import Path

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


def _expected_output(raw, width, height):
    def pixel(x, y):
        if x < 0 or y < 0 or x >= width or y >= height:
            return (0, 0, 0, 0)
        offset = (y * width + x) * 4
        return tuple(raw[offset:offset + 4])

    x, y = width // 2, height // 2
    neighbors = [pixel(x, y), pixel(x + 1, y),
                 pixel(x, y + 1), pixel(x + 1, y + 1)]
    subpixel = tuple(round(sum(p[c] for p in neighbors) / 4) for c in range(4))
    alpha_sum = sum(p[3] for p in neighbors)
    area = tuple(round(sum(p[c] * p[3] for p in neighbors) / alpha_sum)
                 for c in range(3)) + (round(alpha_sum / 4),)
    samples = [pixel(x, y), pixel(x + 1, y + 1), subpixel, area,
               pixel(0, y), (0, 0, 0, 0)]
    return bytes(channel for row in range(height) for column in range(width)
                 for channel in samples[column % len(samples)])


@pytest.mark.parametrize(("mode", "format_name"), (
    ("--render-image", "argb8"),
    ("--render-image16", "argb16"),
    ("--render-image32", "argb32f"),
))
def test_real_probe_depth_matrix_has_numeric_oracle(tmp_path, mode, format_name):
    output = tmp_path / f"sampling-{format_name}.rgba"
    completed = subprocess.run([
        str(WORKER), mode, str(AEX), hashlib.sha256(AEX.read_bytes()).hexdigest(),
        "v5|", str(INPUT), str(output), "37", "23", "0", "1", "1", "1",
    ], cwd=ROOT, text=True, capture_output=True, timeout=30)
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["pixel_format"] == format_name
    assert report["suite_acquires"] == report["suite_releases"] == 1
    assert report["suite_leases_balanced"] is True
    assert report["guard_bytes_intact"] is True
    assert report["last_seh_exception_code"] == 0
    assert output.read_bytes() == _expected_output(INPUT.read_bytes(), 37, 23)
