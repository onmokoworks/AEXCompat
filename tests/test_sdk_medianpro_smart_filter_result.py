import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_MEDIANPRO_SMART_FILTER_RESULT_2026-07-15.json"
SOURCE = source_owners.L2_MAIN
PARAMETER_RUNTIME = ROOT / "minihost" / "src" / "worker_parameter_runtime.hpp"


def test_medianpro_smart_cpu_image_io_contract():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    render = result["render"]

    assert result["result"] == "smart_cpu_image_io_completed"
    assert result["descriptor"]["parameter_count"] == 5
    assert render["radius_one_median_independent_oracle_exact"] is True
    assert render["mix_zero_identity_exact"] is True
    assert render["argb16_cpu_passthrough_exact"] is True
    assert render["argb32f_cpu_passthrough_exact"] is True
    assert render["parameter_checkout_count"] == 5
    assert render["automatic_parameter_checkin_count"] == 5
    for ownership in (
        "suite_leases_balanced",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "parameter_checkouts_balanced",
    ):
        assert render[ownership] is True


def test_reused_paramdef_addresses_are_reference_counted_until_auto_checkin():
    source = source_owners.worker_text()
    runtime = PARAMETER_RUNTIME.read_text(encoding="utf-8")

    assert "std::unordered_map<void*, uint32_t> live" in runtime
    assert "g_live_param_checkouts = g_parameter_runtime.checkout.live" in source
    assert "++g_live_param_checkouts[definition]" in source
    assert "checkout_count += checkout.second" in source


def test_gpu_is_not_emulated_without_a_real_backend():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    policy = result["gpu_policy"]

    assert policy["normal_image_mode_dispatches_cpu"] is True
    assert policy["gpu_execution_requires_a_real_backend_and_matching_suites"] is True
    assert policy["unsupported_framework_setup_isolated_to_worker"] is True
