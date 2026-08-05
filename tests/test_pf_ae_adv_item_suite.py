import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "instruments/pf-ae-adv-item-probe/pf_ae_adv_item_probe.cpp"
BUILD = ROOT / "tools/build-pf-ae-adv-item-probe.ps1"

def test_probe_builds_and_fixes_all_five_sdk_slots_and_lease_balance():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
                    "-File", str(BUILD)], cwd=ROOT, check=True, timeout=180)
    assert (ROOT / "target/pf-ae-adv-item-probe-build/Release/pf_ae_adv_item_probe.aex").is_file()
    text = PROBE.read_text(encoding="utf-8")
    for marker in ("kPFAdvItemSuite", "kPFAdvItemSuiteVersion1", "PF_AdvItemSuite1",
                   "PF_MoveTimeStep", "PF_MoveTimeStepActiveItem", "PF_TouchActiveItem",
                   "PF_ForceRerender", "PF_EffectIsActiveOrEnabled", "invalid_direction",
                   "negative_steps", "null_inputs", "AcquireSuite", "ReleaseSuite",
                   "lease_balanced", "sizeof(PF_AdvItemSuite1) == 5 * sizeof(void*)"):
        assert marker in text
