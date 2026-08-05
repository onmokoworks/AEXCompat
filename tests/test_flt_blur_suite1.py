"""Contract tests for the private AE 2026 ``FLT Blur Suite`` version 1.

Bounded call-site observation of Box_Blur, Gaussian_Blur, RoughenEdges, and
Simple_Choker established a two-slot x64 table. Slot 0 is Gaussian blur and
slot 1 is repeated box blur. Both take borrowed source/destination worlds;
the source remains immutable and only the destination pixels are written.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
HEADER = ROOT / "minihost/src/worker_flt_blur_suite.hpp"
SOURCE = ROOT / "minihost/src/worker_flt_blur_suite.cpp"
WIRING = ROOT / "minihost/src/worker_host_suite_wiring.cpp"
ENTRY = ROOT / "minihost/src/l2_main_entry.inc"
GENERATOR = ROOT / "tools/generate-aex-abi-contract.py"


def test_flt_blur_suite1_abi_and_normal_wiring_are_explicit() -> None:
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    wiring = WIRING.read_text(encoding="utf-8")
    entry = ENTRY.read_text(encoding="utf-8")
    generator = GENERATOR.read_text(encoding="utf-8")

    assert 'kSuiteName[] = "FLT Blur Suite"' in header
    assert "kSuiteVersion1 = 1" in header
    assert "struct Suite1" in header
    assert "GaussianBlur gaussian_blur" in header
    assert "BoxBlur box_blur" in header
    assert "sizeof(Suite1) == 2 * sizeof(void*)" in header
    assert "source is read-only" in header
    assert "destination is the only caller-owned storage written" in header
    assert "PF_Err_BAD_CALLBACK_PARAM (4)" in header

    assert "effect_ref != g_hooks.effect_ref" in source
    assert "source.pixel_format != destination.pixel_format" in source
    assert "source.width != destination.width" in source
    assert "valid_progress(progress_base, progress_final)" in source
    assert "catch (const std::bad_alloc&)" in source
    assert "return aexcompat::flt_blur::suite1();" in wiring
    assert "aexcompat::flt_blur::kSuiteName" in wiring

    # RoughenEdges consumes this legacy utility callback immediately after
    # its blur. The implementation already existed; the ABI bootstrap must
    # place it at the observed PF_UtilCallbacks + 0x80 slot.
    assert '"utils.iterate_origin"' in generator
    assert "reinterpret_cast<void*>(&iterate_origin8)" in entry


def test_native_flt_blur_suite1_passes_all_three_workers() -> None:
    expected = {
        "flt_blur_suite1": "passed",
        "slots": 2,
        "formats": ["argb8", "argb16", "argb32f"],
        "source_borrowed": True,
        "destination_borrowed": True,
        "fail_closed": True,
    }
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-flt-blur-suite1"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout) == expected
