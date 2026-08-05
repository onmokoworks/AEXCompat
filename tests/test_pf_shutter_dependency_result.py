import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def test_shutter_transport_is_independent_from_dependency_advertisement():
    data = json.loads((ROOT / "analysis" / "PF_SHUTTER_DEPENDENCY_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    cases = {case["fixture"]: case for case in data["cases"]}
    advertised = cases["pf_shutter_dependency_probe"]
    unadvertised = cases["pf_shutter_unadvertised_probe"]
    assert data["sdk_constant"]["PF_OutFlag_I_USE_SHUTTER_ANGLE"] == 524288
    assert advertised["shutter_dependency_advertised"] is True
    assert unadvertised["shutter_dependency_advertised"] is False
    for case in cases.values():
        assert case["status"] == "render_completed"
        assert case["shutter_angle_fixed"] == 32768
        assert case["shutter_phase_fixed"] == -16384
        assert case["guard_bytes_intact"] is True
    assert advertised["output_sha256"] == unadvertised["output_sha256"]
    assert data["render_cache_implemented"] is False


