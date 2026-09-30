import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "tools/build-pf-color-oracle.ps1"


def test_pf_color_oracle_builds():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(BUILD)],
                   cwd=ROOT, check=True, timeout=180)
    assert (ROOT / "target/pf-color-oracle-build/Release/pf_color_oracle.aex").is_file()
