import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_real_aex_spatial_context_result():
    result = json.loads(
        (ROOT / "analysis/REAL_AEX_SPATIAL_CONTEXT_RESULT_2026-07-15.json").read_text(
            encoding="utf-8"
        )
    )
    expected = {
        "downsample_x": [1, 2],
        "downsample_y": [1, 2],
        "pixel_aspect_ratio": [10, 11],
        "full_resolution_dimensions": [74, 46],
        "pre_effect_source_origin": [-7, 9],
    }
    assert result["requested_spatial_context"] == expected
    assert result["sdk_layout"] == {
        "downsample_x_offset": 284,
        "downsample_y_offset": 292,
        "pixel_aspect_ratio_offset": 300,
        "source": "Adobe After Effects SDK AE_Effect.h and local ABI layout probe",
    }
    for path in ("classic", "smartfx"):
        assert result[path]["passed"]
        assert result[path]["reported_spatial_context_matches"]
        assert result[path]["guard_bytes_intact"]
        assert result[path]["world_lifetimes_balanced"]
        assert result[path]["param_checkouts_balanced"]
        assert result[path]["quality"] == 1
        assert result[path]["local_time_step"] == 1
        assert result[path]["in_data_dimensions"] == [74, 46]
        assert result[path]["pre_effect_source_origin"] == [-7, 9]
    assert result["classic"]["output_origin"] == [0, 0]
    assert result["smartfx"]["output_origin"] == [2000, 2000]
    assert result["transport"]["zero_denominator_rejected_before_native_dispatch"]
    assert result["transport"]["worker_revalidates_payload"]
    assert result["transport"]["broker_rejects_worker_echo_mismatch"]
    assert result["transport"]["world_dimensions"] == [37, 23]
    assert result["transport"]["full_resolution_dimensions"] == [74, 46]
    assert result["transport"]["smartfx_frame_context_populated_before_conditional_ui"]
    assert result["transport"]["unpaired_origin_rejected_before_native_dispatch"]
