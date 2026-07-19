from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
ABI = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"
REGISTRY = ROOT / "minihost" / "src" / "worker_aegp_render_options.cpp"
RENDER_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_staged_item_runtime.cpp"
ASYNC_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_async_layer_runtime.cpp"


def test_layer_render_options_suite1_has_exact_typed_14_slot_abi():
    text = ABI.read_text(encoding="utf-8")
    assert "struct AegpLayerRenderOptionsSuite1" in text
    assert "sizeof(AegpLayerRenderOptionsSuite1) == 14 * sizeof(void*)" in text
    assert "AEXCOMPAT_ASSERT_LAYER1_SLOT(new_from_layer, 0)" in text
    assert "AEXCOMPAT_ASSERT_LAYER1_SLOT(get_matte, 13)" in text
    assert "std::array<void*, 14> g_layer_render_options_suite1" not in text
    main = SOURCE.read_text(encoding="utf-8")
    for function in (
        "new_layer_render_options",
        "new_from_upstream_of_effect",
        "duplicate_layer_render_options",
        "dispose_layer_render_options",
        "set_layer_render_time",
        "get_layer_render_time",
        "set_layer_render_time_step",
        "get_layer_render_time_step",
        "set_layer_render_world_type",
        "get_layer_render_world_type",
        "set_layer_render_downsample",
        "get_layer_render_downsample",
        "set_layer_render_matte",
        "get_layer_render_matte",
    ):
        assert f"&{function}" in main


def test_layer_render_options_registry_is_bounded_and_aba_resistant():
    text = REGISTRY.read_text(encoding="utf-8")
    for marker in (
        "kMaxLayerOptions = 256",
        "std::atomic<uint64_t> g_layer_generation{1}",
        "next_handle(g_layer_generation, 2, 2)",
        "std::unordered_map<uintptr_t, LayerValue> g_layers",
        "g_layers.size() >= kMaxLayerOptions",
        "g_layers.erase(i)",
    ):
        assert marker in text


def test_layer_render_paths_snapshot_each_independent_handle():
    text = (SOURCE.read_text(encoding="utf-8") + RENDER_RUNTIME.read_text(encoding="utf-8") +
            ASYNC_RUNTIME.read_text(encoding="utf-8"))
    assert text.count("snapshot_layer_render_options(options, snapshot)") >= 3
    assert "g_hooks.snapshot_options(options, snapshot)" in text
    assert "request->options = snapshot" in text
    assert "snapshot.world_type == 1 ? 4" in text
    assert "publish_loaded_layer_receipt(snapshot, out)" in text
    assert "g_layer_render_world_type" not in text
    assert "g_layer_render_options_live" not in text
