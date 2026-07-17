import hashlib
import json
import subprocess
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
RUNNER = ROOT / "tools" / "capture-ae-probe-oracle.ps1"
EVIDENCE = ROOT / "analysis" / "AE_ORACLE_COLORGRID_CAPTURE_PLAN_2026-07-17.json"


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
    result = subprocess.run(command, capture_output=True, text=True, check=False)
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
    ], capture_output=True, text=True, check=False)
    assert result.returncode != 0
    assert "ExpectedRaw" in result.stderr


def test_colorgrid_plan_records_real_fixture_without_claiming_capture():
    payload = json.loads(EVIDENCE.read_text(encoding="utf-8-sig"))
    assert payload["status"] == "blocked_existing_ae_session"
    assert payload["side_effects_performed"] is False
    assert payload["plan_mode"] == "PlanOnly"
    assert payload["running_process_ids"] == [52384]
    assert payload["blocker"] == "After Effects is already running; capture was not launched."
    assert payload["fixture"]["sha256"] == (
        "64b0de13f978222bb40649bdf48cc533aebb3e03f98270af073b8351284ba64c"
    )
    assert payload["expected_raw"]["sha256"] == (
        "91d436a039c7f5ef56c4418e97dcb48dd4a69b8b9d984f224a06f97fe3ff8578"
    )
    assert payload["expected_raw"]["width"] == 16
    assert payload["expected_raw"]["height"] == 12
    assert payload["expected_raw"]["format"] == "rgba8"
    assert payload["input"]["size_bytes"] == Path(payload["input"]["path"]).stat().st_size
    assert any(value.endswith("capture-ae-probe-oracle.ps1") for value in payload["capture_argv"])
    assert any(value.endswith("compare-pixel-oracles.py") for value in payload["compare_argv"])

    for artifact in payload["current_artifact_snapshot"].values():
        path = Path(artifact["path"])
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]

    provenance = payload["expected_raw_provenance"]
    runtime_path = ROOT / provenance["runtime_evidence"]
    timeline = json.loads(runtime_path.read_text(encoding="utf-8"))
    runtime_auth = payload["runtime_evidence_authentication"]
    assert Path(runtime_auth["path"]) == runtime_path
    assert runtime_path.stat().st_size == runtime_auth["size_bytes"]
    assert hashlib.sha256(runtime_path.read_bytes()).hexdigest() == runtime_auth["sha256"]
    left = timeline["authenticated_artifacts"][provenance["artifact_role"]]
    snapshot = payload["current_artifact_snapshot"]
    assert set(snapshot) == {"source", "l2_worker", "render_worker", "smart_worker", "harness"}
    authenticated = timeline["authenticated_artifacts"]
    assert authenticated["source"]["sha256"] == snapshot["source"]["sha256"]
    assert authenticated["source"]["size_bytes"] == snapshot["source"]["size_bytes"]
    assert authenticated["worker"]["sha256"] == snapshot["render_worker"]["sha256"]
    assert authenticated["worker"]["size_bytes"] == snapshot["render_worker"]["size_bytes"]
    assert left["size_bytes"] == payload["expected_raw"]["size_bytes"]
    assert left["sha256"] == payload["expected_raw"]["sha256"]
    assert "not been compared" in provenance["note"]
