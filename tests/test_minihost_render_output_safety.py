import json
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
DISPATCH = ROOT / "minihost" / "src" / "worker_selector_dispatch.cpp"
SMART_FINALIZE = ROOT / "minihost" / "src" / "worker_smart_finalize.cpp"
PIXEL_BUFFER = ROOT / "minihost" / "src" / "render_pixel_buffer.cpp"
WORKERS = [
    ROOT / "target" / "minihost-build" / "aex_l2_worker.exe",
    ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
    ROOT / "target" / "minihost-build" / "aex_smart_worker.exe",
]


def test_source_routes_cleanup_through_seh_and_uses_virtual_output_storage():
    source = SOURCE.read_text(encoding="utf-8")
    dispatch = DISPATCH.read_text(encoding="utf-8")
    smart_finalize = SMART_FINALIZE.read_text(encoding="utf-8")
    pixel_buffer = PIXEL_BUFFER.read_text(encoding="utf-8")
    assert 'g_telemetry.selector = "SMART_PRE_RENDER_CLEANUP"' in dispatch
    assert "invoke_smart_pre_render_cleanup_seh(cleanup, pre_render_data);" in smart_finalize
    assert "cleanup(pre_render_data);" not in smart_finalize
    assert "OutputPixelBuffer::reset" in pixel_buffer
    assert "MEM_RESERVE, PAGE_NOACCESS" in pixel_buffer
    assert "r.guarded->sentinels_intact()" in smart_finalize


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
