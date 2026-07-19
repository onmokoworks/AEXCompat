from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost/src/worker_aegp_init_runtime.hpp"
SOURCE = ROOT / "minihost/src/worker_aegp_init_runtime.cpp"
MAIN = source_owners.L2_MAIN
def test_aegp_init_runtime_owns_hook_dtos_registration_and_event_runners():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")
    for marker in (
        "struct CommandRegistration",
        "struct UpdateMenuRegistration",
        "struct IdleRegistration",
        "struct DeathRegistration",
        "struct EventResult",
        "dispatch_update_menu",
        "dispatch_idle",
        "dispatch_command",
        "dispatch_death",
        "kMaxHooks = 64",
    ):
        assert marker in header + source
    assert "aegp_init::dispatch_basic_events" in main
    assert "aegp_init::dispatch_death" in main
    assert '#include "worker_aegp_init_runtime.cpp"' not in main


def test_aegp_hook_callbacks_preserve_argument_order_and_fail_closed_bounds():
    source = SOURCE.read_text(encoding="utf-8")
    assert "registration.hook(global_refcon, registration.refcon" in source
    assert "requested_sleep < 0 || requested_sleep > 3600" in source
    assert "handled > 1" in source
    assert "registration.command != 0 && registration.command != command" in source
