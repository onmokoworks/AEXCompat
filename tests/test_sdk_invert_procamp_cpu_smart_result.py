import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_INVERT_PROCAMP_CPU_SMART_RESULT_2026-07-15.json"
BUILD_PROPS = ROOT / "tools" / "sdk-fixtures" / "invert-procamp-cpu.props"


def test_invert_procamp_cpu_fixture_preserves_the_official_effect_source():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    boundary = result["build_boundary"]
    props = BUILD_PROPS.read_text(encoding="utf-8")

    assert boundary["official_effect_source_modified"] is False
    assert boundary["cpu_filter_source_retained"] is True
    assert boundary["gpu_execution_enabled"] is False
    assert 'CustomBuild Remove="..\\SDK_Invert_ProcAmp_Kernel.cu"' in props
    assert 'CustomBuild Remove="..\\SDK_Invert_ProcAmp_Kernel.chlsl"' in props


def test_invert_procamp_cpu_render_matches_depth_specific_oracles():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    render = result["render"]

    assert result["result"] == "smart_cpu_procamp_image_io_completed"
    assert render["argb8_fixed_point_oracle_exact"] is True
    assert render["argb8_ideal_invert_max_error"] == 1
    assert render["argb16_default_invert_oracle_exact"] is True
    assert render["argb32f_default_invert_oracle_exact"] is True
    assert render["argb32f_adjusted_procamp_oracle_exact"] is True


def test_invert_procamp_checkout_and_cpu_fallback_ownership_is_balanced():
    render = json.loads(RESULT.read_text(encoding="utf-8"))["render"]

    assert render["parameter_checkout_count"] == 4
    assert render["parameter_checkout_count"] == render["parameter_checkin_count"]
    assert render["automatic_parameter_checkin_count"] == 4
    assert render["gpu_render_possible"] is True
    assert render["gpu_render_dispatched"] is False
    assert render["suite_acquire_count"] == render["suite_release_count"]
    for invariant in (
        "suite_leases_balanced",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "parameter_checkouts_balanced",
        "guard_bytes_intact",
    ):
        assert render[invariant] is True
