import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_PATHMASTER_HARD_EDGE_RESULT_2026-07-15.json"
WORKER_SOURCES = source_owners.contract_files("sdk_pathmaster_hard_edge_result")
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def test_pathmaster_hard_edge_mask_matches_the_independent_alpha_oracle():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    render = result["render"]

    assert result["result"] == "classic_path_bezier_and_feather_image_io_completed"
    assert render["dimensions"] == [37, 23]
    assert render["path_bounds"] == [8, 5, 29, 18]
    assert render["inside_nonzero_alpha_pixels"] == render["expected_inside_pixels"] == 21 * 13
    assert render["outside_nonzero_alpha_pixels"] == 0
    assert render["inside_alpha_values"] == [255]
    assert render["outside_alpha_values"] == [0]
    assert render["input_sha256"] != render["output_sha256"]


def test_pf_path_checkout_mask_and_lifecycle_ownership_are_balanced():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    path = result["path_contract"]
    lifecycle = result["lifecycle"]

    assert path["path_query_suite_version"] == path["path_data_suite_version"] == 1
    assert path["path_parameter_uses_stable_mask_id"] is True
    assert path["checkout_calls"] == path["checkin_calls"] == path["mask_world_calls"] == 1
    assert path["invalid_operations"] == path["reject_reason"] == 0
    assert path["lifetimes_balanced"] is True
    assert path["nonzero_feather_supported"] is True
    assert path["maximum_flattened_points"] == 1024
    assert lifecycle["mask_suite5_abi_uses_suite_version_6"] is True
    for key, value in lifecycle.items():
        if key.endswith("_error"):
            assert value == 0
    for invariant in ("handles_balanced", "suites_balanced", "worlds_balanced", "guards_intact"):
        assert lifecycle[invariant] is True


def test_worker_and_broker_keep_the_path_boundary_explicit_and_observable():
    worker = "\n".join(path.read_text(encoding="utf-8") for path in WORKER_SOURCES)
    broker = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")

    for marker in (
        '{"PF Path Query Suite", 1, nullptr, &provide_path_query1',
        '{"PF Path Data Suite", 1, nullptr, &provide_path_data1',
        "struct MaskSuite5",
        '{"AEGP Layer Mask Suite", 6',
        "write_rect(lifecycle_world.data() + 44, 1, 1)",
        "transfer_mode < 0 || transfer_mode > 38",
        "bool flatten(",
        "double edge_distance(",
        "out.size()<=64*16",
        "pf_path_runtime::lifetimes_balanced()",
    ):
        assert marker in worker
    for marker in (
        'worker_report.get("pf_path_lifetimes_balanced") == Some(&Value::Bool(true))',
        '"pf_path_checkout_calls"',
        '"pf_path_checkin_calls"',
        '"pf_path_mask_calls"',
        '"invalid_pf_path_operations"',
    ):
        assert marker in broker


def test_anisotropic_feather_produces_bounded_directional_alpha_gradients():
    feather = json.loads(RESULT.read_text(encoding="utf-8"))["anisotropic_feather"]

    assert feather["feather_xy"] == [6.0, 3.0]
    assert feather["unique_alpha_value_count"] == 13
    assert feather["partial_alpha_pixels"] == 240
    assert feather["zero_alpha_pixels"] + feather["partial_alpha_pixels"] + feather["full_alpha_pixels"] == 37 * 23
    assert feather["horizontal_centerline_partial_pixels"] == 12
    assert feather["vertical_centerline_partial_pixels"] == 4
    assert feather["horizontal_centerline_partial_pixels"] > feather["vertical_centerline_partial_pixels"]
    assert feather["ownership_balanced"] is True


def test_bezier_tangents_produce_a_curve_instead_of_the_vertex_diamond():
    bezier = json.loads(RESULT.read_text(encoding="utf-8"))["bezier_path"]

    assert bezier["curve_vertices"] == 4
    assert bezier["nonzero_tangents"] == 8
    assert bezier["partial_alpha_pixels"] == 0
    assert abs(bezier["nonzero_alpha_pixels"] - bezier["ideal_ellipse_area"]) < 2
    assert bezier["nonzero_alpha_pixels"] > bezier["straight_vertex_diamond_area"] + 100
    assert bezier["ownership_balanced"] is True
