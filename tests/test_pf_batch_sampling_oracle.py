import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "tools/build-pf-batch-sampling-oracle.ps1"






def test_probe_builds_against_installed_public_sdk():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(BUILD)],
        cwd=ROOT, check=True, timeout=180,
    )
    artifact = ROOT / "target/pf-batch-sampling-oracle-build/Release/pf_batch_sampling_oracle.aex"
    assert artifact.stat().st_size > 0
