import subprocess
from pathlib import Path

from _render_session import HARNESS, assert_artifact_fresh, run_session_render
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/"instruments/pf-transform-affine-probe/pf_transform_affine_probe.cpp"
SCRIPT=ROOT/"tools/build-pf-transform-affine-probe.ps1"
PROBE=ROOT/"target/pf-transform-affine-probe-build/Release/pf_transform_affine_probe.aex"
WORKER=ROOT/"target/minihost-build/aex_worker.exe"
INPUT=ROOT/"target/gpu-effects/opencl-input.rgba"
def test_build_and_vectors():
    subprocess.run(["powershell","-NoProfile","-ExecutionPolicy","Bypass","-File",str(SCRIPT)],cwd=ROOT,check=True,timeout=180)
    assert PROBE.is_file()
def test_real_aex_affine_vectors(tmp_path):
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    output=tmp_path/"affine.rgba"
    report=run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"]=="render_completed" and report["render_error"]==0
