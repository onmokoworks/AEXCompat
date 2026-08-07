import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _workers():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-build-v18/Release/aex_render_worker.exe",
        ROOT / "target/minihost-build-v18/aex_render_worker.exe",
        ROOT / "target/minihost-build-vs2022/Release/aex_render_worker.exe",
        ROOT / "target/minihost-build-vs2022/aex_render_worker.exe",
    ]
    return [path for path in candidates if path and path.is_file()]


def test_pf_state_registry_native_adversarial_self_test():
    workers = _workers()
    assert workers, "build a VS2022 render worker before running the native test"
    for worker in workers:
        completed = subprocess.run(
            [str(worker), "--self-test-pf-param-utils-suite"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert completed.stdout.strip() == '{"pf_param_utils_suite3":"passed"}'
