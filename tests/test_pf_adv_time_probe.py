import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-adv-time-probe/pf_adv_time_probe.cpp"
RC = ROOT / "instruments/pf-adv-time-probe/pf_adv_time_probe.rc"
BUILD = ROOT / "tools/build-pf-adv-time-probe.ps1"


def test_pf_adv_time_probe_release_build():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(BUILD)],
                   cwd=ROOT, check=True, timeout=180)
    assert (ROOT / "target/pf-adv-time-probe-build/Release/pf_adv_time_probe.aex").is_file()








