"""Process-local suppression for headless worker system-alert sounds (#1163)."""

import json
import subprocess


def test_worker_intercepts_system_alert_before_windows_audio(
    canonical_release_worker,
) -> None:
    completed = subprocess.run(
        [
            str(canonical_release_worker),
            "--kind",
            "classic",
            "--self-test-headless-system-sound-suppression",
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {
        "headless_system_sound_suppression": "passed",
        "process_local": True,
        "dialog_containment_unchanged": True,
    }
