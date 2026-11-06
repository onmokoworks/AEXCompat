import hashlib
import json
import struct
import subprocess
import unittest
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render

import pytest


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "instruments" / "pf-sampling-probe"
SOURCE = PROBE / "pf_sampling_probe.cpp"
WORKER = ROOT / "target/minihost-build/aex_worker.exe"
AEX = ROOT / "target/pf-sampling-probe-build/Release/pf_sampling_probe.aex"




if __name__ == "__main__":
    unittest.main()


def _constant_output(width, height, format_name):
    # This AEX integration check uses an opaque unit basis so its result follows
    # from a constant field. Nonuniform alpha, edge colors and float rounding
    # are tested against captured AE words by worker_pf_sampling_wiring_selftest.
    maximum = {"argb8": 255, "argb16": 32768, "argb32f": 1.0}[format_name]
    value = (maximum, 0, maximum, maximum)
    layout = {"argb8": "<4B", "argb16": "<4H", "argb32f": "<4f"}[format_name]
    return b"".join(struct.pack(layout, *((0, 0, 0, 0) if column % 6 == 5 else value))
                    for row in range(height) for column in range(width))


@pytest.mark.parametrize(("pixel_format", "format_name"), (
    ("argb8", "argb8"),
    ("argb16", "argb16"),
    ("argb32f", "argb32f"),
))
def test_real_probe_depth_matrix_has_numeric_oracle(tmp_path, pixel_format, format_name):
    assert_artifact_fresh(AEX, SOURCE, WORKER, HARNESS)
    output = tmp_path / f"sampling-{format_name}.rgba"
    input_path = tmp_path / "opaque-basis.rgba"
    input_path.write_bytes(bytes((255, 0, 255, 255)) * (37*23))
    report = run_session_render(
        tmp_path, AEX, input_path, output, width=37, height=23,
        pixel_format=pixel_format,
    )
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["pixel_format"] == format_name
    assert report["suite_acquires"] == report["suite_releases"] == 1
    assert report["suite_leases_balanced"] is True
    assert report["guard_bytes_intact"] is True
    assert report["last_seh_exception_code"] == 0
    assert output.read_bytes() == _constant_output(37, 23, format_name)
