import json
import subprocess
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
RUNNER = ROOT / "tools" / "capture-ae-probe-oracle.ps1"


def test_plan_only_is_hash_bound_and_has_capture_and_compare_argv(tmp_path):
    fixture = tmp_path / "fixture.aex"
    raw = tmp_path / "expected.rgba"
    image = tmp_path / "input.png"
    fixture.write_bytes(b"fixture")
    raw.write_bytes(bytes((1, 2, 3, 255)))
    Image.new("RGBA", (1, 1), (1, 2, 3, 255)).save(image)
    plan = tmp_path / "plan.json"
    command = [
        "powershell", "-NoProfile", "-File", str(RUNNER), "-PlanOnly",
        "-PlanPath", str(plan), "-AfterEffects", str(Path("C:/Windows/System32/notepad.exe")),
        "-ProbeAex", str(fixture), "-InputImage", str(image),
        "-OutputPng", str(tmp_path / "ae.png"), "-EffectName", "Fixture Effect",
        "-ExpectedRaw", str(raw), "-Width", "1", "-Height", "1",
        "-ComparisonReport", str(tmp_path / "comparison.json"),
    ]
    result = subprocess.run(command, capture_output=True, text=True, errors="replace", check=False)
    assert result.returncode == 0, result.stderr
    payload = json.loads(plan.read_text(encoding="utf-8-sig"))
    assert payload["side_effects_performed"] is False
    assert payload["fixture"]["sha256"]
    assert payload["expected_raw"]["size_bytes"] == 4
    assert any(value.endswith("capture-ae-probe-oracle.ps1") for value in payload["capture_argv"])
    assert any(value.endswith("compare-pixel-oracles.py") for value in payload["compare_argv"])
    assert not (tmp_path / "ae.png").exists()
    assert not (tmp_path / "comparison.json").exists()


def test_plan_only_requires_complete_comparison_contract(tmp_path):
    fixture = tmp_path / "fixture.aex"
    image = tmp_path / "input.png"
    fixture.write_bytes(b"fixture")
    Image.new("RGBA", (1, 1)).save(image)
    result = subprocess.run([
        "powershell", "-NoProfile", "-File", str(RUNNER), "-PlanOnly",
        "-PlanPath", str(tmp_path / "plan.json"),
        "-AfterEffects", "C:/Windows/System32/notepad.exe",
        "-ProbeAex", str(fixture), "-InputImage", str(image),
        "-OutputPng", str(tmp_path / "ae.png"), "-EffectName", "Fixture Effect",
    ], capture_output=True, text=True, errors="replace", check=False)
    assert result.returncode != 0
    assert "ExpectedRaw" in result.stderr


def test_plan_only_records_no_effect_control_commands(tmp_path):
    fixture = tmp_path / "fixture.aex"
    image = tmp_path / "input.png"
    raw = tmp_path / "effect.rgba"
    control_raw = tmp_path / "control.rgba"
    fixture.write_bytes(b"fixture")
    raw.write_bytes(bytes((1, 2, 3, 255)))
    control_raw.write_bytes(bytes((4, 5, 6, 255)))
    Image.new("RGBA", (1, 1)).save(image)
    plan = tmp_path / "plan.json"
    result = subprocess.run([
        "powershell", "-NoProfile", "-File", str(RUNNER), "-PlanOnly",
        "-PlanPath", str(plan), "-AfterEffects", "C:/Windows/System32/notepad.exe",
        "-ProbeAex", str(fixture), "-InputImage", str(image),
        "-OutputPng", str(tmp_path / "effect.png"), "-EffectName", "Fixture Effect",
        "-ExpectedRaw", str(raw), "-Width", "1", "-Height", "1",
        "-ComparisonReport", str(tmp_path / "effect.json"),
        "-ControlOutputPng", str(tmp_path / "control.png"),
        "-ControlExpectedRaw", str(control_raw),
        "-ControlComparisonReport", str(tmp_path / "control.json"),
    ], capture_output=True, text=True, errors="replace", check=False)
    assert result.returncode == 0, result.stderr
    payload = json.loads(plan.read_text(encoding="utf-8-sig"))
    control = payload["no_effect_control"]
    assert control["expected_raw"]["sha256"]
    assert "-NoEffect" in control["capture_argv"]
    assert any(value.endswith("compare-pixel-oracles.py") for value in control["compare_argv"])

