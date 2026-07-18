from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")


def test_smart_pre_render_exposes_bounded_thread_safe_guid_mix_callback():
    assert "constexpr uint32_t kMaxGuidMixInBytes = 1024 * 1024" in SOURCE
    assert "std::atomic<uint32_t> g_guid_mix_in_calls" in SOURCE
    assert "int32_t __cdecl guid_mix_in_ptr" in SOURCE
    assert "effect_ref == &g_effect && bytes && size > 0" in SOURCE
    assert "size <= kMaxGuidMixInBytes ? 0 : 4" in SOURCE
    assert 'write<void*>(pre_callbacks, 8, reinterpret_cast<void*>(&guid_mix_in_ptr))' in SOURCE


def test_comp_suite_v10_slot_four_records_successes_and_rejections():
    assert 'std::strcmp(name, "AEGP Comp Suite") == 0 && version == 21' in SOURCE
    assert "g_aegp_comp_suite10[4] = reinterpret_cast<void*>(&aegp_get_comp_bg_color)" in SOURCE
    callback = SOURCE[SOURCE.index("int32_t __cdecl aegp_get_comp_bg_color") :]
    callback = callback[: callback.index("\n}")]
    assert "g_comp_bg_color_rejections.fetch_add" in callback
    assert "g_comp_bg_color_successes.fetch_add" in callback


def test_telemetry_is_reset_per_smart_render_and_is_additive_json():
    render = SOURCE
    assert render.index("reset_smart_host_telemetry();") < render.index("stage:smart_pre_render_begin")
    for field in (
        "comp_bg_color_success_count",
        "comp_bg_color_rejection_count",
        "guid_mix_in_call_count",
        "guid_mix_in_success_count",
        "guid_mix_in_rejection_count",
        "guid_mix_in_last_size",
        "guid_mix_in_max_size",
        "guid_mix_in_size_limit",
        "guid_mix_in_last_result",
    ):
        assert f'\\\"{field}\\\"' in SOURCE
