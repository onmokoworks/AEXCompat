from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HEADER = (ROOT / "minihost" / "src" / "worker_effect_bootstrap.hpp").read_text(
    encoding="utf-8")
SOURCE = (ROOT / "minihost" / "src" / "worker_effect_bootstrap.cpp").read_text(
    encoding="utf-8")
CONTRACT = (
    ROOT / "minihost" / "src" / "generated" / "aex_abi_contract.hpp"
).read_text(encoding="utf-8")


def test_bootstrap_owns_stable_abi_buffers_and_callback_offsets():
    assert "abi::x86_64_windows::PF_IN_DATA_SIZE> input" in HEADER
    assert "abi::x86_64_windows::PF_UTIL_CALLBACKS_SIZE> utils" in HEADER
    assert "contract::INPUT_CALLBACK_OFFSETS" in SOURCE
    assert "contract::UTILITY_CALLBACK_OFFSETS" in SOURCE
    assert "contract::UTILS_COLOR_CALLBACKS_OFFSET" in SOURCE
    assert "contract::IN_UTILS_OFFSET, state.utils.data()" in SOURCE


def test_utility_table_wires_handle_callbacks_at_sdk_offsets():
    # in_data->utils must expose host_new_handle/lock/unlock/dispose/get_size/
    # resize, not only the PF Handle Suite (issue #220). The offsets are the
    # PF_UtilCallbacks member offsets pinned to the SDK by abi-layout-probe.
    assert "contract::UTILITY_CALLBACK_OFFSETS" in SOURCE
    assert "std::array<void*, 31> utility_callbacks" in HEADER
    for offset in ("160", "168", "176", "184", "440", "464"):
        assert offset in CONTRACT
    # The wiring is shared with run() through an extracted installer so a
    # behavioral self-test can drive the exact production write path.
    assert "void install_callback_tables(State& state, const AbiHooks& abi)" in SOURCE
    assert "install_callback_tables(state, abi)" in SOURCE


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
        "contract::IN_GLOBAL_DATA_OFFSET",
        "contract::OUT_GLOBAL_DATA_OFFSET",
        "parameter_count_contract_valid",
    ):
        assert marker in SOURCE
