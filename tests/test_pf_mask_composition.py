import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RUNTIME_SOURCE = ROOT / "minihost" / "src" / "worker_pf_path_runtime.cpp"
RUNTIME_HEADER = ROOT / "minihost" / "src" / "worker_pf_path_runtime.hpp"
SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_pf_path_selftests.cpp"
SELFTEST_HEADER = ROOT / "minihost" / "src" / "worker_pf_path_selftests.hpp"
CALLBACK_SOURCE = ROOT / "minihost" / "src" / "worker_mask_runtime_callbacks.cpp"
ROUTING_SOURCE = ROOT / "minihost" / "src" / "worker_custom_selftest_routing.cpp"
CMAKE = ROOT / "minihost" / "CMakeLists.txt"


def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)


def test_mask_composition_engine_pins_the_documented_host_policy():
    source = RUNTIME_SOURCE.read_text(encoding="utf-8")
    assert "int32_t __cdecl mask_world_with_scene(" in source
    assert "verified against an After Effects oracle" in source
    assert "has_add ? 0.0 : 1.0" in source
    assert "a = std::max(a, c);" in source
    assert "a = a * (1 - c);" in source
    assert "a = std::min(a, c);" in source
    assert "if (path.mode == 0) continue;" in source


def test_mask_composition_is_fail_closed_in_source():
    source = RUNTIME_SOURCE.read_text(encoding="utf-8")
    composition = source[source.index("int32_t __cdecl mask_world_with_scene(") :]
    assert "path.mode < 0 || path.mode > 7 || path.mode >= 4" in composition
    assert "g_report.reject_reason = 11;" in composition
    assert "g_report.reject_reason = 12;" in composition
    assert "path.open || !checked(path.handle, curve)" in composition
    assert "catch (const std::bad_alloc&)" in composition
    assert "lock(g_mutex)" in composition
    assert "kPixelFormatArgb128" in composition


def test_mask_composition_enumeration_carries_opacity():
    header = RUNTIME_HEADER.read_text(encoding="utf-8")
    callbacks = CALLBACK_SOURCE.read_text(encoding="utf-8")
    assert "double opacity{100.0};" in header
    assert "uint32_t composition_calls{};" in header
    assert "mask_world_with_scene" in header
    assert "mask->invert, mask->mode, mask->opacity" in callbacks


def test_mask_composition_selftest_is_wired_as_a_true_translation_unit():
    implementation = SELFTEST_SOURCE.read_text(encoding="utf-8")
    header = SELFTEST_HEADER.read_text(encoding="utf-8")
    routing = ROUTING_SOURCE.read_text(encoding="utf-8")
    assert "bool verify_pf_mask_composition(" in implementation
    assert "bool verify_pf_mask_composition(" in header
    assert "g_mask_scene[index].mode" in implementation
    assert '--self-test-pf-mask-composition' in routing
    assert "run_pf_mask_composition" in routing
    assert "src/worker_pf_path_selftests.cpp" in CMAKE.read_text(encoding="utf-8")


def test_mask_composition_runtime_self_test():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(worker), "--self-test-pf-mask-composition"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    report = json.loads(completed.stdout)
    assert report == {
        "pf_mask_composition": "passed",
        "composition_calls": 10,
        "balanced": True,
    }
