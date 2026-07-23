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


def test_scene_host_boundary_is_compiled_for_each_worker_target():
    cmake = CMAKE_SOURCE.read_text(encoding="utf-8")
    l2_source = SOURCE.read_text(encoding="utf-8")
    scene_source = (ROOT / "minihost" / "src" / "worker_aegp_scene.cpp").read_text(
        encoding="utf-8")
    selftest_source = SCENE_SELFTEST_SOURCE.read_text(encoding="utf-8")
    assert "src/worker_aegp_scene.cpp" in cmake
    assert "src/worker_aegp_scene_selftests.cpp" in cmake
    assert '#include "worker_aegp_scene.cpp"' not in l2_source
    assert "configure_scene_context" in scene_source
    assert "scene_selftests_translation_unit_linked" in selftest_source


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
    assert "bool valid_spatial_ratio(const aexcompat::render::SpatialRatio& ratio)" in source
    assert "bool valid_camera_spatial_context()" in source
    assert "if (!valid_camera_spatial_context()) return 4;" in source
    assert "width > INT16_MAX || height > INT16_MAX" in source
    assert "result.mat[index][index] = 1.0" in source
    assert "*distance_to_image_plane = static_cast<double>(width);" in source
    assert "std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0" in source


def test_active_camera_matrix_uses_scene_world_transform_and_fail_closed():
    source = (ROOT / "minihost" / "src" / "worker_aegp_pf_interface_suite.cpp").read_text(
        encoding="utf-8")
    assert "bool invert_affine_matrix(const AegpMatrix4& input, AegpMatrix4& output)" in source
    assert "aegp_get_layer_to_world_xform(camera_layer, comp_time, &world)" in source
    assert "std::abs(determinant) <= kDeterminantEpsilon" in source
    assert "!invert_affine_matrix(world, result)" in source
    assert "*camera_matrix = result" in source


def test_camera_lookup_is_fail_closed_and_preserves_output_on_error():
    source = scene_source()
    assert "effect != &g_effect || !effect_is_live()" in source
    assert "!valid_comp_time(*comp_time)" in source
    assert "if (index >= g_aegp_layers.size()) return 4;" in source
    assert "if (layer_active_at_time(index, *comp_time)) result = &g_aegp_layers[index];" in source
    body = source[source.index("int32_t __cdecl get_effect_camera(\n    void* effect, const AegpTime* comp_time, void** camera_layer) {") :]
    body = body[: body.index("int32_t __cdecl get_effect_camera_matrix")]
    assert body.index("if (effect !=") < body.index("*camera_layer = result")
    assert "*camera_layer = nullptr" not in body
    assert "get_effect_camera(&g_effect, &before_in, &camera) == 0 && camera == nullptr" in source


def test_single_layer_authored_transform_is_bounded_and_fail_closed():
    source = scene_source() + (ROOT / "minihost" / "src" / "worker_aegp_scene.cpp").read_text(
        encoding="utf-8")
    assert "struct AegpLayerTransform" in source
    assert "std::array<AegpLayerTransform, 3> layer_transforms{}" in source
    assert "T(position) * Rz * Ry * Rx" in source
    assert "authored.scale[index] == 0.0" in source
    assert "finite_bounded(authored.rotation_degrees[index], kRotationLimit)" in source
    assert "std::isfinite(result.mat[row][column])" in source
    assert "*transform = result" in source
    assert "std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0" in source


def test_parent_layer_transform_chain_is_bounded_and_fail_closed():
    source = scene_source() + (ROOT / "minihost" / "src" / "worker_aegp_scene.cpp").read_text(
        encoding="utf-8")
    assert "std::array<int32_t, 3> layer_parent_indices{{-1, -1, -1}}" in source
    assert "constexpr std::size_t kMaxParentDepth = 8" in source
    assert "visited[current]" in source
    assert "world = multiply(local, world)" in source
    assert "g_aegp_layer_parent_indices" in source
    assert "parent_index < 0" in source
    assert "parent_index >= 0" not in source


def test_animated_layer_transform_snapshot_is_rational_and_fail_closed():
    source = (ROOT / "minihost" / "src" / "worker_aegp_scene.cpp").read_text(
        encoding="utf-8")
    runtime = (ROOT / "minihost" / "src" / "worker_aegp_scene_runtime.hpp").read_text(
        encoding="utf-8")
    assert "struct AegpLayerTransformKeyframe" in runtime
    assert "layer_transform_keyframes" in runtime
    assert "resolve_layer_transform" in source
    assert "first_time < second_time" in source
    assert "current_time <= first_time" in source
    assert "current_time >= second_time" in source
    assert "blend(output.position" in source
    assert "!keyframes[0].valid || !keyframes[1].valid" in source


def test_camera_zoom_snapshot_is_rational_and_fail_closed():
    source = (ROOT / "minihost" / "src" / "worker_aegp_scene.cpp").read_text(
        encoding="utf-8")
    runtime = (ROOT / "minihost" / "src" / "worker_aegp_scene_runtime.hpp").read_text(
        encoding="utf-8")
    assert "struct AegpCameraZoomKeyframe" in runtime
    assert "layer_camera_zoom_keyframes" in runtime
    assert "resolve_layer_camera_zoom" in source
    assert "kZoomLimit" in source
    assert "fallback <= 0.0" in source
    assert "!(first_time < second_time)" in source
    assert "value->one_d = zoom" in source


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
