import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "refresh-classic-failure-stage-evidence.ps1"
CRASH_KIT = (
    ROOT
    / "target"
    / "classic-failure-probes-build"
    / "pf-crashkit"
    / "Release"
    / "pf_crashkit.aex"
)
INPUT_WRITE_DENIED = (
    ROOT
    / "target"
    / "classic-failure-probes-build"
    / "pf-input-write-probe"
    / "Release"
    / "pf_input_write_denied_probe.aex"
)


def test_refresh_replays_three_classic_failures_before_updating_evidence(tmp_path):
    crash_in = tmp_path / "stale-crashkit.json"
    input_write_in = tmp_path / "stale-input-write.json"
    crash_out = tmp_path / "crashkit.json"
    input_write_out = tmp_path / "input-write.json"

    crash_expected = json.loads(
        (
            ROOT / "analysis" / "PF_CRASHKIT_UI_ISOLATION_RESULT_2026-07-15.json"
        ).read_text(encoding="utf-8")
    )
    for case in crash_expected["cases"]:
        if case["mode"] in {"crash", "hang"}:
            case["failure_stage"] = "render"
    crash_in.write_text(json.dumps(crash_expected, indent=2) + "\n", encoding="utf-8")

    input_write_expected = json.loads(
        (ROOT / "analysis" / "PF_INPUT_BUFFER_WRITE_RESULT_2026-07-15.json").read_text(
            encoding="utf-8"
        )
    )
    input_write_expected["unadvertised_write"]["failure_stage"] = "render"
    input_write_in.write_text(
        json.dumps(input_write_expected, indent=2) + "\n", encoding="utf-8"
    )

    completed = subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(SCRIPT),
            "-SkipBuild",
            "-CrashKit",
            str(CRASH_KIT),
            "-InputWriteDenied",
            str(INPUT_WRITE_DENIED),
            "-CrashEvidence",
            str(crash_in),
            "-InputWriteEvidence",
            str(input_write_in),
            "-CrashEvidenceOut",
            str(crash_out),
            "-InputWriteEvidenceOut",
            str(input_write_out),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        # The hang case replays the classic_render deadline in real time
        # (~35s), so the whole refresh run measures 55-65s on a hosted
        # runner; 60s flaked on runner variance (issue #1535). 180s keeps
        # the run bounded while leaving margin well inside the job's
        # 10-minute limit.
        timeout=180,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    summary = json.loads(completed.stdout)
    assert summary == {
        "crash_failure_stage": "classic_render",
        "hang_failure_stage": "classic_render",
        "input_write_failure_stage": "classic_render",
        "hang_exit_code": 0xDEAD,
    }

    for case in crash_expected["cases"]:
        if case["mode"] in {"crash", "hang"}:
            case["failure_stage"] = "classic_render"
    crash_actual = json.loads(crash_out.read_text(encoding="utf-8"))
    assert crash_actual == crash_expected
    crash_cases = {case["mode"]: case for case in crash_actual["cases"]}
    assert crash_cases["crash"]["failure_stage"] == "classic_render"
    assert crash_cases["hang"]["failure_stage"] == "classic_render"
    assert crash_cases["bigalloc"]["failure_stage"] == "render"
    assert crash_cases["pf_error"]["failure_stage"] == "render"

    input_write_expected["unadvertised_write"]["failure_stage"] = "classic_render"
    input_write = json.loads(input_write_out.read_text(encoding="utf-8"))
    assert input_write == input_write_expected
    assert input_write["unadvertised_write"]["failure_stage"] == "classic_render"
    assert (
        input_write["smartfx_unadvertised_write"]["failure_stage"] == "smart_render_cpu"
    )
