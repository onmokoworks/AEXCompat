from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.L2_SOURCE.read_text(encoding="utf-8")
HEADER = (ROOT / "minihost/src/worker_invocation_orchestration.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost/src/worker_invocation_orchestration.cpp").read_text(
    encoding="utf-8"
)
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")


def test_invocation_state_and_mode_mapping_have_a_compiled_owner():
    assert CMAKE.count("src/worker_invocation_orchestration.cpp") == 1
    assert "struct InvocationState" in HEADER
    assert "void apply_render(" in SOURCE
    assert "void apply_smart(" in SOURCE
    assert "struct InvocationState" not in MAIN
    assert "invocation::apply_render(" in MAIN
    assert "invocation::apply_smart(" in MAIN


def test_render_and_smart_mapping_preserve_request_side_effect_hooks():
    # #365 deleted the one-shot argv modes, so the audio source now arrives
    # through the session trailer and the click/draw hooks are gone entirely
    # (the session drives custom UI per frame via the v:2 ui_action field,
    # worker_render_session.cpp::apply_session_ui_action).
    for marker in (
        "target.audio_session_mode = mode.audio_session_mode",
        "mode.session_audio && hooks.set_audio_source",
        "target.smart_force_cpu = mode.force_cpu",
        "target.mask_count_error_mode = mode.mask_count_error_mode",
        "hooks.set_mask_fault",
    ):
        assert marker in SOURCE
    for gone in ("hooks.set_click", "hooks.enable_draw", "mode.image_audio_mode"):
        assert gone not in SOURCE, gone
    assert "request_parser::parse(" in MAIN
    assert "const invocation::ApplyHooks" not in MAIN
    assert "invocation::ApplyHooks invocation_hooks" in MAIN
