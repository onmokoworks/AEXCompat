import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "PF_CRASHKIT_UI_ISOLATION_RESULT_2026-07-15.json"

def test_all_crashkit_modes_have_distinct_bounded_outcomes():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    cases = {case["mode"]: case for case in evidence["cases"]}
    assert set(cases) == {"none", "crash", "hang", "bigalloc", "pf_error"}
    assert cases["none"]["classification"] == "ok"
    assert cases["none"]["output_created"] is True
    assert cases["crash"]["classification"] == "crashed"
    assert cases["crash"]["exit_code"] == 0xC0000005
    assert cases["hang"]["classification"] == "timeout_killed"
    assert cases["hang"]["exit_code"] == 0xDEAD
    assert cases["bigalloc"]["render_error"] == 4
    assert cases["pf_error"]["render_error"] == 512
    assert all(not case["output_created"] for name, case in cases.items() if name != "none")
    assert all(case.get("failure_stage") == "render" for name, case in cases.items() if name != "none")

def test_custom_ui_crash_and_hang_are_parameterized_and_isolated():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    ui = evidence["custom_ui_event_cases"]
    assert ui["event_sequence"] == [
        "new_context", "activate", "idle", "deactivate", "close_context"
    ]
    assert ui["parameter_assignment"]["applied_in_same_worker"]
    assert ui["crash"]["lifecycle_errors"] == [0, 0, 512, 0, 0]
    assert ui["crash"]["close_context_completed"]
    assert ui["crash"]["global_setdown_completed"]
    assert ui["hang"]["classification"] == "timeout_killed"
    assert ui["hang"]["bounded_timeout_ms"] == 5000
    assert ui["hang"]["parent_test_process_survived"]
