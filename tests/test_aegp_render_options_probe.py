import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-aegp-render-options-probe.ps1"
RESULT = ROOT / "target" / "aegp-render-options-probe-build" / "aegp-render-options-result.json"

def test_fixture_builds_and_exercises_lifecycle():
    tmp = ROOT / "target" / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "TEMP": str(tmp), "TMP": str(tmp)}
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)], cwd=ROOT, env=env, check=True, timeout=180)
    report = json.loads(RESULT.read_text(encoding="utf-8"))
    assert report["source_kind"] == "native_sdk_abi_fixture"
    assert report["suite_slots"] == {"render_options": 17, "render_suite4": 12}
    assert report["render_suite_version"] == 5
    assert report["all_set_get_exercised"] and report["duplicate_independent"]
    assert [x["world_type"] for x in report["observations"]] == [1, 2, 3]
    assert [(x["width"], x["height"]) for x in report["observations"]] == [(9, 4)] * 3
    assert [x["rowbytes"] for x in report["observations"]] == [36, 72, 144]
    assert all(x["stale_receipt_rejected"] for x in report["observations"])
    assert report["live_options"] == 0 and report["passed"] is True

