import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


def _worker() -> Path:
    candidates = (
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    )
    return next(path for path in candidates if path.exists())


def test_path_hardening_is_fail_closed_in_source():
    source = SOURCE.read_text(encoding="utf-8")
    assert "curve.open && vertices > 0 ? vertices - 1 : vertices" in source
    assert "snapshot_checked_pf_path(path, curve)" in source
    assert "path.open && count > 0 ? count - 1 : count" in source
    assert "registered_pf_segment_prep" in source
    cleanup = source[source.index("int32_t __cdecl pf_path_cleanup_seg_length") :]
    cleanup = cleanup[: cleanup.index("bool verify_pf_path_data_hardening")]
    assert "registered_pf_segment_prep" in cleanup
    assert "checked_pf_segment_prep" not in cleanup
    assert "catch (const std::bad_alloc&)" in source
    assert "kPfBadCallbackParam" in cleanup
    assert "g_pf_path_segment_preps_mutex" in cleanup


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
