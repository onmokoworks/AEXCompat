import subprocess
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-transfer-mask-probe/pf_transfer_mask_probe.cpp"
SCRIPT = ROOT / "tools/build-pf-transfer-mask-probe.ps1"
WORKER = ROOT / "target/minihost-build/aex_render_worker.exe"
PROBE = ROOT / "target/pf-transfer-mask-probe-build/Release/pf_transfer_mask_probe.aex"
INPUT = ROOT / "target/gpu-effects/opencl-input.rgba"


def test_probe_builds_with_exact_mask_abi_and_flags():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT,
        check=True,
        timeout=180,
    )
    assert PROBE.is_file()


def test_real_mask_probe_crosses_the_aex_boundary(tmp_path):
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    output = tmp_path / "transfer-mask-output.rgba"
    report = run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"] == "render_completed" and report["render_error"] == 0
    assert report["suite_leases_balanced"] is True
    assert output.read_bytes() == bytes(37 * 23 * 4)
