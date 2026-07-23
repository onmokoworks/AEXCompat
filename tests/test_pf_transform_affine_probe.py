import hashlib,json,subprocess
from pathlib import Path

from _render_session import run_session_render
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/"instruments/pf-transform-affine-probe/pf_transform_affine_probe.cpp"
SCRIPT=ROOT/"tools/build-pf-transform-affine-probe.ps1"
PROBE=ROOT/"target/pf-transform-affine-probe-build/Release/pf_transform_affine_probe.aex"
WORKER=ROOT/"target/minihost-build/aex_render_worker.exe"
INPUT=ROOT/"target/gpu-effects/opencl-input.rgba"
def test_build_and_vectors():
    subprocess.run(["powershell","-NoProfile","-ExecutionPolicy","Bypass","-File",str(SCRIPT)],cwd=ROOT,check=True,timeout=180)
    text=SOURCE.read_text(encoding="utf-8")
    for marker in ("identity{{1,0,0,0,1,0,0,0,1}}","forward{{1,0,0,0,1,0,1,1,1}}","scale{{2,0,0,0,2,0,0,0,1}}","rotate{{0,1,0,-1,0,0,2,0,1}}","PF_MaskFlag_NONE","projective_inverse{{1,0,0.25,0,1,0,0,0,1}}","projective_forward{{1,0,-0.25,0,1,0,0,0,1}}"):
        assert marker in text
def test_real_aex_affine_vectors(tmp_path):
    output=tmp_path/"affine.rgba"
    report=run_session_render(tmp_path, PROBE, INPUT, output, width=37, height=23)
    assert report["status"]=="render_completed" and report["render_error"]==0
