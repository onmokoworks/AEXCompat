from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.worker_text()
HEADER = (ROOT / "minihost/src/worker_smart_setup.hpp").read_text(encoding="utf-8")
SOURCE = (ROOT / "minihost/src/worker_smart_setup.cpp").read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")


def test_smart_parameter_world_plan_has_a_compiled_owner():
    assert CMAKE.count("src/worker_smart_setup.cpp") == 1
    assert "struct Context" in HEADER
    assert "struct Request" in HEADER
    assert "struct Plan" in HEADER
    assert "Plan prepare(" in SOURCE
    assert "smart_setup::prepare(" in MAIN


def test_plan_preserves_gpu_format_dimensions_and_time_setup():
    for marker in (
        "state.wide_time_checkout_allowed",
        "state.current_time = request.external_current_time",
        'case_id == "gpu_fallback_float32"',
        'case_id == "gpu_opencl_float32"',
        'case_id == "gpu_directx_float32"',
        'case_id == "partial_output_request"',
        'case_id == "connected_map"',
        "plan.width > 4096",
        "plan.pixel_bytes = plan.float32 ? 16 : (plan.deep16 ? 8 : 4)",
    ):
        assert marker in SOURCE


def test_world_buffers_and_parameter_definitions_are_owned_by_setup_tu():
    for marker in (
        "bool prepare_world_buffers(",
        "bool prepare_parameters(",
        "parameter_execution::apply_arbitrary_text_assignments",
        "parameter_execution::apply_requested_assignments",
        "parameter_execution::apply_arbitrary_parameter_animation",
        "checkout.definitions.emplace",
        'hooks.dump_world("smart-input"',
    ):
        assert marker in SOURCE
    assert "smart_setup::prepare_world_buffers(" in MAIN
    assert "smart_setup::prepare_parameters(" in MAIN
