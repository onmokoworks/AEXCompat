from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost" / "src" / "worker_classic_runtime.hpp"
SOURCE = ROOT / "minihost" / "src" / "worker_classic_runtime.cpp"
WORKER = source_owners.L2_MAIN
CMAKE = ROOT / "minihost" / "CMakeLists.txt"


def test_classic_runtime_owns_per_render_state_and_dispatch_boundary():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    worker = WORKER.read_text(encoding="utf-8")

    assert "thread_local Context* g_active_context" in source
    assert "previous_(g_active_context)" in source
    assert "g_active_context = previous_" in source
    assert "std::vector<TimedLayerDefinition> timed_layers_" in header
    assert "std::map<int32_t, ParameterDefinition> definitions_" in header
    assert "aexcompat::render::dispatch(render_context)" in source
    checkout = source_owners.contract_text("classic_param_checkout")
    assert "worker_runtime::classic::dispatch(context)" in worker
    assert "g_timed_classic_layers" not in worker
    assert "g_classic_render_selector_dispatched" not in worker
    assert "g_checkout_layer_definitions.find(index)" in checkout
    assert "!classic_context && aexcompat::worker_runtime::classic::dispatch_active()" in checkout
    assert "classic_context->record_checkout" in checkout
    # The classic completion report assembly lives in worker_classic_report.
    assert ("classic_diagnostics.shutter_dependency_advertised"
            in source_owners.contract_text("classic_report"))
    classic_branch = checkout[checkout.index("if (classic_context) {"):
                              checkout.index("const auto hosted =", checkout.index("if (classic_context) {"))]
    assert "copy_definition" in classic_branch


def test_classic_runtime_preserves_timed_checkout_and_cleanup_hooks():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    worker = WORKER.read_text(encoding="utf-8")

    assert "same_rational_time" in source
    assert "copy_timed_layer" in header
    assert "classic_context->copy_timed_layer" in source_owners.contract_text(
        "classic_param_checkout")
    assert "classic_render_cleanup" in worker
    assert "classic_render_dependencies_ready" in worker
    assert "src/worker_classic_runtime.cpp" in CMAKE.read_text(encoding="utf-8")
    assert "worker_classic_runtime_selftest" in CMAKE.read_text(encoding="utf-8")
