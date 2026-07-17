import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_PARAMARAMA_PARAMETER_MATRIX_RESULT_2026-07-15.json"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_paramarama_exposes_the_full_parameter_type_sample():
    inspection = result()["inspection"]
    assert inspection["reported_num_params_including_input"] == 9
    assert inspection["editable_parameter_count"] == 8
    assert [parameter["kind"] for parameter in inspection["parameters"]] == [
        "integer",
        "color",
        "float",
        "integer",
        "angle",
        "integer",
        "point3d",
        "button",
    ]
    assert inspection["parameters"][5]["choices"] == [
        "Make Slower",
        "Make Jaggy",
        "(-",
        "Plan A",
        "Plan B",
    ]
    assert inspection["parameters"][7]["supervised"] is True


def test_zero_amount_is_pixel_exact_and_nonzero_amount_convolves():
    evidence = result()
    copy = evidence["copy_oracle"]
    convolve = evidence["typed_convolve_oracle"]
    assert copy["status"] == convolve["status"] == "render_completed"
    assert copy["pixel_exact_copy"] is True
    assert copy["input_sha256"] == copy["output_sha256"]
    assert convolve["input_sha256"] != convolve["output_sha256"]
    assert convolve["differing_input_pixels"] == 851
    assert convolve["requested_values"]["angle"] == [45.0]
    assert convolve["requested_values"]["point3d"] == [25.0, 50.0, 75.0]


def test_classic_depth_negotiation_and_button_lifecycle_are_bounded():
    evidence = result()
    matrix = evidence["classic_matrix"]
    button = evidence["supervised_button"]
    safety = evidence["safety"]
    assert matrix["case_count"] == 6
    assert matrix["applicable_count"] == matrix["passed_count"] == 2
    assert matrix["unsupported_count"] == 4
    assert matrix["failed_count"] == 0
    assert button["selector_error"] == 0
    assert button["return_message"] == "Paramarama button hit!"
    assert button["display_error_message_flag_set"] is True
    assert safety["guard_bytes_intact"] is True
    assert safety["handle_lifetimes_balanced"] is True
    assert safety["world_lifetimes_balanced"] is True
    assert safety["suite_acquires"] == safety["suite_releases"] == 1
    assert safety["suite_leases_balanced"] is True


def test_fixture_build_keeps_the_sdk_source_outside_the_repository():
    evidence = result()
    props = (ROOT / "tools" / "sdk-fixtures" / "paramarama-v143.props").read_text(
        encoding="utf-8"
    )
    assert evidence["fixture"]["source_modified"] is False
    assert evidence["fixture"]["sha256"] == "4AB18C55F9FC20EDED09663F50C45623272B0655AB0CBF06E7C121B6A35523B0"
    assert "DisableSpecificWarnings" in props
    assert "Adobe SDK sample source unchanged" in props
