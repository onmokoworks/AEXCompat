import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_SHIFTER_PRODUCTION_MATRIX_RESULT_2026-07-18.json"


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
