from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.L2_MAIN.read_text(encoding="utf-8")
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
    for marker in (
        "target.image_audio_mode = mode.image_audio_mode",
        "hooks.set_audio_source",
        "target.smart_force_cpu = mode.force_cpu",
        "target.mask_count_error_mode = mode.mask_count_error_mode",
        "hooks.set_mask_fault",
        "hooks.set_click",
        "hooks.enable_draw",
    ):
        assert marker in SOURCE
    assert "request_parser::parse(" in MAIN
    assert "const invocation::ApplyHooks" not in MAIN
    assert "invocation::ApplyHooks invocation_hooks" in MAIN
