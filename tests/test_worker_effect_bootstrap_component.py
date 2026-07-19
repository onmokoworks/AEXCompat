from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HEADER = (ROOT / "minihost" / "src" / "worker_effect_bootstrap.hpp").read_text(
    encoding="utf-8")
SOURCE = (ROOT / "minihost" / "src" / "worker_effect_bootstrap.cpp").read_text(
    encoding="utf-8")


def test_bootstrap_owns_stable_abi_buffers_and_callback_offsets():
    assert "alignas(8) std::array<std::byte, 408> input" in HEADER
    assert "alignas(8) std::array<std::byte, 552> utils" in HEADER
    assert "kInputCallbackOffsets" in SOURCE
    assert "kUtilityCallbackOffsets" in SOURCE
    assert "kUtilityColorCallbacksOffset + 64 == kUtilityPlatformDataOffset" in SOURCE
    assert "write<void*>(state.input, 176, state.utils.data())" in SOURCE


def test_selector_order_and_error_priority_remain_explicit():
    global_setup = SOURCE.index("hooks.invoke(entry, 1")
    about = SOURCE.index("hooks.invoke(entry, 0")
    params_setup = SOURCE.index("hooks.invoke(entry, 4")
    assert global_setup < about < params_setup
    assert "result.global_error == 0" in SOURCE[about - 100:about]
    assert "result.global_error == 0" in SOURCE[params_setup - 100:params_setup]
    assert "set_global_setup_active(false)" in SOURCE


def test_global_data_flags_depth_audio_and_parameter_contract_are_owned():
    for marker in (
        "configure_audio_admission",
        "image_render_supported",
        "nop_render_advertised",
        "input_write_advertised",
        "expand_buffer_advertised",
        "shrink_buffer_advertised",
        "depth_supported",
        "smart_render_supported",
        "update_params_ui_advertised",
        "query_dynamic_flags_advertised",
        "write(state.input, 312, read<void*>(state.output, 40))",
        "parameter_count_contract_valid",
    ):
        assert marker in SOURCE
