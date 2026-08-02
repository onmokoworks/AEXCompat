import source_owners

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "PF_CRASHKIT_UI_ISOLATION_RESULT_2026-07-15.json"
RUNNER = ROOT / "broker" / "crates" / "broker" / "src" / "windows_process.rs"
IMAGE_RENDER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"


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


def test_job_object_enforces_memory_and_lifetime_bounds():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    runner = RUNNER.read_text(encoding="utf-8")
    assert evidence["limits"]["process_memory_bytes"] == 512 * 1024 * 1024
    assert "JOB_OBJECT_LIMIT_PROCESS_MEMORY" in runner
    assert "limits.ProcessMemoryLimit = process_memory_limit" in runner
    assert "terminate_job_and_wait(job.raw(), process_handle.raw(), TERMINATION_GRACE_MS)" in runner
    assert "WaitForSingleObject(process, wait_ms)" in runner


def test_nested_failure_stage_and_parameterized_cli_are_fixed():
    renderer = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    harness = source_owners.harness_windows_text()
    assert "let mut active_stages: Vec<String>" in renderer
    assert 'failure_stage = active_stage.clone()' in renderer
    assert '"--render-experimental-param"' in harness
    assert "parameters.iter_mut().find(|item| item.slot == slot)" in harness


def test_selector_failure_is_checked_before_output_read():
    renderer = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    # The gate lives in validate_interactive_worker_report (issue #98 W2
    # extraction); the ordering contract is that its call site rejects a
    # failed worker before any output bytes are read.
    assert "if !worker_passed" in renderer
    failure_check = renderer.index(
        "= validate_interactive_worker_report("
    )
    # The one-shot read its output back from a raw sidecar
    # (`fs::read(&output_raw)`), which #365 deleted. The session's frame pixels
    # arrive in the shared section, so the "no output byte before the gate"
    # ordering is now about destructuring FrameStatus::Rendered and writing the
    # PNG, both of which must still follow the gate.
    output_use = renderer.index("let (pixels, rendered_width, rendered_height) = match outcome.status")
    assert failure_check < output_use
    assert output_use < renderer.index("file.write_all(&pixels)")


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
