import json
import pathlib
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCES = (
    ROOT / "minihost" / "src" / "l2_main.cpp",
    ROOT / "minihost" / "src" / "worker_aegp_scene_callbacks.hpp",
)


def source_text() -> str:
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)
HARNESS = ROOT / "broker" / "target" / "debug" / "aexcompat-harness.exe"
GRABBA = ROOT / "target" / "sdk-fixtures" / "grabba" / "Grabba.aex"


def test_legacy_item_suite6_layout_matches_sdk_slots():
    source = source_text()

    assert "struct AegpLegacyItemSuite6" in source
    assert "offsetof(AegpLegacyItemSuite6, get_active_item) == 16" in source
    assert "offsetof(AegpLegacyItemSuite6, get_item_type) == 40" in source
    assert "sizeof(AegpLegacyItemSuite6) == 208" in source
    assert 'version == 10' in source
    assert "g_aegp_legacy_item_suite6.get_active_item = &aegp_get_active_item" in source
    assert "g_aegp_legacy_item_suite6.get_item_type = &aegp_get_item_type" in source
    assert "aegp_unsupported_suite_call" in source


def test_render_suite2_has_dedicated_sdk_layout_for_grabba():
    source = source_text()
    assert "struct AegpRenderSuite2" in source
    assert "sizeof(AegpRenderSuite2) == 10 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, render_frame) == 0 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, checkin) == 1 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, get_world) == 2 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, get_region) == 3 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, sufficient) == 4 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, render_sound) == 5 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, timestamp) == 6 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, changed) == 7 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, worthwhile) == 8 * sizeof(void*)" in source
    assert "offsetof(AegpRenderSuite2, checkin_rendered) == 9 * sizeof(void*)" in source
    assert "using AegpRenderCancelV1" in source
    assert "const int32_t cancel_error = check_cancel(cancel_refcon, &cancelled)" in source
    assert "allow_render_suite2 || g_loaded_effect_receipt_context.entry != nullptr" in source
    assert 'std::strcmp(name, "AEGP Render Suite") == 0 && version == 2' in source
    assert "g_aegp_render_suite2 = {&render_checkout_frame_reject, &checkin_frame" in source


def test_official_sdk_grabba_update_menu_dispatches_successfully():
    completed = subprocess.run(
        [str(HARNESS), "--dispatch-experimental-aegp-update-menu", str(GRABBA)],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=30,
    )

    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    assert report["event_error"] == 0
    assert report["hooks_invoked"] > 0
    assert report["suite_leases_balanced"] is True


def test_legacy_item_suite6_is_available_to_grabba_command_and_idle_roundtrips():
    source = source_text()
    for mode in (
        "g_aegp_update_menu_mode",
        "g_aegp_command_roundtrip_mode",
        "g_aegp_active_idle_roundtrip_mode",
        "g_aegp_comp_idle_roundtrip_mode",
    ):
        assert mode in source[source.index("version == 10") - 300 : source.index("version == 10")]


def _dispatch(command: str) -> dict:
    completed = subprocess.run(
        [str(HARNESS), command, str(GRABBA)],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    return json.loads(completed.stdout)


def test_official_sdk_grabba_idle_and_command_hooks_dispatch_successfully():
    idle = _dispatch("--dispatch-experimental-aegp-idle")
    assert idle["event_error"] == 0
    assert idle["hooks_invoked"] == 1
    assert idle["idle_max_sleep"] == 0
    assert idle["suite_acquires"] == idle["suite_releases"] == 3
    assert idle["suite_leases_balanced"] is True

    command = _dispatch("--dispatch-experimental-aegp-command-roundtrip")
    assert command["event_error"] == 0
    assert command["command_hooks_invoked"] == 2
    assert command["command_handled_count"] == 2
    assert command["receipts_created"] == command["receipts_checked_in"] == 2
    assert command["live_receipts"] == 0
    assert command["render_performed"] is True
    assert command["suite_acquires"] == command["suite_releases"] == 9
    assert command["suite_leases_balanced"] is True
