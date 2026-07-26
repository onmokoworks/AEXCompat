from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = (ROOT / "minihost/src/worker_aegp_init_orchestration.hpp").read_text(
    encoding="utf-8")
SOURCE = (ROOT / "minihost/src/worker_aegp_init_orchestration.cpp").read_text(
    encoding="utf-8")
MAIN = source_owners.L2_MAIN.read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")


def test_aegp_init_orchestration_is_a_true_translation_unit():
    assert "src/worker_aegp_init_orchestration.cpp" in CMAKE
    assert '#include "worker_aegp_init_orchestration.hpp"' in MAIN
    assert "run_orchestration(" in SOURCE
    assert "run_orchestration(" in MAIN
    assert '#include "worker_aegp_init_orchestration.cpp"' not in MAIN


def test_entry_events_roundtrips_and_death_keep_their_order():
    entry = SOURCE.index("aegp_entry_guard::invoke(")
    basic_events = SOURCE.index("dispatch_basic_events(")
    roundtrips = SOURCE.index("run_roundtrips(")
    death = SOURCE.index("dispatch_death(")
    assert entry < basic_events < roundtrips < death
    assert SOURCE.count("result.init_error == 0") == 3
    assert "result.entry_fault" in SOURCE
    assert "force_release_all()" in SOURCE


def test_error_priority_and_event_telemetry_are_owned():
    for marker in (
        "record_error(events.error, result.event_error)",
        "record_error(roundtrip.error, result.event_error)",
        "record_error(death.error, result.death_error)",
        "result.hooks_invoked += roundtrip.hooks_invoked",
        "result.command_handled_count += roundtrip.command_handled_count",
        "roundtrip.idle_max_sleep < result.idle_max_sleep",
    ):
        assert marker in SOURCE
