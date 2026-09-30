import subprocess

from _native_selftest import ROOT, locate


def test_render_session_contained_exit_skips_dll_detach_and_preserves_report():
    selftest = locate("worker_session_contained_exit_selftest.exe")
    fixture = locate("worker_session_detach_fixture.dll")
    result = subprocess.run(
        [str(selftest), str(fixture)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
