from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost" / "src" / "worker_parameter_runtime.hpp"
SOURCE = ROOT / "minihost" / "src" / "worker_parameter_runtime.cpp"
MAIN = ROOT / "minihost" / "src" / "l2_main.cpp"
CMAKE = ROOT / "minihost" / "CMakeLists.txt"


def test_parameter_runtime_owns_descriptors_ui_arbitrary_and_checkout_state():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")
    for marker in (
        "struct ParamRecord",
        "struct RequestedAssignment",
        "struct ArbitraryTelemetry",
        "struct UiState",
        "struct CheckoutState",
        "std::vector<ParamRecord> records",
        "std::unordered_map<void*, uint32_t> live",
        "State& state() noexcept",
    ):
        assert marker in header + source
    assert "struct ParamRecord" not in main
    assert "enum class RequestedKind" not in main
    assert "worker_parameter_runtime.cpp" in CMAKE.read_text(encoding="utf-8")
    assert '#include "worker_parameter_runtime.cpp"' not in main


def test_parameter_definition_abi_and_animation_timeline_stay_bound():
    header = HEADER.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")
    assert "kDefinitionSize = 176" in header
    assert "parameter_animation::ParameterTimeline" in header
    assert "kDefinitionSize == kParamSize" in main
    assert "parameters::timeline(slot)" in main
