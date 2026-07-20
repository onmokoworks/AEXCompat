import source_owners


def source() -> str:
    return source_owners.contract_text("pf_ae_app_suite_complete")


def test_app_suite_versions_use_sdk_slot_counts_and_acquire_versions():
    text = source()
    assert "std::array<void*, 11> app4" in text
    assert "std::array<void*, 12> app5" in text
    assert "std::array<void*, 15> app6" in text
    assert '{"PF AE App Suite", 6, nullptr, &provide_app_suite4}' in text
    assert '{"PF AE App Suite", 7, nullptr, &provide_app_suite5}' in text
    assert '{"PF AE App Suite", 1, nullptr, &provide_app_suite6}' in text
    assert "populate_app(c.app4,false,false)" in text
    assert "populate_app(c.app5,true,false)" in text
    assert "populate_app(c.app6,true,true)" in text


def test_all_sdk_slots_have_explicit_host_semantics():
    text = source()
    callbacks = [
        "app_get_background_color", "app_get_color", "app_get_language",
        "app_get_personal_info", "app_get_font_style", "app_set_cursor",
        "app_is_render_engine", "app_color_picker", "app_get_mouse",
        "app_invalidate_rect", "app_convert_local_to_global",
        "app_get_color_at_global_point", "app_create_progress_dialog",
        "app_update_progress_dialog", "app_dispose_progress_dialog",
    ]
    for callback in callbacks:
        assert f"reinterpret_cast<void*>(&{callback})" in text


def test_context_dependent_operations_fail_closed_and_headless_contract_is_explicit():
    text = source()
    assert "app_get_font_style(int16_t, char*, int16_t*, int16_t*, int16_t*) { return 4; }" in text
    assert "app_set_cursor(int16_t) { return 4; }" in text
    assert "app_get_mouse(int32_t*) { return 4; }" in text
    assert "app_convert_local_to_global(const int32_t*, int32_t*) { return 4; }" in text
    assert "*render_engine = 1;" in text
    assert "app_is_render_engine(uint8_t* render_engine)" in text
    assert "app_is_render_engine(int32_t* render_engine)" not in text
    assert "context != &g_ui_context_pointer || !g_render_ui_context_active" in text
    assert "else g_app_invalidated_rect.fill(0);" in text


def test_v6_progress_noop_sessions_are_owned_bounded_and_single_dispose():
    text = source()
    assert "g_app_progress_dialogs.size() >= 32" in text
    assert "g_app_progress_dialogs.emplace(key, std::move(progress))" in text
    assert "g_app_progress_dialogs.find(dialog) == g_app_progress_dialogs.end()" in text
    assert "g_app_progress_dialogs.erase(dialog) != 1" in text
