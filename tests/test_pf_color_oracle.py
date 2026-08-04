import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-color-oracle/pf_color_oracle.cpp"
RC = ROOT / "instruments/pf-color-oracle/pf_color_oracle.rc"
BUILD = ROOT / "tools/build-pf-color-oracle.ps1"
RUNNER = ROOT / "tools/ae-color-oracle-run.jsx"
ATTEMPT = ROOT / "analysis/PF_COLOR_AE_ORACLE_ATTEMPT_2026-07-16.json"


def test_pf_color_oracle_builds():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(BUILD)],
                   cwd=ROOT, check=True, timeout=180)
    assert (ROOT / "target/pf-color-oracle-build/Release/pf_color_oracle.aex").is_file()




def test_runner_and_attempt_do_not_claim_uncaptured_values():
    runner = RUNNER.read_text(encoding="utf-8")
    assert "bitsPerChannel" in runner and "oracle_not_captured" in runner
    for marker in ("canAddProperty", "can_add_property", "effect.name", "effect.matchName",
                   "depth_results", "native.remove()"):
        assert marker in runner
    attempt = json.loads(ATTEMPT.read_text(encoding="utf-8"))
    assert attempt["status"] == "oracle_not_captured"
    assert attempt["captured_values"] is None
