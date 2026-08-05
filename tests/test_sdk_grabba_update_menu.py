import json
import pathlib
import subprocess
import source_owners

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCES = source_owners.contract_files("sdk_grabba_update_menu")
def source_text() -> str:
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)
HARNESS = ROOT / "broker" / "target" / "debug" / "aexcompat-harness.exe"
GRABBA = ROOT / "target" / "sdk-fixtures" / "grabba" / "Grabba.aex"

def test_legacy_item_suite6_layout_matches_sdk_slots():
    source = source_text()

def test_render_suite2_has_dedicated_sdk_layout_for_grabba():
    source = source_text()

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
    branch_start = source.index("version == 10")
    branch = source[branch_start : branch_start + 900]
    for mode in (
        "state().update_menu_mode",
        "state().command_roundtrip_mode",
        "state().active_idle_roundtrip_mode",
        "state().comp_idle_roundtrip_mode",
    ):
        assert mode in branch

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
