from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
HEADER = (ROOT / "minihost/src/worker_smart_execution.hpp").read_text(encoding="utf-8")
SOURCE = (ROOT / "minihost/src/worker_smart_execution.cpp").read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")


def test_smart_dispatch_orchestration_has_a_compiled_owner():
    assert CMAKE.count("src/worker_smart_execution.cpp") == 1
    assert "struct Result" in HEADER
    assert "struct Request" in SOURCE
    assert "render::RenderKind::SmartPreRenderAndRender" in SOURCE
    assert "render::dispatch(context)" in SOURCE
    assert "struct SmartRenderRequest" not in MAIN
    assert "smart_execution::render_once(" in MAIN


def test_smart_admission_and_error_priority_are_preserved():
    assert "request.external_time_scale != 0" in SOURCE
    assert "request.external_time_step > 0" in SOURCE
    assert "request.external_total_time >= request.external_current_time" in SOURCE
    assert SOURCE.index("gpu_setup_error != 0") < SOURCE.index("pre_error != 0")
    assert "if (!context.selector_started && dispatch_error != 0)" in SOURCE
    assert "module_audit_required" in SOURCE
