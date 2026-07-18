from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCES = (
    ROOT / "minihost" / "src" / "l2_main.cpp",
    ROOT / "minihost" / "src" / "worker_pf_suites.hpp",
)


def source() -> str:
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)


def test_helper_suite2_uses_the_sdk_name_version_and_three_slot_abi() -> None:
    text = source()
    assert 'std::strcmp(name, "AE Plugin Helper Suite2") == 0' in text
    assert "name && version == 2" in text
    declaration = text[text.index("std::array<void*, 3> g_pf_helper_suite2") :]
    slots = [
        "pf_parse_clipboard",
        "pf_set_current_extended_tool",
        "pf_get_current_extended_tool",
    ]
    assert [declaration.index(slot) for slot in slots] == sorted(
        declaration.index(slot) for slot in slots
    )
    assert "*suite = g_pf_helper_suite2.data();" in text


def test_headless_clipboard_parse_is_explicitly_fail_closed() -> None:
    text = source()
    body = text[text.index("int32_t __cdecl pf_parse_clipboard()") :]
    body = body[: body.index("\n}")]
    assert "return kPfBadCallbackParam;" in body
    assert "OpenClipboard" not in body
    assert "GetClipboardData" not in body


def test_extended_tool_validation_covers_null_and_the_sdk_enum_bounds() -> None:
    text = source()
    assert "constexpr int32_t kPfExtendedSuiteToolMin = 0;" in text
    assert "constexpr int32_t kPfExtendedSuiteToolMax = 44;" in text
    assert "tool < kPfExtendedSuiteToolMin || tool > kPfExtendedSuiteToolMax" in text
    assert "if (!tool) return kPfBadCallbackParam;" in text
    assert "*tool = current_pf_helper_tool().load(std::memory_order_acquire);" in text


def test_set_rejects_calls_outside_a_proven_ui_event_context() -> None:
    text = source()
    setter = text[text.index("int32_t __cdecl pf_set_current_extended_tool") :]
    setter = setter[: setter.index("\n}")]
    assert "!g_render_ui_context_active || g_pf_helper_ui_context < 0" in setter
    assert "g_pf_helper_ui_context >= static_cast<int32_t>(g_pf_helper_ui_tools.size())" in setter
    assert setter.index("return kPfBadCallbackParam;") < setter.index(".store(")

    getter = text[text.index("int32_t __cdecl pf_get_current_extended_tool") :]
    getter = getter[: getter.index("\n}")]
    assert "g_render_ui_context_active" not in getter
    assert "current_pf_helper_tool().load" in getter
    assert "g_pf_helper_effect_tool{kPfExtendedSuiteToolMin}" in text


def test_tool_state_is_thread_safe_context_bounded_and_reset_on_teardown() -> None:
    text = source()
    assert "std::atomic<int32_t> g_pf_helper_effect_tool" in text
    assert "std::array<std::atomic<int32_t>, kPfHelperUiContextCount>" in text
    assert "thread_local int32_t g_pf_helper_ui_context = -1;" in text
    assert "PfHelperUiContextScope helper_ui_scope(g_ui_context.window_type);" in text
    assert "g_pf_helper_ui_context = context >= 0 && context < 3 ? context : -1;" in text
    assert "g_render_ui_context_active = g_pf_helper_ui_context >= 0;" in text
    assert "g_pf_helper_ui_tools[static_cast<std::size_t>(g_ui_context.window_type)].store" in text
    setdown = text[text.index("int32_t invoke_global_setdown") :]
    setdown = setdown[: setdown.index("\n}")]
    assert "reset_pf_helper_tools();" in setdown
