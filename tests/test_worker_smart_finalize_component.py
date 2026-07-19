from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.worker_text()
SOURCE = (ROOT / "minihost/src/worker_smart_finalize.cpp").read_text(encoding="utf-8")
RUNTIME = (ROOT / "minihost/src/worker_smart_render_runtime.cpp").read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")


def test_smart_finalize_cleanup_has_a_compiled_owner():
    assert CMAKE.count("src/worker_smart_finalize.cpp") == 1
    assert "invoke_smart_pre_render_cleanup_seh" in SOURCE
    assert "handles::dispose_handle" in SOURCE
    assert "h.end_lifecycle" in SOURCE
    assert "state.hosted_layers.clear()" in SOURCE
    assert "render::copy_packed_world" in SOURCE
    assert "render::finite_float_world" in SOURCE
    assert "smart_finalize::finalize(" in RUNTIME


def test_finalize_preserves_output_validation_and_error_priority():
    assert "result.render_error == 0) result.render_error = -5" in SOURCE
    assert "!result.output_pixels_valid" in SOURCE
    assert "result.render_error = -6" in SOURCE
    assert "result.render_error = -4" in SOURCE
    assert "result.guards_intact = r.guarded->sentinels_intact()" in SOURCE
