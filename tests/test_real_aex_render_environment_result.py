import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_real_aex_render_environment_result():
    result = json.loads(
        (ROOT / "analysis/REAL_AEX_RENDER_ENVIRONMENT_RESULT_2026-07-15.json").read_text(
            encoding="utf-8"
        )
    )
    requested = result["requested"]
    assert requested["quality_value"] == 0
    assert requested["field_value"] == 1
    assert requested["shutter_angle_fixed"] == 32768
    assert requested["shutter_phase_fixed"] == -16384
    for path in ("classic", "smartfx"):
        assert result[path]["passed"]
        assert result[path]["worker_echo_matches"]
        assert result[path]["guard_bytes_intact"]
        assert result[path]["world_lifetimes_balanced"]
        assert result[path]["param_checkouts_balanced"]
    assert result["transport"]["worker_revalidates_payload"]
    assert result["transport"]["broker_rejects_worker_echo_mismatch"]
