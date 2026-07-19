from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
ABI_PROBE = ROOT / "instruments" / "abi-layout-probe" / "main.cpp"
ABI = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"
REGISTRY = ROOT / "minihost" / "src" / "worker_aegp_render_options.cpp"
STAGED_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_staged_item_runtime.cpp"
EXTERNAL_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_external_render_runtime.cpp"
ASYNC_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_async_layer_runtime.cpp"


def test_sdk_probe_freezes_all_layer_render_options_suite2_slots():
    text = ABI_PROBE.read_text(encoding="utf-8")
    assert "sizeof(AEGP_LayerRenderOptionsSuite2) == 15 * sizeof(void*)" in text
    members = (
        "AEGP_NewFromLayer",
        "AEGP_NewFromUpstreamOfEffect",
        "AEGP_NewFromDownstreamOfEffect",
        "AEGP_Duplicate",
        "AEGP_Dispose",
        "AEGP_SetTime",
        "AEGP_GetTime",
        "AEGP_SetTimeStep",
        "AEGP_GetTimeStep",
        "AEGP_SetWorldType",
        "AEGP_GetWorldType",
        "AEGP_SetDownsampleFactor",
        "AEGP_GetDownsampleFactor",
        "AEGP_SetMatteMode",
        "AEGP_GetMatteMode",
    )
    for slot, member in enumerate(members):
        assert f"offsetof(AEGP_LayerRenderOptionsSuite2, {member})" in text
        assert f"{slot} * sizeof(void*)" in text


def test_minihost_publishes_typed_suite2_without_changing_suite1():
    text = SOURCE.read_text(encoding="utf-8")
    abi = ABI.read_text(encoding="utf-8")
    assert "sizeof(AegpLayerRenderOptionsSuite1) == 14 * sizeof(void*)" in abi
    assert "sizeof(AegpLayerRenderOptionsSuite2) == 15 * sizeof(void*)" in abi
    assert "AEXCOMPAT_ASSERT_LAYER2_SLOT(new_from_downstream_of_effect, 2)" in abi
    assert '{"AEGP Layer Render Options Suite", 2, nullptr' in text
    assert "&provide_layer_render_options2" in text
    assert "&new_from_downstream_of_effect" in text


def test_layer_checkout_applies_options_to_real_source_pixels():
    text = SOURCE.read_text(encoding="utf-8")
    body = text[text.index("int32_t publish_loaded_layer_receipt_from_context(") :]
    body = body[: body.index("int32_t publish_loaded_layer_receipt(")]
    for marker in (
        "options.time.value",
        "options.time_step.value <= 0",
        "options.downsample_x",
        "options.downsample_y",
        "options.world_type == 2",
        "selected_pixels->data()",
        "channels[c] *= channels[0]",
        "std::array<uint16_t, 4> values",
        "std::memcpy(destination, channels.data(), sizeof(channels))",
    ):
        assert marker in body


def test_effect_boundaries_accept_only_finalized_staged_downstream():
    text = SOURCE.read_text(encoding="utf-8")
    registry = REGISTRY.read_text(encoding="utf-8")
    assert "LayerEffectBoundary::upstream" in registry
    assert "LayerEffectBoundary::downstream" in registry
    assert "layer_effect_boundary_is_live(options)" in text
    assert "context.downstream_finalized" in text
    assert "wants_downstream && !context.downstream_finalized" in text
    assert "context.downstream_argb" in text
    assert "context.all_effects_finalized" in text
    assert "g_layers.size() >= kMaxLayerOptions" in registry


def test_sync_and_async_paths_share_the_same_pixel_publisher():
    text = (SOURCE.read_text(encoding="utf-8") +
            STAGED_RUNTIME.read_text(encoding="utf-8") +
            EXTERNAL_RUNTIME.read_text(encoding="utf-8") +
            ASYNC_RUNTIME.read_text(encoding="utf-8"))
    assert text.count("publish_loaded_layer_receipt(snapshot, out)") >= 2
    assert "request->options = snapshot" in text
    assert "g_hooks.publish(request->source, request->options, &receipt)" in text
    assert "context.downstream_finalized = true" in text


def test_native_selftest_covers_boundary_hashes_cycle_async_and_ownership():
    text = SOURCE.read_text(encoding="utf-8")
    body = text[text.index("bool verify_aegp_layer_render_options_suite2()") :]
    body = body[: body.index("int worker_main_impl(")]
    for marker in (
        "upstream_hash != all_hash",
        "upstream_hash != downstream_hash",
        "all_hash != downstream_hash",
        "render_checkout_layer_v5(downstream",
        "downstream_finalized = true",
        "render_checkout_layer_async_reject(downstream",
        "async_hash == downstream_hash",
        "checkin_frame(async_result.receipt)",
        "layer_created_count() == created_before + 3",
        "layer_disposed_count() == disposed_before + 3",
    ):
        assert marker in body
