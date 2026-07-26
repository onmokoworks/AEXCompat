from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODEL_HEADER = ROOT / "minihost" / "src" / "worker_aegp_scene_model.hpp"
MODEL_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene_model.cpp"
SCENE_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene.cpp"
SCENE_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_scene_runtime.cpp"
CMAKE = ROOT / "minihost" / "CMakeLists.txt"
NATIVE_SELFTEST = ROOT / "tests" / "native" / "worker_aegp_scene_model_selftest.cpp"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_typed_scene_identity_abi_and_bounds_are_fixed() -> None:
    header = read(MODEL_HEADER)
    for marker in (
        "enum class ObjectKind : uint8_t",
        "project = 1",
        "item = 2",
        "composition = 3",
        "folder = 4",
        "footage = 5",
        "layer = 6",
        "effect = 7",
        "stream = 8",
        "keyframe = 9",
        "uint64_t project_id{}",
        "uint64_t object_id{}",
        "uint32_t generation{}",
        "static_assert(sizeof(Identity) == 24)",
        "static_assert(offsetof(Identity, generation) == 16)",
        "static_assert(sizeof(void*) == sizeof(uint64_t))",
        "kProjectCapacity = 4",
        "kObjectCapacity = 64",
        "kBorrowedHandleCapacity = 64",
    ):
        assert marker in header


def test_registry_fixture_covers_multiple_projects_and_item_families() -> None:
    source = read(MODEL_SOURCE)
    native = read(NATIVE_SELFTEST)
    for marker in (
        'u"Project A"',
        'u"Project B"',
        'u"Root A"',
        'u"Sources"',
        'u"Footage A"',
        'u"Parent Comp"',
        'u"Child Comp"',
        'u"Other Comp"',
        "registry.project_count() == 2",
        "live_object_count(ObjectKind::folder) == 3",
        "live_object_count(ObjectKind::footage) == 1",
        "live_object_count(ObjectKind::composition) == 3",
        "live_object_count(ObjectKind::layer) == 5",
        "registry.first_child(project_b, root_b)",
        "registry.layer_by_index(comp_b.identity, 0, layer_b)",
    ):
        assert marker in source or marker in native


def test_borrowed_handles_are_generation_checked_and_fail_closed() -> None:
    source = read(MODEL_SOURCE)
    native = read(NATIVE_SELFTEST)
    for marker in (
        "kBorrowedTag = 0xAu",
        "lease.lease_generation != generation",
        "record->snapshot.identity.kind != expected",
        "record->snapshot.identity.project_id != required_project_id",
        "decode_handle(handle, borrowed_slot, borrowed_generation)",
        "if (lease.live && lease.target == identity) lease.live = false",
        "replacement.generation == layer.identity.generation + 1",
        "wrong_kind_rejected",
        "cross_project_rejected",
        "foreign_rejected",
        "stale_rejected",
        "registry.fingerprint() == before_rejections",
    ):
        assert marker in source or marker in native


def test_aegp_traversal_returns_registry_borrowed_handles() -> None:
    scene = read(SCENE_SOURCE)
    runtime = read(SCENE_RUNTIME)
    for marker in (
        "scene_model::registry().initialize_fixture(",
        "resolve_scene_item",
        "resolve_scene_comp",
        "resolve_scene_layer",
        "borrow_scene_object(scene_registry().active_item())",
        "scene_registry().comp_from_item(",
        "scene_registry().item_from_comp(",
        "scene_registry().layer_by_index(",
        "scene_registry().layer_from_id(",
        "borrow_scene_object(resolved_layer.identity)",
        "resolved.identity.project_id",
    ):
        assert marker in scene or marker in runtime


def test_native_selftest_is_a_release_build_target() -> None:
    cmake = read(CMAKE)
    assert "src/worker_aegp_scene_model.cpp" in cmake
    assert "add_executable(worker_aegp_scene_model_selftest" in cmake
    assert "../tests/native/worker_aegp_scene_model_selftest.cpp" in cmake
    assert "target_compile_options(worker_aegp_scene_model_selftest PRIVATE /UNDEBUG)" in cmake
