import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "run-shifter-production-matrix-gate.ps1"
RESULT = ROOT / "analysis" / "SDK_SHIFTER_PRODUCTION_MATRIX_RESULT_2026-07-18.json"


def test_runner_uses_the_six_production_routes_twice():
    script = SCRIPT.read_text(encoding="utf-8-sig")
    commands = (
        "--render-experimental'",
        "--render-experimental-16'",
        "--render-experimental-32'",
        "--render-experimental-smart'",
        "--render-experimental-smart-16'",
        "--render-experimental-smart-32-cpu'",
    )
    assert all(command in script for command in commands)
    assert "foreach ($runNumber in 1..2)" in script
    assert "Start-Process" in script


def test_runner_pins_artifacts_and_checks_production_invariants():
    script = SCRIPT.read_text(encoding="utf-8-sig")
    for marker in (
        "Assert-ArtifactIdentity",
        "output_identity",
        "worker_ok",
        "guards_intact",
        "lifetimes_balanced",
        "stage_diagnostics",
        "smart32_cpu_only",
        "is not deterministic",
    ):
        assert marker in script
    assert script.count("sha256 = '") >= 9


def test_current_evidence_is_complete_deterministic_and_redacted():
    data = json.loads(RESULT.read_text(encoding="utf-8-sig"))
    assert data["status"] == "passed"
    assert data["run_count"] == 12 and data["runs_per_case"] == 2
    assert data["coverage"] == {
        "route_count": 6,
        "classic": ["argb8", "argb16", "argb32f"],
        "smart_cpu": ["argb8", "argb16", "argb32f"],
    }
    assert len(data["cases"]) == 6
    for case in data["cases"]:
        assert case["deterministic"] is True and len(case["runs"]) == 2
        assert {run["output_sha256"] for run in case["runs"]} == {case["expected_output_sha256"]}
        assert all(all(run["checks"].values()) for run in case["runs"])
    serialized = RESULT.read_text(encoding="utf-8-sig")
    assert not re.search(r"[A-Za-z]:[\\/]", serialized)

