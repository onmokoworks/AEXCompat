from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCES = (
    ROOT / "minihost" / "src" / "l2_main.cpp",
    ROOT / "minihost" / "src" / "worker_l2_suite_abi.hpp",
    ROOT / "minihost" / "src" / "worker_pf_helper_runtime.cpp",
    ROOT / "minihost" / "src" / "worker_pf_helper_runtime.hpp",
)


def source() -> str:
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)


def test_helper_suite2_uses_the_sdk_name_version_and_three_slot_abi() -> None:
    text = source()
    assert 'std::strcmp(name, "AE Plugin Helper Suite2") == 0' in text
    assert "name && version == 2" in text
    declaration = text[text.index("using Suite2 = std::array<void*, 3>") :]
    slots = [
        "&parse_clipboard",
        "&set_current_extended_tool",
        "&get_current_extended_tool",
    ]
    assert [declaration.index(slot) for slot in slots] == sorted(
        declaration.index(slot) for slot in slots
    )
    assert "*suite = aexcompat::pf_helper::suite2();" in text


def test_headless_clipboard_parse_is_explicitly_fail_closed() -> None:
    text = source()
    body = text[text.index("int32_t __cdecl parse_clipboard()") :]
    body = body[: body.index("\n}")]
    assert "return kBadCallbackParam;" in body
    assert "OpenClipboard" not in body
    assert "GetClipboardData" not in body


def test_extended_tool_validation_covers_null_and_the_sdk_enum_bounds() -> None:
    text = source()
    assert "constexpr int32_t kExtendedToolMin = 0;" in text
    assert "constexpr int32_t kExtendedToolMax = 44;" in text
    assert "tool < kExtendedToolMin || tool > kExtendedToolMax" in text
    assert "if (!tool) return kBadCallbackParam;" in text
    assert "*tool = current_tool().load(std::memory_order_acquire);" in text


def test_set_rejects_calls_outside_a_proven_ui_event_context() -> None:
    text = source()
    setter = text[text.index("int32_t __cdecl set_current_extended_tool") :]
    setter = setter[: setter.index("\n}")]
    assert "!g_ui_context_active || g_ui_context < 0" in setter
    assert "g_ui_context >= static_cast<int32_t>(g_ui_tools.size())" in setter
    assert setter.index("return kBadCallbackParam;") < setter.index(".store(")

    getter = text[text.index("int32_t __cdecl get_current_extended_tool") :]
    getter = getter[: getter.index("\n}")]
    assert "g_ui_context_active" not in getter
    assert "current_tool().load" in getter
    assert "g_effect_tool{kExtendedToolMin}" in text


def test_tool_state_is_thread_safe_context_bounded_and_reset_on_teardown() -> None:
    text = source()
    assert "std::atomic<int32_t> g_effect_tool" in text
    assert "std::array<std::atomic<int32_t>, kUiContextCount>" in text
    assert "thread_local int32_t g_ui_context = -1;" in text
    assert "PfHelperUiContextScope helper_ui_scope(g_ui_context.window_type);" in text
    assert "g_ui_context = context >= 0" in text
    assert "g_ui_context_active = g_ui_context >= 0;" in text
    assert "set_context_tool(" in text
    setdown = text[text.index("int32_t invoke_global_setdown") :]
    setdown = setdown[: setdown.index("\n}")]
    assert "aexcompat::pf_helper::reset();" in setdown
