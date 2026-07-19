import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_SMARTYPANTS_DYNAMIC_SMART_RESULT_2026-07-15.json"
SOURCE = source_owners.L2_MAIN
def test_smartypants_dynamic_flags_have_a_valid_time_and_checkout_context():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    lifecycle = result["lifecycle"]

    assert result["result"] == "dynamic_flags_and_smart_image_io_completed"
    assert lifecycle["query_dynamic_flags_dispatched"] is True
    assert lifecycle["query_dynamic_flags_error"] == 0
    assert lifecycle["query_checkout_time_step"] > 0
    assert lifecycle["query_checkout_time_scale"] > 0
    assert lifecycle["sequence_setup_error"] == 0
    assert lifecycle["sequence_resetup_error"] == 0
    assert lifecycle["sequence_setdown_error"] == 0
    assert lifecycle["lifecycle_data_null"] is True


def test_smartypants_rgb_invert_is_exact_at_all_supported_cpu_depths():
    render = json.loads(RESULT.read_text(encoding="utf-8"))["render"]

    assert render["argb8_rgb_invert_oracle_exact"] is True
    assert render["argb16_rgb_invert_oracle_exact"] is True
    assert render["argb32f_rgb_invert_oracle_exact"] is True
    assert render["parameter_checkout_count"] == render["parameter_checkin_count"]
    assert render["suite_acquire_count"] == render["suite_release_count"]
    for invariant in (
        "suite_leases_balanced",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "parameter_checkouts_balanced",
        "guard_bytes_intact",
    ):
        assert render[invariant] is True


def test_conditional_selectors_run_after_render_time_is_initialized():
    source = source_owners.worker_text()

    assert "constexpr std::size_t kInCurrentTime = 224" in source
    assert "constexpr std::size_t kInTimeStep = 228" in source
    assert "constexpr std::size_t kInTimeScale = 240" in source
    classic_time = source.index("write<int32_t>(input, 224, external_current_time);")
    classic_dispatch = source.index(
        "dispatch_conditional_ui_selectors(entry, input, command_output, params.data())",
        classic_time,
    )
    assert classic_time < classic_dispatch


def test_l2_current_parameters_are_available_to_checkout_callbacks():
    source = source_owners.worker_text()

    assert "LifecycleCheckoutDefinitionsScope" in source
    assert "g_checkout_layer_definitions[static_cast<int32_t>(i)] = lifecycle_definitions[i]" in source
