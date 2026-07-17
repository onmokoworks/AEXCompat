import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "PF_ADV_TIME_V4_RUNTIME_RESULT_2026-07-16.json"


def _load() -> dict:
    return json.loads(EVIDENCE.read_text(encoding="utf-8"))


def test_adv_time_runtime_authenticates_current_artifacts() -> None:
    for artifact in _load()["authenticated_artifacts"].values():
        path = ROOT / artifact["path"]
        assert path.is_file(), artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]


def test_adv_time_runtime_records_balanced_raw_v4_results() -> None:
    evidence = _load()
    runtime = evidence["runtime"]
    sidecar = json.loads((ROOT / evidence["authenticated_artifacts"]["sidecar"]["path"]).read_text())
    report = json.loads((ROOT / evidence["authenticated_artifacts"]["report"]["path"]).read_text())
    assert evidence["result"] == "current_worker_exercised_all_adv_time_v4_slots"
    assert runtime["exit_code"] == 0
    assert report["status"] == runtime["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["guard_bytes_intact"] is True
    assert report["suite_leases_balanced"] is True
    assert runtime["suite_acquires"] == runtime["suite_releases"] == 2
    assert sidecar["acquire_err"] == sidecar["release_err"] == 0
    assert sidecar["lease_balanced"] is True
    assert all(case["exception"] == 0 and case["guard_intact"] for case in sidecar["format_cases"])
    assert sidecar["display_pref"]["raw_bytes"] == runtime["display_pref_raw_bytes"]
    counts = {case["case"]: case for case in sidecar["count_cases"]}
    assert counts["exact"]["raw_output"] == 4
    assert counts["partial_excluded"]["raw_output"] == 4
    assert counts["partial_included"]["raw_output"] == 5
    assert counts["invalid_scale_zero"]["err"] == counts["overflow"]["err"] == 4


def test_adv_time_evidence_does_not_claim_ae_display_equivalence() -> None:
    assert any("Adobe After Effects" in claim for claim in _load()["not_claimed"])
