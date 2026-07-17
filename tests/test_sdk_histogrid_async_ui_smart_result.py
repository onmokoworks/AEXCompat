import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_HISTOGRID_ASYNC_UI_SMART_RESULT_2026-07-15.json"
BUILD_PROPS = ROOT / "tools" / "sdk-fixtures" / "histogrid-v143.props"


def test_histogrid_build_keeps_the_official_sdk_effect_source_unchanged():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    boundary = result["build_boundary"]
    props = BUILD_PROPS.read_text(encoding="utf-8")

    assert boundary["official_effect_source_modified"] is False
    assert boundary["platform_toolset_override"] == "v143"
    assert boundary["disabled_warning"] == "C4103"
    assert boundary["all_other_warnings_remain_errors"] is True
    assert "<DisableSpecificWarnings>4103;%(DisableSpecificWarnings)" in props


def test_histogrid_async_custom_ui_draw_closes_all_owned_resources():
    draw = json.loads(RESULT.read_text(encoding="utf-8"))["custom_ui_draw"]

    assert draw["event_error"] == 0
    assert draw["event_out_flags"] & 1
    assert draw["paint_rect_calls"] == 1
    assert draw["stroke_path_calls"] == 100
    assert draw["drawbot_objects_created"] == draw["drawbot_objects_released"]
    assert draw["drawbot_invalid_operations"] == 0
    assert draw["async_checkout_completed_without_receipt"] is True
    assert draw["sequence_setdown_error"] == 0
    assert draw["suite_leases_balanced"] is True
    assert draw["handle_lifetimes_balanced"] is True


def test_histogrid_smartfx_image_is_valid_while_sdk_suite_warning_is_preserved():
    render = json.loads(RESULT.read_text(encoding="utf-8"))["smartfx_render"]

    assert render["dimensions"] == [37, 23]
    assert render["pre_render_error"] == 0
    assert render["smart_render_error"] == 0
    for invariant in (
        "result_rects_valid",
        "guard_bytes_intact",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "parameter_checkouts_balanced",
    ):
        assert render[invariant] is True
    assert render["input_sha256"] != render["output_sha256"]
    assert render["suite_leases_balanced"] is False
    assert render["suite_lease_warning"] is True
    assert render["live_suite_leases"] == "PF World Suite@2=99"
    assert render["image_retained_despite_non_ownership_warning"] is True


def test_histogrid_draw_and_smartfx_share_the_production_broker_worker():
    integrated = json.loads(RESULT.read_text(encoding="utf-8"))[
        "integrated_draw_smartfx_render"
    ]

    assert integrated["single_isolated_worker"] is True
    assert integrated["action_payload"] == "draw:v1"
    assert integrated["draw_dispatched"] is True
    assert integrated["draw_error"] == 0
    assert integrated["draw_out_flags"] & 1
    assert integrated["ui_lifecycle_errors"] == [0, 0, 0, 0]
    assert integrated["ui_context_closed_before_setdown"] is True
    assert integrated["pre_render_error"] == 0
    assert integrated["smart_render_error"] == 0
    cases = integrated["depth_cases"]
    assert [case["pixel_format"] for case in cases] == ["argb8", "argb16", "argb32f"]
    assert len({case["input_sha256"] for case in cases}) == 3
    assert len({case["output_sha256"] for case in cases}) == 3
    assert integrated["distinct_depth_output_hashes"] == 3
    assert integrated["png_saved"] is True
    assert integrated["passed"] is True
