import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "target" / "debug" / "aexcompat-harness.exe"
GRABBA = ROOT / "target" / "sdk-fixtures" / "grabba" / "Grabba.aex"

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
