import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
WORKERS = [
    ROOT / "target" / "minihost-build" / "aex_l2_worker.exe",
    ROOT / "target" / "minihost-build" / "aex_smart_worker.exe",
]


def test_source_routes_cleanup_through_seh_and_uses_virtual_output_storage():
    source = SOURCE.read_text(encoding="utf-8")
    assert 'g_last_seh_selector = "SMART_PRE_RENDER_CLEANUP"' in source
    assert "invoke_smart_pre_render_cleanup_seh(delete_pre_render_data, pre_render_data);" in source
    assert "delete_pre_render_data(pre_render_data);" not in source
    assert "class OutputPixelBuffer" in source
    assert "MEM_RESERVE, PAGE_NOACCESS" in source
    assert "guarded.sentinels_intact()" in source


def test_native_cleanup_and_output_guard_selftest():
    for worker in WORKERS:
        assert worker.exists(), f"missing worker: {worker}"
        completed = subprocess.run(
            [str(worker), "--self-test-render-output-safety"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        report = json.loads(completed.stdout)
        assert report["render_output_safety"] == "passed"
        assert report["cleanup_selector"] == "SMART_PRE_RENDER_CLEANUP"
        assert report["cleanup_error"] == 512
        assert report["cleanup_calls"] == 1
        assert report["guard_pages"] is True
        assert report["overrun_beyond_64_detected"] is True
