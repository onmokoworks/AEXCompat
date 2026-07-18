import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
SCENE_SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene_selftests.cpp"


def scene_source() -> str:
    return SOURCE.read_text(encoding="utf-8") + SCENE_SELFTEST_SOURCE.read_text(encoding="utf-8")
BUILD = ROOT / "target" / "minihost-build"


def test_pf_interface_slot_3_uses_exact_sdk_shape_and_offset():
    source = scene_source()
    assert "int32_t __cdecl get_effect_camera(\n    void* effect, const AegpTime* comp_time, void** camera_layer)" in source
    assert "decltype(&get_effect_camera) get_effect_camera;" in source
    assert "offsetof(PfInterfaceSuite, get_effect_camera) == 3 * sizeof(void*)" in source
    assert "offsetof(PfInterfaceSuite, get_effect_camera) == 24" in source
    assert "&convert_effect_to_comp_time, &get_effect_camera" in source


def test_pf_interface_slot_4_camera_matrix_uses_exact_sdk_shape_and_offset():
    source = scene_source()
    assert "struct AegpMatrix4 { double mat[4][4]{}; };" in source
    assert "static_assert(sizeof(AegpMatrix4) == 16 * sizeof(double));" in source
    assert "decltype(&get_effect_camera_matrix) get_effect_camera_matrix;" in source
    assert "offsetof(PfInterfaceSuite, get_effect_camera_matrix) == 32" in source
    assert "&get_effect_camera_matrix};" in source


def test_camera_matrix_is_atomic_bounded_and_deterministic():
    source = scene_source()
    assert "!camera_matrix || !distance_to_image_plane || !image_plane_width" in source
    assert "width > INT16_MAX || height > INT16_MAX" in source
    assert "result.mat[index][index] = 1.0" in source
    assert "*distance_to_image_plane = static_cast<double>(width);" in source
    assert "std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0" in source


def test_camera_lookup_is_fail_closed_and_preserves_output_on_error():
    source = scene_source()
    assert "effect != &g_effect || !g_pf_state_effect_live" in source
    assert "!valid_comp_time(*comp_time)" in source
    assert "if (index >= g_aegp_layers.size()) return 4;" in source
    assert "if (layer_active_at_time(index, *comp_time)) result = &g_aegp_layers[index];" in source
    body = source[source.index("int32_t __cdecl get_effect_camera(\n    void* effect, const AegpTime* comp_time, void** camera_layer) {") :]
    body = body[: body.index("struct AegpSelectionCollection")]
    assert body.index("if (effect !=") < body.index("*camera_layer = result")
    assert "*camera_layer = nullptr" not in body
    assert "get_effect_camera(&g_effect, &before_in, &camera) == 0 && camera == nullptr" in source


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
