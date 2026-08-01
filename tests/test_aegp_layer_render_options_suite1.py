from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_SOURCE
ABI = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"
REGISTRY = ROOT / "minihost" / "src" / "worker_aegp_render_options.cpp"
RENDER_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_staged_item_runtime.cpp"
ASYNC_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_async_layer_runtime.cpp"
LAYER_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_layer_render_runtime.cpp"


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
    host = source_owners.contract_text("aegp_receipt_callbacks")
    layer = LAYER_RUNTIME.read_text(encoding="utf-8")
    async_runtime = ASYNC_RUNTIME.read_text(encoding="utf-8")
    assert host.count("snapshot_layer_render_options(options, snapshot)") >= 3
    assert "g_hooks.snapshot_options(options, snapshot)" in async_runtime
    assert "request->options = snapshot" in async_runtime
    assert "publish_from_context" in layer
    assert "g_layer_render_world_type" not in host + layer
    assert "g_layer_render_options_live" not in host + layer
