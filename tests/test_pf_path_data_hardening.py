import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "worker_pf_path_runtime.cpp"


def _worker() -> Path:
    candidates = (
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    )
    return next(path for path in candidates if path.exists())


def test_path_hardening_is_fail_closed_in_source():
    source = SOURCE.read_text(encoding="utf-8")
    assert "c.open&&n?n-1:n" in source
    assert "checked(path,c)" in source
    assert "registered(prep,path,segment,false)" in source
    cleanup = source[source.index("int32_t __cdecl path_cleanup_seg_length") :]
    cleanup = cleanup[: cleanup.index("int32_t __cdecl path_is_inverted")]
    assert "registered(prep,path,segment,false)" in cleanup
    assert "catch(const std::bad_alloc&)" in source
    assert "kBad" in cleanup
    assert "lock(g_mutex)" in cleanup


def test_path_hardening_runtime_self_test():
    completed = subprocess.run(
        [str(_worker()), "--self-test-pf-path-data-hardening"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    assert report == {
        "pf_path_data_hardening": "passed",
        "created": 6,
        "disposed": 6,
        "live": 0,
        "balanced": True,
    }
