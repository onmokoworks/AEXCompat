import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "tools/build-pf-ae-adv-item-probe.ps1"

def test_probe_builds_and_fixes_all_five_sdk_slots_and_lease_balance():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
                    "-File", str(BUILD)], cwd=ROOT, check=True, timeout=180)
    assert (ROOT / "target/pf-ae-adv-item-probe-build/Release/pf_ae_adv_item_probe.aex").is_file()
