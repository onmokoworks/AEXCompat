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
    # Slot 200 additionally wires the legacy `app` callback for PIN-era
    # effects (issue #362 selector families); like every other offset it is
    # generated into UTILITY_CALLBACK_OFFSETS from the ABI observation.
    assert "contract::UTILITY_CALLBACK_OFFSETS" in SOURCE
    assert "UTILITY_CALLBACK_OFFSETS.size()>" in HEADER
    assert "std::array<std::size_t, 35> UTILITY_CALLBACK_OFFSETS" in CONTRACT
    for offset in ("160", "168", "176", "184", "440", "464", "200"):
        assert offset in CONTRACT
    assert "UTILS_APP_OFFSET = 200" in CONTRACT
    # The wiring is shared with run() through an extracted installer so a
    # behavioral self-test can drive the exact production write path.
    assert "void install_callback_tables(State& state, const AbiHooks& abi)" in SOURCE
    assert "install_callback_tables(state, abi)" in SOURCE


def test_native_utility_hooks_match_generated_middle_slot_order():
    # Keep this source-level guard in addition to the C++ length static_assert:
    # a same-length insertion in the middle must not silently shift later hooks.
    l2 = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
    assert (
        "reinterpret_cast<void*>(&fill_world16), "
        "reinterpret_cast<void*>(&premultiply_color16),\n"
        "    reinterpret_cast<void*>(&iterate_world16), "
        "reinterpret_cast<void*>(&iterate_world8)"
    ) in l2
    assert (
        "reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_fabs),\n"
        "    reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_hypot),\n"
        "    reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_pow)"
    ) in l2
    assert "utility_callbacks.size() ==" in l2
    assert "UTILITY_CALLBACK_OFFSETS.size()" in l2


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
