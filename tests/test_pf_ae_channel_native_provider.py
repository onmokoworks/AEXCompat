import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-timed-layers/Release/aex_worker.exe",
        ROOT / "target/minihost-build-v18/Release/aex_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)

def test_native_provider_oracle_normalizes_8_16_float_and_pins_receipt():
    executable = worker()
    assert executable is not None, "build the render worker first"
    completed = subprocess.run(
        [str(executable), "--kind", "classic", "--self-test-pf-ae-channel-native-provider"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {
        "pf_ae_channel_native_provider": "passed",
        "coverage_depths": [8, 16, 32],
        "mfr_checkouts": 2048,
        "fabricated_planes": False,
    }
