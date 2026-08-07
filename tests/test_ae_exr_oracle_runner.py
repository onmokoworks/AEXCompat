import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RUNNER = ROOT / "tools" / "capture-ae-exr-oracle.ps1"


def test_plan_only_is_identity_bound_and_has_no_output_side_effects(tmp_path):
    afterfx = tmp_path / "AfterFX.exe"
    aerender = tmp_path / "aerender.exe"
    tested = tmp_path / "fixture.aex"
    installed = tmp_path / "installed.aex"
    image = tmp_path / "input.png"
    raw = tmp_path / "expected.rgba"
    for path, data in (
        (afterfx, b"exe"), (aerender, b"render"), (tested, b"fixture"),
        (installed, b"fixture"), (image, b"png"), (raw, b"\0" * 16),
    ):
        path.write_bytes(data)
    plan = tmp_path / "plan.json"
    output = tmp_path / "run"
    result = subprocess.run([
        "powershell", "-NoProfile", "-File", str(RUNNER), "-PlanOnly",
        "-PlanPath", str(plan), "-AfterEffects", str(afterfx),
        "-TestedAex", str(tested), "-InstalledAex", str(installed),
        "-InputImage", str(image), "-ExpectedRaw", str(raw),
        "-EffectName", "Fixture", "-OutputRoot", str(output),
        "-Width", "1", "-Height", "1",
    ], capture_output=True, text=True, check=False)
    assert result.returncode == 0, result.stderr
    payload = json.loads(plan.read_text(encoding="utf-8-sig"))
    assert payload["side_effects_performed"] is False
    assert payload["fixture"]["sha256"] == hashlib.sha256(b"fixture").hexdigest()
    assert payload["expected_raw"]["format"] == "rgba32f-le"
    assert not output.exists()
