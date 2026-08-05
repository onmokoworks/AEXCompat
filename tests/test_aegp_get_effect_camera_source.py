import json
import subprocess
from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
CMAKE_SOURCE = ROOT / "minihost" / "CMakeLists.txt"
SCENE_SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene_selftests.cpp"
SCENE_SELFTEST_IMPL = ROOT / "minihost" / "src" / "worker_aegp_scene_selftests_impl.inc"
SCENE_HEADER = ROOT / "minihost" / "src" / "worker_aegp_scene.hpp"
SCENE_RUNTIME_HEADER = ROOT / "minihost" / "src" / "worker_aegp_scene_runtime.hpp"
PF_SUITE_SOURCE = ROOT / "minihost" / "src" / "worker_pf_suites_internal.hpp"
PF_STATE_SOURCE = ROOT / "minihost" / "src" / "worker_pf_state_runtime.cpp"
PF_INTERFACE_SUITE_HEADER = ROOT / "minihost" / "src" / "worker_aegp_pf_interface_suite.hpp"
PF_INTERFACE_SUITE_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_pf_interface_suite.cpp"

def scene_source() -> str:
    return "\n".join(path.read_text(encoding="utf-8") for path in
                     (SOURCE, SCENE_HEADER, SCENE_RUNTIME_HEADER,
                      SCENE_SELFTEST_SOURCE, SCENE_SELFTEST_IMPL, PF_SUITE_SOURCE,
                      PF_STATE_SOURCE, PF_INTERFACE_SUITE_HEADER,
                      PF_INTERFACE_SUITE_SOURCE))
BUILD = ROOT / "target" / "minihost-build"

def test_pf_interface_slot_3_uses_exact_sdk_shape_and_offset():
    source = scene_source()

def test_pf_interface_slot_4_camera_matrix_uses_exact_sdk_shape_and_offset():
    source = scene_source()

def test_camera_matrix_is_atomic_bounded_and_deterministic():
    source = scene_source()

def test_camera_matrix_distance_uses_active_camera_zoom_snapshot():
    source = scene_source()

def test_camera_lookup_is_fail_closed_and_preserves_output_on_error():
    source = scene_source()
    body = source[source.index("int32_t __cdecl get_effect_camera(\n    void* effect, const AegpTime* comp_time, void** camera_layer) {") :]
    body = body[: body.index("int32_t __cdecl get_effect_camera_matrix")]
    assert body.index("if (effect !=") < body.index("*camera_layer = result")
    assert "*camera_layer = nullptr" not in body

def test_native_self_test_covers_all_three_release_workers():
    for worker_name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / worker_name
        assert worker.exists(), f"build {worker_name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-aegp-get-effect-camera"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        result = json.loads(completed.stdout)
        assert result == {
            "aegp_get_effect_camera": "passed",
            "camera_slot": 3,
            "camera_offset_x64": 24,
            "matrix_slot": 4,
            "matrix_offset_x64": 32,
            "classic": "tested",
            "smart": "tested",
        }
